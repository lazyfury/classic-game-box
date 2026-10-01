# 总线架构 Bus Architecture

> 目标：解释为什么模拟器需要 Bus 这一层，以及它如何让 CPU 对硬件一无所知。
>
> 相关：[../nes/memory-map.md](../nes/memory-map.md) — 具体的内存映射表

---

## 1. 问题：CPU 不该知道 PPU 存在

AGENTS.md 里有一条硬性规则：

```
错误                          正确
CPU                           CPU
 |                             |
memory[]                       Bus
 |                             |
PPU                           PPU / RAM / Cartridge
```

### 为什么"错误"的写法看起来更简单

最直觉的实现是让 CPU 直接持有一块内存数组：

```cpp
class Cpu {
    u8 memory[0x10000];
    u8 read(u16 address) { return memory[address]; }
};
```

**这在 Phase 0 能跑通，然后会毁掉整个项目。** 因为：

1. **CPU 无法独立测试** —— 任何测试都要搭起整台机器
2. **PPU 寄存器有副作用** —— 读 `$2002` 清除 vblank 标志，这不是"内存读"
3. **卡带是动态的** —— 换一张卡带要重新接线，而不是读取不同的数据
4. **地址译码本来就是独立的硬件** —— 它不在 CPU 里

### 真实硬件的结构

```
        CPU
         |
    [16 根地址线]  [8 根数据线]
         |
    +----+----+----+----+----+
    |         |         |    |
  片选A      片选B    片选C  片选D
    |         |         |    |
   RAM       PPU       APU  卡带
```

**片选逻辑是一堆独立的门电路，和 CPU 没有任何关系。** Bus 就是这堆门电路。

---

## 2. 接口设计

```cpp
// custom_nes_core/src/core/bus.hpp
class Bus {
public:
    virtual ~Bus() = default;

    [[nodiscard]] virtual u8 read(u16 address) = 0;
    virtual void write(u16 address, u8 value) = 0;

    /// Cycles the CPU has to wait because the BUS did something on its own.
    virtual int take_stall_cycles() { return 0; }
};
```

CPU 只持有 `Bus*`：

```cpp
class Cpu {
    explicit Cpu(Bus& bus) noexcept : bus_(&bus) {}
    [[nodiscard]] u8 read(u16 address) noexcept { return bus_->read(address); }
};
```

### 为什么 `read()` 不是 `const`

```cpp
[[nodiscard]] virtual u8 read(u16 address) = 0;   // 注意：没有 const
```

因为**真实的读取会有副作用**：

| 地址 | 读取的副作用 |
|------|-------------|
| `$2002` PPUSTATUS | 清除 vblank 标志，复位地址锁存 |
| `$2004` OAMDATA | 某些情况下会递增 OAM 地址 |
| `$2007` PPUDATA | VRAM 地址递增（+1 或 +32） |
| `$4015` APU_STATUS | 清除声道的中断标志 |

**如果用 `const`，编译期就会阻止这些行为**，然后你会开始用 `mutable` 或
`const_cast` 来绕过去——那是代码在告诉你设计错了。

> **`const` 不是装饰，它是接口的一部分。** 一个"读"如果会改变状态，
> 它就不该是 `const`，无论名字里有没有 read。

---

## 3. 设备接口：接缝在哪里

```cpp
// custom_nes_core/src/core/nes/device.hpp
class Device {
public:
    virtual ~Device() = default;

    /// `address` is the FULL 16 bit CPU address, not an offset.
    [[nodiscard]] virtual u8 read(u16 address) = 0;
    virtual void write(u16 address, u8 value) = 0;
};
```

### 为什么传完整地址而不是偏移

```cpp
// 传偏移（不好）
virtual u8 read(u16 offset) = 0;   // 谁来做镜像？

// 传完整地址（好）
virtual u8 read(u16 address) = 0;
```

**因为镜像的规则属于设备自己。**

看 `Ram`：

```cpp
class Ram {
    [[nodiscard]] u8 read(u16 address) const noexcept
    {
        return bytes_[address & kMask];   // 芯片只用低 11 位
    }
};
```

**这行掩码就是那块芯片上没有接的 5 根线。** 如果 Bus 先做了 `& 0x07FF`
再传过来，`Ram` 里的掩码就变成了冗余——而**"为什么是 2KB 而不是 8KB"
这个知识就消失在了错误的地方**。

把地址完整传下去，让每个设备表达自己的约束。

---

## 4. 插槽：设备如何接进来

```cpp
class NesBus : public Bus {
public:
    /// 卡带槽，$4020-$FFFF。Phase 3
    void set_cartridge(Device* device) noexcept { cartridge_ = device; }

    /// PPU 寄存器窗口，$2000-$3FFF。Phase 4
    void set_ppu(Device* device) noexcept { ppu_ = device; }

    /// APU 与 I/O，$4000-$4017（除 $4014）。Phase 5、6
    void set_apu(Device* device) noexcept { apu_ = device; }

    /// OAM DMA 的目标。Phase 4 让 PPU 实现它
    void set_oam_target(OamTarget* target) noexcept { oam_target_ = target; }
};
```

**每个槽对应真实机器上的一块芯片。** 没插的时候总线返回 open bus。

### 为什么用裸指针而不是智能指针

**总线不拥有设备。** 真实机器上芯片是独立的元件，总线只是把它们连起来。
所有权在别处（`Machine` 类，或者测试里的局部变量）。

```cpp
// 测试里
nes::RamCartridge cart;      // 拥有
nes::NesBus bus;             // 引用
bus.set_cartridge(&cart);
```

如果总线用 `std::unique_ptr` 持有设备，就没法在测试里替换不同实现（比如
一个只读的 ROM 卡带 vs 一个可写的 RAM 卡带）。

---

