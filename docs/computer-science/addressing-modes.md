# 寻址模式 Addressing Modes

> 目标：理解寻址模式不是"数据在哪"，而是"**如何计算出**数据在哪"。
> 计算出来的结果叫**有效地址**（effective address）。
>
> 本文输出都来自 `tools/demo_addressing.cpp`，并可由 `custom_nes_core/tests/core/test_addressing.cpp` 验证。

---

## 1. 从"配方"到地址

```
LDA $0200,X       with X = 5

  mode      = absolute,X      <- 配方
  operand   = $0200           <- 基址，来自字节流
  index     = 5               <- 来自寄存器 X
  --------------------------------
  effective = $0200 + 5 = $0205

  现在 CPU 才真正去读 [$0205]
```

**寻址模式是一个计算过程，不是一个位置。** 13 种模式就是 13 种不同的配方。

Phase 0.3 我们做到"知道 `0xBD` 是 `LDA $0200,X`"，
Phase 0.4 做到"**算出 `$0205` 并读到里面的字节**"。

---

## 2. 13 种配方的完整表

`demo_addressing` 第 2 节的真实输出（X = 2, Y = 5）：

```
  syntax       mode          result                       note
  ------------------------------------------------------------
  TAX          implied       (no operand)                 no operand
  ASL A        accumulator   (no operand)                 operand is A
  LDA #$42     immediate     value = $42                  the byte itself
  LDA $42      zero page     address = $0042              high byte is 0
  LDA $F0,X    zero page,X   address = $0010              wraps in page 0
  LDX $10,Y    zero page,Y   address = $0015              wraps in page 0
  LDA $1234    absolute      address = $1234              full 16 bits
  LDA $0200,X  absolute,X    address = $0205              base + X
  LDA $0200,Y  absolute,Y    address = $0207              base + Y
  JMP ($3000)  indirect      address = $9000              pointer at $3000
  LDA ($40,X)  indirect,X    address = $0400              index pointer
  LDA ($40),Y  indirect,Y    address = $0205              index result
  BNE $8007    relative      target = $8007               signed offset
```

### 结果有三种类型

```cpp
enum class OperandKind {
    None,      // implied / accumulator：没有操作数可取
    Value,     // immediate：值本身就是数据
    Address,   // 内存模式：地址处才是数据
    Target,    // relative：这是跳转目标，不是数据地址
};
```

**为什么必须区分 `Address` 和 `Target`？**

因为对分支来说，"跳过去"和"读那里"是截然不同的动作：

```cpp
if (operand.is_address()) cpu.write(operand.address, value);  // STA
if (operand.is_target())  cpu.pc = operand.address;           // BNE
```

如果都用 `Address`，写 STA 处理程序时就会不小心把分支目标当成写入地址。

---

## 3. Zero Page：一个压缩技巧，和它的回绕

### 为什么存在

```
LDA $42       A5 42        2 字节   3 周期
LDA $0042     AD 42 00     3 字节   4 周期
```

**但 `LDA $0042` 这种写法并不存在**——没有"高字节为 0 的 absolute"编码。

Zero Page 模式把 1 字节操作数**硬接线**到地址总线的高字节上：

```
        operand byte (8 根线)
              |
              v
    +-------------------+
    | 0 0 0 0 0 0 0 0 0 |  operand   [低 8 位]
    | 0 0 0 0 0 0 0 0 0 |            [高 8 位，硬接线为 0]
    +-------------------+
              |
              v
        地址总线 16 位
```

于是省下 1 字节，也快 1 个周期。**RAM 只有 2KB，省字节很值钱。**

### 代价：加上索引后会回绕

既然高字节永远从 0 开始、且不能产生进位，那么 `$42 + $C0 = $0102` 的进位就**丢掉了**：

```
  base   index   naive sum  actual     note
  -----------------------------------------
  $42    $01     $0043      $0043      no wrap
  $42    $C0     $0102      $0002      carry thrown away
  $F0    $20     $0110      $0010      carry thrown away
  $FF    $01     $0100      $0000      carry thrown away
  $FF    $FF     $01FE      $00FE      carry thrown away
```

