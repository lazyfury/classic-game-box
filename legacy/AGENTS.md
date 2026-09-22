# Classic Game Box Project — Agent Instructions

## Project Name

Classic Game Box for macOS

---

# 1. Project Vision

本项目目标：从零实现一个 macOS 平台上的 FC / NES 模拟器。

但目标**不仅是完成模拟器**。本项目同时是一个**计算机科学学习工程**。

实现过程中必须理解：

- 二进制表示
- 数字逻辑
- CPU 工作原理
- 汇编语言
- 指令执行
- 内存系统
- 总线结构
- 图形渲染
- 音频系统
- 操作系统图形接口

最终目标：能够解释

```
一个 NES 游戏 ROM 如何变成屏幕上的像素
```

完整链路：

```
ROM
 ↓
Machine Code
 ↓
Opcode
 ↓
CPU Execute
 ↓
Memory Access
 ↓
Bus
 ↓
PPU
 ↓
Framebuffer
 ↓
GPU
 ↓
Screen
```

---

# 2. Agent 工作原则

## 2.1 教学优先

每一次代码修改必须包含：

1. 本次目标
2. 相关计算机科学知识
3. NES 原理解释
4. 设计方案
5. 代码实现
6. 测试
7. 测试结果解释
8. 下一阶段目标

禁止：

```
直接输出大量代码
然后告诉用户运行
```

## 2.2 分阶段开发

必须按照：

```
理解 → 设计 → 实现 → 测试 → 总结 → 下一阶段
```

执行。禁止跳跃，例如：

错误：

```
先写 PPU
再补 CPU
```

正确：

```
Binary
 ↓
CPU
 ↓
Assembly
 ↓
Memory
 ↓
Bus
 ↓
Cartridge
 ↓
PPU
 ↓
APU
 ↓
Frontend
```

## 2.3 项目名称

一个仓库里有三个名字，别把它们混起来：

| 名字 | 用在哪 | 例子 |
|---|---|---|
| `Classic Game Box` | 给玩家看的显示名 | 窗口标题、`productName`、dmg/app 名、libretro 的 `library_name` |
| `ClassicGameBox` | 必须是单个标识符的地方 | `project()`、编辑器配置 |
| `classic-game-box` | 目录名、npm 包名、临时文件名 | `package.json` 的 `name` |

`scripts/release.sh` 不再自己拼这些字符串：它从 `electron/package.json` 里读出
`build.productName`，用它拼出 `TITLE`、产物名和安装说明。所以**改显示名只要改
`package.json` 一处**，发版脚本跟着走。

## 2.4 上下文纪律（Skills）

仓库里每个顶层部件都有一份指南，放在 `.pi/skills/<名字>/SKILL.md`：

| 技能 | 覆盖 |
|---|---|
| `fc-repo` | monorepo 总览、路由表、构建/版本/发版、Git 约定 |
| `fc-core` | `packages/fc-core`：CPU / Bus / Cartridge / Mapper / PPU / APU / State / FFI |
| `fc-libretro` | `packages/fc-libretro`：`retro_*` 适配层、金手指、custom 扩展 |
| `fc-wasm` | `wasm/`：Emscripten 构建、JS 绑定、侧模块 |
| `fc-frontend` | `electron/`：主进程 / preload / 渲染进程 / 手柄助手 |
| `fc-tools` | `tools/`：教学 demo 与命令行工具 |
| `fc-docs` | `docs/` 与教学契约 |

**动手之前先读技能，再读源码。** 每个技能的开头都有一条硬规则：

```
先定位，再精读。一次只打开一个包。
```

具体地：

- **禁止**读 `build*/`、`wasm/dist/`、`electron/node_modules`、`electron/dist`、
  `electron/release`、`.git/` —— 那些是产物，不是源码。
- **禁止**整读 `packages/fc-libretro/third_party/libretro/libretro.h`（8716 行）
  或 700+ 行的 `README.md` / `AGENTS.md`。
- 要读大文件时，先 `rg -n '符号' <文件>` 拿行号，再 `read offset/limit`
  只读那一段。
- 一轮对话默认最多打开 3 个文件；要更多先说明理由。

「读整个项目」既慢又会把真正相关的那 20 行埋掉。

## 2.5 验证：不写复杂的 UI 测试，不截图

**前端的改动不需要 agent 自己验证。** 跑这三样，然后把改动交给用户看：

```bash
cd electron
pnpm run typecheck    # 两端类型检查，不产出
pnpm test             # test/*.mjs，纯 Node，不需要 Electron
pnpm run build        # 编译过就算通过
```

**禁止**为了证明一个界面改对了而去做这些事：

- 搭 DOM 检查：驱动点击、派发键盘事件、量几何尺寸、读 computed style、
  数规则个数（`--list` 之类）；
