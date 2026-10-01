# 完整指令集 The Instruction Set

> 目标：理解剩下 14 个操作的语义，尤其是 ALU 的标志位、栈协议、子程序的不对称性，
> 以及中断机制。
>
> 本文输出都来自 `tools/demo_instructions.cpp`，并可由 `custom_nes_core/tests/core/test_instruction_set.cpp` 验证。

---

## 1. 完成度

```
Defined opcodes   : 151
Undefined opcodes : 105
Mnemonics         : 56
```

**151 个合法 opcode 全部可执行**（`demo_instructions` 第 1 节逐个运行验证，无一停机）。

### 按功能分类

| 类别 | 数量 | 助记符 |
|------|------|--------|
| 加载 / 存储 | 6 | `LDA` `LDX` `LDY` `STA` `STX` `STY` |
| 寄存器传输 | 6 | `TAX` `TAY` `TSX` `TXA` `TXS` `TYA` |
| 栈 | 4 | `PHA` `PHP` `PLA` `PLP` |
| 逻辑 | 4 | `AND` `EOR` `ORA` `BIT` |
| 算术 / 比较 | 11 | `ADC` `SBC` `CMP` `CPX` `CPY` `INC` `INX` `INY` `DEC` `DEX` `DEY` |
| 移位 / 循环 | 4 | `ASL` `LSR` `ROL` `ROR` |
| 跳转 / 子程序 | 5 | `JMP` `JSR` `RTS` `RTI` `BRK` |
| 条件分支 | 8 | `BCC` `BCS` `BEQ` `BMI` `BNE` `BPL` `BVC` `BVS` |
| 标志控制 | 7 | `CLC` `CLD` `CLI` `CLV` `SEC` `SED` `SEI` |
| 其他 | 1 | `NOP` |

Phase 0.4 完成了 42 个，Phase 1 补齐最后 14 个：

```
ALU      ADC SBC AND ORA EOR BIT
栈       PHA PHP PLA PLP
子程序   JSR RTS RTI BRK
```

---

## 2. ADC：一个加法器，两种溢出

```
ADC   A = A + M + C
```

**注意 `+ C`。6502 没有"不带进位的加法"。** 想算纯加法必须先 `CLC`。

`demo_instructions` 第 2 节：

```
  A     M     C    result  C  V  N  Z   meaning
  ----------------------------------------------------------------
  $02   $02   0    $04     0  0  0  0   2 + 2 = 4, nothing special
  $7F   $01   0    $80     0  1  1  0   +127 + 1 overflows signed, but not unsigned
  $FF   $01   0    $00     1  0  0  1   -1 + 1 wraps unsigned, but is correct signed
  $80   $80   0    $00     1  1  0  1   -128 + -128 overflows both ways
  $50   $50   0    $A0     0  1  1  0   80 + 80 = 160 overflows signed only
  $02   $02   1    $05     0  0  0  0   2 + 2 + carry = 5
```

- **C** = 无符号溢出（bit 7 的进位出）
- **V** = 有符号溢出（`carry_into7 XOR carry_out7`）

两者独立，完整推导见 [overflow-flag.md](overflow-flag.md)。

实现（`custom_nes_core/src/core/cpu/cpu.cpp`）：

```cpp
case Operation::ADC: {
    const alu::AddResult result = alu::add(
        reg_.a, operand_value(info, operand), reg_.flag(Flag::Carry));

    reg_.a = result.result;
    reg_.set_flag(Flag::Carry, result.carry);       // unsigned overflow
    reg_.set_flag(Flag::Overflow, result.overflow); // signed overflow
    reg_.update_nz(reg_.a);
    break;
}
```

**测试用 131072 种组合（256 × 256 × 2 种进位）对照宽整数真值。**
这是唯一能真正证明标志位正确的办法。

---

## 3. SBC：为什么 SEC 必须在前

```
SBC   A = A - M - (1 - C)
```

这个公式看起来别扭，但它是补码的必然结果：

```
A - M - (1 - C)
= A - M - 1 + C
= A + ~M + C          <- 硬件实际执行的
```

所以 **C 在这里表示"没有借位"**：

| C | 含义 |
|---|------|
| 1 | 没有借位（不需要减 1） |
| 0 | 有借位（需要再减 1） |

这就是 6502 汇编的经典写法：

