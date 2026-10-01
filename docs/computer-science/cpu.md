# CPU 基础 CPU Fundamentals

> 目标：理解 CPU 是什么、它内部有什么、以及它如何一条接一条地执行指令。
> 本文所有输出都来自 `tools/demo_cpu.cpp`，并可由 `custom_nes_core/tests/core/test_cpu.cpp` 验证。

---

## 1. CPU 是一个状态机

剥掉所有细节，CPU 就是一个循环：

```
        ┌──────────────────────────────┐
        │                              │
        v                              │
     fetch   读 PC 指向的字节，PC += 1  │
        │                              │
        v                              │
     decode  这个字节决定执行哪条指令    │
        │                              │
        v                              │
     execute 干活（可能还要再取操作数）  │
        │                              │
        v                              │
     update state  更新标志位/寄存器/周期 │
        │                              │
        └──────────────────────────────┘
```

**6502 一共只有 6 个寄存器，总共不到 4 个字节。** 这就是整台机器的"心智"。

---

## 2. 六个寄存器

| 寄存器 | 宽度 | 作用 |
|--------|------|------|
| **A** — Accumulator | 8 bit | 累加器。**ALU 唯一能"加进去"的寄存器** |
| **X** — Index X | 8 bit | 索引 / 计数器 |
| **Y** — Index Y | 8 bit | 索引 / 计数器 |
| **SP** — Stack Pointer | 8 bit | 栈顶指针（只是地址的低字节） |
| **PC** — Program Counter | 16 bit | **下一条**要执行的指令的地址 |
| **P** — Status | 8 bit | 八个独立的 1 bit 标志 |

### 为什么只有 6 个？

因为 1975 年晶体管很贵。寄存器在芯片内部，每个 bit 都要占用面积。
RAM 很便宜（可以外挂），寄存器很贵。

**这也解释了 6502 的设计风格：几乎所有运算都必须经过 A。**

```
LDA #$10   ; 把 0x10 放进 A
ADC #$20   ; A = A + 0x20
STA $0300  ; 把 A 写到内存 0x0300
```

没有 "memory[m1] + memory[m2] -> memory[m3]" 这种指令。
**所有数据都要流过累加器。** 这就是为什么它叫"累加器"。

---

## 3. 状态寄存器 P

```
bit:  7    6    5    4    3    2    1    0
      N    V    -    B    D    I    Z    C
```

| 位 | 名称 | 含义 |
|----|------|------|
| 0 | **C** Carry | 无符号溢出 / SBC 后表示"没有借位" |
| 1 | **Z** Zero | 上一条结果是否为 0 |
| 2 | **I** IRQ Disable | 1 = 屏蔽 IRQ 中断 |
| 3 | **D** Decimal | 十进制模式。**NES 上无效**（2A03 删掉了） |
| 4 | **B** Break | 表示这次入栈的 P 来自 BRK/PHP |
| 5 | — Unused | 永远读作 1 |
| 6 | **V** Overflow | 有符号溢出，见 [overflow-flag.md](overflow-flag.md) |
| 7 | **N** Negative | 上一条结果的 bit 7 |

**P 不是"一个数"，它是八个独立的开关。** 读一个 flag 就是 `bit::test`，
写一个 flag 就是 `bit::assign` —— 见 [bitwise-operations.md](bitwise-operations.md)。

### 复位后的值

```
P  = 0x24 = 0010 0100   -> I = 1（屏蔽中断），未用位 = 1
SP = 0xFD
```

为什么 SP 是 `0xFD` 而不是 `0xFF`？因为复位相当于一次"中断"，
硬件会假装把 3 个字节压栈（PC 的高低字节 + P），把 SP 从 `0xFF` 减到 `0xFD`，
**但并不真的写入内存**。

`demo_cpu` 的 reset 行输出：

```
     0  0x8000  (reset)      0x00  0x00  0x00  0xFD  ..-..I..       0
```

`..-..I..` 是 P 的可视化：N、V 未置位，未用位显示为 `-`，I 置位。

---

## 4. 指令、Opcode、Operand

| 术语 | 含义 |
|------|------|
| **Instruction** | CPU 能做的一个动作，如"加载累加器" |
| **Mnemonic** | 这个动作的助记符，如 `LDA` |
| **Opcode** | 这个动作的机器码，如 `0xA9` |
| **Operand** | 操作数，这条指令要用的数据或地址 |