- 截图对比，或写任何“看一眼这个区域对不对”的自动化；
- 为了测试去搭临时 Electron 脚本、临时调试输出、临时的「先改回去量一组数再改回来」。

理由不是懒，是这些东西**代价高、收益低、而且它自己会坏**：

- 它们验证的是 agent 猜的那个布局，而 agent 猜错的时候，测试会一起错，
  却仍然报「通过」—— 上一轮就发生过：检查脚本自己的语法错误
  把窗口弄崩了，进程带着 exit 0 退出，什么都没验；
- 写它们花的时间比写功能本身多，而真正的验证（用户看一眼）只要两秒；
- UI 是审美，能表达它的不是断言，是用户说「这个可以」。

真正的验证只保留两类：

| 验什么 | 怎么验 |
|---|---|
| 逻辑（比较器、排序、路径守卫、迁移） | `test/*.mjs`，纯 Node，不改动 UI |
| 整体能不能跑起来 | `pnpm run build`（编译过）+ 用户自己测 |

需要看图的时候，是**请用户看**，不是自己量：说清楚看到的该是什么，
用户回一句就行。

## 2.6 不确定就问，不要猜着往下做

**需求模糊时停下来问，不要替用户选一个然后实现。** 这个仓库里反复出现的
歧义都是同一类：

- “功能区”指哪一块？（侧边栏？工具条？）
- “小一点”是小多少？（padding？字号？列数？）
- 弹窗要不要模态？要不要记住筛选？删掉的 section 还要不要留数据？

做法：

- 一次把选项列出来，让用户选（「A：只改 padding；B：顺手缩字号」）；
- 改之前先问，不要先做完再问 —— 猜错的代价是用户花时间看你改错的东西；
- 如果确实需要先做点什么才能问得清楚，只做**最小的一步**，然后停下来。

少数情况下可以自己做决定：改动很小且可逆（一行 CSS）、或者用户已经说
“你先直接做”。就算那样，也要在回复里**一句话说明你选了什么、备选是什么**。

---

# 3. 技术架构

## Monorepo 结构

仓库是一个 monorepo。每个 `packages/*` 都是一个能单独配置、单独构建、
单独测试的 CMake 项目；根 `CMakeLists.txt` 只负责组装，不含任何模拟逻辑。

```
classic-game-box/
├── packages/fc-core/       自定义 FC / NES 核心（fc_core, fc_ffi）
│   ├── src/core/           纯 C++ 机器，禁止依赖 UI
│   ├── src/ffi/            emulator_api.h 纯 C 接口
│   └── tests/
├── packages/fc-libretro/   libretro 包装（fc_libretro）—— 独立项目
│   ├── src/libretro/       retro_* 适配层 + custom 扩展 + 金手指解码
│   ├── third_party/libretro/libretro.h
│   └── tests/
├── cmake/Version.cmake     C++ 端版本号（release.sh 与 package.json 同步）；
│                           GoogleTest.cmake 供两个包复用
├── wasm/  tools/  electron/  消费者
└── docs/
```

依赖方向只有一条，且不允许反向：

```
wasm / tools / electron  ->  packages/fc-libretro  ->  packages/fc-core
```

单独构建某个包：

```bash
cmake -S packages/fc-core     -B build-core     -G Ninja
cmake -S packages/fc-libretro -B build-libretro -G Ninja
```

## Core

语言：`C++20`

负责：CPU / Bus / Memory / Cartridge / Mapper / PPU / APU / Controller

位于 `packages/fc-core/src/core`。它对 UI / 文件 IO / 线程 / 异常无依赖，
所以同一份源码能原样编译成原生库和 WebAssembly。

## Electron 前端

技术：`TypeScript` `Electron` `WebAssembly` `Canvas` `Web Audio`

负责：Window / Input / Rendering / Audio output

核心禁止依赖 UI。

```
                    macOS
                      |
                Electron 窗口
                      |
          Canvas (2D) / Web Audio
                      |
                Emulator API
                      |
+-------------------------------------------+
|                  NES Core                 |
|                                           |
|   CPU ---- Bus ---- PPU                   |
|    |        |        |                    |
|   APU    Cartridge  VRAM                  |
|    |                                      |
|  Controller                               |
+-------------------------------------------+
```

同一份 Core 也编译成 WebAssembly（`wasm/`），跑在 Electron 的渲染进程里。
原生手柄助手（`electron/native/gamepad`，Swift + GameController）是唯一的
非 TypeScript 部件，它只负责读手柄。

---

# 4. 开发阶段总览

## Phase 0 — 工程基础

目标：建立 CMake / C++ / Test Framework
学习：编译、链接、项目结构

## Phase 0.1 — 二进制基础

学习：bit / byte / binary / hexadecimal / bit operation / signed number / two's complement

