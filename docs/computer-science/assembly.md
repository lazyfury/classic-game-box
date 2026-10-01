# 6502 汇编 Assembly

> 目标：理解汇编语言、机器码、汇编器和反汇编器的关系，
> 以及一个字节如何同时编码"做什么"和"数据在哪"。
>
> 所有输出都来自 `tools/demo_disasm.cpp`，并可由 `custom_nes_core/tests/core/test_disassembler.cpp` 验证。

---

## 1. 三个层次

```
人类写的                  汇编器             CPU 执行的
────────────            ────────           ──────────────
LDA #$42        ──────>   A9 42     ──────>   8 个 bit 的选择
（汇编语言）              （机器码）             （硬件行为）
```

| 层次 | 例子 | 谁在用 |
|------|------|--------|
| **汇编语言** | `LDA #$42` | 人类 |
| **机器码** | `A9 42` | CPU |
| **电压** | 高低电平 | 晶体管 |

**CPU 不认识 `LDA`。** 它只认识 `10101001`。
汇编器的工作就是把人能读的助记符翻译成一串字节，仅此而已。

**关键认识：汇编语言和机器码一一对应。** 一条汇编指令对应一段固定字节，
没有"优化"或"编译期计算"。这是它和高级语言的本质区别：

```
C:      x = x + 1;          <- 编译器可以优化、删除、重排
6502:   INC $10             <- 就是这一条，永远是这两个字节 E6 10
```

> 这就是为什么模拟器不需要"解析汇编"——它直接执行字节。
> 汇编只在我们阅读和调试时才出现。

---

## 2. Mnemonic（助记符）

**Mnemonic** 是"帮助记忆"的意思。它是指令的人类可读名字：

```
LDA  ->  LoaD Accumulator       把数据装进累加器
STA  ->  STore Accumulator      把累加器的值存进内存
JMP  ->  JuMP                   跳转
BNE  ->  Branch if Not Equal    不等则跳转
```

6502 一共有 **56 个助记符**，分成十类：

| 类别 | 数量 | 助记符 |
|------|------|--------|
| 加载 / 存储 | 6 | `LDA` `LDX` `LDY` `STA` `STX` `STY` |
| 寄存器传输 | 6 | `TAX` `TAY` `TSX` `TXA` `TXS` `TYA` |
| 栈操作 | 4 | `PHA` `PHP` `PLA` `PLP` |
| 逻辑 | 4 | `AND` `EOR` `ORA` `BIT` |
| 算术 / 比较 | 11 | `ADC` `SBC` `CMP` `CPX` `CPY` `INC` `INX` `INY` `DEC` `DEX` `DEY` |
| 移位 / 循环 | 4 | `ASL` `LSR` `ROL` `ROR` |
| 跳转 / 子程序 | 5 | `JMP` `JSR` `RTS` `RTI` `BRK` |
| 条件分支 | 8 | `BCC` `BCS` `BEQ` `BMI` `BNE` `BPL` `BVC` `BVS` |
| 标志控制 | 7 | `CLC` `CLD` `CLI` `CLV` `SEC` `SED` `SEI` |
| 其他 | 1 | `NOP` |

**注意：56 个助记符 = 151 个 opcode。** 因为很多助记符有多种寻址模式，
每种模式有自己的 opcode。`LDA` 一个助记符就占了 8 个 opcode。

---

## 3. 四个符号

AGENTS.md 要求出现这些符号必须解释。

### `#` — Immediate（立即数）

```
LDA #$42     A = 0x42
LDA $42      A = 内存[0x0042]
```

**`#` 表示"操作数就是数据本身"，不是"数据所在的地址"。**

这是最常见的错误来源：

```
LDA #$10   ->  A = 16
LDA $10    ->  A = 内存[0x0010]，可能是 200
```

一个字有没有 `#`，是两条完全不同的指令，两个不同的 opcode：

```
A9 10    LDA #$10    immediate
A5 10    LDA $10     zero page
```

### `$` — 十六进制

```
$42  ==  0x42  ==  66
$FF  ==  0xFF  ==  255
```

