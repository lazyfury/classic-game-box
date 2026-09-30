# Classic Game Box

macOS 上的经典游戏机模拟器。**打开就能玩**：把 ROM 拖进窗口，接上手柄或者用键盘。

> **迁移进行中。** 这个仓库正在从 Electron + WebAssembly + 自研 FC 核心，重写成
> **原生 Rust（[igui](https://github.com/lazyfury/igui) UI + winit/wgpu）+ 标准 libretro 核心**。
> 旧栈完整保留在 [`legacy/`](legacy/)，可继续构建与参考，但不再演进。
>
> 权威设计：[`docs/architecture/quill-native-migration.md`](docs/architecture/quill-native-migration.md)。

## 目标形态

| | |
|---|---|
| 🎨 **界面** | Rust + [igui](https://github.com/lazyfury/igui)（应用内 `src/ui`），原生窗口，wgpu 上屏 |
| 🧩 **核心** | 标准 libretro：**Mesen**（NES）、**mGBA**（GB / GBC / GBA） |
| 🔌 **兼容层** | `crates/cgb-libretro` 直接 `dlopen` 原生 `.dylib`，实现 libretro frontend |
| 🔊 **音频** | `crates/cgb-audio`：cpal 输出 + 无锁环形队列 |
| 🕹 **输入** | 键盘 + `gilrs` 手柄（`crates/cgb-input`） |
| 💾 **库与存档** | `crates/cgb-library`：SQLite 游戏库、设置、存档槽、`.srm` |

**不做**：自研模拟器（已在 `legacy/`）。本仓库只做 **UI 和 libretro 兼容**。

## 从源码运行

需要：macOS（Apple Silicon）、Rust stable、Xcode 命令行工具。UI 栈 igui 以
GitHub **git 依赖**引入（`Cargo.lock` 固定 commit），无需相邻 checkout；首次构建需要联网。

```bash
# 1. 构建原生 libretro 核心（第三方项目，首次要联网、几分钟）
#    Q1 只构建 Mesen（NES）；GB/GBA 推迟到 Q4，加 --with-mgba 才构建
./scripts/build-cores.sh
#    → cores/dist/mesen_libretro.dylib

# 2. 构建并运行（根包即应用，`cargo run` 即可）
cargo run                                     # 打开库界面
cargo run -- --rom mario.nes                  # 直接开始
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
| Q2 | 音频 + gilrs 手柄 + 存档槽 + `.srm` | |
| Q3 | 最小闭环 UI + 库 + 打开目录对话框 | |
| Q4 | mGBA 接入 + 机种路由 | |
| Q5 | 打包 `.app`、无头自检 | |

## 旧版（Electron）

`legacy/` 里是迁移前的完整实现：Electron + TypeScript 前端、自研 C++ FC 核心
（`packages/fc-core`）、libretro 适配（`packages/fc-libretro`）、Emscripten
wasm 构建（`legacy/wasm`），以及教学文档。它记录了大量 libretro 迁移的调研与
踩坑（[`legacy/docs/architecture/libretro-migration.md`](legacy/docs/architecture/libretro-migration.md)），
是本项目现在这套设计的重要依据。