## Phase 0.2 — CPU 基础

学习：register / ALU / instruction / opcode / operand / PC / fetch-decode-execute

## Phase 0.3 — 6502 Assembly

学习：mnemonic / machine code / assembler / disassembler

例如：

```
Assembly:  LDA #$42
Machine:   A9 42
```

## Phase 0.4 — Opcode / Addressing Mode

```
Immediate / Zero Page / Absolute / Indexed / Indirect / Relative
```

## Phase 1 — 6502 CPU

实现 NES CPU。包含：Registers / Flags / Instruction / Stack / Interrupt / Timing

## Phase 2 — NES Bus

```
CPU
 |
Bus
 |
RAM / PPU / APU / Controller / Cartridge
```

学习：Address Bus / Data Bus / Memory Mapping / IO Mapping

## Phase 3 — Cartridge

实现：iNES parser / ROM loading / Mapper system。第一目标：Mapper 0

## Phase 4 — PPU

学习：Pixel / Tile / Sprite / VRAM / Rasterization。实现 256×240 framebuffer

## Phase 5 — Controller

实现 NES controller protocol

## Phase 6 — APU

实现 Pulse / Triangle / Noise

## Phase 7 — Electron 前端

```
Framebuffer → WebAssembly 线性内存 → Canvas → GPU → 屏幕
```

---

# 5. Computer Science Knowledge Requirements

Agent 在进入下一阶段前必须确认用户理解：

## Binary

解释：为什么计算机使用 `0` `1`；什么是 bit / byte / word

## Hexadecimal

必须解释 `0x42` 表示 `01000010`

## Two's Complement

必须解释为什么 `0xFF` 可以表示 `255`，也可以表示 `-1`

## Overflow Flag (V)

必须解释 C 与 V 的区别：

```
C = 无符号溢出（bit 7 的进位出）
V = 有符号溢出（carry_into_bit7 XOR carry_out_of_bit7）
N = result 的 bit 7，不是判决
```

并必须解释：`A - M` 后的真实符号是 `N XOR V`。

见 `docs/computer-science/overflow-flag.md` 与 `packages/fc-core/src/core/alu.hpp`。

## CPU Concepts

### Register

CPU 内部高速存储，例如 `A` `X` `Y`

### Program Counter

PC 保存下一条指令地址。例如：

```
Memory:   8000 A9
          8001 42

CPU:      PC=8000  fetch A9
          PC=8001  fetch 42
          PC=8002
```

### Instruction Cycle

```
Fetch → Decode → Execute → Update State
```

## Assembly Rules

出现以下符号必须解释：`#` `$` `()` `,`

例如 `LDA #$42`：

```
LDA  load accumulator
#    immediate
$    hexadecimal
```

结果：`A = 0x42`

---

# 6. Coding Rules

## Architecture

禁止 CPU 直接访问 PPU。

错误：

```
CPU
memory[]
PPU
```

正确：

```
CPU
 |
Bus
 |
PPU
```

## Testing

所有核心模块必须测试。测试等级：

```
Unit Test → Instruction Test → Timing Test → Integration Test
```

**这只管核心。前端不写自动化测试，也不截图** —— 见 §2.5。

---

# 7. Current Implementation Status

当前：**Phase 7 完成 —— 项目完成**；仓库已重构为 monorepo（`packages/fc-core`
与 `packages/fc-libretro` 是两个独立项目，可各自单独构建）。

已完成：

- **monorepo 布局**：`packages/fc-core`（`fc_core` + `fc_ffi`）与
  `packages/fc-libretro`（`fc_libretro`）各自有 `project()`、版本号、测试，
  可单独 `cmake -S packages/<name> -B build-<name>`；根 `CMakeLists.txt`
  只做组装。版本号在两处：`cmake/Version.cmake`（C++ / libretro 的
  `library_version`）与 `electron/package.json`（npm / electron-builder）；
  `scripts/release.sh` 同时写这两个并检测它们是否已经不一致。
  libretro 的 `libretro.h` 也随之移入 `packages/fc-libretro/third_party/`。

- CMake + C++20 + Ninja
- GoogleTest 测试框架（370 个单元测试全通过）
- `docs/` 十三篇（computer-science 十章 + nes 两篇 + architecture 一篇）
- `packages/fc-core/src/core/bit.{hpp,cpp}` `types.hpp` `alu.hpp`
- `packages/fc-core/src/core/bus.hpp` 总线抽象（含 `take_stall_cycles()`）
- `packages/fc-core/src/core/cpu/` 全部 151 个 opcode、256 项周期表、反汇编器、13 种寻址
- `packages/fc-core/src/core/nes/device.hpp` Device / OamTarget 接口
- `packages/fc-core/src/core/nes/ram.hpp` 2KB RAM（掩码就是未接的地址线）
- `packages/fc-core/src/core/nes/bus.{hpp,cpp}` 地址译码、镜像、open bus、OAM DMA
- `packages/fc-core/src/core/nes/ines.{hpp,cpp}` iNES 文件头解析
- `packages/fc-core/src/core/nes/mapper.hpp` 映射器接口 + `mapper0.hpp` Mapper 0 (NROM)
- `packages/fc-core/src/core/nes/mapper1.hpp` Mapper 1 (MMC1)：串行移位寄存器、4/8KB CHR 分页、
  16/32KB PRG 分页、运行时可切换镜像（Zelda II、Tetris 用）
