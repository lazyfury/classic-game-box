# iNES 文件格式与 Mapper The iNES Format and Mappers

> 目标：理解 `.nes` 文件的布局、卡带如何决定地址含义、以及图形数据是怎么存的。
>
> 本文输出都来自 `tools/demo_cartridge.cpp`，并可由 `custom_nes_core/tests/core/test_cartridge.cpp`
> 与 `custom_nes_core/tests/core/test_real_rom.cpp` 验证。

---

## 1. 文件布局

```
0x0000   16 字节    iNES 文件头
0x0010   512 字节   trainer（仅当 flags6 bit 2 置位）
         16KB × N   PRG ROM —— 程序，回答 CPU 的 $8000-$FFFF
         8KB  × M   CHR ROM —— 图形，回答 PPU（不是 CPU）
```

**一个真实的例子**（`超级玛丽.nes`，40976 字节）：

```
  offset  bytes                     meaning
  ------  -----------------------   ---------------------------
     0    4e 45 53 1a               "NES" + 0x1A: the signature
     4    02                        PRG ROM pages, 16KB each
     5    01                        CHR ROM pages, 8KB each
     6    01                        flags 6: mapper low nibble, mirroring
     7    00                        flags 7: mapper high nibble
     8    00 00 00 00 00 00 00 00   unused here

  0x0000      16 bytes   iNES header
  0x0010   32768 bytes   PRG ROM
  0x8010    8192 bytes   CHR ROM
  total: 40976 bytes, header claims 40976
```

`0x8010 = 16 + 32768`。**文件大小必须与文件头声称的一致**，否则多半是个被截断的
dump —— 这是个很常见的失败情形，所以 `parse_ines_header()` 专门检查它。

---

## 2. 那 16 个字节

```
byte 0-3    "NES" 0x1A          签名
byte 4      PRG pages           每页 16KB
byte 5      CHR pages           每页 8KB；0 表示 CHR RAM
byte 6      flags 6             mapper 低 4 位 / 镜像 / 电池 / trainer
byte 7      flags 7             mapper 高 4 位 / NES 2.0 标记
byte 8-15   通常为 0
```

### mapper 号为什么被拆成两半

```
flags6:  NNNN BBMM     N=3, B=1, M=1
flags7:  NNNN xxxx     N=4

mapper = (flags6 >> 4) | (flags7 & 0xF0)
```

看起来很怪，但原因很实际：**flags 7 是后来才加的**（因为 mapper 号不够用了），
它必须塞进当时没人使用的位里。而 flags 6 里只有高 4 位是空的。

真实例子：

```
mapper 3   -> flags6 = 0x30, flags7 = 0x00
mapper 4   -> flags6 = 0x40, flags7 = 0x00
mapper 0x21-> flags6 = 0x10, flags7 = 0x20
```

实测（`demo_cartridge`）：

```
      flags6 = 0000 0001  = $01   high nibble $00 is the LOW nibble of the mapper
      flags7 = 0000 0000  = $00   high nibble $00 is the HIGH nibble of the mapper
```

### 镜像模式

| flags6 bit 3 | flags6 bit 0 | 模式 |
|---|---|---|
| 1 | — | FourScreen（覆盖 bit 0） |
| 0 | 1 | **Vertical** |
| 0 | 0 | Horizontal |

**镜像由卡带决定，因为额外的 VRAM 芯片在卡带上。** 4KB VRAM 可以水平接也可以垂直接；
自带额外 RAM 的卡带能提供四个屏幕。

---

## 3. Mapper：卡带自己的地址译码器

### 为什么需要 mapper

**6502 只能寻址 32KB 卡带空间（`$8000-$FFFF`）。**

早期游戏放得下，所以不需要 mapper。后来的游戏放不下，于是 mapper 加了一个
**bank 寄存器**：

```
CPU 往 $8000 写 $05  ->  卡带把 bank 5 换到 $A000-$BFFF
```

**ROM 芯片本身没变。变的只是卡带内部的连线。**

**这就是为什么 mapper 要用代码模拟，而不是用数据**：它是逻辑，不是存储。

### Mapper 0（NROM）

```
PRG ROM:  16KB 或 32KB，完全不做 bank 切换
CHR ROM:  8KB，完全不做 bank 切换
```

这是**板上没有任何额外逻辑**的卡带。没有东西可以切换，所以往 PRG 区写入
不会有任何效果。

#### 16KB 的情况：第三种镜像

16KB 的 PRG ROM 填不满 `$8000-$FFFF`，所以卡带把它接到**两半**上：

```
$8000-$BFFF  ->  ROM[0x0000-0x3FFF]
$C000-$FFFF  ->  ROM[0x0000-0x3FFF]   又一次
```

```cpp
[[nodiscard]] u8 read_prg(u16 address) override
{
    const std::size_t offset = static_cast<std::size_t>(address - 0x8000u);
    // 16KB 会镜像，32KB 不会。一个取模同时覆盖两种情况。
    return prg_[offset % prg_.size()];
}
```