```
SEC        ; C = 1，表示"无借位"
SBC #$03   ; A = A - 3
```

### C 是同一根线，两种读法

`demo_instructions` 第 3 节：

```
  A     M     C     result  C  V    meaning
  --------------------------------------------------
  $05   $03   1     $02     1  0    5 - 3
  $03   $05   1     $FE     0  0    3 - 5
  $05   $03   0     $01     1  0    5 - 3 - 1
```

**同一份加法器电路，加法时读作 carry，减法时读作 no-borrow。**
这是补码让一个电路干两件事的直接后果，见 [twos-complement.md](twos-complement.md)。

实现：

```cpp
case Operation::SBC: {
    const bool borrow_in = !reg_.flag(Flag::Carry);
    const alu::AddResult result = alu::subtract(
        reg_.a, operand_value(info, operand), borrow_in);

    reg_.a = result.result;
    reg_.set_flag(Flag::Carry, result.carry);       // carry out == no borrow
    reg_.set_flag(Flag::Overflow, result.overflow);
    reg_.update_nz(reg_.a);
    break;
}
```

> **Phase 0.4 的 `compare()` 把 C 取反了。** Phase 1 写测试时抓到：
> `alu::subtract` 的 `carry` **已经**是"无借位"，不该再取反。
> 这个 bug 在 36 个寻址测试里全部漏过，因为没有一条测试检查 C 标志。

---

## 4. 逻辑组

```
AND   A = A & M     掩码（取出某些位）
ORA   A = A | M     置位
EOR   A = A ^ M     翻转
BIT   只测试，不改 A
```

前三个都更新 N 和 Z。

### BIT 是特例

```
BIT  M:
   Z = ((A & M) == 0)
   N = M 的 bit 7
   V = M 的 bit 6
   A 不变，C 不变
```

**BIT 不从 A 里取 N 和 V，而是从操作数里取。** 这看起来奇怪，但它正是 BIT 的用途：

```
; 轮询一个硬件状态寄存器
loop: BIT status     ; 一条指令同时测试两个位
      BPL loop       ; bit 7 是 0 就继续等
```

一条指令完成"测试 + 分支"，在 1975 年这是很值钱的。

```cpp
case Operation::BIT: {
    const u8 value = operand_value(info, operand);
    reg_.set_flag(Flag::Zero, (reg_.a & value) == 0);
    reg_.set_flag(Flag::Negative, (value & 0x80u) != 0);
    reg_.set_flag(Flag::Overflow, (value & 0x40u) != 0);
    break;
}
```

**BIT 没有立即数模式**：`0x89` 是非法 opcode。因为"测试一个立即数"没有意义——
立即数不是硬件寄存器。

---

## 5. 状态寄存器 P 的完整语义

```
bit:  7    6    5    4    3    2    1    0
      N    V    -    B    D    I    Z    C
```

| 位 | 真实存在？ | 说明 |
|----|-----------|------|
| N V D I Z C | ✅ | 真正的触发器 |
| bit 5 | ❌ | 永远读作 1 |
| **B (4)** | ❌ | **只存在于压栈的副本里** |

### B 标志的真相

**6502 没有 B 触发器。** bit 4 只在 `P` 被推入栈时才被设置，而设置与否取决于**谁**在推：

```
PHP / BRK      推入时 bit 4 = 1
NMI / IRQ      推入时 bit 4 = 0
```

这是软件区分「程序主动执行了 BRK」和「硬件打断了程序」的**唯一**办法，因为
两者的向量完全相同（都是 `$FFFE`）。

```cpp
u8 Cpu::status_for_push(bool break_flag) const noexcept
{
    u8 value = reg_.p;
    value = bit::assign(value, static_cast<int>(Flag::Unused), true);
    value = bit::assign(value, static_cast<int>(Flag::Break), break_flag);
    return value;
}
```

**这就是为什么 `status_string()` 里那位总是 `.`** —— 它不是机器的状态，而是
栈上那一个字节的属性。

---

## 6. 栈协议

6502 的栈只有 page 1（`$0100`–`$01FF`），256 字节，**向下生长**。

```
push   write 0x0100 | SP, then SP--
pop    SP++, then read 0x0100 | SP
```

### 16 位入栈：高字节先