代码上就是**把结果截断成 8 位**：

```cpp
case AddressingMode::ZeroPageX:
    // 强转成 u8 会丢掉进位，这正是硬件干的事
    return address_operand(static_cast<u8>(request.operand_lo + request.x));
```

### 为什么这是"正确行为"而不是 bug

因为高字节不是"碰巧是 0"，而是**根本没有对应的线**。少一根线，就没有进位通道。

**游戏代码依赖这一点**，所以模拟器必须精确复现。测试穷举了全部 65536 种组合：

```cpp
TEST(Addressing, ZeroPageIndexedNeverCrossesAPage)   // 256 × 256 全部验证
```

> 这是"约束来自硬件，不是来自实现"的典型例子。
> 如果你写成 `u16(base) + index`，模拟器会在某些游戏里静默地读错地址。

---

## 4. 间接寻址：JMP 的硬件 bug

### 正常的间接

```
JMP ($3000)

  [$3000] = $00     <- 低字节
  [$3001] = $90     <- 高字节
  ---------------
  目标 = $9000
```

这是一个**指针**：操作数给出的不是目标，而是"目标存在哪里"。

用途是跳转表（状态机、中断向量分发）：

```
      LDA index
      ASL A              ; 每条目 2 字节
      TAX
      LDA table,X
      STA $10
      LDA table+1,X
      STA $11
      JMP ($0010)        ; 跳到选中的处理程序
```

### bug：高字节从错误的页取

```
JMP ($10FF)

  [$10FF] = $34     低字节
  [$1100] = $99     你"以为"的高字节
  [$1000] = $12     芯片实际读的高字节
  ---------------
  目标 = $1234，不是 $9934
```

**指针的高字节是从与低字节同一个 256 字节页里读的。**

```
   地址 $10FF
   ├─ 页号  = $10FF & $FF00 = $1000
   └─ 页内偏移 = $10FF & $00FF = $00FF
        +1 后再截断到页内: ($00FF + 1) & $00FF = $00
        -> 高字节地址 = $1000 | $00 = $1000
```

实现：

```cpp
u16 read_pointer_indirect(Bus& bus, u16 address) noexcept
{
    const u8 low = bus.read(address);
    const u16 high_address = static_cast<u16>((address & 0xFF00) |
                                              static_cast<u16>((address + 1u) & 0x00FF));
    const u8 high = bus.read(high_address);
    return bit::make_u16(low, high);
}
```

### 这是真的硬件 bug

- 1975 年的 NMOS 6502 就是这样连线的
- 65C02 修正了它（还原成"正常"行为）
- **但 NES 用的是有 bug 的 NMOS 版本**，而且这个 bug 罕见地被某些程序依赖

**所以我们不能"修正"它。** 模拟器的目标是复现硬件，不是改进硬件。

> 对比一下真正的软件 bug：这一条属于"规格"，必须实现；
> 而我们在 `cpu.cpp` 里遇到未实现 opcode 就停机，那才是防止我们自己的 bug。

---

## 5. `(zp,X)` 与 `(zp),Y`：求值顺序

```
  LDA ($42,X)   with X = 2
    1. 先索引指针:   $42 + X = $0044
    2. 读指针:       [$0044] = $0200
    3. 有效地址    = $0200

  LDA ($44),Y   with Y = 2
    1. 先读指针:     [$0044] = $0200
    2. 再索引结果:   $0200 + Y = $0202
```

**逗号在括号内 = 先索引指针；逗号在括号外 = 后索引结果。**

```
($42,X)     括号内 -> 先加
($42),Y     括号外 -> 后加
```

### 为什么 X 只在第一种、Y 只在第二种

因为 **6502 根本没有 `($42),X` 和 `($42,Y)` 指令**。

这不是遗漏，而是编码空间的取舍：

