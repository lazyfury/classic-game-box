# Mapper：卡带上的那块逻辑

> 状态：**Mapper 0 (NROM)、Mapper 1 (MMC1) 已实现**
> 代码：`custom_nes_core/src/core/nes/mapper.hpp`、`mapper0.hpp`、`mapper1.hpp`

## 1. 为什么需要 Mapper

6502 只有 16 根地址线，所以 CPU 一次最多看到 64KB。NES 把其中
`$8000-$FFFF`（32KB）留给卡带。早期游戏整盘 ROM 塞得进去，卡带上
**只有一颗 ROM 芯片，没有任何逻辑**——这就是 Mapper 0 (NROM)。

后来的游戏塞不进去了。解决办法不是加宽地址总线（CPU 改不了），而是
在卡带上加一块**小逻辑**：CPU 往某个地址写一个数，卡带就把 32KB
窗口里的某一段重新指向 ROM 的另一块。

```
CPU 写 $8000 <- $05
        |
        v
卡带内部：$A000-$BFFF 现在指向 ROM 的第 5 个 16KB
```

ROM 芯片本身从没变过，变的只是卡带板子上的连线。这就是为什么
Mapper 必须是**代码而不是数据**：它是逻辑，不是存储。

## 2. 两种独立的地址空间

一个容易忽略的事实：卡带有两条互不相干的地址总线。

```
CPU  --$8000-$FFFF-->  PRG（程序）   mapper.read_prg / write_prg
PPU  --$0000-$1FFF-->  CHR（图形）   mapper.read_chr / write_chr
```

分页寄存器也分成两套。`Mapper` 接口里 `read_prg` 和 `read_chr` 是分开的，
正是因为这个。

## 3. Mapper 0：NROM

没有寄存器，没有分页。唯一的细节是 16KB 的 PRG 会被接到两个半区：

```
$8000-$BFFF  ->  ROM[0x0000-0x3FFF]
$C000-$FFFF  ->  ROM[0x0000-0x3FFF]   <- 同一块
```

这是第三种"镜像"：因为没花钱去译码区分两个半区，同一颗芯片就都得应答。
超级玛丽就是这个。

## 4. Mapper 1：MMC1

MMC1 是第一块真正有意思的板子。CPU 只有五根线，它却有四个寄存器要填。

### 4.1 串行口

对 `$8000-$FFFF` 的写**不是**直接选 bank，而是把一个 bit 移进一个
5 位移位寄存器：

```
第一次写  ->  移入 bit0
第二次写  ->  移入 bit1
...
第五次写  ->  寄存器装满，写入某个寄存器
```

**装到哪个寄存器由地址决定，不由数据决定：**

| 地址 | 寄存器 |
|------|--------|
| `$8000-$9FFF` | control（镜像 + PRG 模式 + CHR 模式） |
| `$A000-$BFFF` | CHR bank 0 |
| `$C000-$DFFF` | CHR bank 1 |
| `$E000-$FFFF` | PRG bank |

所以 MMC1 游戏的代码里会看到"连着五次写同一个地址"——那是在一位一位地
把这五个 bit 敲进去。

如果数据 bit7 = 1，则是**复位**移位寄存器（而不是移入数据），同时强制
control 的 PRG 模式为 3。程序丢了同步就用这一招重新开始。

```
写 $8000 = 0x80   ->  移位寄存器清空，control |= 0x0C
```

### 4.2 control 寄存器：四两拨千斤

control 的五个 bit 决定了**另外三个寄存器怎么解释**：

```
bit 0-1  镜像：  0 = 单屏 低
                 1 = 单屏 高
                 2 = 垂直
                 3 = 水平
bit 2-3  PRG 模式：
                 0/1 = $8000 处放一个 32KB bank（bank 号的 bit0 被忽略）
                 2   = $8000 固定第一个 bank，$C000 可切换
                 3   = $8000 可切换，$C000 固定最后一个 bank
bit 4    CHR 模式：
                 0 = 8KB 分页
                 1 = 4KB 分页（CHR bank 0 / bank 1 各管一半）
```