- `packages/fc-core/src/core/nes/mapper2.hpp` Mapper 2 (UxROM)：16KB PRG 分页、CHR RAM（洛克人）
- `packages/fc-core/src/core/nes/mapper3.hpp` Mapper 3 (CNROM)：8KB CHR 分页（越野摩托）
- `packages/fc-core/src/core/nes/mapper4.hpp` Mapper 4 (MMC3)：8KB PRG、1/2KB CHR、**扫描线 IRQ**
  （超级玛丽 3、星之卡比）；配套 `Mapper::on_ppu_address` / `irq_asserted` 钩子
- `packages/fc-core/src/core/nes/mapper7.hpp` Mapper 7 (AxROM)：32KB PRG、单屏镜像（大理石疯疯）
- `packages/fc-core/src/core/nes/mapper9.hpp` Mapper 9 (MMC2)：PPU 取 tile $FD/$FE 翻转 CHR latch
- `packages/fc-core/src/core/nes/mapper10.hpp` Mapper 10 (MMC4)：同 MMC2 的 latch，16KB PRG
- `packages/fc-core/src/core/nes/mapper11.hpp` Mapper 11 (Color Dreams)：8KB CHR 分页
- `packages/fc-core/src/core/nes/mapper13.hpp` Mapper 13 (CPROM)：自带 16KB CHR RAM，4KB 分页
- `packages/fc-core/src/core/nes/mapper15.hpp` Mapper 15 (100-in-1)：16KB PRG 可切换 + 顶部 16KB 固定、
  单屏镜像、8KB CHR RAM（`100合1.NES` 1MB 多合一卡实测能启动到菜单）
- `packages/fc-core/src/core/nes/mapper163.hpp` Mapper 163 (Nanjing FC-001)：32KB PRG 分页、
  寄存器在扩展区 $5000、防拷反馈位、自动 4KB CHR RAM 切换。
  （`金庸群侠传.nes` 2MB 实测能进标题并开始游戏）
- `packages/fc-core/src/core/nes/mapper226.hpp` Mapper 226 (76-in-1)：7 位 PRG bank 拆在
  $8000/$8001、32KB/16KB 两种模式、寄存器 bit6 选镜像。
- `packages/fc-core/src/core/nes/mapper18.hpp` Mapper 18 (SS88006)、`mapper21.hpp`
  Mapper 21/22/23/25 (VRC2/VRC4)：8KB PRG、1KB CHR、CPU 周期 IRQ
  （靠新增的 `clocks_on_cpu_cycles()` / `on_cpu_cycle()` 钩子）
- `packages/fc-core/src/core/nes/mapper32.hpp` (IREM)、`mapper33.hpp` (Taito)、
  `mapper66.hpp` (GxROM)、`mapper68.hpp` (Sunsoft-4)、`mapper71.hpp`
  (Codemasters)、`mapper78.hpp` / `mapper87.hpp` (Jaleco)
- `packages/fc-core/src/core/nes/mapper162/164/178/242.hpp` (Waixing)、`mapper190.hpp`、
  `mapper227.hpp` / `mapper246.hpp`（中文/多合一）
- 接口钩子共六个（全部默认空实现）：`on_ppu_address` / `irq_asserted` /
  `read_expansion` / `write_expansion` / `on_scanline` /
  `clocks_on_cpu_cycles` + `on_cpu_cycle` / `has_work_ram`
- `packages/fc-core/src/core/nes/cartridge.{hpp,cpp}` 真正的卡带
- `packages/fc-core/src/core/nes/ppu.{hpp,cpp}` PPU：8 个寄存器、VRAM、调色板、OAM、扫描线时序、背景/精灵渲染、sprite 0 hit
- `packages/fc-core/src/core/nes/machine.{hpp,cpp}` CPU 与 PPU 的 3:1 同步、NMI、脚本输入接口
- `packages/fc-core/src/core/nes/controller.hpp` 手柄串行协议，接在 `$4016`/`$4017`
- `packages/fc-core/src/core/nes/apu.{hpp,cpp}` 五个声道、包络、长度/线性计数器、扫频、帧序列器、非线性混音、DMC
- `packages/fc-core/src/core/nes/ram_cartridge.hpp` 卡带槽占位（测试用）
- `packages/fc-core/src/ffi/emulator_api.h` 纯 C 接口（22 个测试）
- `electron/` Electron + TypeScript 前端：Core 编译成 WebAssembly 在渲染进程里跑，
  canvas 出画面，Web Audio 出声，SQLite 游戏库，含可验证的无头模式
