# 时序 Timing

> 目标：理解为什么模拟器的"周期数"不是性能指标，而是**正确性**指标。
>
> 本文输出都来自 `tools/demo_instructions.cpp` 第 7 节，
> 并可由 `custom_nes_core/tests/core/test_cycles.cpp` 验证。

---

## 1. 为什么周期数是正确性

NES 的 PPU 以 CPU 的**精确 3 倍**频率运行：

```
CPU   1.789773 MHz
PPU   5.369318 MHz
比例  1 : 3
```

PPU 一条扫描线 = 341 个 PPU 周期 ≈ 113.67 个 CPU 周期
一帧 = 262 条扫描线 ≈ 29780 个 CPU 周期 ≈ 60 Hz

**游戏靠数 CPU 周期来和光栅同步。** 这是"扫描线时机"技术：在中途改滚动寄存器、
改调色板、改精灵位置，就能做出分屏状态栏、波纹、视差等效果。

```
; 等到扫描线 30
wait: BIT $2002
      BPL wait
      ; 此时写寄存器就能影响第 30 行
```

如果模拟器的周期数不对，这一写就会落在错误的扫描线上——**画面撕裂、状态栏抖动**。

**所以周期错误 = 功能错误，不是性能问题。**

---

## 2. 时序是数据，不是算术

一个 opcode 的周期数**不是**从它的寻址模式推出来的——它是芯片设计时定死的，
记录在数据手册里。

```
custom_nes_core/src/core/cpu/opcode.cpp

constexpr u8 kCycleTable[256] = {
    // 0x0_   BRK  ORA  ---  ---  ---  ORA  ASL  ---  PHP  ORA  ASL  ---  ---  ORA  ASL  ---
              7,   6,   2,   2,   2,   3,   5,   2,   3,   2,   2,   2,   2,   4,   6,   2,
    ...
};
```

**256 个数字，逐项对照数据手册抄写。** 这是模拟器里最"没有技术含量"、却最容易抄错的部分。

### 为什么不做成公式

因为例外太多：

| 指令 | 模式 | 周期 | 公式算出来 |
|------|------|------|-----------|
| `JMP` | absolute | 3 | 4（模式基值） |
| `JMP` | indirect | 5 | 5 |
| `PHA` | implied | 3 | 2 |
| `PLA` | implied | 4 | 2 |
| `JSR` | absolute | 6 | 4 |
| `BRK` | implied | 7 | 2 |
| `ASL` | abs,X | 7 | 4+2=6 |
| `INC` | abs,X | 7 | 4+2=6 |

**规律存在，但例外足够多，公式反而更难维护。** 真实模拟器都用表。

---

## 3. 只有两件事随数据变化

表的数字包含了指令**永远**要付的成本：取指、取操作数、访存、读-改-写的写回。

只有两件事依赖具体数据：

```
1. 索引读取跨页                  +1
2. 分支被采用                    +1
   ...并且跨页                   +1（再多一个）
```

```cpp
int Cpu::cycle_cost(u8 opcode, const Operand& operand) const noexcept
{
    const OpcodeInfo& info = opcode_info(opcode);
    int cycles = opcode_cycles(opcode);

    if (pays_page_penalty(info.op, info.mode) && operand.page_crossed) {
        ++cycles;
    }
    cycles += branch_extra_cycles_;

    return cycles;
}
```

### 为什么写入不享受跨页惩罚

```
LDA $0200,X     4 周期；跨页时 5 周期
STA $0200,X     永远 5 周期
```

因为**读**跨页时，硬件先算出一个错误的高字节，发现后要在下一个周期修正。
而**写**发生在固定周期上，地址早就准备好了，没有"修正"这一步——它只是恒定地
比非索引版本慢一个周期。

```cpp
constexpr bool pays_page_penalty(Operation op, AddressingMode mode) noexcept
{
    if (mode != AbsoluteX && mode != AbsoluteY && mode != IndirectY) {
        return false;   // 只有这三种模式可能跨页
    }
    return !is_store_operation(op) && !is_read_modify_write(op);
}
```