同一个 CHR bank 寄存器，在 bit4 不同的情况下含义完全不同；同一个 PRG
bank 寄存器，在 bit2-3 不同的情况下含义也完全不同。**一切皆模式位**，
真实板子就是这么省的。

上电时 control = `0x0C`（PRG 模式 3，8KB CHR，单屏低）。

### 4.3 8KB CHR 模式：寄存器里存的是“bank 号 × 2”

这是最容易写错的一处，而且错了以后**游戏照样能跑，只是所有 tile 都
偏了两个 bank**——画面全是方块，但不会崩。

8KB 分页时：

```
8KB bank = chr_bank0 >> 1        （CHR bank 1 在 8KB 模式下不接）
```

为什么是右移？因为 CHR bank 0 的 bit0 在 4KB 模式下用来选 bank 内的
哪一半 4KB；8KB 模式下这一半由 PPU 的 A12 决定，所以 bit0 被忽略。
结果寄存器里存的其实是 **bank 号先左移了一位**：

```
写 $02  ->  0b00010  ->  8KB bank 1
写 $10  ->  0b10000  ->  8KB bank 8
```

Zelda II 正好是证据：它的 reset 把 CHR0 写成 $00，而**大地图**的加载
程序（PRG bank 0 的 $8149）写 `LDA #$10 / JSR $BFB1`，也就是选 bank 8。
如果错把寄存器的值当成 bank 号（或者用 `(chr0 & 0x1E) | (chr1 & 1)`
这种老写法），$10 会落到 bank 0——于是大地图用上了标题画面的方块字库，
整张地图一片方块。侧视关卡用的是 bank 1（写 $02），也会错位到 bank 2，
所以看起来"瓦片不对"。

> 回归测试：`custom_nes_core/tests/core/test_cartridge.cpp` 的
> `Mapper1.ChrEightKiloByteModeUsesBankZeroShiftedRight`。

### 4.4 运行时可切换镜像

注意 control 的 bit0-1 是**镜像**。也就是说 MMC1 游戏可以在运行中把
两张 nametable 的关系从垂直改成水平，用 2KB VRAM 做出四屏的效果。
所以 PPU 取 nametable 时必须**问 mapper**，而不是读 iNES 文件头：

```cpp
// custom_nes_core/src/core/nes/ppu.cpp
const Mirroring mode = (cartridge_ != nullptr)
    ? cartridge_->mapper().mirroring()
    : Mirroring::Horizontal;
```

（Zelda II 的标题画面和状态栏切换就依赖这个。）

## 5. 一次银行切换的完整例子

把 PRG 模式设为 3，然后让 `$8000` 指向第 3 个 16KB bank：

```cpp
// 1. control = 0b11100：4KB CHR (bit4=1)，PRG 模式 3 (bit3-2=11)，
//    单屏低 (bit1-0=00)。0b11100 = 0x1C。
write(0x8000, 0x1C);
// (helper 会把 0x1C = 11100 按 LSB 先行的顺序写成五次)
```

`control` 的写入是五次写：

```
bit0=0  bit1=0  bit2=1  bit3=1  bit4=1   ->  0b11100 = 0x1C
```

然后选 PRG bank 3：

```cpp
// 2. PRG bank = 00011
write(0xE000, 3);
```

结果是：

```
$8000-$BFFF  ->  PRG 16KB bank 3
$C000-$FFFF  ->  PRG 16KB 最后一个 bank
$0000-$0FFF  ->  CHR 4KB bank 0（$A000 里写的）
$1000-$1FFF  ->  CHR 4KB bank 1（$C000 里写的）
```

`custom_nes_core/tests/core/test_cartridge.cpp` 的 `Mapper1.*` 把每一步都钉住了。

## 6. 两个可选钩子：让 MMC3 这类卡带能反过来看 PPU

