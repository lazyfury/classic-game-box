---
name: fc-docs
description: 教学文档 docs/（computer-science 十章、assembly 索引、nes 硬件规范、architecture 架构）与 AGENTS.md 的教学契约。用于写/改文档、给一次代码改动补「目标-原理-设计-测试-总结」的教学说明、把文档与实现对齐（例如 overflow-flag.md 与 alu.hpp、mappers.md 与 mapperN.hpp），或需要向读者解释 ROM→机器码→CPU→PPU→像素这条链路时。
---

# fc-docs —— 文档与教学契约

这个项目的主张是：**不只是写模拟器，而是用实现一个真实系统来学完整的
计算机科学链路**。文档是交付物的一部分，不是注释的副产品。

`AGENTS.md` 是宪法。**任何代码改动都要按它的 2.1 节给出教学说明**：

```
本次目标 → 相关 CS 知识 → NES 原理解释 → 设计方案 → 代码实现
        → 测试 → 测试结果解释 → 下一阶段目标
```

禁止「直接贴一大堆代码然后让用户运行」。所以在本仓库里，
**只改代码不解释**和**只写文档不对齐代码**都是未完成的工作。

## 0. 上下文纪律

**白名单**（读一篇就够）：

```
AGENTS.md                                 宪法；先 rg -n '^# ' AGENTS.md 拿目录
docs/README.md                            学习入口索引
docs/computer-science/<一章>.md
docs/assembly/README.md
docs/nes/<一篇>.md
docs/architecture/<一篇>.md
docs/images/app.png                       仅在 README 截图相关时
```

**禁读**：整个 `docs/`（13+ 篇，全部读进来就没有上下文了）、`build*/`。

**规则**：

- 一次只打开**一篇**文档 + 它对应的**一个**源码文件。
- 文档偏大时先 `rg -n '^#{1,3} ' <file>` 拿目录，再 `read offset/limit`。
- 改代码时读的是「被这次改动影响的那一篇」，不是整套文档。

## 1. 结构与文档 ↔ 代码配对

| 文档 | 对着哪份代码 |
|---|---|
| `computer-science/binary.md` | `core/bit.hpp`、`tools/demo_bitwise.cpp` |
| `computer-science/hexadecimal.md` | `core/bit.hpp`、`tools/demo_bitwise.cpp` |
| `computer-science/twos-complement.md` | `core/alu.hpp`、`tools/demo_overflow.cpp` |
| `computer-science/bitwise-operations.md` | `core/bit.cpp` |
| `computer-science/overflow-flag.md` | `core/alu.hpp`、`tools/demo_overflow.cpp` |
| `computer-science/cpu.md` | `core/cpu/cpu.cpp`、`tools/demo_cpu.cpp` |
| `computer-science/assembly.md` | `core/cpu/disassembler.cpp`、`tools/demo_disasm.cpp` |
| `computer-science/instruction-set.md` | `core/cpu/opcode.cpp`、`core/cpu/cpu.cpp` |
| `computer-science/addressing-modes.md` | `core/cpu/addressing.cpp`、`tools/demo_addressing.cpp` |
| `computer-science/timing.md` | 周期表、`tests/test_cycles.cpp` |
| `nes/memory-map.md` | `core/nes/bus.cpp`、`tests/test_nes_bus.cpp` |
| `nes/ines-format.md` | `core/nes/ines.cpp` |
| `nes/mappers.md` | `core/nes/mapper*.hpp`、`cartridge.cpp`、`tests/test_cartridge.cpp` |
| `nes/ppu.md` | `core/nes/ppu.{hpp,cpp}`、`tests/test_ppu.cpp` |
| `nes/apu.md` | `core/nes/apu.{hpp,cpp}`、`tests/test_apu.cpp` |
| `nes/controllers.md` | `core/nes/controller.hpp` |
| `architecture/bus.md` | `core/bus.hpp`、`core/nes/bus.cpp` |
| `architecture/frontend.md` | `electron/`（见 `fc-frontend` 技能） |
| `architecture/libretro-migration.md` | `packages/fc-libretro/`（见 `fc-libretro` 技能） |
| `architecture/mame-integration.md` | `build-mame/`、第三方核集成 |

**改了表格左边的实现，就要检查右边那篇文档是否还成立。**

## 2. 教学内容的硬性要求（来自 AGENTS.md 第 5 节）

写/改文档时，下列概念出现就必须解释清楚：

- **二进制**：为什么用 0/1，bit / byte / word。
- **十六进制**：`0x42` 是 `01000010`。
- **补码**：为什么 `0xFF` 既是 255 也是 −1。
- **C 与 V 的区别**：C = 无符号进位出；V = `carry_into_bit7 XOR carry_out_of_bit7`；
  N 只是结果的 bit 7，**不是判决**；`A - M` 的真实符号是 `N XOR V`。
- **寄存器 / PC / 指令周期**：Fetch → Decode → Execute → Update State；
  `PC=8000` 取 `A9`、`PC=8001` 取 `42`、`PC=8002`。
- **汇编符号**：`#`（立即数）、`$`（十六进制）、`(` `)`（间接）、`,`（变址）
  必须在首次出现时解释。`LDA #$42` → `A = 0x42`。

## 3. 任务菜谱

**给一次代码改动补教学说明**：按 2.1 的八项，逐项写进提交信息正文或对应的
`docs/` 文档；先给原理，再给代码，再给测试结果与解释。

**新增一篇文档**：

1. 放在对应目录（`computer-science/` / `nes/` / `architecture/` / `assembly/`）。
2. 在 `docs/README.md` 与所在目录的 `README.md` 里加链接（`rg -n '<邻居标题>' docs/README.md`）。
3. 若它是某篇的下一章，更新前一篇末尾的「下一阶段」。

**记录一次 bug 的根因**：参考 `AGENTS.md` 第 7 节的写法 ——
现象 → 根因（讲清硬件原理）→ 修法 → 回归测试（给出测试名）→ 验证数据。
这比「修了个 bug」有价值得多。

## 4. 检索菜谱

```bash
rg -n '^#{1,3} ' AGENTS.md docs/README.md
rg -n 'overflow|N XOR V' docs/computer-science/overflow-flag.md
rg -n 'mapper' docs/nes/mappers.md | head -20
rg -n '\[.*\]\(.*\.md\)' docs/README.md        # 学习路径里的链接
```