见 [hexadecimal.md](hexadecimal.md)。

> **注意项目里现在有两种十六进制前缀：**
> - `0x42` — C++ 源码（我们自己的代码）
> - `$42` — 6502 汇编（ROM 里的内容）
>
> 同一个数字，两种语言，两种约定。反汇编器输出 `$`，因为它在说汇编语言。

### `,` — Indexed（变址）

```
LDA $0200,X
```

**实际地址 = `$0200 + X`。**

```
X = 0  ->  读 $0200
X = 1  ->  读 $0201
X = 5  ->  读 $0205
```

用途：**数组**。

```
; 把 16 个字节写入 $0200..$020F
      LDX #$00
loop: LDA $0300,X     ; 从数组 2 读
      STA $0200,X     ; 写到数组 1
      INX
      CPX #$10
      BNE loop
```

**`X` 和 `Y` 的区别：** 只有 `Absolute,X`、`Absolute,Y`、`ZeroPage,X`、
`ZeroPage,Y` 等在某些指令上不同。它们是**两个独立的索引寄存器**，
因为这样一条指令里可以同时用两个索引（`LDA $0200,X` 之后再 `STA $0300,Y`）。

### `()` — Indirect（间接）

```
JMP ($8000)
```

**不是跳到 `$8000`，而是：**

```
1. 读 [$8000] 作为低字节
2. 读 [$8001] 作为高字节
3. 拼成一个地址
4. 跳到那个地址
```

例：

```
[$8000] = $34
[$8001] = $12
JMP ($8000)  ->  PC = $1234
```

**用途：跳转表。** 游戏用它在多个状态之间切换：

```
      ASL A           ; A * 2，因为每个地址 2 字节
      TAX
      LDA table,X
      STA $10
      LDA table+1,X
      STA $11
      JMP ($0010)     ; 跳到选中的处理程序

table: .word init, playing, paused, gameover
```

### `(zp,X)` 和 `(zp),Y` 的区别

这是最容易搞混的一对：

```
LDA ($42,X)     先把 X 加到指针上，再解引用
                地址 = 内存[$0042 + X] + 内存[$0042 + X + 1] * 256

LDA ($42),Y     先解引用，再把 Y 加到结果上
                地址 = 内存[$0042] + 内存[$0043] * 256 + Y
```

**记忆：逗号在括号里 = 先加；逗号在括号外 = 后加。**

```
($42,X)  括号内 -> 先索引指针
($42),Y  括号外 -> 后索引结果
```

---

## 4. 13 种寻址模式

`demo_disasm` 第 3 节的真实输出：

```
  syntax          opcode  len  mode
  --------------  ------  ---  ------------------------------------
  TAX             $AA    1   implied
  ASL A           $0A    1   accumulator
  LDA #$42        $A9    2   immediate
  LDA $42         $A5    2   zero page
  LDA $42,X       $B5    2   zero page,X
  LDX $42,Y       $B6    2   zero page,Y
  LDA $1234       $AD    3   absolute
  LDA $1234,X     $BD    3   absolute,X
  LDA $1234,Y     $B9    3   absolute,Y
  JMP ($1234)     $6C    3   indirect
  LDA ($42,X)     $A1    2   indirect,X
  LDA ($42),Y     $B1    2   indirect,Y
  BNE $8008       $D0    2   relative
```

### 为什么要有 Zero Page

`LDA $42` 只用 **2 字节**，`LDA $0042` 要用 **3 字节**，而且后者不存在
（没有 "absolute with high byte 0" 这种编码）。

**Zero Page 是一种压缩技巧。** 6502 规定 `$0000`–`$00FF` 这 256 字节可以用
1 字节地址访问。代价是：

- 省 1 字节（RAM 只有 2KB，省下来很值）
- 快 1 个周期
- **回绕陷阱**：`$42,X` 当 X = `$C0` 时，`$42 + $C0 = $102`，
  但高字节固定为 0，所以实际读 **`$0002`**，不是 `$0102`。