## 5. 总线可以让 CPU 等待

这是 Phase 2 引入的新能力。

```cpp
/// Cycles the CPU has to wait because the BUS did something on its own.
virtual int take_stall_cycles() { return 0; }
```

```cpp
// CPU 每执行完一条指令就排空一次
cycles_ += static_cast<u64>(cycle_cost(last_opcode_, operand));
cycles_ += static_cast<u64>(bus_->take_stall_cycles());
```

### 为什么这是正确的位置

OAM DMA 的 513 个周期**不是指令成本**——它是总线从 CPU 手里抢走的时间。

| 放在哪里 | 问题 |
|---------|------|
| CPU 里硬编码 `if (address == 0x4014) cycles += 513` | CPU 就知道了 $4014 是 DMA，违反规则 1 |
| PPU 里直接改 CPU 的周期计数 | PPU 需要持有 CPU 指针，依赖反向 |
| **总线请求，CPU 排空** | **"等待"是时间概念，"为什么等"在设备里** |

**这是依赖倒置的一个具体例子。**

Phase 4 接入真正的 PPU 后，OAM DMA 的 CPU 侧代码**一行都不用改**。

---

## 6. 依赖方向

```
                    +-----------+
                    |   Bus     |   (抽象接口)
                    +-----+-----+
                          ^
                          | 依赖
                    +-----+-----+
                    |   Cpu     |
                    +-----------+

                    +-----------+
                    |  NesBus   |   实现
                    +-----+-----+
                          |
              +-----------+-----------+
              |           |           |
            Ram        Device*     OamTarget*
                       (PPU 等)
```

**规则：箭头只能向上。**

```
✅ CPU 依赖 Bus（抽象）
✅ NesBus 依赖 Device（抽象）
❌ CPU 依赖 Ppu（具体）
❌ Bus 依赖 Cpu
```

### 怎么检验

**如果 `custom_nes_core/src/core/cpu/` 里出现了 `ppu` 这个词，就是违规。**

```bash
grep -ri "ppu" custom_nes_core/src/core/cpu/     # 应该没有输出
grep -ri "nes" custom_nes_core/src/core/cpu/     # 应该没有输出（CPU 不知道 NES）
```

> 让这条 grep 保持空，依赖方向就是对的。

---

## 7. 测试替身

有两个"假"实现，用途不同：

### `FlatBus`（`custom_nes_core/src/core/flat_bus.hpp`）

```
64KB 平铺数组，没有译码
```

**用途：** CPU 单元测试。当测试关心的是"CPU 是否正确执行了指令"而不是
"地址是否正确译码"时，用 `FlatBus` 更简单、更专注。

**局限：** 它没有镜像。写 `$0800` 不会影响 `$0000`。

### `RamCartridge`（`custom_nes_core/src/core/nes/ram_cartridge.hpp`）

```
$4020-$FFFF 映射成 RAM
```

**用途：** 在 Phase 3 的 iNES 加载器存在之前，给程序和向量一个落脚点。

**它不是卡带：** 没有 ROM、没有文件头、没有 Mapper。

### 什么时候用哪个

| 测试对象 | 用哪个 |
|---------|--------|
| 指令语义、寻址模式、周期 | `FlatBus` + `Machine` |
| 内存映射、镜像、OAM DMA | `NesBus` + `RamCartridge` |
| 反汇编器（只关心字节） | `FlatBus` |

**两者都是合法的测试替身。** 区别是 `FlatBus` 替换的是"整个地址空间"，
`RamCartridge` 替换的是"一块还没实现的芯片"。

---

## 8. 代码对应

| 概念 | 文件 |
|------|------|
| 总线抽象 | `custom_nes_core/src/core/bus.hpp` |
| 设备接口 | `custom_nes_core/src/core/nes/device.hpp` |
| NES 地址译码 | `custom_nes_core/src/core/nes/bus.{hpp,cpp}` |
| 测试替身 1 | `custom_nes_core/src/core/flat_bus.hpp` |
| 测试替身 2 | `custom_nes_core/src/core/nes/ram_cartridge.hpp` |
| 集成测试 | `custom_nes_core/tests/core/test_nes_bus.cpp` |
| 可运行讲解 | `tools/demo_bus.cpp` |

---

## 9. 自测

1. 为什么 `Bus::read()` 不是 `const`？
2. 为什么 `Device::read()` 传的是完整 16 位地址而不是偏移？
3. `Ram` 里的 `& 0x07FF` 如果挪到 `NesBus` 里，会损失什么信息？
4. OAM DMA 的 513 个周期为什么由总线报告，而不是 CPU 自己知道？
5. 为什么总线用裸指针持有设备，而不是 `unique_ptr`？
6. `FlatBus` 和 `RamCartridge` 分别是什么的替身？

<details>
<summary>答案</summary>

1. 因为真实硬件的读取有副作用（读 `$2002` 清除 vblank 标志）。用 `const` 会迫使你 `const_cast`
2. 因为镜像规则属于设备。传完整地址让设备能表达"我只用了低 11 位"这个约束
3. 会丢失"为什么 RAM 是 2KB 却被映射到 8KB"这个知识。掩码在 `Ram` 里时，它直接对应芯片上没有接的地址线
4. 因为"让 CPU 等一会儿"是时间概念，"为什么等"是设备知识。分开后 CPU 不需要知道 PPU 存在
5. 因为总线不拥有设备。真实机器上芯片是独立元件。所有权在别处，测试才能替换不同实现
6. `FlatBus` 替身"整个地址空间"（用于 CPU 单元测试）；`RamCartridge` 替身"还没实现的卡带芯片"

</details>

---

**相关：** [../nes/memory-map.md](../nes/memory-map.md)
**上一阶段：** [../computer-science/timing.md](../computer-science/timing.md)
