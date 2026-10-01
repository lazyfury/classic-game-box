# V 标志：有符号溢出 Overflow Flag

> 目标：彻底理解 `V`。这是 6502 中最容易被写错、又最难调试的标志位。
> **本文所有结论都由 `tools/demo_overflow.cpp` 和 `custom_nes_core/tests/core/test_alu.cpp` 验证过。**

---

## 1. 破除误解：有两种"溢出"

大多数人把 V 叫做"溢出标志"，然后就去用 carry 判断它。**这是错的。**

同一个加法，有两个完全不同的问题：

| 问题 | 标志 | 取值范围 |
|------|------|---------|
| 按**无符号**读，放得下吗？ | **C** | `0 .. 255` |
| 按**有符号**读，放得下吗？ | **V** | `-128 .. 127` |

### 实测（`demo_overflow` 第 1 节）

```
  a     b      result   C     V    a+b (signed)
  -------------------------------------------------
  0x10  0x20   0x30     0     0       16 +   32
  0x7F  0x01   0x80     0     1      127 +    1
  0xFF  0x01   0x00     1     0       -1 +    1
  0x80  0x80   0x00     1     1     -128 + -128
```

**第二行是关键：`0x7F + 0x01`，C = 0 但 V = 1。**

```
  0111 1111   (+127)
+ 0000 0001   (+1)
-----------
  1000 0000
```

- bit 7 没有向外进位 → **C = 0**（无符号 127+1=128，放得下）
- 但结果按有符号是 **-128**，`+127 + 1` 竟然变成负数 → **V = 1**

**如果用 C 判断"有符号运算是否出错"，这里就会漏报，程序逻辑从此错误。**

---

## 2. 为什么不能只看 bit 7 的进位

因为 **bit 7 有双重身份**：

```
无符号视角：bit 7 的权重是 +128
有符号视角：bit 7 是符号位，权重是 -128
```

加法器本身**不知道**你在用哪种视角。它只是 8 个全加器串起来，每个产生一个进位信号。

硬件因此引入了第二个信号：**bit 7 的"进位进"和"进位出"**。

```
      bit7   bit6  bit5 ... bit0
        |      |
        |      +---- carry_into_bit7    (bit6 产生的进位)
        |
        +----------- carry_out_of_bit7  (= C)

   V  =  carry_into_bit7  XOR  carry_out_of_bit7
```

### 为什么是 XOR

符号位做的事就是 `a7 + b7 + carry_in`：

| carry_in | carry_out | 发生了什么 | V |
|----------|-----------|-----------|---|
| 0 | 0 | 符号位没被影响，符号正确 | 0 |
| 1 | 1 | 低位溢上来，符号位又溢出去，抵消 | 0 |
| **1** | **0** | 正数被低位"顶"成负数 | **1** |
| **0** | **1** | 两个负数的符号位相加溢出，变成正数 | **1** |

**只有"进"和"出"不一致时，符号才被破坏。**

### 实测（`demo_overflow` 第 2 节）

```
  operands      carry_into7  carry_out7(C)  XOR  V
  ----------------------------------------------------
  0x7F + 0x01        1            0          1
      +127 + 1  -> carry in, no carry out
  0xFF + 0x01        1            1          0
        -1 + 1  -> carry in and carry out
  0x80 + 0xFF        0            1          1
      -128 - 1  -> carry out, no carry in
  0x10 + 0x20        0            0          0
        16 + 32 -> neither
```

逐位验证 `0x7F + 0x01`：

```
bit0: 1+1 = 0, carry 1
bit1: 1+0+1 = 0, carry 1     <- 进位一路向上传播
...
bit6: 1+0+1 = 0, carry 1     <- carry_into_bit7 = 1
bit7: 0+0+1 = 1, carry 0     <- carry_out_of_bit7 = 0

V = 1 XOR 0 = 1  ✓
```

---

## 3. 实用规则（写代码时用这个）

硬件规则很直观，但代码里通常写这个版本：

```
V = 1  当且仅当  两个操作数符号相同，而结果符号与它们不同
```

### 为什么"符号相同"是前提

**正数 + 负数，结果必然落在两者之间，绝不可能越界。**

```
+100 + (-50) = +50     在 -128..127 内
-100 +  +50  = -50     在 -128..127 内
 +127 + (-128) = -1    正好在边界内
```

所以只有"正+正"和"负+负"才可能溢出。这是可以证明的定理，不是经验。

