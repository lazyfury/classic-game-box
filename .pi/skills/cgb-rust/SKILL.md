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
crates/cgb-systems/src/       纯领域：机种、核心注册表、joypad id（无依赖）
crates/cgb-libretro/src/      ffi.rs / loader.rs / host.rs（libretro frontend）
crates/cgb-audio/src/lib.rs   cpal + ringbuf
crates/cgb-input/src/lib.rs   键盘绑定 + gilrs
crates/cgb-library/src/       paths / settings / library / saves / cores（自定义核心清单）
crates/cgb-ui/src/            model.rs / view.rs / frame.rs
crates/cgb-app/src/           cli.rs（启动参数）/ app.rs（帧循环）/ session.rs（一局游戏）
cores/*/build.sh              原生核心构建
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
./scripts/build-cores.sh               # 原生核心（需网络一次）
```

## 3. 任务菜谱

**加一个 libretro environment 命令**：`cgb-libretro/src/ffi.rs` 加常量 →
`host.rs::environment` 加分支（返回 true/false 要诚实）→ 有副作用的加测试。

**试一个新核心**（不改代码）：`cargo run -p cgb-app -- --rom game.nes --core ./foo_libretro.dylib`。
`--core` 也收注册表 key（`mesen`/`mgba`）或 `cores/custom/cores.json` 里声明的 key。
路径按原样 dlopen；机种由 ROM 扩展名推断，帧率/采样率在 load 后从核心 `av_info` 读。
解析在 `cgb-app/src/app.rs::{resolve_core, find_module}`；清单解析在
`cgb-library/src/cores.rs`。

**正式接一个自定义核心（不改 Rust）**：建 `cores/custom/<name>/build.sh`（产出到
`cores/dist/`）+ 在 `cores/custom/cores.json` 加一行（key/name/system/dylib）→
`./scripts/build-cores.sh` 会跑每个 `cores/custom/*/build.sh`。

**把核心做成内置**（进注册表、设置页可选）：`cores/<name>/build.sh`（platform=osx）→
`cgb-systems/src/core_choice.rs` 的 `CORES` / `CORES_BY_SYSTEM` 加一行 →
若新机种，`SystemId` 加变体 + extensions。UI 不需要改。

**改 UI 视图**：`cgb-ui/src/model.rs` 加字段 → `view.rs` 构建树 →
`cgb-app` 把状态投影进 `ViewModel`。回调只 push `Action`，由 app drain。

**改音频**：`cgb-audio` 的环形队列；**回调里不加锁**（实时约束）。

**改输入**：键盘在 `cgb-input` 的 `KeyboardBindings`，手柄在 `Gamepads`；
两者独立记录、取 OR，不要互相覆盖。

## 4. 陷阱

- core 的 `pitch` 是行字节数，不是 `width*4`；按 pitch 逐行拷。
- `retro_get_system_av_info` **必须在 load 之后读**（mGBA 尤其）。
- XRGB8888 内存里是 `B,G,R,X`，要 swizzle 成 RGBA8。
- 分辨率/帧率/采样率随 core 变，不能写死（GBA 240×160、GB 160×144）。
- quill 没有 Image 组件，`cgb-ui` 目前发不出 `DrawImage`（见 `frame.rs`）。
- 切换机种要拆掉旧 `Session` 重建新机器，不能复用。