```
($42,X)  : 1 字节操作数，X 加在页内（会回绕），然后读 16 位指针
($42),Y  : 1 字节操作数，读 16 位指针，然后 Y 加在 16 位上（可跨页）
```

### 为什么 `(zp,X)` 的索引要回绕

指针存在 page 0，所以 `$42 + X` 也会**在页内回绕**：

```
($FF,X) with X = 0   -> 指针槽 $FF
                        低字节从 $00FF 读
                        高字节从 $0000 读   <- 回绕！
```

```cpp
case AddressingMode::IndirectX: {
    const u8 pointer = static_cast<u8>(request.operand_lo + request.x);  // 页内
    return address_operand(read_pointer_zero_page(bus, pointer));
}
```

而 `read_pointer_zero_page` 里的 `static_cast<u8>(address + 1)` 又再回绕一次。

**这是同一个约束（高字节硬接线为 0）的第二次体现。**

---

## 6. 页边界与周期代价

### 什么算"跨页"

地址的高字节变了，就叫跨页：

```
  base     index   effective   page crossed
  --------------------------------------------
  $0200    $05     $0205       no
  $02FF    $01     $0300       yes
  $0200    $FF     $02FF       no
  $FFFF    $02     $0001       yes     <- 整个地址空间回绕
```

```cpp
const bool page_crossed = bit::hi_byte(base) != bit::hi_byte(address);
```

### 为什么要关心

**因为真实的 6502 会因此多花一个周期。**

这不是性能细节，而是**时序**：PPU 以 CPU 的 3 倍频率运行，游戏靠精确计数
CPU 周期来和光栅同步（这就是"扫描线时机"技术）。周期错了，画面就会撕裂。

```
LDA $0200,X     4 周期，跨页时 5 周期
STA $0200,X     5 周期，永远 5 周期（写入不享受跨页优惠）
```

**所以有效地址解析必须同时报告 `page_crossed`，即使当前不精确计时。**

> `demo_addressing` 第 6 节和 `custom_nes_core/tests/core/test_addressing.cpp` 都验证了这一点。
> 完整周期精确是 Phase 1。

---

## 7. 相对寻址：分支的配方

```
target = 指令地址 + 2 + (有符号)操作数
```

这是唯一一个**不产生数据地址、只产生跳转目标**的模式。两个来源：

1. **`+2`**：分支指令本身 2 字节，执行时 PC 已经越过操作数
2. **有符号**：偏移是补码，`0xFB` = -5

```cpp
case AddressingMode::Relative: {
    const int offset = static_cast<int>(bit::as_signed(request.operand_lo));
    const int next_instruction = static_cast<int>(request.instruction_pc) + 2;

    Operand out{};
    out.kind = OperandKind::Target;   // 注意是 Target，不是 Address
    out.address = static_cast<u16>(next_instruction + offset);
    return out;
}
```

见 [twos-complement.md](twos-complement.md) 第 8.2 节和 [assembly.md](assembly.md) 第 7 节。

---

## 8. 这一步带来的架构变化

### 之前：每个 (operation, mode) 组合一个 case

```cpp
switch (opcode) {
case 0xA9: /* LDA #imm */ ...
case 0xA5: /* LDA zp   */ ...
case 0xAD: /* LDA abs  */ ...
case 0xBD: /* LDA abs,X*/ ...
// ... 151 个 case
}
```

### 之后：每个 operation 一个 case

```cpp
switch (info.op) {              // 只有 56 个 case
case Operation::LDA:
    reg_.a = operand_value(info, operand);   // 自动支持全部 8 种寻址模式
    reg_.update_nz(reg_.a);
    break;
}
```

**寻址模式在进入 `execute()` 之前就已经解析完了。**

```
step():
  1. fetch opcode
  2. 查表 -> (operation, mode)
  3. 按 mode 取操作数字节
  4. resolve() -> Operand（有效地址 / 立即值 / 跳转目标）
  5. execute(operation, operand)     <- 这里完全不关心模式
```

### 收益

