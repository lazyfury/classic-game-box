# Classic Game Box

macOS 上的经典游戏机模拟器。**打开就能玩**：把 ROM 拖进窗口，接上手柄或者用键盘。

> **迁移进行中。** 这个仓库正在从 Electron + WebAssembly + 自研 FC 核心，重写成
> **原生 Rust（[igui](https://github.com/lazyfury/igui)）+ 标准 libretro 核心**。
> 自研 FC 核心的 C++ 源码在 [`custom_nes_core/`](custom_nes_core/)（单个现代
> CMake 项目），Electron / wasm / tools 前端已删除。
>
> 权威设计：[`docs/architecture/quill-native-migration.md`](docs/architecture/quill-native-migration.md)。

## 目标形态

| | |
|---|---|
| 🎨 **界面** | Rust + [igui](https://github.com/lazyfury/igui)（应用内 `src/ui`），原生窗口，wgpu 上屏 |
| 🧩 **核心** | 标准 libretro：**Mesen**（NES）、**mGBA**（GB / GBC / GBA） |
| 🔌 **兼容层** | `crates/cgb-libretro` 直接 `dlopen` 原生 `.dylib`，实现 libretro frontend |
| 🔊 **音频** | `src/audio`：cpal 输出 + 无锁环形队列 |
| 🕹 **输入** | 键盘 + Swift `GameController` 手柄（快照在 `cgb-libretro` 的 `input.rs`） |
| 💾 **库与存档** | `src/library`：SQLite 游戏库、设置、存档槽、`.srm` |

**不做**：自研模拟器（它的源码在 `custom_nes_core/`）。本仓库只做 **UI 和 libretro 兼容**。

## 从源码运行

需要：macOS（Apple Silicon）、Rust stable、Xcode 命令行工具。UI 栈 igui 以
GitHub **git 依赖**引入（`Cargo.lock` 固定 commit），无需相邻 checkout；首次构建需要联网。

```bash
# 1. 构建原生 libretro 核心（第三方项目，首次要联网、几分钟）
#    Q1 只构建 Mesen（NES）；GB/GBA 推迟到 Q4，加 --with-mgba 才构建
./scripts/build-cores.sh
#    → cores/dist/mesen_libretro.dylib

# 2. 构建并运行（Rust 侧是库，Swift 才是入口）
cargo build                                   # → target/debug/libcgb_app.a
macos/scripts/run.sh                          # 打开库界面
macos/scripts/run.sh /path/to/mario.nes       # 直接开始
```

每个阶段的门槛：

```bash
./scripts/dev.sh    # cargo fmt --check + clippy -D warnings + cargo test
```

> 仓库里**不包含任何 ROM**（版权且体积大）。请使用你合法拥有的游戏文件。

## 路线

| 阶段 | 内容 | 状态 |
|---|---|---|
| Q0 | 计划、目录结构、Rust 工作区骨架 | ✅ |
| Q1 | Mesen arm64 原生编译 + dlopen + 出画面 + 键盘 | 进行中 |
| Q2 | 音频 + Swift `GameController` 手柄 + 存档槽 + `.srm` | |
| Q3 | 最小闭环 UI + 库 + 打开目录对话框 | |
| Q4 | mGBA 接入 + 机种路由 | |
| Q5 | 打包 `.app`、无头自检 | |

## macOS host（主要产品）

前端在 `macos/`：Swift 只做窗口与原生事件（`CAMetalLayer` + AppKit），igui UI、
wgpu 渲染与 libretro 模拟器全部留在 Rust（`src/native/` 的统一 host，`cgb_host_*` C ABI；
产出 `libcgb_app.a` 供 SwiftPM 静态链接）。手柄走 Swift `GameController`。

```bash
macos/scripts/run.sh          # 开库界面
macos/scripts/package.sh      # → dist/Classic Game Box (Swift).app
```

计划与剩余缺口见 [`docs/architecture/swift-macos-host-plan.md`](docs/architecture/swift-macos-host-plan.md)。

## 自研 FC 核心（custom_nes_core）

`custom_nes_core/` 是一个独立的现代 CMake 项目（`src/` 布局）：机器本体在
`src/core/`，libretro 包装在 `src/libretro/`；`./cores/custom_nes_core/build.sh`
把它构建成 `cores/dist/custom_nes_core_libretro.dylib` 供产品加载，同时保留
作对照与兼容性测试基准（ctest 可跑）。旧的 Electron + TypeScript 前端、
Emscripten wasm 构建与教学 tools 已删除。迁移的调研与踩坑见
[`docs/architecture/libretro-migration.md`](docs/architecture/libretro-migration.md)。
