---
name: fc-frontend
description: Electron + TypeScript 前端 electron/：主进程（窗口、app:// 协议、SQLite 游戏库、IPC）、preload、渲染进程（Canvas 显示、Web Audio、键盘/手柄输入、回退、金手指面板）、原生手柄助手（macOS Swift + GameController / Windows C++ + XInput），以及 selftest/keytest/audiotest/verify.sh 这些可验证的无头检查。用于前端界面、输入、音频队列、库管理、设置面板的任何改动。
---

# fc-frontend —— 前端（TypeScript + Electron）

上游：`wasm/dist/*`（由 `wasm/build.sh` 产出）、`packages/fc-core/src/ffi/emulator_api.h`。
前端里**没有**模拟逻辑；它只驱动 Core 并显示结果。

## 0. 上下文纪律

**白名单**（按任务挑，不要整个目录一起读）：

```
electron/package.json          脚本清单（命令的真正来源）
electron/README.md             794 行 —— 先 rg '^#{1,3} ' 拿目录，只读相关节
electron/src/main/index.ts     窗口 / 协议 / IPC / --selftest 等钩子
electron/src/main/library.ts   SQLite 库模型（不含 Electron，纯 Node 可测）
electron/src/main/gamepad.ts   手柄助手子进程：spawn、解析 JSON Lines、去重
electron/src/preload/index.ts  暴露给页面的函数
electron/src/renderer/useEmulator.ts   帧循环、状态
electron/src/renderer/systems.ts       按扩展名选核（自研 / mGBA / Mesen）
electron/src/renderer/input.ts         键盘 + 手柄取 OR
electron/src/renderer/audio/output.ts  音频队列接线
electron/src/renderer/{rewind,cheats,bindings,usePixelScale}.ts
electron/src/renderer/components/*.tsx   面板（改哪块读哪块）
electron/src/shared/{api,boot}.ts
electron/test/*.test.mjs       改哪个模块就只读对应测试
electron/native/gamepad/**     macOS 助手（Swift）
electron/native/gamepad-cpp/** Windows 助手（C++）
```

**禁读**：`electron/node_modules/`、`electron/dist/`、`electron/release/`、
`electron/build/`、`electron/native/bin/`、`electron/pnpm-lock.yaml`、
`*.png` 资源。

**单个文件超过 400 行时**（`index.ts`、`useEmulator.ts`、`App.tsx`、`README.md`）：
先 `rg -n '^export |^function |^const |^#{1,3} ' <file>` 定位，再 `read offset/limit`。

## 1. 目录

```
src/main/       主进程：窗口、app:// 协议、碰文件系统的 IPC、无头钩子
src/preload/    页面被允许调用的函数（contextBridge）
src/renderer/   UI：Canvas 显示、Web Audio、输入、库/设置/金手指面板、回退
src/shared/     两端共用的类型与启动参数解析
native/         手柄助手（独立进程，JSON Lines 协议）
test/           node:test，跑在纯 Node 上
verify.sh       逐像素/逐采样对比原生构建与前端
```

## 2. 命令（先看 `package.json` 确认，不要凭记忆）

| 命令 | 作用 |
|---|---|
| `./wasm/build.sh` | 先构建模拟器（`src/` 改过就要重跑） |
| `pnpm install && pnpm run dev` | Vite + Electron 开发模式 |
| `pnpm run build` | 主进程 `tsc` + 渲染进程 `vite build` + 原生助手 |
| `pnpm start` | 构建后跑生产版 |
| `pnpm run typecheck` | 两端类型检查，不产出 |
| `pnpm test` | `node --test test/` |
| `pnpm run test:native` | 手柄助手自己的测试 |
| `pnpm run selftest` | 无头跑 300 帧，打印画面哈希 + `selftest.png` |
| `pnpm run keytest` | 对窗口发真按键，检查模拟器听到了什么 |
| `pnpm run audiotest` | 实时播 8 秒，报告音频环形队列 |
| `./verify.sh [rom-dir]` | 每个 ROM 走原生与前端两条路，逐像素/逐采样比对 |

全仓库一条命令构建 + 测试（含前端的 111 个测试）：`./scripts/build-all.sh`。

无头模式是**唯一的自动化验证手段**（画面无法截图比对之外的断言），
改渲染/输入/音频后至少要跑 `selftest` + `keytest`（+ `audiotest`）。

## 3. 任务菜谱

**改画面尺寸/缩放**：`renderer/usePixelScale.ts` + 相关 CSS。
宽度/height 来自 Core 的 AV info；不要在 `electron/` 里改像素。

**加一个设置项**：`shared/api.ts`（类型）→ `main/index.ts`（持久化/IPC）
→ `renderer/components/SettingsPanel.tsx` → `test/` 加断言。

**改音频**：环形队列在 C 侧（`fc_audio_queue_*`），`renderer/audio/output.ts`
只做接线。**音频回调里不许加锁**（实时性违规）——历史上就是这个问题。

**改手柄**：两个 source（浏览器 Gamepad API / 原生助手）各自记状态、取 OR，
不要互相覆盖；见 `renderer/gamepad.ts` 与 `main/gamepad.ts`。

**改库**：`main/library.ts` 有 SQLite schema 与扫描-对账逻辑，
测试在 `test/library.test.mjs`（纯 Node，不需要 Electron）。

## 4. 检索菜谱

```bash
rg -n '"scripts"' -A 30 electron/package.json
rg -n '^#{1,3} ' electron/README.md
rg -n 'systems|byExtension|mgba|mesen' electron/src/renderer/systems.ts
rg -n 'selftest|keytest|audiotest|--headless|--dump' electron/src/main/index.ts
rg -n 'contextBridge' electron/src/preload/index.ts
```

## 5. 陷阱

- **别把模拟逻辑写进前端**。它属于 `packages/fc-core`。
- **pitch**：帧的行字节数来自 Core，不要假设 `width * 4`。
- **heap 视图缓存**：wasm 固定 64MB heap（不增长），所以可以安全缓存 typed array，
  但**加内存增长选项会让这个前提失效**。
- **`ERR_PNPM_IGNORED_BUILDS`** 的答案在 `electron/pnpm-workspace.yaml`，
  不要在 `node_modules` 里找。
