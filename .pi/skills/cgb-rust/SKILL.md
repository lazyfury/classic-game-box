---
name: cgb-rust
description: Classic Game Box 的 Rust 前端（分支 refactor/app-root）：根包 cgb-app（src/ 应用 + src/ui 视图 + src/app/* 功能模块）与 crates/ 下的 cgb-libretro / cgb-systems / cgb-audio / cgb-input / cgb-library，cores/ 里的原生核心构建，以及只做 UI 与 libretro 兼容的约束。用于改 Rust 代码、接 libretro core、改 UI 视图、加音频/手柄，或 “cargo check 不过 / 核心加载不了” 这类问题。
---

# cgb-rust —— 原生 Rust + igui + libretro

迁移总设计：`docs/architecture/quill-native-migration.md`。动手前先读它和根 `AGENTS.md`。

## 0. 布局与上下文纪律

产品是 **Swift/macOS app**（`macos/`）；Rust 侧**全是 library crate**，共享逻辑在
`crates/cgb-app`，嵌入 host 的 C ABI 在 `crates/cgb-mac`。**白名单**：

```
Cargo.toml                    虚拟 workspace（[workspace] + [workspace.dependencies]，无 [package]）
crates/cgb-app/Cargo.toml     package cgb-app（lib + 次要 winit bin `classic-game-box`）
crates/cgb-app/src/main.rs lib.rs cli.rs   瘦 CLI / 库面 / 启动参数
crates/cgb-app/src/app/       应用：mod.rs(App/AppLogic/run) + host.rs(HostWindow/GamepadSource) + 按功能拆的
                              library / screenshots / saves / cheats / settings / cores /
                              input / textures / window / project / helpers / tests
crates/cgb-app/src/session.rs 一局游戏
crates/cgb-app/src/selfcheck.rs cores_cli.rs 无头自检 / 核心管理 CLI
crates/cgb-app/src/ui/        视图：mod.rs(model/theme/icons/frame/view/*)；只吃 ViewModel
crates/cgb-app/benches/ui.rs  UI CPU 基准
crates/cgb-systems/src/       纯领域：机种、CoreSpec、选核、joypad id（无依赖）
crates/cgb-libretro/src/      ffi.rs / loader.rs / host.rs（libretro frontend）
crates/cgb-audio/src/lib.rs   cpal + ringbuf
crates/cgb-input/src/lib.rs   键盘绑定 + GamepadSnapshot + gilrs(可选 feature)
crates/cgb-paths/src/         paths（目录布局）+ settings（设置 JSON）
crates/cgb-cores/src/         cores（cores.json 清单）/ catalog（buildbot）/ download
crates/cgb-library/src/       library（SQLite 库）/ import / saves / cheats / png_codec
crates/cgb-mac/               嵌入 host（crate cgb-mac）：CAMetalLayer→wgpu surface、事件、`cgb_mac_*` C ABI
cores/cores.json              核心清单
cores/<name>/build.sh         每个核心的原生构建（产出到 cores/dist/）
macos/                        Swift app（主要产品）：SwiftPM 包（窗口/CAMetalLayer/事件/手柄）
```

**禁读**：`target/`、`legacy/`（除非查历史决策）、`cores/sources/`、`cores/dist/`、
`Cargo.lock`、`cores/libretro/libretro.h` 的正文（`rg` 定位再看）。
igui 的源码可看相邻 `../igui/crates/`，只读需要的模块。

## 1. 依赖方向（不许反向）

```
crates/cgb-app (src/)  → { cgb-libretro, cgb-audio, cgb-input, cgb-paths, cgb-cores, cgb-library, cgb-systems }
cgb-app/src/ui         → cgb-systems, igui_*
crates/cgb-mac         → cgb-app (default-features = false), cgb-input, igui, arboard
cgb-libretro          → cgb-systems, libloading
cgb-input             → cgb-systems, gilrs（自带 `Key`，不依赖 UI）
cgb-audio             → cpal, ringbuf
cgb-paths             → cgb-systems, serde, dirs
cgb-cores             → cgb-systems, serde, ureq, zip
cgb-library           → cgb-paths, cgb-systems, rusqlite, png
cgb-systems           → 无
```

`src/ui` 不认识 libretro；`cgb-libretro` 不认识 UI / 音频设备；`crates/` 不反向依赖根 app。

**host 抽象（实验路径）**：`cgb-app` 不直接依赖窗口/手柄实现——窗口经
`src/app/host.rs` 的 `HostWindow` trait（服务 `SharedHostWindow`），手柄经 `GamepadSource`
（服务 `SharedGamepad`）；默认由 `winit-host` feature 提供 winit + gilrs，嵌入版
`cgb-app = { default-features = false }` 则一个都不编。`App` / `App::new` / `drop_sink` 为嵌入 host 公开。

## 2. 命令

```bash
./scripts/dev.sh                       # fmt --check + clippy -D warnings + test
cargo check --workspace
cargo run -- --rom game.nes            # 根包，cargo run 即可（-p cgb-app 等价）
cargo bench --bench ui                 # UI 帧管线基准
./scripts/build-cores.sh               # 原生核心（需网络一次；mGBA 需 cmake）
```

## 3. 任务菜谱

**加一个 libretro environment 命令**：`cgb-libretro/src/ffi.rs` 加常量 →
`host.rs::environment` 加分支（返回 true/false 要诚实）→ 有副作用的加测试。

**核心清单是数据（不改 Rust）**：所有核心都在单一 `cores/cores.json` 里，每行
`key` / `name` / `system` / `dylib`（+ 可选 `sample_rate` / `fps`）；`key` 每机种唯一。
清单解析在 `cgb-cores/src/cores.rs::load_cores`；选核在
`src/app/{mod,cores}.rs::{load_core_manifest, resolve_core, find_module}` 与
`cgb-systems::choose_core`。设置持久化按 key 字符串（`Settings::core_key`）。

**加一个核心**：完整流程见 `cores/README.md` 的 "Adding a core"。顺序：

1. **先构建**：从 `cores/build.sh.example` 抄一个 `cores/<name>/build.sh`（产出到
   `cores/dist/`）并跑通；无源码构建的（buildbot dylib / 随包二进制）就固定那份产物。
2. **对齐 Rust 端 API**：用 `--core <path>` 或 host 测试跑真实核心，看它调了哪些
   libretro environment，对照 `cgb-libretro/src/host.rs`（接受的像素格式
   `XRGB8888`/`RGB565`、`need_fullpath`、`SET_HW_RENDER` 只给 OpenGL、core options
   v1/v2、输入/rumble、system/save 目录）；缺的补 `host.rs`/`ffi.rs` 并加测试，私有扩展忽略。
3. **清单**：在 `cores/cores.json` 加一行（`key`/`name`/`system`/`dylib`）。
4. **新机种**：`cgb-systems/src/system.rs` 的 `SystemId` 变体 + extensions，
   `cgb-library/src/settings.rs` 加 `<system>_core` 字段 + key 匹配；UI 不用改。
5. **验证**：`nm -gU` 看 `retro_*`，并加进
   `crates/cgb-libretro/tests/cores_run_through_the_host.rs`。要从 `--core <path>`
   直接试，连清单都不用。
6. **定发布方式**：必须离线 / 干净可分发 / buildbot 没有 → 加进 `scripts/core-profiles.sh`
   的 `CGB_MINIMAL_CORES` 随包；buildbot 有且可商用 → 留作运行时下载（必要时
   `./scripts/update-core-catalog.sh`）；有但跑不了 → 加 `BLOCKED_CORES`。

**改 UI 视图**：`src/ui/model.rs` 加字段 → `src/ui/view/` 构建树 →
`src/app/`（`app::mod` 的投影 + 对应功能模块）把状态投影进 `ViewModel`。
回调只 push `Action`，由 app drain。

**改音频**：`cgb-audio` 的环形队列；**回调里不加锁**（实时约束）。

**改输入**：键盘在 `cgb-input` 的 `KeyboardBindings`，手柄在 `Gamepads`；
两者独立记录、取 OR，不要互相覆盖。热键/动作分发在 `src/app/input.rs`。

## 4. 陷阱

- core 的 `pitch` 是行字节数；每像素字节数（`bpp`）由像素格式决定，
  行切片按 `width * bpp`（不是写死 4）。
- `SET_PIXEL_FORMAT` 只在接受时才记录格式：`XRGB8888` 与 `RGB565` 接受，
  其余返回 false 并保持 `0RGB1555`（上游 mGBA 输出 RGB565）。
- `retro_get_system_av_info` **必须在 load 之后读**（mGBA 尤其）。
- 部分 core（Mesen）声明 `need_fullpath`，会读 `game_info.path` 而非内存指针。
- XRGB8888 内存里是 `B,G,R,X`，要 swizzle 成 RGBA8。
- 分辨率/帧率/采样率随 core 变，不能写死（GBA 240×160、GB 160×144）。
- igui 没有标准 Image 内容类型，`src/ui/frame.rs` 的 `FrameImage` 仍自绘 `DrawImage`。
- 切换机种要拆掉旧 `Session` 重建新机器，不能复用。