- `electron/src/renderer/{input,gamepad}.ts` 键盘与手柄汇入同一个 `InputManager`
  （两个 source 各自记状态、取 OR，互不覆盖）；浏览器手柄走 Gamepad API，
  原生手柄走独立进程助手：macOS 是 `electron/native/gamepad`（Swift +
  GameController，`GCExtendedGamepad` / `GCMicroGamepad`），Windows 是
  `electron/native/gamepad-cpp`（C++ + XInput）；两者写同一份 JSON Lines
  协议，自动识别已连接的手柄并处理插拔
- `wasm/` 同一份 Core 的 Emscripten 构建与 JS 绑定
- 11 个教学 demo；15 个测试文件

已修复：屏幕乱码 / 地面"空洞" / HUD 填充成一片 "0"

```
根因（PPU）：强制消隐期间渲染管线仍在动 v。

v 既是 CPU 通过 $2006/$2007 写 VRAM 的地址，也是渲染取 tile 的
指针，两者共用同一个 15 bit 寄存器。当 PPUMASK 把背景和精灵都
关掉（forced blanking）时，真机不再推进 v，CPU 可以把整屏数据
连续写进去；而我们之前在每条扫描线的 dot 256 无条件调用
increment_y()、dot 257 无条件调用 copy_x()、预渲染线无条件调用
copy_y()，于是 CPU 每写十几个字节就被管线把 v 拨走一次。

超级玛丽每关开始都用 forced blanking 清空 nametable（填 $24 空格）。
清屏循环在 $8E19，本应写 768 + 64 字节；由于 v 被拨走，实际只
写了大约一行就散掉了，没写到的地方保留上电值 $00 —— 而 tile $00
正是字库里的数字 "0"。这就是标题画面和 HUD 里成片的 "0"、地面
固定列的空洞、以及按下 Start 后残留图形的来源。

修复：render_dot() 中 increment_y / copy_x / copy_y / 精灵评估全部
用 rendering_enabled() 包住。render_pixel() 仍在运行，所以消隐时
屏幕正确显示 $3F00 背景色。

回归测试：packages/fc-core/tests/test_ppu.cpp
  Ppu.ABlockWriteDuringForcedBlankingLandsWhereItWasPointed
复现方式：F12 截图（shot_0001.ppm 等）与 --headless --dump 完全一致，
说明是 Core 而不是 Metal。349 → 350 个测试全通过。
```

已修复：窗口缩放 / 全屏时的黑角与斜向拉伸

```
根因（Metal 渲染器）：缩放改的是顶点而不是纹理坐标。

全屏用一个 oversize 三角形覆盖屏幕，它只是刚好盖满 [-1,1]^2。
之前 updateScale() 把三角形顶点乘上 s<1 来留黑边，等于把三角形的
直角边往里拉，右上角 (1,1) 就露了出来（黑三角），而可见区域沿被
切掉的斜边被非均匀拉伸（全屏右侧的斜向拉伸）。

修复：顶点不动，改成缩放纹理坐标 uvScale = 1/s（围绕 0.5 缩放），
片元里 uv 超出 [0,1] 就输出黑色，黑边由片元负责而不是几何负责。
```

已修复：背景音的"呲"/"沙沙"声

```
三个原因，最后一个是主因：

1. Core APU：从 894886 Hz 降采样到 44100 Hz 用的是点采样，没有抗
   混叠。现在每个输出样本对约 20 个 APU 周期取平均（box 低通）。
2. Frontend：音频回调里的 NSLock 是实时性违规。现在换成 C 里的
   无锁 SPSC 环形队列（fc_audio_queue_*，release/acquire 原子
   操作），欠载时 C 侧补零并计数，标题栏显示 underruns。
3. **APU 单位混淆（主因）**：数据手册的表以 CPU 周期为单位，而项目
   的 APU tick 是半个 CPU 周期。把表直接当 tick 用，使得帧序列器、
   噪声、DMC、三角波全部慢一半（低一个八度），只有脉冲波恰好正确。
   耳机的表现：地上关卡的军鼓/踩镲变成一个持续的 "沙沙"；地下关卡
   不用噪声声道，所以听不出来。
   修法：噪声周期表和 DMC 速率表用前除以 2 再减 1；帧序列器 step
   改为 3729 tick；三角波定时器每个 tick 走两次。
   回归测试：packages/fc-core/tests/test_apu.cpp ApuRates.*
```

