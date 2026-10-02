# Classic Game Box — 工作约定（Rust 版）

本仓库正在从 **Electron + WebAssembly + 自研核心** 迁移到 **原生 Rust + igui + libretro**。
自研 FC 核心的 C++ 源码在 `custom_nes_core/`（单个现代 CMake 项目，`src/`
布局；不参与 Rust 构建；其余旧栈已清理）。
权威设计见 [`docs/architecture/quill-native-migration.md`](docs/architecture/quill-native-migration.md)。

UI 栈是 [`igui`](https://github.com/lazyfury/igui)（`quill` 改名后的上游），以 git 依赖固定到
`v0.2.0` 的 commit；见 `Cargo.toml` 的 `igui` / `igui_svg` / `igui_winit` / `igui_core`。
不再需要相邻的 `../quill` checkout。

## 现状

- **布局**：产品是 **Swift/macOS app**（`macos/`，AppKit + `CAMetalLayer` + 原生事件）。
  另有一个实验性的 **C++/Win32 host**（`windows/`）：两个壳共用一份 **`src/native/`**
  的统一原生 host + `cgb_host_*` C ABI（见 `docs/architecture/windows-host-plan.md`；
  已在 Win11 VM 跑通窗口 + 反复 resize）。
  Rust 侧两个包：**根包 `cgb-app`**（`src/`：`src/ui` + `src/app` 应用逻辑、`src/library`
  游戏库、`src/paths`、`src/cores` 清单/下载、`src/audio`、`src/host` 契约、`src/native`
  是共享的原生 host + C ABI，产出 `libcgb_app.a` 供 Swift / C++ 链接）；**`crates/cgb-libretro`**
  （libretro front end + `system.rs`/`joypad.rs`/`core_choice.rs`/`input.rs` 纯域类型）。
  工作区根 `Cargo.toml` 同时是 workspace 与根包。
- 分支 `refactor/app-root`（合并后可回 `main`）。
- **Q0 完成**：计划、目录结构、Rust 工作区骨架、`cargo check/test/clippy` 全绿。
- **Q1 完成**：Mesen 原生 arm64 编译 + dlopen + 出画面（`ui::frame::FrameImage`）+ 键盘。
- **Q2 进行中**：音频（cpal）+ Swift `GameController` 手柄已接线，`.srm` 电池存档与即时存档
  （`Session::{quick_save,quick_load,save_state,load_state}`）。**快速存档是 3 档 LIFO 轮转**
  （`fast01` 最新；F5 存 / F6 读，读只取最新不弹出），另见 Q6 的手动槽；
  待人眼验收“能玩、能存读”。
- **Q3 完成**：库模型重建——DB 是模型（`games` + `tags`/`game_tags` + `screenshots`），
  **单库、自包含、可切换**：`--library-dir` / “打开游戏库…”选定唯一库根，
  DB / 截图 / 存档 / 金手指都在库根下，整个文件夹拷走即备份（不合并多个目录）；
  旧 `library_dirs` 只在迁移时读一次（取第一个）。app data 只留 `settings.json`、
  `cores` 与 `system`（BIOS 不放库里，避免 `neogeo.zip` 被扫成街机 ROM）。未选库时
  沿用旧布局。
  `name` 与 `file_name` 分离、可改名（`Library::rename`，UI 后置）；游玩次数/时长/最近；
  库顶统计总数量 + 按机种分别计数并点选筛选；
  置顶；排序（名称/大小/最近/时长/加入）；标签；截图（F12 / ⇧F12 设封面）+ 真实封面；
  卡片 SVG 图标 + 截图收藏页 + 大图预览。
  单游戏可用卡片右键的**「选择核心…」**覆盖该游戏用的核心（存在库 `games.core`，
  `Library::set_core`，`None` 即跟随该机种设置；rescan 不覆盖）。优先级：
  `--core` > 游戏覆盖 > 机种设置 > 清单首个核心。
  ROM 路径以**相对库根**存入 DB（schema v5，`Library::{key,resolve}`；旧库打开时自动
  迁移），且 `sync` **非破坏**（父目录不存在就不删行）——所以同一个库被 macOS / Windows
  两个 host 打开也不会互相删游戏/截图。注意：截图 PNG 一旦被删本地无法恢复。
- **性能**：DrawList 复用（运行游戏不重排重绘）+ 图标纹理化 + 库网格可见行虚拟化；
  `CGB_PERF=1` 打点。
- **Q5 打包**：`scripts/package-macos.sh` / `scripts/release.sh` 出 macOS `.app` + zip
  （cores/assets 入 Resources）；`--selfcheck` 无头自检。
- **Q6 定制**：输入能力对齐（16 键 + 模拟轴 + capabilities + 按机种绑定 + core descriptors）；
  即时存档（按 core 隔离 + 缩略图）：**快速存档是 3 档 LIFO 轮转**（`fast01` 最新，
  存档时旧的依次下移、超出 3 档丢最旧，避免死档；`Library::{roll_quick,compact_quick}`，
  `stateqN` 独立命名空间），另有固定手动槽 `save01–save09`（`stateN`，F1–F3 存 /
  Shift+F1–F3 读）；金手指 `.cht`；倒带（每 2 帧 / 10s）；画面后处理
  shader（扫描线/CRT/LCD/锐化）；core options + `SET_CONTROLLER_PORT_DEVICE` + core 消息；
  截图多选删除；中栏可拖动；全屏游玩（`ViewModel::fullscreen`，F11/play 列
  “全屏”按钮进入，Esc 退出；只挂载右栏游戏视图，库网格与侧栏不入树）。
  后处理用 `igui_backend_wgpu::TextureEffect`（`igui` 自 `v0.2.0` 提供）。
- **运行时**：`cgb-app` 跑在 igui 的 `igui_app` 插件运行时上，平台插件由
  `src/native/`（Swift host）提供：`NativeGpuPlugin`（CAMetalLayer → wgpu surface）/
  `NativeInputPlugin` / `NativeTextMeasurePlugin` / `NativeClipboardPlugin` /
  `NativeGamepadPlugin`；`App` 实现 `AppLogic`（`update/layout/paint`）。帧由
  `Session::advance(dt)` 的时间累积驱动，`needs_frame` 在跑游戏/带动画 overlay/倒带时为真。
  改名 / 搜索 / 标签编辑用上游 `igui_components::TextInput`（自带 caret/选区/IME 预编辑）。
- **核心清单统一**：所有核心都从单一 `cores/cores.json` 加载
  （mesen / mgba / nestopia / custom_nes_core / fbneo / snes9x / genesis_plus_gx /
  picodrive / parallel_n64 / ppsspp / mednafen_psx_hw / freej2me_plus）；
  `--core` 按 key 或路径选核。
  mGBA 用上游 `libretro/mgba`（CMake）构建，输出 **RGB565**，宿主已接受并转换。
  Sega 系（genesis / sms / gg / sg1000）有两个核心：Genesis Plus GX 与轻量的
  PicoDrive（同一模块四个机种，PicoDrive 是首个带 git submodule 的核心）。
  街机是 `SystemId::Arcade`（`.zip` → FBNeo，按 CRC 读标准 Neo Geo 套）。
- **动态下载核心（下载源）**：可运行时从 libretro buildbot 下载核心，不用打包全部
  `.dylib`。目录（下载源）落两份：内置快照 `cores/catalog.json`（`include_str!`
  进二进制，离线可搜，`scripts/update-core-catalog.sh` 重生成）+ 用户缓存
  `<app data>/cores/catalog.json`（`--force-update` 写，优先）。CLI：
  `--search-core <q>` / `--force-update` / `--download-core <name>` /
  `--core-base-url <url>`；设置页「下载核心」卡片是同一功能的 UI（搜索 + 下载 +
  进度 + 刷新源，下载在后台线程）。下载落到 `<app data>/cores/`，并登记到
  `downloaded.json`，经 `load_core_manifest` 合并进清单（core-info 的 `systemid`
  由 `SystemId::parse_key` 的别名映射，如 `super_nes`→`snes`）；不支持的机种不登记。
  新增依赖：`ureq`（rustls + proxy-from-env）+ `zip`（只 deflate）。
- **超任（SNES/SFC）**：`SystemId::Snes`（`.sfc/.smc/.fig/.swc`）→ **Snes9x**
  （`libretro/snes9x`，Makefile 在 `libretro/` 子目录，无 submodule）。**软件渲染**，
  输出 RGB565（宿主转换），不走 `SET_HW_RENDER`；SuperFX/SA-1/CX4/DSP1–4/MSU-1 等
  全内建，常规游戏**无 BIOS**（仅 BS-X/Sufami Turbo 可选，未开放该扩展名）。
  倒带**保留**（state ~804 KB，与 mGBA 同量级）。`.bin` 仍归 Genesis，SNES 的
  `.bin` 用卡片「选择机种…」覆盖。真机：
  `macos/scripts/run.sh game.sfc`（`./cores/snes9x/build.sh` 构建）。
- **N64 硬件加速（GL 路径）**：`SystemId::N64`（`.z64/.n64/.v64`）→
  ParaLLEl-N64 + GLideN64。`cgb-libretro` 实现 `SET_HW_RENDER`：用一个
  **离屏 CGL 4.1 core 上下文 + FBO**（`crates/cgb-libretro/src/gl.rs`）接管核心
  的 GL 渲染，每帧 `glReadPixels` 回读成 RGBA8，复用现有 `Frame`/`FrameImage`
  纹理路径。N64 关闭倒带（state 太大）。计划与进度见
  `docs/architecture/n64-gl-hw-render-plan.md`；真机验收：
  `macos/scripts/run.sh game.z64 --core parallel_n64`。
  **注意**：不要换成 Mupen64Plus-Next——它的 GLideN64 在这台 macOS 26 / M4 上
  渲染黑屏（RetroArch 里同样黑），ParaLLEl-N64 才正常。ParaLLEl-N64 带 arm64
  dynarec，速度也够。
- **PSP 硬件加速（GL 路径）**：`SystemId::Psp`（`.iso/.cso/.pbp/.chd`）→
  **PPSSPP**。它复用同一条 `SET_HW_RENDER` 离屏 GL 路径：`cores/ppsspp/build.sh`
  装 libretro buildbot 的 `apple/osx/arm64` dylib（上游 libretro Makefile 把 arm64
  当 x86_64、且 ffmpeg 子模块无 arm64 slice，源码构建不现实）；它请求
  `OPENGL_CORE 3.1`，正好被现有 4.1 core 上下文满足，GL 命令只在 `retro_run`
  线程执行（PPSSPP 的 emu 线程只排命令）。PSP 关闭倒带（state 大）。assets
  （`compat.ini`、字体、shader）由 `build.sh` 取上游 `assets/` 到
  `cores/dist/ppsspp/`，app 启动时递归 seed 到 `<system>/PPSSPP/`；否则核心在
  `retro_init` 告警 “Core system files missing, expect bugs.”。**已完成人工验收**
  （真实 PSP 游戏出画面 + 声音）。真机：
  `macos/scripts/run.sh game.iso --core ppsspp`。
  **注意**：`CoreHost::drop` 必须先调核心的 `context_destroy()` 再
  `retro_unload_game()`（RetroArch 同序）；PPSSPP 在 `retro_unload_game` 里
  `delete ctx`，反序会空指针崩溃。
- **PS1 硬件加速（GL 路径）**：`SystemId::PlayStation`（`.cue/.ccd/.toc/.m3u/.img`）→
  **Beetle PSX HW**（`libretro/beetle-psx-libretro`，源码构建 `HAVE_OPENGL=1`）。
  请求 `OPENGL_CORE 3.3`，复用同一条离屏 GL 路径。BIOS 可选（HLE/OpenBIOS；
  真实 BIOS 放 `<system>/scph550x.bin`）。PS1 关闭倒带（state 大）。`.iso/.chd/.pbp`
  与 PSP 冲突，仍归 PSP；单游戏用卡片右键的**「选择机种…」**覆盖（存在库
  `games.system`，`Library::set_system`，rescan 不覆盖）。**已完成人工验收**。
- **J2ME（Java ME）**：`SystemId::J2me`（`.jar`/`.kjx`）→ **FreeJ2ME-Plus**
  （`TASEmulators/freej2me-plus`）。它的 libretro 模块只是 C shim，用
  `fork/exec` 起一个 Java VM（`freej2me_plus-lr.jar`）走 stdin/stdout 管道；
  软件 XRGB8888 输出。`cores/freej2me_plus/build.sh` 除了 dylib 还编译 jar、
  用 `jlink` 出精简 JRE（`java.base,java.desktop,jdk.charsets`），产物在
  `cores/dist/freej2me_plus/`（dylib 在 `cores/dist/`）。app 启动时把 jar seed
  到 `<app data>/system/`（覆盖旧 jar）、把 `runtime/bin` 前置到 `PATH`（core 用
  `execvp("java")`），无需系统装 Java。宿主为此支持了 **core options v2** 与
  **`GET_RUMBLE_INTERFACE`**：FreeJ2ME-Plus 在 v1 下会把 v2 数组当 v1 读
  （分辨率变 0），且无 rumble 接口时会调空指针而 SIGSEGV；`build.sh` 还给
  `Libretro.java` 打补丁把管道路径按 UTF-8 解码（否则中文游戏名找不到文件、
  一直黑屏）、并把按住键的 `keyRepeated` 从每帧 60Hz 限速到“400ms 首延迟 +
  80ms 间隔”。manifest 的 `option_defaults` 在 load 前把 `freej2me_backlightcolor`
  默认设成 `Disabled`（核心默认 `Green`，会给整个画面蒙一层绿）。**音频不走 libretro**：
  声音由 Java 子进程用 JavaSound 直接播到 CoreAudio，能出声但不听前端控制（暂停/音量）
  ——当前按“保持现状”收尾。已知缺口：
  **无即时存档（倒带已禁用）、键盘回调未接**（joypad 可玩，手机键盘已映射到手柄）。
- **Swift/macOS host（主要产品）**：`macos/` 是产品前端——Swift 只做窗口 /
  `CAMetalLayer` / 原生事件，igui UI + wgpu 渲染 + libretro 模拟器全在 Rust
  （`src/native/`，经 `cgb_host_*` C ABI；FFI 边界**包含 UI**）。窗口依赖抽成
  `HostWindow` trait（`src/host.rs`），手柄抽成 `GamepadSource`，走 Swift
  `GameController`（快照经 `cgb_host_gamepad_*` 回灌）。Rust 侧产出 `libcgb_app.a`，
  SwiftPM 静态链接。打包 `macos/scripts/package.sh`。剩余缺口见
  `docs/architecture/swift-macos-host-plan.md`。

## 硬规则

1. **产品是 Swift/macOS app；Rust 两个包。** 根包 `cgb-app`（`src/`）+ 成员包
   `crates/cgb-libretro`。顶层只有 `Cargo.toml`、`src/`、`crates/`、`cores/`、`scripts/`、
   `assets/`、`docs/`、`custom_nes_core/`、`macos/`、`windows/`。不要往根目录丢构建产物或临时文件。
2. **只做 UI 与 libretro 兼容。** 不实现/移植自研模拟器；它已在 `custom_nes_core/`。
   新功能先问「libretro 有没有标准对应」。
3. **libretro 是唯一对外契约。** 只加载标准 libretro core（`cores/cores.json`
   里声明的，含 Mesen、mGBA、nestopia 与自研的 `custom_nes_core`）；
   不使用 `fc_*` 私有扩展（`custom_nes_core` 会导出该扩展，但被忽略）。
   ABI 头是 `cores/libretro/libretro.h`；**不要整读**（≈8700 行），`rg` 定位再看。
4. **依赖方向单向**：根包 `cgb-app` → `cgb-libretro` + `igui`；`cgb-libretro` 不认识
   UI、音频设备与游戏库（只暴露 `Frame` / `Vec<i16>` 与纯域类型）。
   `src/ui` 不认识 libretro；应用逻辑与设备/库在 `src/{app,library,paths,cores,audio,host,mac}`。
   成员包不得反向依赖根包（测试用的 dev-dependency 除外）。
5. **igui 的边界**：`igui_core` / `igui_scene` / `igui_ui` / `igui_components`
   不得依赖 `web_sys`/`wgpu`/DOM。应用内 `src/ui` 只用 `igui_*` 的公开 API。
6. **不写截图 / 录屏测试。** 用 `igui_backend_recording` 录 `DrawList` +
   `igui_profile::inspect`，或 core 侧假 frontend 单测。UI 好不好看由人看。
7. **`custom_nes_core/` 源码只读**：自研 FC 核心的单个 CMake 项目（`src/`
   布局；产品构建入口是 `cores/custom_nes_core/build.sh`），只在与本仓库
   的 libretro 契约对照时改动；要改需明确要求。
8. **不确定就问，不要猜。** 需求模糊、要动公共 API 或路线图时，先停下来问。
9. **不擅自开工。** 只实现已确认的任务；顺手发现的问题只汇报，不动手。

## 构建与验证（每阶段 gate）

```bash
./scripts/dev.sh          # fmt --check + clippy -D warnings + test
# 等价于：
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

原生 core 是第三方项目（外加自研的 custom_nes_core），按需构建（需网络，
首次几分钟）：

```bash
./scripts/build-cores.sh                    # → cores/dist/*_libretro.dylib
./cores/custom_nes_core/build.sh            # → cores/dist/custom_nes_core_libretro.dylib
cargo build                                 # → target/debug/libcgb_app.a（Swift 链接）
macos/scripts/run.sh                        # 开库界面
macos/scripts/run.sh /path/to/mario.nes                       # 直接开始
macos/scripts/run.sh mario.nes --core mesen                   # 强制核心
macos/scripts/run.sh mario.nes --core ./mycore_libretro.dylib # 任意模块
```

`cli` / `cores_cli` / `selfcheck` 仍是根包的公开模块，但当前没有独立 bin：
宿主（Swift）通过 `cli::Args` 传入同样的参数；无头自检走 `cargo test`。

## 目录地图

| 需要… | 看这里 |
|---|---|
| 迁移总设计、范围、里程碑、风险 | `docs/architecture/quill-native-migration.md` |
| 旧架构的来龙去脉（为什么用 wasm、为什么现在不用） | `docs/architecture/libretro-migration.md` |
| crate 职责与依赖 | `crates/README.md` |
| libretro frontend（dlopen / 回调 / 视频音频输入存档） | `crates/cgb-libretro/src/host.rs` |
| 机种 / CoreSpec 选核、joypad id | `crates/cgb-libretro/src/{system,core_choice,joypad}.rs` |
| UI 视图与帧循环 | `src/ui/`（视图 + `ViewModel`）、`src/app/`（`App` + `AppLogic` 组装） |
| 原生 core 构建 / 加核心流程 | `cores/README.md`、`cores/build.sh.example`、`cores/*/build.sh` |
| J2ME（Java ME）核心与随包 JRE | `cores/freej2me_plus/build.sh`、`src/app/mod.rs`（`j2me_dir` / `prepend_path`） |
| 核心清单（启动选核） | `cores/cores.json`、`src/cores/`、`src/cli.rs` |
| Swift/macOS host（主要产品） | `macos/`（Swift 窗口/事件）、`src/native/`（surface + 事件 + `cgb_host_*` C ABI）、`docs/architecture/swift-macos-host-plan.md` |
| C++/Win32 host（实验、未编译） | `windows/`（C++ 窗口/消息循环）、`src/native/`（surface + 事件 + `cgb_host_*` C ABI）、`docs/architecture/windows-host-plan.md` |
| 窗口/手柄 host 抽象 | `src/host.rs`（`HostWindow` / `GamepadSource`）、`src/app/mod.rs`（`App::init` 取源） |
| 自研 FC/NES 核心 C++ 源码（历史对照 / `custom_nes_core` 来源） | `custom_nes_core/`（只读；`src/` 布局单 CMake 项目） |

## 已知缺口（先记录，不擅自补）

- **Nestopia 的 Blargg NTSC filter 跨核崩溃**：同一个进程里先跑过 Mesen（至少一帧）、
  再加载 Nestopia 并启用该 filter 时，Nestopia 的 `FilterNtsc` 会读到野指针而段错误。
  只发生在 Mesen 之后（`dlclose`、日志回调、后台线程都已排除）；其它核心（mGBA /
  Genesis / FBNeo / PicoDrive / custom_nes_core）之后都正常。已用
  `cores/cores.json` 的 `option_defaults` 把 `nestopia_blargg_ntsc_filter` 默认设成
  `disabled` 规避，Nestopia 因而输出 256×224。根因在第三方核心，未深挖。
- **J2ME / FreeJ2ME-Plus** 的 `retro_audio_sample_batch` 从不被核心调用：声音是 **Java 子进程
  自己**用 JavaSound 直接输出到 CoreAudio（能出声，但不进 `cgb-audio`、不随暂停静音、
  应用内音量无效）。要做统一控制需把 PCM 经管道转给 libretro；当前按“保持现状”收尾。
  `retro_serialize` 返回 false（**无即时存档/倒带**，`Session` 已按空串拒绝）；
  核心用 `RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK` 收键盘、用 `MOUSE`/`POINTER`
  收触摸指针，宿主**均未接**（joypad 可用，手机键盘已映射到 16 键）。宿主的
  `GET_RUMBLE_INTERFACE` 只给一个 no-op（不真震动），仅为避免核心空指针崩溃。
- **原生 host 统一（`src/native/`）**：macOS（Swift）与 Windows（C++）两个壳共用一份
  原生 host + `cgb_host_*` C ABI。平台差异只有两处：`surface::create_surface`
  （mac `CoreAnimationLayer` 分支 `#[cfg(target_os="macos")]`；win 用跨平台的
  `raw-window-handle`，刻意不 gate，从而被 macOS 的 gate 类型检查）和
  `input::key_from_code`（AppKit keyCode / Win32 VK 两张表，两张都在所有平台编译 + 单测）。
  壳本身（AppKit/ Swift、Win32/ C++）仍必然分开。
- **Windows（C++/Win32）host**：`windows/`（C++ 壳）+ `src/native/`（`cgb_host_*`
  C ABI）对标 Swift/macOS host。C++ 侧可用 `windows/scripts/cross-build-mingw.sh`
  从 macOS 交叉编译出自包含 `.exe`，**已在一台 Parallels Win11 VM 上跑通窗口 +
  反复 resize**。实测两个 Windows 专属问题：DX12 `ResizeBuffers` 因 backend 保留
  上一帧 surface view 而失败（**已修在 igui 上游 `v0.3.1`**，cgb 已 bump），以及
  wgpu 校验错误在 `extern "C"` 边界 panic 导致 `__fastfail`（host 改为
  `on_uncaptured_error` 打印）。
  尚未验证：MSVC 构建、键鼠/IME/拖放/全屏/XInput、强制 WARP、硬件 GL 核心的 WGL。
  见 `docs/architecture/windows-host-plan.md`。
- igui 仍没有**标准 Image 内容类型**（上游 Stage 33 删了 `Widget`，改用
  `ControlContent`）：`src/ui/frame.rs` 的 `FrameImage` 仍用 `Component` +
  `Spec::foreground` 自绘 `DrawImage`。上游若有 image 内容类型，可考虑替换本地组件。
- **中文输入法（IME）** 已接：改名 / 搜索 / 标签用 `igui_components::TextInput`
  （自带 caret / 选区 / IME 预编辑），窗口由 `igui_winit::ImePlugin` 驱动，候选窗按
  `AppLogic::caret`（`focused_caret`）定位；但还未经人眼验证 CJK 组合。
- **帧循环**改由 `igui_app` 运行时驱动。`AppLogic::needs_frame` 在跑游戏 / 带动画的
  overlay / 倒带时为真；帧速由 wgpu `PresentMode::Fifo`(vsync) 限速，`Session::advance(dt)`
  的时间累积保证模拟速度。若要精确按核心帧率 `WaitUntil` 节流，参考 `../archiver` 的
  本地 `EventLoop<UserEvent>` runner（`src/host/runner.rs`）。