**这是同一件事的第三次出现**（前两次是 RAM 和 PPU 寄存器）：
**没有剩余的地址线来区分两半，于是同一个芯片应答两次。**

### 卡带内部的分区

```
$4020-$5FFF   expansion area    NROM 上什么都没有
$6000-$7FFF   PRG RAM           8KB 可写（存档 / 工作内存）
$8000-$FFFF   PRG ROM           经由 mapper
```

---

## 4. CHR：像素到底怎么存的

CHR ROM **不能从 CPU 访问**。它接在 PPU 自己的总线上——所以
`read_chr`/`write_chr` 是独立于 `Device` 接口的函数，而不是 `read`/`write`。

### 一个 tile 是 16 字节，而且位平面是**分离**的

**这是读 pattern table 时最常见的错误：两个位平面不是交错的。**

```
byte 0-7   = plane 0, row 0 .. row 7
byte 8-15  = plane 1, row 0 .. row 7

每个像素的颜色索引 = (plane1 的对应位 << 1) | plane0 的对应位
                   -> 0, 1, 2, 3 四种
```

对着写：

```
byte 0   plane 0, row 0      byte 8   plane 1, row 0
byte 1   plane 0, row 1      byte 9   plane 1, row 1
...                          ...
byte 7   plane 0, row 7      byte 15  plane 1, row 7
```

**如果是交错的（`byte 2y` / `byte 2y+1`），读出来的图形会完全错乱，
但看起来又“像是”有图案，所以很难发现。** 这个项目一开始就犯了这个错，
在 `demo_cartridge` 里画出了错误的 tile；PPU 实现反而是对的。

### 渲染出来（`demo_cartridge` 第 8 节）

```
  tile 0 ($0):
      |      ..|
      |    ....|
      |   .....|
      |   .....|
      |   OOOoo|
      |  OooOoo|
      |  OooOOo|
      | OOooOOo|
```

**注意这只显示形状。** 四种颜色由 PPU 的调色板决定，那是 Phase 4。

### 一个值得注意的发现

`demo_cartridge` 的实测输出：

```
  512 tiles total, 2 of them blank.
```

**原版《超级玛丽》的 CHR 应该有大量空白 tile。** 而 PRG 部分明确是《超级玛丽》
（启动代码和中断向量都对得上）。

```
  A note on this particular dump: the tile shapes above do not look
  like the standard Super Mario Bros tileset, and only 2 of 512 tiles
  are blank where an unmodified ROM would have many more. The PRG ROM
  is clearly Super Mario Bros - the startup code and the vectors are
  correct - so this file is very likely a modified or bootleg version.
```

**这是模拟器开发中的真实情况：不能假设手上的 ROM 是标准的。**
工具的价值之一就是**让这种差异变得可见**，而不是把它平滑掉。

---

## 5. 跑一个真实 ROM

`demo_cartridge` 第 6、7 节的真实输出：

```
    $8000  78         SEI                 ; implied
    $8001  D8         CLD                 ; implied
    $8002  A9 10      LDA #$10            ; immediate
    $8004  8D 00 20   STA $2000           ; absolute
    $8007  A2 FF      LDX #$FF            ; immediate
    $8009  9A         TXS                 ; implied
    $800A  AD 02 20   LDA $2002           ; absolute
    $800D  10 FB      BPL $800A           ; relative
    $800F  AD 02 20   LDA $2002           ; absolute
    $8012  10 FB      BPL $800F           ; relative
```

**这是用我们自己写的反汇编器读出来的真实商业 ROM。**

### 运行轨迹

```
    $8000  SEI             A=$00 X=$00 SP=$FD  ..-..I..
    $8001  CLD             A=$00 X=$00 SP=$FD  ..-..I..
    $8002  LDA #$10        A=$10 X=$00 SP=$FD  ..-..I..
    $8004  STA $2000       A=$10 X=$00 SP=$FD  ..-..I..
    $8007  LDX #$FF        A=$10 X=$FF SP=$FD  N.-..I..
    $8009  TXS             A=$10 X=$FF SP=$FF  N.-..I..
```

### 然后它卡住了 —— 而这是正确的

```
  Ran 10000 more instructions (35000 cycles).
  PC is now $800A.
  A = $20, N flag = 0
```

**为什么卡在这里，原因非常精确：**

```
$2002 是 PPU 的状态寄存器。现在还没有 PPU，所以这次读返回 open bus ——
CPU 最后放上数据总线的那个字节，即 $20（$2002 操作数的高字节）。
$20 的 bit 7 是 0，所以 N 清零，所以 BPL 一直跳。
```

> **注意 open bus 的值是 `$20` 而不是 `$10`（最后写入的值）。**
> 因为 `LDA $2002` 取操作数时把 `$20` 放上了总线。这是真实行为，
> 测试专门断言了它。

**这不是 bug，而是 Phase 3 的精确边界：**