```cpp
// 真实硬件行为
u8 low = static_cast<u8>(0x42 + x);   // 溢出丢弃高字节
address = low;                        // 永远在 $0000-$00FF
```

**这是 Phase 0.4 实现寻址模式时的第一个坑。**

---

## 5. 指令长度由寻址模式决定

| 寻址模式 | 操作数字节 | 总长度 |
|---------|-----------|--------|
| implied | 0 | 1 |
| accumulator | 0 | 1 |
| immediate | 1 | 2 |
| zero page | 1 | 2 |
| zero page,X / ,Y | 1 | 2 |
| indirect,X / ,Y | 1 | 2 |
| relative | 1 | 2 |
| absolute | 2 | 3 |
| absolute,X / ,Y | 2 | 3 |
| indirect | 2 | 3 |

**151 个合法 opcode 的实际分布：**

```
1 字节: 29 个
2 字节: 74 个
3 字节: 48 个
```

### 为什么这条规则至关重要

它让 CPU 可以**在不理解指令的情况下正确地跳过它**：

```
1. 读 PC 处的字节 -> 这就是 opcode
2. 查表 -> 得知长度是 2
3. PC += 2
4. 现在 PC 指向下一条指令
```

如果长度不固定，CPU 就必须"理解"每条指令才能前进——那就没法做流水线，
也没法做反汇编。

**这条规则也是我们能在 `disassembler.cpp` 里靠 opcode 直接算出长度、
不用读操作数的原因。**

---

## 6. 汇编器 / 反汇编器

### 汇编器：文本 → 字节

```
输入:  LDA #$42
输出:  A9 42
```

真正的汇编器还要处理：

| 功能 | 例子 |
|------|------|
| 标签 | `loop:` 记录当前地址 |
| 符号引用 | `JMP loop` |
| 伪指令 | `.org $8000`、`.byte`、`.word` |
| 表达式 | `LDA #<table`、`LDA #>table`（低/高字节） |
| 前向引用 | 跳转到还没定义的标签（需要两遍扫描） |

**"两遍扫描"是汇编器的核心难点：** 第一遍记录所有标签的地址，
第二遍才生成字节。因为 `JMP loop` 里的 `loop` 可能在后面才定义。

### 反汇编器：字节 → 文本

```
输入:  A9 42
输出:  LDA #$42
```

**反汇编器比汇编器简单得多**，因为只有单向、没有歧义：

- 从任意地址开始，opcode 决定长度，长度决定下一条指令的起点
- 不需要两遍扫描
- 不需要符号表（但无法还原标签名，只能输出绝对地址）

**这就是为什么反汇编器是调试 ROM 的第一工具。** 当游戏行为异常，
你把相关内存 dump 成指令列表，读程序员当初写了什么。

### 汇编器无法完全逆向

```
LDA #$42
```

我写了 `LDA #$42`、`LDA #66` 还是 `LDA #$2A+$18`，**反汇编出来都一样**。
注释、标签名、格式化——这些信息在汇编后就永远丢失了。

**这直接说明：机器码只保留语义，不保留意图。**

---

## 7. 相对寻址：分支的补码

分支指令的操作数是**有符号 8 位偏移**，从**下一条指令**的地址算起：

```
target = 分支地址 + 2 + (有符号)偏移
```

**`+2` 是因为分支本身 2 字节。** 分支被执行时，PC 已经越过了操作数。

`demo_disasm` 第 5 节：

```
  branch    operand   signed   target    note
  --------  --------  -------  --------  ----
  $8000     $05       5        $8007     0x05 = +5    forward
  $8000     $FB       -5       $7FFD     0xFB = -5    backward
  $8000     $7F       127      $8081     0x7F = +127  furthest forward
  $8000     $80       -128     $7F82     0x80 = -128  furthest backward
  $FFFE     $00       0        $0000     next instruction would be $10000 -> $0000
```

代码实现（`custom_nes_core/src/core/cpu/disassembler.cpp`）：

```cpp
u16 branch_target(u16 address, u8 offset) noexcept
{
    const int signed_offset = static_cast<int>(bit::as_signed(offset));
    const int next_instruction = static_cast<int>(address) + 2;
    return static_cast<u16>(next_instruction + signed_offset);
}
```

