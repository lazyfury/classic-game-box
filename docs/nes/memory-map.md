# NES 内存映射 NES Memory Map

> 目标：理解 16 位地址空间如何被译码到不同设备，以及**镜像为什么不是功能而是必然**。
>
> 本文输出都来自 `tools/demo_bus.cpp`，并可由 `custom_nes_core/tests/core/test_nes_bus.cpp` 验证。

---

## 1. 完整的地址空间

```
  range         size   what
  ------------  -----  ------------------------------------------
  $0000-$07FF   2KB    work RAM
  $0800-$1FFF   6KB    work RAM again, three more times (mirroring)
  $2000-$3FFF   8KB    PPU registers: 8 bytes, mirrored 1024 times
  $4000-$4017   24B    APU, controllers, and OAM DMA at $4014
  $4018-$401F   8B     disabled
  $4020-$FFFF   ~48KB  the cartridge slot
```

CPU 放一个 16 位地址到总线上，**由译码逻辑决定谁来应答**。CPU 完全不知道
RAM、PPU、卡带的存在——这正是 `custom_nes_core/src/core/bus.hpp` 里那个 `Bus` 抽象要表达的东西。

### 详细的寄存器分布

| 地址 | 读 | 写 |
|------|-----|-----|
| `$2000` | PPUCTRL | PPUCTRL |
| `$2001` | PPUMASK | PPUMASK |
| `$2002` | **PPUSTATUS**（读会清除 vblank 标志） | — |
| `$2003` | OAMADDR | OAMADDR |
| `$2004` | **OAMDATA**（读会额外行为） | OAMDATA |
| `$2005` | PPUSCROLL | PPUSCROLL |
| `$2006` | PPUADDR | PPUADDR |
| `$2007` | **PPUDATA**（读会推进 VRAM 地址） | PPUDATA |
| `$4014` | — | **OAM DMA** |
| `$4016` | 手柄 1 | 手柄选通 |
| `$4017` | 手柄 2 | APU 帧计数器 |
| `$4015` | APU 状态 | APU 声道使能 |

**注意 `$2002` 和 `$2007` 的读取有副作用。** 这是为什么整个项目的
`Bus::read()` 都不是 `const` 的——见 [../architecture/bus.md](../architecture/bus.md)。

---

## 2. 镜像：没有接的地址线

这是本阶段最重要的概念。

### 为什么会有镜像

**2KB 的芯片需要 11 根地址线**（2¹¹ = 2048）。6502 有 16 根。

```
      A15 A14 A13 A12 A11 | A10 .. A0
       |   |   |   |   |      \_____/
       |   |   |   |   |         |
       |   |   |   |   |     the RAM chip
       \___|___|___|___/
               |
        chip select logic
```

**上面 5 根线根本没接到 RAM 上。** 片选逻辑只说"这个地址在 `$0000-$1FFF` 里"，
但具体是这 8KB 中的哪一个，**从没传到 RAM**。

结果：

```
$0000  ---+
$0800  ---+-- 同一个字节
$1000  ---+
$1800  ---+
```

**镜像不是"实现了的功能"，而是"没接线"的物理后果。**

### 代码里的对应

```cpp
class Ram {
    static constexpr u16 kSize = 0x0800;
    static constexpr u16 kMask = kSize - 1;   // 0x07FF

    [[nodiscard]] u8 read(u16 address) const noexcept
    {
        return bytes_[address & kMask];   // <- 这就是那 5 根没接的线
    }
};
```

**掩码放在 `Ram` 里而不是 `Bus` 里，是物理上更准确的建模：**

- **Bus 做片选**（判断"是不是 `$0000-$1FFF`"）
- **芯片做回绕**（只用低 11 位）

### 穷举验证

`demo_bus` 第 3 节：

```
  Wrote 2048 distinct values, then read all 8192 addresses in
  $0000-$1FFF and compared each against RAM[address & $07FF].

    addresses checked : 8192
    mismatches        : 0
```

测试同样穷举：

```cpp
TEST(NesBus, AWriteIsVisibleThroughEveryMirror)          // 2048 × 4
TEST(NesBus, EveryByteInTheMirroredRangeAgreesWithTheRamChip)  // 8192
```

### 为什么模拟器必须复现它

因为**真实硬件就是这样**。如果模拟器只实现 `$0000-$07FF` 而把
`$0800-$1FFF` 当成未映射：