前四个 mapper 只靠 CPU 的读写和 CHR 就够。MMC3 不够：它要数扫描线，而
CPU 根本不知道 PPU 在哪一行。于是 `Mapper` 接口加了两个**默认空实现**的
钩子：

```cpp
/// PPU 每次访问 $0000-$1FFF 都会叫一下
virtual void on_ppu_address(u16) {}

/// 卡带自己的 /IRQ 线，电平触发
[[nodiscard]] virtual bool irq_asserted() const noexcept { return false; }
```

- PPU 在 `read_vram()` 里无条件调用 `on_ppu_address()`；
- Machine 每条指令后把 `irq_asserted()` 接到 CPU 的 `/IRQ` 线上。

空实现意味着 NROM / MMC1 / UxROM / CNROM 一行都不用改。MMC3 在
`on_ppu_address()` 里盯住 PPU 地址的 bit12（也就是选哪张 pattern table）：
每行渲染精灵时 A12 会 0→1 一次，这个上升沿就是它的扫描线时钟。

## 7. 已实现的 Mapper

| 编号 | 名字 | 关键点 | 代表游戏 |
|------|------|--------|----------|
| 0 | NROM | 无分页，16KB PRG 镜像 | 超级玛丽、吃豆人 |
| 1 | MMC1 | 串行移位寄存器、4/8KB CHR、运行时镜像 | 塞尔达 II、俄罗斯方块 |
| 2 | UxROM | 16KB PRG 分页，CHR RAM | 洛克人、恶魔城 |
| 3 | CNROM | 8KB CHR 分页，PRG 固定 | 越野摩托、宇宙巡航舰 |
| 4 | MMC3 | 8KB PRG、1/2KB CHR、**扫描线 IRQ** | 超级玛丽 3、星之卡比 |
| 7 | AxROM | 32KB PRG、单屏镜像 | 大理石疯疯、Gauntlet |
| 9 | MMC2 | PPU 取 tile $FD/$FE 翻转 CHR latch | Punch-Out!! |
| 10 | MMC4 | 同 MMC2 的 CHR latch，16KB PRG | Fire Emblem |
| 11 | Color Dreams | 8KB CHR 分页 | 圣经冒险（非授权） |
| 13 | CPROM | 自带 16KB CHR RAM，4KB 分页 | Videomation |
| 15 | 100-in-1 | **16KB PRG（顶部固定）、单屏镜像、8KB CHR RAM** | 多合一卡菜单 |
| 18 | SS88006 | 8KB PRG/1KB CHR、**CPU 周期 IRQ** | Magical Doropie |
| 19 | Namco 163 | 8KB PRG、8x1KB CHR、**CPU 周期 IRQ**、声音 RAM 端口 | 妖怪道中记、Rolling Thunder |
| 21/22/23/25 | VRC2/VRC4 | 8KB PRG、8x1KB CHR、运行时镜像、**VRC4 有 CPU 周期 IRQ** | 魂斗罗(JP)、宇宙巡航机 II |
| 32 | IREM G-101 | 8KB PRG 两种版式、8x1KB CHR | Image Fight |
| 33 | Taito TC0190 | 8KB PRG、8x1KB CHR | Insector X |
| 66 | GxROM | 一个寄存器同时换 32KB PRG 和 8KB CHR | 超级玛丽+打鸭子 |
| 68 | Sunsoft-4 | 16KB PRG、4x2KB CHR、运行时镜像 | After Burner |
| 71 | Codemasters | 16KB PRG、CHR RAM、单屏镜像 | Micro Machines |
| 78 | Jaleco JF-16 | 16KB PRG、8KB CHR | Holy Diver |
| 87 | Jaleco JF-13 | 32KB PRG、8KB CHR，寄存器在 $6000 且无 RAM | 飞龙之翼 |
| 162/164/178/242 | Waixing | 寄存器在扩展区的 32KB/16KB 分页 | 中文 RPG |
| 163 | Nanjing | **32KB PRG 分页**、防拷反馈位、自动 4KB CHR RAM 切换 | 金庸群侠传等南晶 RPG |
| 177 | Henggedianzi | 一个寄存器同时选 32KB PRG 和镜像 | 爆笑三国等 |
| 190 | Magic Kid Goo Goo | 16KB PRG（只低半可切）、4x2KB CHR | 中国人等 |
| 226 | 76-in-1 | 7 位 PRG bank 拆在两个寄存器、32KB/16KB 两种模式 | 76合1、Super 42-in-1 |
| 227/246 | 多合一 | 地址解码 bank / $6000 寄存器 | 1200-in-1 |
| 249 | Waixing T9552 | MMC3 + **bank 线交叉**（按 pattern 0 还原） | 封神榜等 |

