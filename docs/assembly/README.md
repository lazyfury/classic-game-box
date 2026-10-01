# 6502 汇编 assembly/

> 状态：**Phase 0.3 / 0.4 已完成**（主体文档在 `../computer-science/assembly.md` 和 `../computer-science/addressing-modes.md`）

## 已完成

| 内容 | 位置 |
|------|------|
| 助记符 / 机器码 / 汇编器 / 反汇编器 | `../computer-science/assembly.md` |
| `#` `$` `()` `,` 四个符号 | `../computer-science/assembly.md` 第 3 节 |
| **有效地址计算** | `../computer-science/addressing-modes.md` |
| **Zero page 回绕** | `../computer-science/addressing-modes.md` 第 3 节 |
| **JMP ($xxFF) 硬件 bug** | `../computer-science/addressing-modes.md` 第 4 节 |
| 13 种寻址模式的语法 | `custom_nes_core/src/core/cpu/opcode.hpp` |
| 13 种模式的求值 | `custom_nes_core/src/core/cpu/addressing.{hpp,cpp}` |
| 完整 256 项 opcode 表 | `custom_nes_core/src/core/cpu/opcode.cpp` |
| 反汇编器 | `custom_nes_core/src/core/cpu/disassembler.{hpp,cpp}` |
| 表驱动派发 | `custom_nes_core/src/core/cpu/cpu.cpp` |
| 可运行讲解 | `tools/demo_disasm.cpp` `tools/demo_addressing.cpp` |

```bash
./build/demo_disasm
./build/demo_addressing
```

## 两个阶段的分工

```
Phase 0.3   0xBD 00 02  ->  "LDA $0200,X"        知道它是什么
Phase 0.4   0xBD 00 02  ->  读出 内存[$0200 + X]  知道怎么取
```

## 最小例子

```
Assembly:   LDA #$42
Machine:    A9 42
            ^^ opcode (LDA immediate)
               ^^ operand (0x42)
```

CPU 执行后：`A = 0x42`，`Z` 清零（结果非零），`N` 清零（bit7 = 0）。

## 关键事实速查

```
合法 opcode          151 / 256
助记符               56
寻址模式             13
指令长度             1 字节 29 个 / 2 字节 74 个 / 3 字节 48 个
分支范围             -128 .. +127
分支目标公式         target = 分支地址 + 2 + (有符号)偏移
```