- 正常程序不会有问题（它们只用 `$0000-$07FF`）
- 但用到了镜像的程序会读到错误数据
- 而且这种 bug 极难定位，因为**只在特定地址访问时出现**

---

## 3. PPU 寄存器：同样的毛病，严重 1024 倍

```
8KB 地址空间 ($2000-$3FFF)  →  8 个寄存器
```

只有 **3 根地址线**被译码（`A0-A2`），所以每个寄存器应答 1024 次。

```
$2000  \
$2008   >  都是 PPUCTRL
$2010  /
...
$3FF8  /
```

`demo_bus` 第 4 节：

```
  register   how many writes it received
  --------   ---------------------------
  $2000      1024
  $2001      1024
  ...
  $2007      1024
```

```cpp
u8 NesBus::ppu_register_index(u16 address) noexcept
{
    return static_cast<u8>(address & 0x0007);   // 只有 3 根线
}
```

**实践中很重要：写 `$2000` 和写 `$2008` 效果相同。**
只译码 `$2000-$2007` 的模拟器会破坏用到镜像的程序。

---

## 4. Open Bus：未映射的读不是零

### 物理原因

数据总线是 **8 根有电容的线**。它们会保持最后被驱动上去的值。

```
某次写把 $5A 放上数据总线
  -> 线充电到 $5A 的电平
  -> 下一个周期如果没有设备驱动总线
  -> 线仍然维持在 $5A
  -> 读到 $5A
```

**所以未映射地址的读取返回"总线上残留的值"，而不是 0。**

`demo_bus` 第 5 节：

```
  write $5A to $4018 (a disabled address, nothing stores it)
    read $4018 -> $5A   (open bus, not $00)

  write $11 to $0000 (real RAM, the value is on the bus too)
    read $4019 -> $11   (still the last value on the bus)
```

### 实现

```cpp
u8 NesBus::read(u16 address)
{
    const u8 value = decode_read(address);
    open_bus_ = value;      // 无论谁应答，值都上了总线
    return value;
}

void NesBus::write(u16 address, u8 value)
{
    open_bus_ = value;      // 写也一样，即使没人听
    ...
}
```

### 为什么要在意

有几个游戏会读一个**只写**寄存器来探测总线状态。

**返回 0 在绝大多数情况下看起来是对的**，只在最难调试的那些情况下失败。

> 这是模拟器开发的一个普遍规律：**"看起来对"的实现会在你最不能承受的时候出错。**
> 宁可慢一点，把物理模型建对。

---

## 5. OAM DMA：一次写，513 个周期

### 问题

PPU 的 256 字节 OAM（精灵属性表）通过 `$2004` 一个一个访问太慢。

### 解决方案

```
LDA #$02       ; 页号
STA $4014      ; 把 $0200-$02FF 复制进 OAM
```

**写一个字节，硬件自动复制 256 个字节。**

`demo_bus` 第 6 节：

```
    OAM bytes written : 256
    OAM[0]  = $00
    OAM[255]= $FF
    stalls requested  : 513 cycles
```

### 513 从哪来

```
1   周期：暂停 CPU
256 周期：256 次读
256 周期：256 次写
--------
513 周期
```

（真机上如果写在奇数周期，是 514。）

### 总线如何"偷走"CPU 的时间

**这是 Phase 2 引入的一个新架构能力。**

```cpp
// custom_nes_core/src/core/bus.hpp
class Bus {
    /// Cycles the CPU has to wait because the BUS did something on its own.
    virtual int take_stall_cycles() { return 0; }
};
```

```cpp
// custom_nes_core/src/core/cpu/cpu.cpp, at the end of step()
cycles_ += static_cast<u64>(cycle_cost(last_opcode_, operand));
cycles_ += static_cast<u64>(bus_->take_stall_cycles());   // <- 总线偷走的
```

`demo_bus` 第 6b 节：

```
  LDA #$02      : 2 cycles total
  STA $4014     : 517 cycles   (4 for the store + 513 for the DMA)
  running total : 519
  INX           : 521 cycles total
```

**CPU 从不需要知道 PPU 是什么。** 它只是加上总线让它等的时间。

> 这就是抽象的价值：**"让 CPU 等一会儿"是一个纯时间概念，和"为什么等"完全解耦。**
> Phase 4 接入真正的 PPU 时，OAM DMA 的代码一行都不用改。