代码位置：

```
custom_nes_core/src/core/nes/mapper.hpp    接口 + 默认空实现钩子（PPU 地址 / IRQ / 扩展区 / 扫描线 / CPU 周期 / work RAM）
custom_nes_core/src/core/nes/mapper0.hpp   NROM
custom_nes_core/src/core/nes/mapper1.hpp   MMC1
custom_nes_core/src/core/nes/mapper2.hpp   UxROM
custom_nes_core/src/core/nes/mapper3.hpp   CNROM
custom_nes_core/src/core/nes/mapper4.hpp   MMC3（含扫描线 IRQ）
custom_nes_core/src/core/nes/mapper7.hpp   AxROM
custom_nes_core/src/core/nes/mapper9.hpp   MMC2（也提供 mapper10 复用的 CHR latch）
custom_nes_core/src/core/nes/mapper10.hpp  MMC4
custom_nes_core/src/core/nes/mapper11.hpp  Color Dreams
custom_nes_core/src/core/nes/mapper13.hpp  CPROM
custom_nes_core/src/core/nes/mapper15.hpp  100-in-1
custom_nes_core/src/core/nes/mapper18.hpp  Jaleco SS88006（CPU 周期 IRQ）
custom_nes_core/src/core/nes/mapper19.hpp  Namco 163（声音 RAM + CPU 周期 IRQ）
custom_nes_core/src/core/nes/mapper21.hpp  VRC2 / VRC4（21/22/23/25，含 IRQ）
custom_nes_core/src/core/nes/mapper32.hpp  IREM G-101
custom_nes_core/src/core/nes/mapper33.hpp  Taito TC0190
custom_nes_core/src/core/nes/mapper66.hpp  GxROM
custom_nes_core/src/core/nes/mapper68.hpp  Sunsoft-4
custom_nes_core/src/core/nes/mapper71.hpp  Codemasters BF909x
custom_nes_core/src/core/nes/mapper78.hpp  Jaleco JF-16
custom_nes_core/src/core/nes/mapper87.hpp  Jaleco JF-13
custom_nes_core/src/core/nes/mapper162.hpp Waixing 162
custom_nes_core/src/core/nes/mapper163.hpp Nanjing FC-001
custom_nes_core/src/core/nes/mapper164.hpp Waixing 164
custom_nes_core/src/core/nes/mapper177.hpp Henggedianzi 177
custom_nes_core/src/core/nes/mapper178.hpp Waixing 178
custom_nes_core/src/core/nes/mapper190.hpp Magic Kid Goo Goo
custom_nes_core/src/core/nes/mapper226.hpp 76-in-1
custom_nes_core/src/core/nes/mapper227.hpp 227 多合一
custom_nes_core/src/core/nes/mapper242.hpp Waixing 242
custom_nes_core/src/core/nes/mapper246.hpp 246 多合一
custom_nes_core/src/core/nes/mapper249.hpp Waixing T9552（交叉 bank 线）
custom_nes_core/src/core/nes/cartridge.cpp 工厂：按文件头编号构造
```

## 8. 两张中文卡带：163 与 226

这两个号不在“大厂授权”的时间线上，是中文/多合一卡带的代表作，也是
本项目里第一次需要**扩展区寄存器**（163）和**扫描线钩子**（163 的自动
CHR 切换）的 mapper。

### 8.1 Mapper 163：南京 FC-001

