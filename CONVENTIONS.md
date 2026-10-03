# CONVENTIONS — 模块契约与校验

本文件是**模块职责边界**的单一事实来源。它只回答一个问题：**哪个模块能依赖谁，
以及这条边界怎么被机器校验。**

- 原则、构建/发版/工作流程、路线图 → [`AGENTS.md`](AGENTS.md)（不要在本文件重复）
- 每个 crate / 目录的职责导览 → [`crates/README.md`](crates/README.md)
- 校验入口 → [`scripts/check-boundaries.sh`](scripts/check-boundaries.sh)，
  已并入每阶段 gate [`scripts/dev.sh`](scripts/dev.sh)

> 规则要活下来，必须**可判定**。能 grep / 能测试的写成 `[B*]` 规则；只能靠人看的，
> 明确列在「人工评审」里，不要假装它是硬约束。

## 一、规范分层（为什么不是一个大文件）

| 层 | 内容 | 变化频率 | 写在哪 |
|---|---|---|---|
| 不变量 / 原则 | 依赖单向、libretro 是唯一契约、核心不碰 UI | 几乎不变 | `AGENTS.md`「硬规则」 |
| 模块职责 / 边界 | 谁能依赖谁、每个模块禁止做什么 | 随模块演进 | **本文件** |
| 决策记录（ADR） | 「为什么当初这么定」、被否决的方案 | 只追加，不改写 | `docs/architecture/decisions/` |
| 操作流程 | 构建 / 验证 / 发版 / 评审的固定动作 | 偶尔 | `scripts/dev.sh`、`AGENTS.md` |

## 二、依赖方向

允许的边（`→` 读作「可以依赖」）：

```
src/native → {app, cli, host}
src/app    → {cli, cores, host, library, paths, session, ui}
src/session→ {audio, library, paths, ui} + cgb-libretro（前端）
src/library→ {paths} + cgb-libretro
src/ui     → {host, inspect} + igui_* 公开 API + cgb-libretro（纯域类型）
cgb-libretro → {libloading, thiserror}          ← 仅此，正常依赖
```

关键点：

- **`cgb-libretro` 是唯一认识 libretro 的包**：它只暴露纯值（`Frame`、`Vec<i16>`）
  和纯域类型（`SystemId`、`SYSTEMS`、`InputState`、`KeyboardMode`…），不认识 UI、
  音频设备与游戏库。
- **`src/ui` 只认识 libretro 的纯域类型，不认识它的前端**（`CoreHost` / loader / gl）。
  这与 `crates/README.md` 的说法一致，也满足 `AGENTS.md` 硬规则 4 的精神。
- **只有 `src/native` 直接碰 `wgpu::` / Metal / `objc2`**；其余模块通过 igui 与
  `src/host` 的契约（`HostWindow` / `GamepadSource`）与平台解耦。
- 成员包对根包 `cgb-app` 的依赖**只能出现在 `[dev-dependencies]`**（测试用）。

## 三、模块职责矩阵

| 模块 | 必须做 | 不得做 |
|---|---|---|
| `cgb-libretro` | 加载核心、注册回调、GL 离屏路径；导出纯域类型 | 依赖 UI / 音频设备 / 游戏库 |
| `src/ui` | 消费纯 `ViewModel` 画界面；只用 `igui_*` 公开 API | 依赖 app 状态 / 设备 / 库 / 实现；用 libretro 前端或 GPU 后端 |
| `src/app` | 应用状态、帧循环、把功能处理器接起来 | 直接 `dlopen`（归 session / cgb-libretro）；直接碰 `wgpu::` |
| `src/session` | 一个运行中的游戏：`CoreHost` + 音频 + 存读档 | 见 `[B1]`；它只从 `src/ui` 取 `TextureHandle` |
| `src/native` | 两个壳共用的宿主层：surface / 事件 / `cgb_host_*` C ABI | 被 `src/ui` 依赖 |
| `src/library` | SQLite 模型（模型即数据）、导入、截图、存档、金手指 | 依赖 UI 渲染 |
| `src/cores` | `cores.json` 清单、buildbot catalog、运行时下载 | 依赖 UI |
| `src/paths` | 文件布局（`Paths`）与 `Settings` | 依赖 UI |
| `src/audio` | cpal 输出 + SPSC 环形缓冲 | 依赖 UI |
| `src/inspect` | 纯字节 → 图像解码；可无核心 / 无 GPU 测试 | 做 I/O、推进机器、依赖核心 |
| `custom_nes_core/` | 自研 FC 核心（只读，产品入口 `cores/custom_nes_core/build.sh`） | 被 Rust 构建直接引用 |

## 四、边界规则（可判定）

每条规则都有 `scripts/check-boundaries.sh` 里的同名检查，编号一一对应。

- **[B1] `src/ui` 不认识设备 / 状态 / 实现**：代码（非注释）里不得出现
  `crate::{app,audio,cores,library,native,session}`。允许 `crate::host`（纯契约）
  与 `crate::inspect`（纯解码）。
  *为什么*：UI 只从 ViewModel 渲染，才能无头测试、换壳不重写。
- **[B2] `src/ui` 不用 libretro 前端 / GPU 后端**：不得用 `CoreHost` / `Frame` /
  `CoreLibrary` / `MemoryRegion` 等前端符号，也不得用 `igui_backend_wgpu` 或 `wgpu::`。
  *为什么*：front-end 与 GPU 后端属于宿主 / 应用层，UI 只画。
- **[B3] `cgb-libretro` 的正常依赖停在 `libloading` + `thiserror`**。
  *为什么*：成员包不认识 app 形态的东西，才能被任何宿主复用。
- **[B4] 成员包不得在正常依赖里依赖根包**：`crates/*/Cargo.toml` 的
  `[dependencies]` 不得含 `cgb-app`（`[dev-dependencies]` 允许）。
  *为什么*：依赖方向单向，避免环。
- **[B5] 只有 `src/native` 用 `wgpu::` / `objc2` / `metal::`**。
  *为什么*：平台 / GPU 细节收在一处，UI 与应用保持可移植、可测试。
- **[B6] 不提交构建产物**：`target/`、`dist/`、`cores/sources/`、`*.dylib`、`*.o`
  不进 git（`.gitignore` 已覆盖，这里是兜底）。
  *为什么*：仓库只放源码与脚本，产物按需重建。

检查忽略「注释里的提及」：文档注释**命名**一条边不算依赖。

## 五、只能人工评审的

以下无法用 grep 判定，评审时按此把关（**不要**把它们写成 `[B*]` 规则）：

- 不写截图 / 录屏测试（用 `igui_backend_recording` + `igui_profile::inspect`，
  或 core 侧假 frontend 单测）。
- UI 好不好看、交互是否顺手——由人看。
- 新依赖是否必要、是否引入 app 形态的耦合——看 `Cargo.toml` diff。
- `custom_nes_core/` 源码只读，只在与本仓库 libretro 契约对照时改动。

## 六、改动边界

公共 API、依赖方向、路线图要变时：

1. 先在 `docs/architecture/decisions/` 加一条决策记录（为什么、被否决的方案、代价）；
2. 更新本文件的矩阵与规则；
3. 若规则可判定，同步改 `scripts/check-boundaries.sh` 的对应检查。

顺序不能反：先有理由，再改契约，最后让机器守住它。