已修复：Zelda II 大地图全是方块 / 侧视关卡 tile 错位

```
根因（MMC1 的 8KB CHR 分页规则写错了）：

Zelda II 用 8KB CHR 模式（control bit4 = 0），而且**只写 CHR bank 0，
从不写 CHR bank 1**。正确规则是：

    8KB bank = chr_bank0 >> 1

因为寄存器里存的是 bank 号先左移了一位（bit0 在 8KB 模式下由 PPU 的
A12 接管，所以被忽略）。游戏写 $02 是要 bank 1，写 $10 是要 bank 8。

之前实现成了 (chr0 & 0x1E) | (chr1 & 1)：$10 -> 16（对 16 个 8KB
bank 取模后变成 0），$02 -> 2。于是大地图（$8149 处写 CHR0=$10）
用了标题画面的方块字库，整张地图一片方块；侧视关卡（写 $02）也偏了
一个 bank。

这个 bug 的隐蔽之处：游戏照常运行、不会崩，只是 tile 全部错位两个
bank，所以光看“能不能跑”永远发现不了。

修法：packages/fc-core/src/core/nes/mapper1.hpp 的 chr_offset() 8KB 分支改为 chr0 >> 1。
回归测试：packages/fc-core/tests/test_cartridge.cpp
  Mapper1.ChrEightKiloByteModeUsesBankZeroShiftedRight
```

Mapper 覆盖与工作量估计（累计新增 2/3/4/7/11、163/226，以及授权一批 + 中文一批）

```
已完成：0 NROM、1 MMC1、2 UxROM、3 CNROM、4 MMC3（含扫描线 IRQ）、
       7 AxROM、9 MMC2、10 MMC4、11 Color Dreams、13 CPROM、15 100-in-1、
       18 SS88006、19 Namco 163、21/22/23/25 VRC2/VRC4、32 IREM G-101、
       33 Taito TC0190、66 GxROM、68 Sunsoft-4、71 Codemasters、
       78 Jaleco JF-16、87 Jaleco JF-13、162/164/178/242 Waixing、
       163 Nanjing、177 Henggedianzi、190 Magic Kid Goo Goo、
       226 76-in-1、227/246/249 多合一/T9552

架构：Mapper 接口新增六个默认空实现钩子，已有 mapper 一行未改。
  virtual void on_ppu_address(u16) {}              // PPU 地址总线（MMC3）
  virtual bool irq_asserted() const { false; }     // 卡带 /IRQ 线
  virtual u8   read_expansion(u16) { return 0; }   // 扩展区 $4020-$5FFF
  virtual void write_expansion(u16, u8) {}         // 扩展区寄存器
  virtual void on_scanline(int) {}                 // 位置（非地址）事件
  virtual bool clocks_on_cpu_cycles() const { false; } // CPU 周期 IRQ
  virtual void on_cpu_cycle() {}                   // VRC4/SS88006 用
  virtual bool has_work_ram() const { true; }      // $6000 是 RAM 还是寄存器
Mapper 163 靠扩展区放分页寄存器、靠 on_scanline 做自动 4KB CHR 切换；
Mapper 18/21 靠 on_cpu_cycle 数 CPU 周期做 IRQ。

剩余工作量（按投入排序）：
  中   8/12/14 罕见                       ~100 行/个
  中   16 Bandai / 48 Taito TC0690        ~200 行/个
  中高 69 Sunsoft FME-7                   ~250 行   蝙蝠侠 ROTJ（带扩展音源）
  中   45/74/191/192/195/199 MMC3 clone   ~150 行/个 中文卡
  中   176 FK23C                          ~400 行   中文 RPG
  中   185/210/248 多合一                 ~150~250 行
  很高 5  MMC5         1000+ 行  Just Breed、Metal Slader Glory；建议单独立项
  高   19 Namco 163    ~500 行   Rolling Thunder（带扩展音源）
  高   24/26 VRC6      ~300 行   恶魔城传说(JP)（带扩展音源）
  高   85 VRC7         ~400 行   Lagrange Point（FM 音源）
  高   6  FDS          600+ 行   磁盘系统；建议单独立项
```

已修复：`100合1.NES`（1MB Mapper 15 多合一卡）无法启动

```
根因：Mapper 15 的分页粒度写错了。

最初的实现按“32KB PRG bank”写。但那个 ROM 的复位向量在
**最后一个 16KB** 的 $FFFC（= $C001）；32KB 分页会让 CPU 从 bank 0
的 $FFFC（= $8000）读向量，跑进另一段程序，卡在等一个永远不来的
标志位，画面就是一片方块。

正确版式：
  $8000-$BFFF  16KB 可切换（寄存器低位）
  $C000-$FFFF  固定为最后一个 16KB（复位/NMI/IRQ 向量永远在此）
  寄存器 bit6  单屏镜像
  CHR          8KB CHR RAM（文件头写 0 页）

修法：mapper15.hpp 的分页改为 16KB+顶部固定；cartridge.cpp 的 case 15
在 chr_rom_pages==0 时分配 8KB CHR RAM。

实测：`100合1.NES` 现在能启动到多合一菜单，Start 能切换页面，
bank 寄存器从 0x3E→0x39（菜单的 $FFD0 表）正常工作。
```