一张 32KB 窗口的卡带，但分页寄存器被拆到三个地址：

```
$5000  C... PPPP   bit0-3 PRG bank 低位，bit7 自动 4KB CHR RAM 切换
$5200  .... ..PP   PRG bank 高位
$5300  .... .A?B   bit0 写入时交换 D0/D1，bit2 决定 A15/A16 来源
```

复位时所有寄存器为 0，bit2 为 0 把 A15/A16 强制成 `11`——所以**开机
从 32KB bank 3 启动**，不是 bank 0。这一点必须写对，否则复位向量会从
别的 bank 读出，跑进另一段程序。

`$5100/$5101` 是一对防拷寄存器：游戏先锁存一个 F 位（数据 bit2），再
往 `$5101` 写 0 形成 E（数据 bit0）的下降沿把 F 翻转，最后从 `$5500`
读回**取反后的 F 位**。抄板没有这块逻辑，读回来就不对，游戏据此判断
真假卡。

`$5000` 的 bit7 打开“自动 4KB CHR 切换”：真机让 CHR A12 跟着 PPU A9
走，于是 nametable 上半屏用左 pattern table、下半屏用右 pattern table，
与卷轴无关（做“3D 墙”那种效果）。真机是逐 tile 看地址总线；本项目只
有扫描线，所以在 127/239 行切换，与 Nestopia、Mesen 的做法一致。

`金庸群侠传.nes`（2MB PRG、CHR RAM、带电池）实测：能进标题画面，
按 Start 后进入正式游戏画面。

### 8.2 Mapper 226：76 合 1

同一颗 ROM 芯片靠一个 8 位锁存器分页，但 7 位的 bank 号被拆得到处
都是，因为 bit6 已经被镜像占用：

```
$8000  PMOP PPPP   bit0-4 bank 低位，bit5 模式(0=32KB,1=16KB)，
                   bit6 镜像，bit7 bank 的第 6 位
$8001  .... ...H   bank 的第 7 位
```

模式 0 把整个 `$8000-$FFFF` 当作一个 32KB bank（bank 号的最低位被丢掉）；
模式 1 让同一个 16KB bank 同时出现在两个半区，这样小程序不用重定位。
1.5MB 的卡带（Super 42-in-1）把最高两位 bank 通过 `{0,0,1,2}` 的表重排，
这是接线怪癖，不是第二个 mapper。

## 9. 剩下的 Mapper：工作量估计

“实现到 15”不等于“15 个小任务”。难度分布极不均匀，下面是按投入排的
评估（含需要的额外基础设施和测试 ROM）：

| 编号 | 名字 | 难度 | 额外基础设施 | 代码量 | 测试 ROM |
|------|------|------|--------------|--------|----------|
| 8 / 12 / 14 | (罕见) | 中 | 无 | ~100 行 | 少数 |
| 16 | Bandai FCG | 中 | EEPROM 串口 | ~200 行 | 龙珠 |
| 48 | Taito TC0690 | 中 | MMC3 式 IRQ + 延迟 | ~200 行 | 侏罗纪公园 |
| 69 | Sunsoft FME-7 | 中高 | 扩展音源 | ~250 行 | 蝙蝠侠 ROTJ、Gimmick! |
| 45/74/191/192/195/199 | MMC3 clone | 中 | 无 | ~150 行/个 | 大量中文卡 |
| 176 | FK23C | 中高 | 无 | ~400 行 | 中文 RPG |
| 185/210/248 | 多合一 | 中 | 无 | ~150~250 行 | 1200-in-1 等 |
| 5 | MMC5 | **很高** | ExRAM、属性扩展、垂直分割、乘除法、PCM | 1000+ 行 | Just Breed、Metal Slader Glory |
| 24 / 26 | VRC6 | **高** | 扩展音源 | ~300 行 | 恶魔城传说(JP)、Esper Dream 2 |
| 85 | VRC7 | **高** | FM 音源 | ~400 行 | Lagrange Point |
| 6 | FDS | **高** | 磁盘镜像、BIOS、wavetable、IRQ | 600+ 行 | 磁盘版塞尔达 / 银河战士 |