**一个 opcode 是 8 bit，所以最多 256 种指令。** 6502 实际使用了全部 256 个编码。

### 同一个助记符，多个 opcode

`LDA` 有 8 种寻址模式，所以有 8 个 opcode：

```
0xA9  LDA #$42     immediate    操作数就是 0x42
0xA5  LDA $42      zero page    从地址 0x0042 读
0xB5  LDA $42,X    zero page,X
0xAD  LDA $8000    absolute     从地址 0x8000 读
0xBD  LDA $8000,X
0xB9  LDA $8000,Y
0xA1  LDA ($42,X)  indirect
0xB1  LDA ($42),Y
```

**opcode 编码了"动作"和"寻址方式"两个信息。** 这是 Phase 0.4 的主题。

---

## 5. 取指 / 译码 / 执行：逐步展开

`demo_cpu` 第 3 节把 `LDA #$42` 拆成了六步：

```
  1. PC = 0x8000
  2. fetch byte at PC -> 0xA9, PC becomes 0x8001
  3. decode: 0xA9 表示'把后面的那个字节装进累加器'
  4. fetch byte at PC -> 0x42, PC becomes 0x8002
  5. A = 0x42
  6. update flags: N = bit 7 = 0, Z = (A == 0) = 0
  7. cycles += 2
```

对应代码（`custom_nes_core/src/core/cpu/cpu.cpp`）：

```cpp
u8 Cpu::fetch_byte() noexcept
{
    return read(reg_.pc++);   // 读 PC 处的字节，然后 PC 前进
}

case 0xA9: {                  // LDA #imm
    const u8 value = fetch_byte();
    reg_.a = value;
    reg_.update_nz(reg_.a);   // N = bit7, Z = (value == 0)
    cycles_ += 2;
    break;
}
```

### 关键细节：PC 在 fetch 之后就已经前进了

```
执行前:  PC = 0x8000
fetch:   读到 0xA9，PC 变成 0x8001
```

**这不是"先执行再移动 PC"，而是"每读一个字节 PC 就 +1"。**
地址 `0x8000` 处的指令，执行完 PC 会指向 `0x8002`（因为 2 字节）。

测试验证：

```cpp
TEST(Cpu, PcAdvancesPastTheFetchedBytes)   // A9 42 -> PC 0x8000 -> 0x8002
```

### 为什么 PC 会自动回绕

```cpp
return read(reg_.pc++);   // reg_.pc 是 u16
```

`u16` 从 `0xFFFF` 加 1 会自然变成 `0x0000`，**不需要任何特殊处理**。
它精确对应真实的 16 根地址线：`0xFFFF` 再加 1，地址线全部归零。

**这是"用类型表达硬件"的例子。** 如果 PC 是 `int`，就要手写回绕逻辑，还可能忘。

---

## 6. 完整的执行轨迹

`demo_cpu` 第 2 节的真实输出：

```
  step  PC      instruction  A     X     Y     SP    P         cycles
  ----  ------  -----------  ----  ----  ----  ----  --------  ------
     0  0x8000  (reset)      0x00  0x00  0x00  0xFD  ..-..I..       0
     1  0x8000  LDA #0x42    0x42  0x00  0x00  0xFD  ..-..I..       2
     2  0x8002  TAX          0x42  0x42  0x00  0xFD  ..-..I..       4
     3  0x8003  TAY          0x42  0x42  0x42  0xFD  ..-..I..       6
     4  0x8004  INX          0x42  0x43  0x42  0xFD  ..-..I..       8
     5  0x8005  INY          0x42  0x43  0x43  0xFD  ..-..I..      10
     6  0x8006  LDA #0x00    0x00  0x43  0x43  0xFD  ..-..IZ.      12
     7  0x8008  NOP          0x00  0x43  0x43  0xFD  ..-..IZ.      14
     8  0x8009  ???          0x00  0x43  0x43  0xFD  ..-..IZ.      14
```

逐行读：