### 三个必须记住的点

**1. `0x80` 是 -128，不是 +128。**

分支范围是 `-128 .. +127`，不对称。如果按无符号处理 `0x80`，
你会得到 `+128`，跳到完全错误的地方。

**2. 这是 `bit::as_signed` 的唯一用途，也是它存在的理由。**

见 [twos-complement.md](twos-complement.md) 第 8.2 节。

**3. 目标地址会回绕。**

`$FFFE` 处的分支，其"下一条指令"是 `$10000`——16 位地址线上不存在。
所以 `$FFFE + 2 + 0` 回绕成 `$0000`。

**这是新手写 6502 模拟器的第一个大 bug 高发区。** 症状是游戏在
特定位置突然跳到地址 0，然后执行 RAM 里的垃圾数据。

---

## 8. 非法 opcode

**官方 6502 只定义了 256 个中的 151 个。**

```
  151 合法
+ 105 未定义
= 256
```

```
$A9 -> "LDA #$42"   legal, length 2
$02 -> "???"        illegal, length assumed 1
```

### 处理原则：报告，不要猜

有些未定义的编码在真实硬件上**确实会做某些事**（俗称"非法指令"，
在 NES 上常被用于优化）。但：

1. 它们的行为**不在官方文档里**，不同芯片批次可能不同
2. 把它们当成 `NOP` 会**静默地产生错误的游戏行为**
3. 长度未知，所以连"跳过它"都做不到

所以反汇编器输出 `???`，长度假定为 1；CPU 遇到它们直接停机。

**"停机"比"猜错"好得多。** 见 [cpu.md](cpu.md) 第 6 节。

---

## 9. opcode 表不是随机的

256 项的表看起来像一堆要背的数据。但**它有严格的规律**，
这也是我们能用结构性测试验证它的原因。

### 规律 1：ALU 组的列模式

`ORA` `AND` `EOR` `ADC` `STA` `LDA` `CMP` `SBC` 这 8 个助记符
共享完全相同的列布局：

```
             +0x00       +0x04     +0x08     +0x0C
             (zp,X)      zp        #imm      abs

             +0x10       +0x14     +0x18     +0x1C
             (zp),Y      zp,X      abs,Y     abs,X
```

它们的 opcode 只差高半字节：

```
ORA = 0x01     AND = 0x21     EOR = 0x41     ADC = 0x61
STA = 0x81     LDA = 0xA1     CMP = 0xC1     SBC = 0xE1
```

**唯一例外：`STA` 没有立即数模式**，所以 `0x89` 是非法 opcode。
因为"把 A 存到立即数里"没有意义。

### 规律 2：移位组有累加器模式

```
ASL/ROL/LSR/ROR:   zp(+0x00)  A(+0x04)  abs(+0x08)  zp,X(+0x10)  abs,X(+0x18)
```

`(+0x04)` 位置是 `A`（累加器模式），因为 `ASL A` 有意义而 `STA A` 没有意义。

### 规律 3：INC/DEC 没有累加器模式

```
INC/DEC:   zp(+0x00)  abs(+0x08)  zp,X(+0x10)  abs,X(+0x18)
                                    ↑ 注意是 +0x10 不是 +0x14
```

因为 `+0x00` 位置被 `zp` 占了（没有 `(zp,X)` 模式），所以整个模式左移一格。

**这些规律让我们能写出结构性测试**，而不是逐条抄一遍表：

```cpp
TEST(OpcodeTable, AluGroupFollowsTheColumnPattern)
TEST(OpcodeTable, ShiftGroupHasAnAccumulatorMode)
TEST(OpcodeTable, IncDecGroupHasNoAccumulatorMode)
```

**为什么这很重要：** 抄写 256 项表时，人眼会漏掉一个错位。
结构性测试会立刻抓住它——比如把 `0xD6` 写成 `ZeroPage` 而不是 `ZeroPageX`。