> **Mapper 15 不是一个 32KB 分页器。** 早期的实现按“32KB bank”写，
> 结果复位向量从 bank 0 读出，跑进了另一段程序而死循环，画面全是方块。
> 正确版式是：**$8000-$BFFF 16KB 可切换，$C000-$FFFF 固定为最后一个
> 16KB**。这样复位/NMI/IRQ 向量永远在固定那一半，菜单可以常驻，游戏
> 放在可切换那一半。实测 `100合1.NES`（1MB PRG、64 个 16KB bank、
> CHR RAM）现在能启动到菜单，并且响应 Start 切换页面。

结论：**本项目已覆盖绝大多数授权游戏，以及常见的中文/多合一卡**。
剩下的大致分三档：

1. **授权单芯片板**：16 (Bandai)、48 (Taito TC0690)、69 (Sunsoft FME-7)；
2. **中文/非授权卡**：45/74/191/192/195/199 (MMC3 clone)、176 (FK23C)、
   185/210/248 (多合一)；
3. **大件**：5 (MMC5)、6 (FDS)，以及带扩展音源的 24/26 (VRC6)、
   85 (VRC7)，各自是独立子项目。

> **Mapper 19 的分页和 IRQ 已实现**（妖怪道中记能进标题），但 Namco 163
> 的波形音源尚未混入音频，所以游戏没有声音。Mapper 249（Waixing 的
> T9552）按 wiki 的 pattern-0 表实现；实测 `封神榜.nes` 能加载、不崩，
> 但还不能运行——该板的 bank 交叉比 wiki 表描述的更复杂，需要更多
> 逆向工作。

实际要补哪个，以 ROM 加载时报的编号为准。

### 不影响的保证

新增 mapper 的路径只有两条：

1. 新文件 `mapperN.hpp`；
2. `cartridge.cpp` 的 factory 里多一个 `case`。

对已有的 0/1/2/3/4/7/11 没有任何改动；需要额外钩子的 mapper 走的是
`on_ppu_address()` / `irq_asserted()` / `read_expansion()` /
`write_expansion()` / `on_scanline()` 这五个默认空实现，空实现意味着
其他 mapper 一行都不用改。

## 10. 用真实 ROM 验证

```bash
./build/demo_cartridge "/path/to/Super Mario Bros. 3.nes"
./build/tests/fc_tests --gtest_filter='Mapper*:Ines.*:Cartridge.*'
```

本地可以用 `custom_nes_core/tests/core/data/` 放若干游戏（Zelda II 验 MMC1、SMB3 验 MMC3）。

---
---

# Mapper: the logic on the cartridge (English summary)

A NES cartridge is not just a ROM chip; it is a small circuit board. The
**mapper** is the logic on that board. The 6502 can address 32KB of cartridge
space, and when a game was bigger the board added a bank register: the CPU
writes a number somewhere and the cartridge re-points a window at a different
bank.

- **Mapper 0 (NROM)**: no logic at all. Super Mario Bros.
- **Mapper 1 (MMC1)**: a five bit shift register. The CPU writes one bit at a
  time, and the **address** picks which of four registers (control, CHR bank 0,
  CHR bank 1, PRG bank) receives the completed value. The control register
  decides how the other three are interpreted, including runtime nametable
  mirroring. Zelda II and Tetris.
- **Mapper 4 (MMC3)**: adds a scanline IRQ counter. Super Mario Bros. 3.
- **Mapper 163 (Nanjing)**: a 32KB window with its bank registers down in the
  expansion area at $5000, plus a feedback bit a cartridge uses to prove it is
  not a copy. Chinese RPGs such as 金庸群侠传.
- **Mapper 226 (76-in-1)**: a pirate multicart whose seven bit bank number is
  split across two registers because one bit of $8000 is already mirroring.
  76合1 and Super 42-in-1.