```cpp
void Cpu::push_word(u16 value) noexcept
{
    push(bit::hi_byte(value));   // 高字节先压
    push(bit::lo_byte(value));   // 低字节后压，所以在栈顶
}

u16 Cpu::pull_word() noexcept
{
    const u8 lo = pop();         // 低字节先出
    const u8 hi = pop();
    return bit::make_u16(lo, hi);
}
```

**先压高字节，弹出时低字节先出来。** 这样 `make_u16(lo, hi)` 直接就能用。

---

## 7. JSR / RTS 的不对称

`demo_instructions` 第 5 节：

```
  JSR $8005 at $8000

  PC after fetching the instruction : $8003
  But the pushed value is           : $8002

  After JSR, PC = $8005 and SP = $FB
  After RTS, PC = $8003
```

**JSR 压入的是 `PC - 1`，不是下一条指令的地址。**

为什么？因为 RTS 是"弹出后加 1"：

```cpp
case Operation::JSR: {
    const u16 return_address = static_cast<u16>(reg_.pc - 1u);
    push_word(return_address);
    reg_.pc = operand.address;
    break;
}
case Operation::RTS:
    reg_.pc = static_cast<u16>(pull_word() + 1u);
    break;
```

### 为什么这样设计

因为**中断序列**必须压入"下一条指令"的地址：

```
IRQ:
  push PC           <- 下一条指令，没有 -1
  push P
  PC = [$FFFE]
```

而 `RTI` 负责弹出这个：

```cpp
case Operation::RTI:
    restore_status(pop());
    reg_.pc = pull_word();    // 不加 1
    break;
```

所以：

| 指令 | 压入 | 弹出后 |
|------|------|--------|
| `JSR` | `PC - 1` | `RTS` 加 1 |
| 中断 / `BRK` | `PC` | `RTI` 不加 |

**`JSR`/`RTS` 和 `中断`/`RTI` 是两套不同的约定，各自内部自洽。** 混用就会跳飞。

> 这和 [addressing-modes.md](addressing-modes.md) 第 5 节的 `Address` vs `Target`
> 是同一类问题：**看起来一样的东西，语义不同，必须显式区分。**

---

## 8. 中断

### 三个向量

```
$FFFA  NMI     不可屏蔽。PPU 每帧结束拉一次，游戏的主循环节拍
$FFFC  RESET   上电
$FFFE  IRQ     可被 I 标志屏蔽；BRK 共用这个向量
```

### 中断序列

```
1. push PC  (高字节先)
2. push P   (bit 5 = 1；bit 4 = 0 表示硬件中断，= 1 表示 BRK)
3. I = 1
4. PC = 从向量读取的地址（小端）
```

**总共 7 个周期**，作为一个原子操作。

```cpp
void Cpu::enter_interrupt(u16 vector, bool is_break) noexcept
{
    push_word(reg_.pc);
    push(status_for_push(is_break));
    reg_.set_flag(Flag::IrqDisable, true);

    const u8 lo = read(vector);
    const u8 hi = read(static_cast<u16>(vector + 1));
    reg_.pc = bit::make_u16(lo, hi);
}
```

### BRK 是一个"假装是 2 字节"的 1 字节指令

```
$8000  00        BRK
$8001  EA        <- 这个字节被跳过，永远不会执行
```

BRK 执行后压入的是 `$8002`，不是 `$8001`：

```cpp
case Operation::BRK: {
    reg_.pc = static_cast<u16>(reg_.pc + 1u);   // 跳过后面那个字节
    enter_interrupt(kIrqVector, true);
    break;
}
```

**这个"签名字节"是 6502 的一个约定**：BRK 后面那个字节可以被调试器用来存放
断点编号。

### 可屏蔽性

```
NMI  永远被接受
IRQ  I = 1 时被忽略，I = 0 时被接受
```

```cpp
bool Cpu::interrupt_pending() const noexcept
{
    if (nmi_pending_) {
        return true;
    }
    return irq_line_ && !reg_.flag(Flag::IrqDisable);
}
```

`demo_instructions` 第 6 节：

```
  NMI with I=1:   PC -> $9000   (NMI ignores I)
  IRQ with I=1:   PC -> $8001   (masked, so the NOP at $8000 ran)
  IRQ with I=0:   PC -> $9000   (taken)
```

### 中断永远发生在指令之间