> 我们在写这个测试时，自己就在 `INC/DEC` 的偏移上错了两次
> （写成 `+0x14`/`+0x1C`，实际是 `+0x10`/`+0x18`）。
> 测试立刻指出了错误。

---

## 10. 反汇编器 vs CPU：两种"不知道"

`demo_disasm` 第 8 节：

```
    address    instruction        disassembler  CPU
    ---------  -----------------  ------------  ---
    $8000      LDA #$42           yes           yes
    $8002      TAX                yes           yes
    $8003      STA $0200          yes           no
    $8006      LDA $0200,X        yes           no
    $8009      JMP $8000          yes           no
```

**这是两种性质完全不同的"不知道"：**

| | 反汇编器 | CPU |
|---|---------|-----|
| 不知道什么 | 字节的**含义** | 如何**执行** |
| 何时解决 | Phase 0.3（已完成） | Phase 0.4 / 1 |
| 后果 | 无法阅读代码 | 无法运行游戏 |

**反汇编器现在就知道全部 151 个 opcode 了。**
CPU 只知道 12 个——这个差距就是接下来的工作。

> 这也解释了一个设计问题：现在 `cpu.cpp` 里的 `switch` 和
> `opcode.cpp` 里的表是**同一份知识的两个副本**。
> 我们用 `Cpu::implementsAgreesWithExecution` 测试保证它们不脱节，
> 但 Phase 1 会把它们合并成一张驱动两边的表。

---

## 11. 代码对应

| 概念 | 文件 |
|------|------|
| 寻址模式定义 + 长度规则 | `custom_nes_core/src/core/cpu/opcode.hpp` |
| 256 项 opcode 表 | `custom_nes_core/src/core/cpu/opcode.cpp` |
| 反汇编器 | `custom_nes_core/src/core/cpu/disassembler.{hpp,cpp}` |
| 结构性与等价性测试 | `custom_nes_core/tests/core/test_disassembler.cpp` |
| 可运行讲解 | `tools/demo_disasm.cpp` |
| 用反汇编器的 CPU 轨迹 | `tools/demo_cpu.cpp` |

```bash
./build/demo_disasm
./build/demo_cpu
./build/tests/fc_tests --gtest_filter='Disassembler.*:OpcodeTable.*'
```

---

## 12. 自测

1. `LDA #$42` 和 `LDA $42` 有什么区别？机器码分别是什么？
2. `LDA $0200,X` 在 X = 5 时读哪个地址？
3. `LDA ($0200,X)` 和 `LDA ($0200),Y` 的区别是什么？
4. 一个 opcode 是 3 字节，它的寻址模式可能是什么？（说出全部）
5. 分支偏移 `0x80` 是向前还是向后跳几字节？
6. 为什么 `0x89` 是非法 opcode？
7. 官方 6502 有多少个合法 opcode？多少个助记符？
8. 为什么反汇编器无法还原原来的标签名和注释？
9. 为什么 `LDA $0042` 不存在，只有 `LDA $42`？

<details>
<summary>答案</summary>

1. `#` 是立即数：`A = 0x42`。没有 `#` 是地址：`A = 内存[$0042]`。
   机器码分别是 `A9 42` 和 `A5 42`
2. `$0205`
3. `($0200,X)` 先把 X 加到指针上再解引用；`($0200),Y` 先解引用再把 Y 加到结果上
4. `absolute`、`absolute,X`、`absolute,Y`、`indirect`（这四种的操作数是 2 字节）
5. 向后。`0x80` 作为补码是 -128，`target = 分支地址 + 2 - 128`
6. `0x89` 是 `STA` 的立即数模式，而"把累加器存到立即数里"没有意义
7. 151 个 opcode，56 个助记符
8. 因为汇编后只保留语义，不保留意图。注释和标签名在生成机器码时就丢失了
9. 因为 Zero Page 模式的高字节**硬编码为 0**，不需要也不允许指定。
   要用完整 16 位地址必须用 absolute 模式（`AD 42 00`）

</details>

---

**上一章：** [cpu.md](cpu.md) · **下一章：** addressing-modes.md（Phase 0.4 待写）
