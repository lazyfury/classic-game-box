---
name: cgb-rust
description: Classic Game Box 的 Rust 前端（分支 quill-native）：crates/ 下的 cgb-app / cgb-ui / cgb-libretro / cgb-systems / cgb-audio / cgb-input / cgb-library，cores/ 里的 Mesen 与 mGBA 原生构建，以及只做 UI 与 libretro 兼容的约束。用于改 Rust 代码、接 libretro core、改 UI 视图、加音频/手柄，或 “cargo check 不过 / 核心加载不了” 这类问题。
---

# cgb-rust —— 原生 Rust + quill + libretro

迁移总设计：`docs/architecture/quill-native-migration.md`。动手前先读它和根 `AGENTS.md`。

## 0. 上下文纪律

**白名单**：

```
Cargo.toml                    工作区与依赖
crates/cgb-systems/src/       纯领域：机种、CoreSpec、选核、joypad id（无依赖）
crates/cgb-libretro/src/      ffi.rs / loader.rs / host.rs（libretro frontend）
crates/cgb-audio/src/lib.rs   cpal + ringbuf
crates/cgb-input/src/lib.rs   键盘绑定 + gilrs
crates/cgb-library/src/       paths / settings / library / saves / cores（自定义核心清单）
crates/cgb-ui/src/            model.rs / view.rs / frame.rs
crates/cgb-app/src/           cli.rs（启动参数）/ app.rs（帧循环）/ session.rs（一局游戏）
cores/cores.json              核心清单（mesen / mgba / nestopia / custom_nes_core / fbneo）
cores/<name>/build.sh         每个核心的原生构建（产出到 cores/dist/）
```

**禁读**：`target/`、`legacy/`（除非查历史决策）、`cores/sources/`、`cores/dist/`、
`Cargo.lock`、`cores/libretro/libretro.h` 的正文（`rg` 定位再看）。
quill 的源码在 `../quill/crates/`，同样只读需要的模块。

## 1. 依赖方向（不许反向）

```
cgb-app → { cgb-ui, cgb-libretro, cgb-audio, cgb-input, cgb-library, cgb-systems }
cgb-ui        → cgb-systems, draw_*
cgb-libretro  → cgb-systems, libloading
cgb-input     → cgb-systems, gilrs, draw_core
cgb-audio     → cpal, ringbuf
cgb-library   → rusqlite, serde
cgb-systems   → 无
```

`cgb-ui` 不认识 libretro；`cgb-libretro` 不认识 UI / 音频设备。

## 2. 命令

```bash
./scripts/dev.sh                       # fmt --check + clippy -D warnings + test
cargo check --workspace
cargo run -p cgb-app -- --rom game.nes
./scripts/build-cores.sh               # Mesen + mGBA + 自定义核心（需网络一次；mGBA 需 cmake）
```

## 3. 任务菜谱

**加一个 libretro environment 命令**：`cgb-libretro/src/ffi.rs` 加常量 →
`host.rs::environment` 加分支（返回 true/false 要诚实）→ 有副作用的加测试。

**核心清单是数据（不改 Rust）**：所有核心（Mesen / mGBA / nestopia / custom_nes_core）
都在单一 `cores/cores.json` 里，每行 `key` / `name` / `system` / `dylib`
（+ 可选 `sample_rate` / `fps`）；`key` 每机种唯一。清单解析在
`cgb-library/src/cores.rs::load_cores`；选核在 `cgb-app/src/app.rs::{load_core_manifest,
resolve_core, find_module}` 与 `cgb-systems::choose_core`。设置持久化按 key 字符串
（`Settings::core_key`）。

**加一个核心**：完整清单见 `cores/README.md` 的 "Adding a core"。简版：从
`cores/build.sh.example` 抄一个 `cores/<name>/build.sh`（产出到 `cores/dist/`）+
在 `cores/cores.json` 加一行 → `./scripts/build-cores.sh` 跑每个 `cores/*/build.sh`。
新机种还要：`cgb-systems/src/system.rs` 的 `SystemId` 变体 + extensions，
`cgb-library/src/settings.rs` 加 `<system>_core` 字段 + key 匹配；UI 不用改。
验证：`nm -gU` 看 `retro_*`，并加进 `crates/cgb-libretro/tests/cores_run_through_the_host.rs`。
要从 `--core <path>` 直接试，连清单都不用。

**改 UI 视图**：`cgb-ui/src/model.rs` 加字段 → `view.rs` 构建树 →
`cgb-app` 把状态投影进 `ViewModel`。回调只 push `Action`，由 app drain。

**改音频**：`cgb-audio` 的环形队列；**回调里不加锁**（实时约束）。

**改输入**：键盘在 `cgb-input` 的 `KeyboardBindings`，手柄在 `Gamepads`；
两者独立记录、取 OR，不要互相覆盖。

## 4. 陷阱

- core 的 `pitch` 是行字节数；每像素字节数（`bpp`）由像素格式决定，
  行切片按 `width * bpp`（不是写死 4）。
- `SET_PIXEL_FORMAT` 只在接受时才记录格式：`XRGB8888` 与 `RGB565` 接受，
  其余返回 false 并保持 `0RGB1555`（上游 mGBA 输出 RGB565）。
- `retro_get_system_av_info` **必须在 load 之后读**（mGBA 尤其）。
- 部分 core（Mesen）声明 `need_fullpath`，会读 `game_info.path` 而非内存指针。
- XRGB8888 内存里是 `B,G,R,X`，要 swizzle 成 RGBA8。
- 分辨率/帧率/采样率随 core 变，不能写死（GBA 240×160、GB 160×144）。
- quill 没有 Image 组件，`cgb-ui` 目前发不出 `DrawImage`（见 `frame.rs`）。
- 切换机种要拆掉旧 `Session` 重建新机器，不能复用。