读-改-写同理：它已经付出了固定的额外成本（写回），不再另算。

---

## 4. 分支的三个价格

```
BNE +2  ，未采用       2 周期
BNE +2  ，采用         3 周期
BNE +16 ，采用且跨页   4 周期
```

```cpp
void Cpu::branch(bool condition, const Operand& operand) noexcept
{
    if (!condition) {
        return;   // 未采用：什么都不做
    }

    const u16 next_instruction = reg_.pc;
    reg_.pc = operand.address;

    branch_extra_cycles_ = 1;
    if (bit::hi_byte(next_instruction) != bit::hi_byte(operand.address)) {
        branch_extra_cycles_ = 2;   // 跨页再多一个
    }
}
```

**"未采用也要 2 周期"**——因为 CPU 已经把操作数取出来了，才发现不用跳。

---

## 5. 实测

`demo_instructions` 第 7 节：

```
  instruction                    cycles
  -----------------------------  ------
  LDA #$42     immediate         2
  LDA $42      zero page         3
  LDA $02FF,X  crossing          5
  LDA $0200,X  not crossing      4
  LDA ($42),Y  crossing          6
  STA $0200,X  write             5
  ASL $0200    read-modify-write 6
  BNE taken    same page         3
  BNE taken    across page       4
  JSR $8004                      6
  BRK                            7
```

注意前两行的对比：

```
LDA #$42     2 周期     操作数就在指令流里，一次额外读取
LDA $42      3 周期     要额外读一次内存（zero page）
```

**"取数据"这件事本身要花时间。** 这就是 RISC 架构后来拼命想消灭的东西。

---

## 6. 完整程序的实际周期

`demo_instructions` 第 4 节的求和程序：**149 个周期**。

手工核对一部分：

```
LDX #$00        2
LDA #$00        2
------------------
循环体 × 10:
  CLC           2
  ADC $8017,X   4        (不跨页)
  INX           2
  CPX #$0A      2
  BNE $8004     3        (采用，同页)
  = 13 × 10 = 130
------------------
JSR $8013       6
STA $8021       4
RTS             6
------------------
总计  2 + 2 + 130 + 6 + 4 + 6 = 150
```

实测是 **149**。差 1。

为什么？因为**循环的最后一次 `BNE` 没有被采用**（`CPX #$0A` 使 Z=1，BNE 不跳），
所以最后那一次是 2 周期而不是 3 周期。

```
9 次采用  × 13 = 117
1 次不采用 × 12 =  12
                  129   ... 等等
```

让我重算循环体：

```
采用时   CLC(2) + ADC(4) + INX(2) + CPX(2) + BNE(3) = 13
不采用时 CLC(2) + ADC(4) + INX(2) + CPX(2) + BNE(2) = 12
```

9 × 13 + 1 × 12 = 117 + 12 = 129

总计 = 2 + 2 + 129 + 6 + 4 + 6 = **149** ✓

**对上了。** 这个差 1 正是"分支未采用"的那一个周期。

> 手算时漏掉它非常容易。这也是为什么周期表要**测**，不要靠推算。

---

## 7. 中断的成本

```
中断序列        7 周期
```

它不是指令，没有 opcode，所以不在表里：

```cpp
constexpr int kInterruptCycles = 7;
```

测试验证：

```cpp
TEST(CycleTable, InterruptCostsSevenCycles)
```

---

## 8. 我们怎么验证这张表

256 个数字不可能靠人眼全部核对。所以我们用**两种独立编码交叉验证**：

1. **数据表** `kCycleTable[256]` —— 逐项抄自数据手册
2. **推导模型** `derived_cycles(op, mode)` —— 从"操作类别 + 寻址模式"推算