### DMA 通过正常的地址通路读数据

```cpp
for (u16 i = 0; i < 256; ++i) {
    oam_target_->write_oam(static_cast<u8>(i),
                           read(static_cast<u16>(base + i)));   // <- 走正常 read
}
```

**注意这里是 `read()`，不是直接访问 RAM。**

所以如果从页 `$20` 做 DMA，它会真的去读 PPU 寄存器。这是真实行为，
也是为什么游戏**总是从页 `$02` 做 DMA**。

测试验证了这一点：

```cpp
TEST(NesBus, OamDmaReadsThroughTheNormalAddressPath)
```

### 为什么游戏必须在 vblank 做 DMA

DMA 期间 CPU 被冻结 513 个周期（约占一帧的 1.7%）。更重要的是，
渲染期间 PPU 总线被 DMA 占用会导致画面错误。

**所以 NES 游戏的典型结构是：**

```
主循环:
  游戏逻辑
  等待 vblank（读 $2002 等 bit 7）
  做 OAM DMA
  改滚动/调色板
  回到主循环
```

---

## 6. 一个程序跨越三个区域

`demo_bus` 第 7 节：

```
    $8000    LDA #$2A           immediate     Cartridge
    $8002    STA $10            zero page     Cartridge
    $8004    LDX #$05           immediate     Cartridge
    $8006    STA $20,X          zero page,X   Cartridge
    $8008    PHA                implied       Cartridge
    $8009    JMP $8000          absolute      Cartridge

  After 5 instructions:
    $0010       = $2A   (zero page, real RAM)
    $0025       = $2A   (zero page,X)
    $01FD       = $2A   (stack, page 1)
    $0810       = $2A   (the same cell as $0010, through the mirror)
    $1010       = $2A   (and through the second mirror)
```

**程序在卡带空间，变量在 RAM，栈在 page 1，而 CPU 完全不知道区别。**

这就是 `Bus` 抽象要证明的东西。

---

## 7. 代码对应

| 概念 | 文件 |
|------|------|
| 内存映射与译码 | `custom_nes_core/src/core/nes/bus.{hpp,cpp}` |
| 2KB RAM 与其回绕 | `custom_nes_core/src/core/nes/ram.hpp` |
| 卡带槽占位实现 | `custom_nes_core/src/core/nes/ram_cartridge.hpp` |
| 设备接口 | `custom_nes_core/src/core/nes/device.hpp` |
| 测试 | `custom_nes_core/tests/core/test_nes_bus.cpp` |
| 可运行讲解 | `tools/demo_bus.cpp` |

```bash
./build/demo_bus
./build/tests/fc_tests --gtest_filter='NesBus.*'
```

---

## 8. 自测

1. `$0000-$1FFF` 是 8KB，但 RAM 只有 2KB。多出来的地址怎么处理？
2. 为什么写 `$2008` 和写 `$2000` 效果相同？
3. 读 `$4018`（一个被禁用的地址）会得到什么？为什么不是 0？
4. 写 `$4014` 会发生什么？CPU 要等多久？
5. 为什么 OAM DMA 的源码地址必须是页对齐的？
6. 如果要写 `$0300`，实际的 RAM 索引是多少？
7. 为什么 `Ram::read()` 里的掩码不放在 `NesBus` 里？

<details>
<summary>答案</summary>

1. 它们镜像到同一块 2KB 上。每 2KB 重复 4 次
2. 因为只有 3 根地址线到达 PPU，`$2000 & 0x07 == $2008 & 0x07 == 0`
3. 返回总线上残留的值（open bus），可能是任何值，取决于上一次谁驱动了总线
4. 从 `页号 × $100` 开始复制 256 字节到 OAM。CPU 停机 513 个周期
5. 因为写入的只是一个字节，它只能指定页号；源地址固定是该页的 `$00-$FF`
6. `$0300 & 0x07FF = 0x0300`
7. 因为物理上就是芯片只接了 11 根线：Bus 做片选，芯片做回绕。放对位置能让代码直接反映硬件

</details>

---

**相关：** [../architecture/bus.md](../architecture/bus.md) — 为什么要有 Bus 这一层
**下一阶段：** Phase 3 — Cartridge（iNES 格式与 Mapper）