| 部件 | 状态 |
|------|------|
| CPU | ✅ 全部 151 个 opcode |
| 总线 | ✅ 完整地址译码 |
| 卡带 | ✅ iNES + Mapper 0 + 真实 ROM |
| PPU | ❌ Phase 4 |

**模拟器把自己能做的都做完了，然后停在等一个还不存在的硬件上。**

测试用一个 `VblankStubPpu`（只在 `$2002` 返回 `$80`）证明了这一点：

```cpp
TEST_F(SuperMarioBros, TheVblankWaitIsExactlyWhatWasBlocking)
{
    // 同样的 ROM、同样的指令数，唯一的差别是 $2002 由谁应答
    EXPECT_EQ(cpu_a.registers().pc, 0x800A);   // 卡住
    EXPECT_EQ(cpu_b.registers().pc, 0x800F);   // 通过
    EXPECT_FALSE(cpu_a.registers().flag(Flag::Negative));   // BPL 循环
    EXPECT_TRUE(cpu_b.registers().flag(Flag::Negative));    // BPL 落空
}
```

**一个 bit 的差别，就是"运行"和"死机"的差别。**

---

## 6. 真实 ROM 测试怎么跑

真实 ROM 有版权，不能进仓库。所以：

```bash
# 方式一：符号链接
ln -s /path/to/game.nes custom_nes_core/tests/core/data/game.nes

# 方式二：环境变量
FC_TEST_ROM=/path/to/game.nes ./build/tests/fc_tests

# 方式三：demo 直接传路径
./build/demo_cartridge /path/to/game.nes
```

`.gitignore` 里有 `*.nes` 和 `custom_nes_core/tests/core/data/`，所以不会误提交。

**找不到 ROM 时，测试自己跳过**，而不是失败：

```cpp
void SetUp() override
{
    rom_ = find_test_rom();
    if (!rom_) {
        GTEST_SKIP() << "no .nes file in " << FC_TEST_DATA_DIR;
    }
    ...
}
```

**这是测试外部数据时该有的做法：环境不具备就跳过，而不是让 CI 变红。**

---

## 7. 代码对应

| 概念 | 文件 |
|------|------|
| iNES 文件头解析 | `custom_nes_core/src/core/nes/ines.{hpp,cpp}` |
| Mapper 接口 | `custom_nes_core/src/core/nes/mapper.hpp` |
| Mapper 0 (NROM) | `custom_nes_core/src/core/nes/mapper0.hpp` |
| 卡带（PRG RAM、CHR、分派） | `custom_nes_core/src/core/nes/cartridge.{hpp,cpp}` |
| 合成 ROM 测试 | `custom_nes_core/tests/core/test_cartridge.cpp` |
| 真实 ROM 测试 | `custom_nes_core/tests/core/test_real_rom.cpp` |
| 可运行讲解 | `tools/demo_cartridge.cpp` |

```bash
./build/demo_cartridge
./build/tests/fc_tests --gtest_filter='Ines.*:Mapper0.*:Cartridge.*:SuperMarioBros.*'
```

---

## 8. 自测

1. 一个 `.nes` 文件是 81936 字节，文件头说 PRG=2、CHR=1。这有问题吗？
2. 一个文件头的 flags6 = `0x41`，mapper 是多少？镜像是什么？
3. mapper 0 的 16KB PRG ROM，读 `$C000` 会得到什么？
4. 往 `$8000` 写一个字节会发生什么？
5. 为什么 `read_chr` 不是 `Device` 接口的一部分？
6. 一个 tile 的 16 个字节里，第 6、7 个字节是什么？
7. 为什么 `LDA $2002` 读到 open bus 时是 `$20` 而不是上一次写入的值？

<details>
<summary>答案</summary>

1. 有问题。`16 + 2×16384 + 1×8192 = 40976`，不是 81936。多半是重复了数据或者文件头撒谎
2. `flags6 = 0x41` → 高 4 位是 4 → mapper 低 4 位 = 4；bit 0 = 1 → Vertical 镜像。
   若 `flags7` 高 4 位为 0，则 mapper = 4
3. 与 `$8000` 相同的字节。16KB ROM 在 `$8000-$BFFF` 和 `$C000-$FFFF` 两处镜像
4. 什么都不发生。NROM 没有 bank 寄存器，写入是开路
5. 因为 CHR 接在 PPU 的总线上，不在 CPU 的地址空间里。它们是两个独立的地址空间
6. 第 6 个字节 = plane 0 的第 3 行，第 7 个字节 = plane 1 的第 3 行。
   每个 tile 有 8 行，每行 2 字节
7. 因为 `LDA $2002` 要先取两个操作数字节 `$02` 和 `$20`。取 `$20` 时它被放上了数据总线，
   所以 open bus 变成了 `$20`。指令取指和取操作数**也会**驱动总线

</details>

---

**上一阶段：** [../architecture/bus.md](../architecture/bus.md)
**下一阶段：** Phase 4 — PPU（让 `$2002` 真的有东西应答）