### 位运算形式

```cpp
constexpr u8 sign_mask = 0x80;
bool v = ((a ^ result) & (b ^ result) & sign_mask) != 0;
```

- `a ^ result` 的 bit 7 = 1 → a 与 result 符号不同
- `b ^ result` 的 bit 7 = 1 → b 与 result 符号不同
- 两者都为 1 → **a 与 b 符号相同**（都与 result 相反）

### 实测（`demo_overflow` 第 3 节）

```
  a     b     result   a^r    b^r    V
  ----------------------------------------
  0x7F  0x01   0x80   0xFF  0x81   1     <- a^r、b^r 的 bit7 都是 1
  0x7F  0x7F   0xFE   0x81  0x81   1
  0x50  0x50   0xA0   0xF0  0xF0   1
  0x80  0x80   0x00   0x80  0x80   1
  0xFF  0xFF   0xFE   0x01  0x01   0   <- bit7 都不是 1
  0x80  0xFF   0x7F   0xFF  0x80   1   <- 注意 a^r 的 bit7 是 1，b^r 的 bit7 是 0
  0x40  0x40   0x80   0xC0  0xC0   1
  0x80  0x01   0x81   0x01  0x80   0
  0xFF  0x01   0x00   0xFF  0x01   0
```

> 注意 `0xFF + 0xFF`：`a^r = 0x01`，bit 7 是 0，所以 V = 0。
> `-1 + -1 = -2`，确实没有溢出。**只看"是否产生了进位"会误判成溢出。**

### 两种规则等价吗？

**`custom_nes_core/tests/core/test_alu.cpp` 穷举了全部 131072 种输入**（256 × 256 个操作数对 × 2 种 carry-in），
断言"程序员规则"与"硬件逐位进位规则"结果完全一致。**它们永远相等。**

---

## 4. C 与 V 是完全独立的信息

四种组合全部可达（`demo_overflow` 第 4 节）：

```
C=0 V=0   result 0x30   (16 + 32)         无溢出
C=1 V=0   result 0x00   (-1 + 1)          只有无符号溢出
C=0 V=1   result 0x80   (127 + 1)         只有有符号溢出
C=1 V=1   result 0x00   (-128 + -128)     两种都溢出
```

如果 C 和 V 携带相同信息，这张表只会有两行。**它有四行。**

---

## 5. N 和 V 不是一回事

```
N = result 的 bit 7
```

N **只是** bit 7 的副本，它**不是**"结果是不是负数"的判决。

```
0x50 + 0x50 = 0xA0     80 + 80 = 160

N = 1   (bit 7 = 1)
V = 1   (160 超出 +127)
C = 0   (160 放得进无符号)
```

反过来也有：

```
0xFF + 0xFF = 0xFE     -1 + -1 = -2

N = 1   (bit 7 = 1)
V = 0   (-2 完全正常)
C = 1   (第 9 位被产生)
```

**结论：判断有符号结果是否正确，看 V；判断结果正负，看 N。两者不可互换。**

---

## 6. 为什么 NES 离不开 V

### 6.1 6502 没有"有符号比较"指令

它只有 `CMP`（本质是做减法 `A - M`），然后靠分支指令判断。

执行 `A - M` 之后，**差值的真实符号是 `N XOR V`，不是 N。**

### 6.2 实测（`demo_overflow` 第 5 节）

```
  A      M      A-M     N  V  N^V   A<M (signed)?
  -------------------------------------------------
     1      2   0xFF    1  0   1     yes
     2      1   0x01    0  0   0     no
    -1      1   0xFE    1  0   1     yes
     1     -1   0x02    0  0   0     no
  -128      1   0x7F    0  1   1     yes      <-- 关键行
   127     -1   0x80    1  1   0     no       <-- 关键行
    -1   -128   0x7F    0  0   0     no
```

**第 5 行必须仔细看：**

```
  A = 0x80 (-128)
  M = 0x01 (+1)
  A - M = -129  ->  溢出  ->  结果是 0x7F

  N = 0    ("看起来是正数")
  V = 1

  只看 N 会得出 "-128 >= 1" 的错误结论
  N XOR V = 1  ->  正确判定 -128 < 1
```

### 6.3 实际汇编模式