- **step 1**：`A` 从 `0x00` 变成 `0x42`。这是唯一改变 A 的一步。
- **step 2-3**：`TAX`、`TAY` 把 A 复制到 X、Y。**注意 A 没变** —— 传输不会消耗源。
- **step 4-5**：`INX`、`INY` 各加 1。`Y` 是 `0x42` 加 1 得 `0x43`。
- **step 6**：`A = 0x00`，P 变成 `..-..IZ.` —— **Z 标志亮了**。这是 `update_nz` 干的。
- **step 7**：`NOP` 什么都不做，但 PC 前进 1，周期 +2。
- **step 8**：PC 跑到 `0x8009`（程序末尾之后），那里是 `0x00`，也就是 `BRK`。
  Phase 0.2 还没实现它 → **CPU 停机并报告**。

### 为什么程序末尾之后要"停机"而不是继续跑

内存里 `0x00` 到处都是。如果模拟器对未实现的 opcode 什么都不做，程序会
**静默地一条条空转**，看起来"在跑"，实际全错。

所以我们让它**大声失败**：

```cpp
default:
    halt(opcode);   // halted_ = true; unimplemented_opcode_ = opcode;
    break;
```

测试验证：

```cpp
TEST(Cpu, StopsAtAnUnimplementedOpcode)
```

---

## 7. 栈：SP 和 page 1

6502 的栈有一个硬性限制：**它只能在 `0x0100`–`0x01FF` 这 256 字节里。**

```
SP 只有 8 bit，所以栈顶地址 = 0x0100 | SP
                                 ^^^^^^ 高字节固定
```

```cpp
void Cpu::push(u8 value) noexcept
{
    write(kStackPage | reg_.sp, value);   // 写到 0x0100 | SP
    reg_.sp = static_cast<u8>(reg_.sp - 1u);  // 然后 SP 减 1
}
```

### 两个容易搞错的地方

**1. 栈是向下生长的。**

```
push 0x11 -> 写到 0x01FD, SP = 0xFC
push 0x22 -> 写到 0x01FC, SP = 0xFB
pop       -> SP = 0xFC, 读到 0x01FC = 0x22   (后进先出)
```

**2. `|` 而不是 `+`。**

`0x0100 | SP` 和 `0x0100 + SP` 在数学上结果相同，但 `|` 表达了真实意图：
**高字节固定是 `0x01`，SP 只提供低字节。** 这直接对应硬件上"8 根线接到地址总线的低 8 位，高 8 位接地线接到 `$01`"。

### 为什么栈只有 256 字节

因为 SP 只有 8 bit。**这就是"寄存器宽度决定系统能力"的例子。**
如果 SP 是 16 bit，栈就能放在内存任何地方 —— 后来的 CPU（如 65816）正是这么做的。

测试验证：

```cpp
TEST(Cpu, PushWritesToPageOneAndMovesSpDown)
TEST(Cpu, StackPointerCyclesThroughEveryValueAndNeverLeavesPageOne)
```

---

## 8. Reset 向量：CPU 不知道什么是"程序"

CPU 上电后不做任何假设。它只执行一个动作：

```
从 $FFFC 读一个字节（低字节）
从 $FFFD 读一个字节（高字节）
拼成 16 位地址
跳过去
```

```cpp
const u8 lo = read(0xFFFC);
const u8 hi = read(0xFFFD);
reg_.pc = bit::make_u16(lo, hi);
```

### 这意味着什么

- **卡带决定程序从哪里开始。** CPU 不知道，也不需要知道。
- **同一个 CPU 可以启动不同的机器**，只要 $FFFC 接的东西不同。
- **小端序不是可选项。** 如果读反了，得到 `0x3412` 而不是 `0x1234`，
  程序会跳到完全错误的地方。测试专门检查了这一点：

```cpp
TEST(Cpu, ResetVectorIsLittleEndian)
```

### 三个中断向量

| 地址 | 用途 |
|------|------|
| `$FFFA`–`$FFFB` | **NMI** — 每帧结束时 PPU 触发（游戏的主循环节拍） |
| `$FFFC`–`$FFFD` | **RESET** — 上电 / 复位 |
| `$FFFE`–`$FFFF` | **IRQ/BRK** |

Phase 1 会实现这三个。

---

## 9. Bus：为什么 CPU 不能直接访问内存

这是 AGENTS.md 里的硬性架构规则：

```
错误                          正确
CPU                           CPU
 |                             |
memory[]                       Bus
 |                             |
PPU                           PPU / RAM / Cartridge
```

### 代码长什么样