```cpp
TEST(CycleTable, MatchesTheDerivedModelForEveryLegalOpcode)
{
    for (int i = 0; i < 256; ++i) {
        const OpcodeInfo& info = opcode_info(opcode);
        if (!info.is_legal()) continue;

        EXPECT_EQ(opcode_cycles(opcode), derived_cycles(info.op, info.mode));
    }
}
```

**两者一致并不证明它们符合硅片**（如果我的两条思路从一开始就错了，测试照样通过），
但它能抓住**抄写错误**——而抄写错误是 256 项表最现实的失败方式。

再加上：

- 具体的"头条数值"测试（`JMP abs = 3`、`ASL abs,X = 7` 等）
- 结构测试：ALU 组同一列必须同周期
- 端到端程序周期总和

### 已知的诚实边界

- 我们**没有**逐条对照过官方数据手册（离线环境）
- 未定义 opcode 的周期数写成 2，只是占位；它们永远不会被执行到
- 表里最大的官方周期是 7

**如果将来发现某个数字不对，最可能是抄写问题，而不是模型问题。**

---

## 9. 精度验收

AGENTS.md 把 "cycle accurate" 列为 Phase 1 的完成标准。当前的实现：

| 项目 | 状态 |
|------|------|
| 每个 opcode 的基准周期 | ✅ 数据表 |
| 跨页惩罚（索引读取） | ✅ |
| 分支采用 / 跨页惩罚 | ✅ |
| 写入不享受跨页惩罚 | ✅ |
| 读-改-写不加跨页惩罚 | ✅ |
| 中断 7 周期 | ✅ |
| 指令原子性（中断在指令间） | ✅ |
| **RMW 的"伪写"** | ❌ 真实 6502 的 RMW 会先写回旧值再写新值，会触发两次总线写 |
| **PPU 逐周期寄存器访问** | ❌ Phase 2 才需要 |
| **中断在指令的倒数第二个周期被采样** | ❌ 极端边界情况 |

前 6 项覆盖了游戏实际依赖的时序。后 3 项只有在做逐周期精确的 PPU 交互、
或者跑严苛的测试 ROM 时才会暴露。

**诚实的说法：当前实现是"指令级周期精确"，不是"总线级周期精确"。**

---

## 10. 代码对应

| 概念 | 文件 |
|------|------|
| 256 项周期数据表 | `custom_nes_core/src/core/cpu/opcode.cpp` `kCycleTable` |
| 跨页惩罚判定 | `custom_nes_core/src/core/cpu/opcode.hpp` `pays_page_penalty()` |
| 周期累加 | `custom_nes_core/src/core/cpu/cpu.cpp` `cycle_cost()` |
| 分支惩罚 | `custom_nes_core/src/core/cpu/cpu.cpp` `branch()` |
| 测试 | `custom_nes_core/tests/core/test_cycles.cpp` |
| 可运行讲解 | `tools/demo_instructions.cpp` 第 7 节 |

```bash
./build/demo_instructions
./build/tests/fc_tests --gtest_filter='CycleTable.*'
```

---

## 11. 自测

1. `LDA $0200,X` 在跨页和不跨页时分别是几个周期？
2. `STA $0200,X` 跨页时是几个周期？为什么和 `LDA` 不一样？
3. 一个未被采用的分支花几个周期？
4. 中断序列花几个周期？
5. 为什么周期表要写成 256 项数据，而不是用公式推算？
6. 一个循环跑 10 次，每次有 1 条 `BNE`，其中 9 次被采用。这 10 条 `BNE` 总共花多少周期？

<details>
<summary>答案</summary>

1. 不跨页 4，跨页 5
2. 永远 5。写入发生在固定周期，不需要"修正高字节"这一步
3. 2
4. 7
5. 因为例外太多（`JMP abs = 3`、`PHA = 3`、`ASL abs,X = 7`），公式会比表更难维护、更容易错
6. 9 × 3 + 1 × 2 = 29 周期

</details>

---

**上一章：** [instruction-set.md](instruction-set.md)