```
; 有符号比较：A < M ?
CMP  #$80      ; 结果设置 N, Z, C, V
BVC  .v_clear  ; V=0 时 N 可信
BVS  .v_set    ; V=1 时 N 要取反

; 常用的简写是：
CMP  M
BVS  flip
BMI  less      ; N=1 -> A < M
JMP  not_less
flip:
BPL  less      ; N=0，但因为 V=1，实际是 A < M
```

**不理解 V，就无法实现正确的有符号分支。**
后果是：游戏里的坐标比较、碰撞检测、AI 判断会随机出错，而且极难定位——
因为错误只发生在数值接近 `-128` / `+127` 边界时。

### 6.4 ADC / SBC 与 V

- `ADC`（加法）和 `SBC`（减法）都会更新 V
- `BIT` 指令会把操作数的 bit 6 直接复制到 V（这是另一个用途）
- `CLV` 可以清 V
- NES 的 2A03 关掉了十进制模式，所以 ADC/SBC 的 V 行为就是本文描述的标准行为

### 6.5 6502 实际使用的公式

real 6502 的 ADC 计算 V 用的是：

```
V = ~(a ^ b) & (a ^ result) & 0x80
```

这和我们的 `((a ^ result) & (b ^ result) & 0x80)` 是同一个式子
（`~(a^b) & (a^r)` 等价于"a 和 b 同号，且 a 与 r 异号"）。

**注意：carry-in（ADC 的进位输入）也参与，但公式形式不变。**
测试里已经穷举验证了 `carry_in = 0/1` 两种情况。

---

## 7. 硬件实现小结

```
一个 8 位行波进位加法器（ripple carry adder）：

  a0 b0 c_in --> [FA] --> s0, c1
  a1 b1 c1   --> [FA] --> s1, c2
  ...
  a6 b6 c6   --> [FA] --> s6, c7   <- c7 = carry_into_bit7
  a7 b7 c7   --> [FA] --> s7, c8   <- c8 = carry_out_of_bit7 = C

  V = c7 XOR c8

整个 V 逻辑只需要一个 XOR 门。
```

**这就是为什么 V 是"免费"的：它只多一个 XOR 门，却让同一套加法电路能同时服务有符号和无符号运算。**

---

## 8. 自测

1. `0x50 + 0x50` 的 C、V、N 各是多少？
2. `0xC0 + 0xC0` 的 C、V 各是多少？（先想成 -64 + -64）
3. 为什么"正数 + 负数"永远不溢出？
4. `0x7F + 0x00 + carry_in(1)` 会不会溢出？
5. 已知 `A - M` 后 `N=0, V=1`，有符号意义下 A 和 M 谁大？

<details>
<summary>答案</summary>

1. `0xA0`；C=0（160 放得下无符号），V=1（160 > 127），N=1（bit 7 = 1）
2. `0x80`；`-64 + -64 = -128`，正好在边界内 → **V=0**；`0xC0+0xC0 = 0x180` → **C=1**。
   （这是 C=1 但 V=0 的又一例：128+128 在无符号里超了，在补码里刚好放得下）
3. 因为结果介于两个操作数之间，而在 `-128..127` 中"两者之间"必然也在范围内
4. 会。`127 + 0 + 1 = 128 > 127` → V=1。所以 ADC 必须把 carry 算进溢出判断
5. `N XOR V = 1` → 差值为负 → **A < M**

</details>

---

## 9. 代码对应

| 概念 | 代码位置 |
|------|---------|
| 程序员规则 | `custom_nes_core/src/core/alu.hpp` `signed_overflow_rule()` |
| 硬件规则 | `custom_nes_core/src/core/alu.hpp` `trace_add()` + `signed_overflow_from_carries()` |
| Carry / V / N / Z | `custom_nes_core/src/core/alu.hpp` `AddResult` |
| SBC 复用加法器 | `custom_nes_core/src/core/alu.hpp` `subtract()` |
| 穷举等价性证明 | `custom_nes_core/tests/core/test_alu.cpp` `ProgrammerRuleEqualsHardwareRule` |
| 与宽整数真值对比 | `custom_nes_core/tests/core/test_alu.cpp` `MatchesPlainIntegerArithmetic` |
| 可运行讲解 | `tools/demo_overflow.cpp` |

```bash
./build/demo_overflow
./build/tests/fc_tests --gtest_filter='OverflowFlag.*'
```

---

**上一章：** [twos-complement.md](twos-complement.md) · **相关：** [bitwise-operations.md](bitwise-operations.md)