```cpp
int Cpu::step() noexcept
{
    if (halted_) return 0;

    // 在取下一条指令之前检查
    if (interrupt_pending()) { ... }

    // 取指 / 译码 / 寻址 / 执行
}
```

**这意味着每条指令都是原子的：读-改-写永远不会被中断切开。**

NES 游戏依赖这一点：用 `INC` 修改硬件寄存器时不需要关中断。

测试专门验证了它：

```cpp
TEST(InstructionSet, InterruptIsTakenBetweenInstructionsNotInsideOne)
```

---

## 9. 一个完整程序

`demo_instructions` 第 4 节：求 10 个字节的和。

```
    $8000    A2 00         LDX #$00         immediate
    $8002    A9 00         LDA #$00         immediate
    $8004    18            CLC              implied        <- loop
    $8005    7D 17 80      ADC $8017,X      absolute,X
    $8008    E8            INX              implied
    $8009    E0 0A         CPX #$0A         immediate
    $800B    D0 F7         BNE $8004        relative
    $800D    20 13 80      JSR $8013        absolute
    $8010    4C 10 80      JMP $8010        absolute       <- done
    $8013    8D 21 80      STA $8021        absolute       <- store
    $8016    60            RTS              implied

  Data at $8017: 1,2,3,4,5,6,7,8,9,10

  Result at $8021: $37  (55)
  Final state after 55 instructions:
    A  = $37
    X  = $0A
    P  = ..-..IZC
    SP = $FD   (back to its reset value: JSR and RTS balanced)
    cycles = 149
```

这个程序用到了：`ADC`、索引寻址、`CPX`+`BNE` 循环、`JSR`/`RTS`、绝对寻址写入。

**149 个周期**——精确数字，因为周期表是数据。

---

## 10. 代码对应

| 概念 | 文件 |
|------|------|
| 56 个 Operation 枚举 | `custom_nes_core/src/core/cpu/opcode.hpp` |
| ADC / SBC / 逻辑 / 栈 / 子程序 / 中断 | `custom_nes_core/src/core/cpu/cpu.cpp` |
| 加法器与 C/V | `custom_nes_core/src/core/alu.hpp` |
| 指令语义测试 | `custom_nes_core/tests/core/test_instruction_set.cpp` |
| 可运行讲解 | `tools/demo_instructions.cpp` |

```bash
./build/demo_instructions
./build/tests/fc_tests --gtest_filter='InstructionSet.*'
```

---

## 11. 自测

1. `ADC` 的完整公式是什么？为什么必须 `CLC`？
2. `SBC` 前为什么要 `SEC`？C 在 SBC 后表示什么？
3. `0x7F + 0x01` 的 C 和 V 分别是多少？
4. `BIT` 会影响 A 吗？它的 N 和 V 从哪里来？
5. `P` 的 bit 4（B）是真实标志吗？它有什么用？
6. `JSR` 压入的地址比"下一条指令"小 1，为什么？
7. `RTI` 和 `RTS` 有什么区别？
8. `BRK` 是 1 字节还是 2 字节？它压入的返回地址是什么？
9. 中断能否在一条指令执行到一半时发生？为什么这很重要？

<details>
<summary>答案</summary>

1. `A = A + M + C`。6502 没有不带进位的加法，所以必须先 `CLC`
2. 因为 SBC 计算 `A - M - (1 - C)`；C = 1 表示"无借位"。SBC 后的 C 表示结果是否借位（1 = 没借位）
3. C = 0（无符号 128 放得下），V = 1（有符号 +128 超出范围）
4. 不影响 A。N 来自操作数的 bit 7，V 来自 bit 6
5. 不是真实标志。它只在 P 被压栈时存在，用来区分 BRK（置位）和硬件中断（清零）
6. 因为 RTS 会加 1。两边的约定必须互补，否则返回地址会错
7. RTI 还会从栈上恢复状态寄存器 P，而且**不加 1**
8. 机器码 1 字节，但行为上像 2 字节：它跳过后面那个字节，压入 `PC + 2`
9. 不能。中断只在取指之前检查，所以读-改-写是原子的。NES 游戏依赖这一点来安全地修改硬件寄存器

</details>

---

**上一章：** [addressing-modes.md](addressing-modes.md) · **下一章：** [timing.md](timing.md)