| | Phase 0.3 | Phase 0.4 |
|---|---|---|
| `execute()` 的 case 数 | 151（实际只写了 12） | 56（写了 42） |
| `implements()` 的实现 | 手抄 12 个 opcode | `is_legal() && handles(op)` |
| 新增一个寻址模式 | 要改所有指令 | 只改 `resolve()` |
| 已实现 opcode 数 | 12 | **101** |

**这就是抽象的价值：把变化隔离在一个地方。**

---

## 9. 一个真实的循环

`demo_addressing` 第 7 节运行的程序：

```
    $8000   LDX #$00          ; immediate
    $8002   LDA $0300,X       ; absolute,X    <- 循环入口
    $8005   STA $0200,X       ; absolute,X
    $8008   INX               ; implied
    $8009   CPX #$04          ; immediate
    $800B   BNE $8002         ; relative      <- 跳回 $8002
```

运行前：

```
  Source array $0300: $11 $22 $33 $44
  Destination $0200: $00 $00 $00 $00
```

运行后：

```
  Ran 22 instructions then stopped at $00 (BRK)
  Destination $0200: $11 $22 $33 $44   <- copied
  Final X = $04,  P = ..-..IZ.  (Z set because CPX #$04 made them equal)
```

**这 22 条指令里用到了：**
- `immediate`（两次）
- `absolute,X`（两次，读和写各一次）
- `implied`（一次）
- `relative`（一次，而且带了负偏移）

**这是 CPU 第一次真正跑完一个带循环的算法。** 之前的所有 demo 都只是直线执行。

---

## 10. 代码对应

| 概念 | 文件 |
|------|------|
| 有效地址解析、两个硬件怪癖 | `custom_nes_core/src/core/cpu/addressing.{hpp,cpp}` |
| Operation / AddressingMode 枚举 | `custom_nes_core/src/core/cpu/opcode.hpp` |
| 表驱动派发 | `custom_nes_core/src/core/cpu/cpu.cpp` |
| 单元测试（13 种模式 + 回绕 + 跨页） | `custom_nes_core/tests/core/test_addressing.cpp` |
| 可运行讲解 | `tools/demo_addressing.cpp` |

```bash
./build/demo_addressing
./build/tests/fc_tests --gtest_filter='Addressing.*:AddressingCpu.*'
```

---

## 11. 自测

1. `LDA $F0,X` 在 `X = $20` 时读哪个地址？为什么不是 `$0110`？
2. `JMP ($10FF)` 的高字节从哪里读？
3. `LDA ($42,X)` 和 `LDA ($42),Y` 的求值顺序有什么不同？
4. 为什么 `($42),X` 不存在？
5. 什么情况算"跨页"？为什么写操作不享受跨页的那一个周期？
6. `LDA $02FF,X` 在 `X = 1` 时读哪里？跨页吗？
7. 为什么 `resolve()` 要区分 `Address` 和 `Target`？
8. 表驱动派发把 `execute()` 的 case 数从 151 降到 56，代价是什么？

<details>
<summary>答案</summary>

1. `$0010`。高字节硬接线为 0，`$F0 + $20 = $0110` 的进位被丢弃
2. 从 `$1000`——与低字节同一个 256 字节页。这是 6502 的硬件 bug
3. `($42,X)` 先把 X 加到指针上再解引用；`($42),Y` 先解引用再把 Y 加到结果上
4. 因为 6502 的编码空间里没有这条指令，`($42),Y` 才是它的对应形式
5. 高字节发生变化就算跨页。写操作在固定周期发生，不需要"修正"高字节，所以不享受那一个周期的差异（但索引写本身固定慢一个周期）
6. `$0300`，跨页（高字节从 `$02` 变成 `$03`）
7. 因为对分支来说是"跳到那里"而不是"读写那里"。混用会让 STA 的处理程序把分支目标当成写入地址
8. 代价是不再有一张"每个 opcode 的精确周期表"。周期需要从模式推算，Phase 1 会补上真正的周期表

</details>

---

**上一章：** [assembly.md](assembly.md) · **下一章：** Phase 1 — 完整指令集与周期精确