```cpp
class Bus {                                  // custom_nes_core/src/core/bus.hpp
public:
    virtual ~Bus() = default;
    virtual u8 read(u16 address) = 0;
    virtual void write(u16 address, u8 value) = 0;
};

class Cpu {
    explicit Cpu(Bus& bus) : bus_(&bus) {}
    u8 read(u16 address) { return bus_->read(address); }
};
```

### 收益

1. **CPU 可以独立测试。** `custom_nes_core/tests/core/test_cpu.cpp` 用一个 64KB 的 `FlatBus`
   就能跑完整测试，不需要 RAM 映射、PPU、卡带。
2. **换一台机器只需换 Bus。** 同样的 CPU 加上不同的解码逻辑就是别的系统。
3. **真实的行为被正确建模。** 读 PPU 寄存器 `$2002` 会**清除** vblank 标志 ——
   这是个副作用。所以 `read()` 特意不是 `const` 的。

### 注意：FlatBus 不是 NES 的 Bus

`FlatBus` 是 64KB 平铺数组，只为了满足"CPU 需要一个读/写接口"。
真正的 NES 内存映射（RAM 镜像、PPU 寄存器、卡带 ROM）是 **Phase 2**。

---

## 10. 时钟周期：还没到，但已经开始了

每条指令都记了自己的周期数：

```
LDA #imm   2 cycles
TAX        2 cycles
INX        2 cycles
NOP        2 cycles
```

`demo_cpu` 的 cycles 列在 step 7 时是 14 = 7 条指令 × 2。

**注意：这些数字对这几条指令是正确的，但整个 CPU 还不是 cycle accurate。**
`total_cycles()` 目前只是一个累加器。真正的周期级精确（比如 cross-page 惩罚、
read-modify-write 的额外周期）是 **Phase 1** 的事。

> 不要假装精确。`cycles_` 现在的作用是让"周期"这个概念从第一天起就存在。

---

## 11. 代码对应

| 概念 | 文件 |
|------|------|
| 六个寄存器 + flag 读写 | `custom_nes_core/src/core/cpu/registers.hpp` |
| Bus 抽象 | `custom_nes_core/src/core/bus.hpp` |
| 测试用平铺内存 | `custom_nes_core/src/core/flat_bus.hpp` |
| 取指/译码/执行/栈/复位 | `custom_nes_core/src/core/cpu/cpu.hpp` `custom_nes_core/src/core/cpu/cpu.cpp` |
| 单元测试 | `custom_nes_core/tests/core/test_registers.cpp` `custom_nes_core/tests/core/test_cpu.cpp` |
| 可运行讲解 | `tools/demo_cpu.cpp` |

```bash
./build/demo_cpu
./build/tests/fc_tests --gtest_filter='Cpu.*:Registers.*'
```

---

## 12. 自测

1. 6502 有几个寄存器？各多少位？总共多少字节？
2. PC 保存的是"当前指令"还是"下一条指令"的地址？
3. `A9 42` 执行完后 PC 是多少（假设起始 PC = `0x8000`）？
4. `P = 0x24` 表示哪几个标志置位？
5. 栈顶地址怎么由 SP 算出？为什么用 `|` 而不是 `+`？
6. 为什么 `push` 之后 SP 是减小？
7. 为什么 `Bus::read()` 不是 `const` 的？
8. 为什么未实现的 opcode 要让 CPU 停机？

<details>
<summary>答案</summary>

1. 六个：A/X/Y/SP/P（各 8 bit）+ PC（16 bit）= 6 字节
2. 下一条。fetch 会把 PC 加 1
3. `0x8002`（opcode 1 字节 + operand 1 字节）
4. `0x24 = 0010 0100` → 未用位（bit 5）和 I（bit 2）置位
5. `0x0100 | SP`。高字节固定 `$01`，SP 只提供低字节，`|` 表达了这个硬件事实
6. 因为栈向下生长，`$01FF` 是栈底，往 `$0100` 方向增长
7. 因为真实硬件的读取可能有副作用（如读 `$2002` 清除 vblank 标志）
8. 否则模拟器会静默空转，看起来在运行实际全错，bug 极难定位

</details>

---

**上一章：** [overflow-flag.md](overflow-flag.md) · **下一章：** assembly.md（Phase 0.3 待写）