已修复：超级玛丽 3 底部状态栏乱码（MMC3 的扫描线 IRQ 从不触发）

```
现象：SMB3 地图屏幕底部状态栏（应该是文字 + 3 个道具框）变成一整片
重复的花砖图案，并且一直铺到屏幕最下方；`shot_0001.ppm` 与修复前的
输出逐字节一致。

根因（PPU + MMC3）：
  MMC3 的扫描线计数器靠 PPU A12 的上升沿时钟。A12=1 只在 PPU 取
  “精灵图案”时出现：背景图案表在 $0000 时，只有精灵取指会把 A12 拉高。
  真机在**每一条扫描线**都做 8 次精灵取指，即使该行一个精灵都没有，
  空槽会用 OAM 里的垃圾数据凑数，所以 A12 每行必有一次上升沿。

  我们的 PPU 只在 `evaluate_sprites()` 里对**真正在画面上的**精灵读
  CHR，空行一次 A12 上升沿都没有。实测：整个标题画面 3000 帧，
  `irq_clock_count()` 一直是 0，MMC3 的 IRQ 永远不触发，于是靠 IRQ
  分屏的状态栏直接画不出来。

修法：`Ppu::evaluate_sprites()` 末尾对余下的精灵槽做 8 - N 次
  “空取指”（读精灵图案表然后丢掉）。数据不影响渲染，只负责把 A12
  拉高一次；MMC2/MMC4 的 tile $FD/$FE latch 不会被 tile 0 误触。

验证：
  修复前 clocks/帧 = 0        fires/帧 = 0
  修复后 clocks/帧 = 240      fires/帧 = 1   （每扫描线一次、每帧一次）
  地图屏幕底部恢复为文字 + 3 个道具框，下方干净；
  其余 10 张 ROM 回归正常，429 个测试全通过。

回归测试：packages/fc-core/tests/test_cartridge.cpp
  Mapper4.TheCounterIsClockedOncePerScanlineEvenWithNoSprites
  （把 64 个精灵全放到屏幕下方，跑一帧，断言计数器增加 ~240）
```

本轮新增：Mapper 163 (Nanjing FC-001) 与 Mapper 226 (76-in-1)

```
下载目录里出现两张之前无法加载的 ROM：
  金庸群侠传.nes  ->  mapper 163，2MB PRG、CHR RAM、带电池
  76合1.nes      ->  mapper 226，2MB PRG、CHR RAM

Mapper 163（packages/fc-core/src/core/nes/mapper163.hpp）：
  - 32KB 窗口，分页寄存器在扩展区 $5000/$5200/$5300；
  - 复位时 mode bit2=0，把 A15/A16 强制为 11 —— 开机在 bank 3，
    不是 bank 0（复位向量就写在 bank 3）；
  - $5100/$5101 防拷反馈位，$5500 读回取反后的 F（D2）；
  - $5000 bit7 打开自动 4KB CHR RAM 切换，用 on_scanline(127/239)
    近似真机的 PPU A13/A9 锁存；
  - 实测：标题画面稳定，按 Start 后进入正式游戏画面，音乐正常。

Mapper 226（packages/fc-core/src/core/nes/mapper226.hpp）：
  - 7 位 bank 号拆在 $8000（低 5 位 + bit5 模式 + bit6 镜像 + bit7
    第 6 位）和 $8001（第 7 位）；
  - 模式 0 = 一个 32KB bank（丢掉最低位）；模式 1 = 同一 16KB bank
    出现在两个半区；1.5MB 卡带用 {0,0,1,2} 重排最高两位。

接口变化（全部是默认空实现，已有 mapper 一行未改）：
  read_expansion / write_expansion  扩展区 $4020-$5FFF
  on_scanline                       扫描线事件

测试：packages/fc-core/tests/test_cartridge.cpp 新增 13 个（Mapper163.* / Mapper226.*），
389 -> 403 个测试全通过（上一轮：163/226）
403 -> 423 个测试全通过（本轮：授权一批 + 中文一批，共 +20）
423 -> 424（MMC3 / SMB3 状态栏回归）
424 -> 429（mapper 19/177/249）
```

已知边界（都不阻塞使用）：

```
MMC3 的 1KB CHR 模式            已实现（R0/R1 到高 4KB、+1 配对）；
                               SMB3 标题/地图用 1KB 模式渲染正常
Mappers 5, 6, 8, 12, 14         未实现，估计见上
Mapper 163 的自动 CHR 切换      用扫描线近似真机的 PPU A13/A9 锁存
Bus-level cycle accuracy        RMW 伪写、中断采样时机、$2004 渲染期行为
PPU sprite overflow bug         真机的那个著名 bug 没有复现
非精确音频混音                   用标准公式近似，真机是非线性的
存档 / 读档 / 录像回放          Core 还没有 serialize
```

---

# 8. Next Task

下一步必须执行：

```
全部阶段完成。
```

下一步可选方向（按价值排序）：

1. **移植到一台真实机器上验证** —— 用测试 ROM（`nestest`、blargg 的 PPU/APU 测试）
   找出保真度缺口。这比盯着超级玛丽看高效得多
2. **更多 Mapper** —— 授权常见板：21/22/23/25 (VRC2/4)、69 (FME-7)、
   16 (Bandai)、18 (Jaleco)、66/71；中文板：176/178/190/191/195/199、
   45/74/192、185/210/227/248；大件：5 (MMC5)、6 (FDS)、19 (Namco 163)、
   24/26 (VRC6)、85 (VRC7)
3. **存档 / 读档** —— 需要给 Core 加 serialize
4. **录像回放** —— 接口已经支持（`set_button`），只差前端 UI

**推荐先做 1。** 现在能玩、能看、能听，但"能玩"和"正确"是两件事，
而测试 ROM 是唯一能把这两件事分开的工具。

---

# 9. Final Definition of Done

## CPU

- [x] 完整 6502（151/151 opcode）
- [x] cycle accurate（指令级；总线级仍缺 RMW 伪写等）
- [ ] test ROM 通过

## Memory

- [x] NES memory map（含镜像与 open bus）

## Cartridge

- [x] iNES
- [x] Mapper 0 (NROM)
- [x] Mapper 1 (MMC1)
- [x] Mapper 2 (UxROM)
- [x] Mapper 3 (CNROM)
- [x] Mapper 4 (MMC3，含扫描线 IRQ)
- [x] Mapper 7 (AxROM)
- [x] Mapper 9 (MMC2)
- [x] Mapper 10 (MMC4)
- [x] Mapper 11 (Color Dreams)
- [x] Mapper 13 (CPROM)
- [x] Mapper 15 (100-in-1)
- [x] Mapper 18 (SS88006)
- [x] Mapper 21 / 22 / 23 / 25 (VRC2/VRC4)
- [x] Mapper 32 (IREM G-101)
- [x] Mapper 33 (Taito TC0190)
- [x] Mapper 66 (GxROM)
- [x] Mapper 68 (Sunsoft-4)
- [x] Mapper 71 (Codemasters)
- [x] Mapper 78 (Jaleco JF-16)
- [x] Mapper 87 (Jaleco JF-13)
- [x] Mapper 162 / 164 / 178 / 242 (Waixing)
- [x] Mapper 163 (Nanjing)
- [x] Mapper 190 (Magic Kid Goo Goo)
- [x] Mapper 178 (Waixing)
- [x] Mapper 177 (Henggedianzi)
- [x] Mapper 19 (Namco 163)
- [x] Mapper 226 (76-in-1)
- [x] Mapper 227 / 246 (多合一)
- [x] Mapper 249 (Waixing T9552)
- [ ] Mapper 5/6/8/12/14/16/24/26/45/48/69/74/85/176/185/191/192/195/199/210/248

## Graphics

- [x] PPU（寄存器、VRAM、调色板、扫描线时序）
- [x] Sprite（含 8x16、翻转、优先级）
- [x] Scrolling（loopy v/t/x/w）
- [x] 真实 ROM 渲染出正确画面

## Audio

- [x] APU（五声道、包络、帧序列器、混音、DMC）
- [x] 真实游戏导出 WAV
- [ ] 精确混音曲线（现为标准公式近似）

## Input

- [x] Controller（串行协议、两个端口、脚本输入）
- [x] 物理手柄（GameController 框架，player 1 / port 0）
- [x] 键盘与手柄共存（两个 source 各自记状态，取 OR，互不覆盖）

## macOS

- [x] Electron renderer（canvas）

## Tools

- [ ] Debugger
- [ ] Disassembler
- [ ] Save state

## Education

必须存在：

```
docs/
  computer-science/
  assembly/
  architecture/
  nes/
```

并能够解释从 `机器码 → CPU → 像素` 全过程。

---

# Agent 最终原则

不要把这个项目当成"写一个模拟器"。

应该当成"通过实现一个真实系统，学习计算机科学完整链路"。

任何代码，都必须知道：它模拟现实计算机中的哪一个部件。
