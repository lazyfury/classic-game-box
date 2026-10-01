# PPU — Picture Processing Unit

> 目标：理解 PPU 是一台**独立的计算机**，以及它如何把 8 个寄存器变成一幅画面。
>
> 本文输出都来自 `tools/demo_ppu.cpp`，并可由 `custom_nes_core/tests/core/test_ppu.cpp` 与
> `custom_nes_core/tests/core/test_real_rom.cpp` 验证。

---

## 1. PPU 是另一台计算机

```
              CPU                         PPU
         (Ricoh 2A03)                (Ricoh 2C02)
              1.79 MHz                    5.37 MHz
              16 位地址                   14 位地址
              自己的 RAM                  自己的 VRAM
                  |                            |
                  +------ 8 个寄存器 ----------+
                          $2000-$2007
```

**CPU 看不到 PPU 的任何内存。** 它只能读写 8 个寄存器，而整个画面是这
8 个字节的后果。

```
CPU $2000-$2007  ->  eight registers  ->  the whole picture
```

### 两个地址空间

PPU 有**自己的** 14 位总线（`$0000-$3FFF`），只能通过 `$2006`/`$2007` 访问：

```
$0000-$1FFF   pattern tables   （CHR ROM，在卡带上）
$2000-$2FFF   nametables       （主板上的 2KB VRAM）
$3000-$3EFF   nametables 的镜像
$3F00-$3F1F   palette RAM      （32 字节）
$3F20-$3FFF   palette 的镜像
```

**这就是为什么 `Cartridge::read_chr` 不是 `Device` 接口的一部分：**
pattern table 根本不在 CPU 的地址空间里。

---

## 2. 八个寄存器

| 地址 | 名称 | 读 | 写 |
|------|------|-----|-----|
| `$2000` | PPUCTRL | open bus | NMI 使能、精灵大小、pattern table 选择、地址增量、nametable 选择 |
| `$2001` | PPUMASK | open bus | 灰度、左列遮罩、背景/精灵开关、颜色强调 |
| `$2002` | PPUSTATUS | vblank / sprite0 / overflow | — |
| `$2003` | OAMADDR | open bus | OAM 地址 |
| `$2004` | OAMDATA | OAM 字节 | OAM 字节（并自增） |
| `$2005` | PPUSCROLL | open bus | 滚动，两次写入 |
| `$2006` | PPUADDR | open bus | VRAM 地址，两次写入 |
| `$2007` | PPUDATA | **缓冲**的 VRAM 字节 | VRAM 字节（并自增） |

### `$2002` 的三个副作用

```cpp
case 2: {   // PPUSTATUS
    const u8 result = (status_ & 0xE0) | (io_latch_ & 0x1F);
    status_ &= ~0x80;      // 清除 vblank
    nmi_occurred_ = false; // 并取消尚未被取走的 NMI
    write_toggle_ = false; // 并复位两次写入的开关
    return result;
}
```

**读一次状态寄存器同时做三件事。** 这是 NES 上最有副作用的读操作。

### `$2007` 的读取是缓冲的

```cpp
result = read_buffer_;             // 返回上一次取到的字节
read_buffer_ = read_vram(addr);    // 同时预取这一次的
```

**但调色板例外：** palette 太小，硬件没有为它做缓冲，所以读 `$3F00` 是立即的。

---

## 3. 滚动：五个字段和两个寄存器

**PPU 没有"滚动 X"这个字节。** 滚动是把一个 15 位地址拆成五个字段，
在两个寄存器之间复制。

```
    yyy NN YYYYY XXXXX
    ||| || ||||| +++++-- coarse X   横向第几个 tile（0-31）
    ||| || +++++-------- coarse Y   纵向第几个 tile（0-29）
    ||| ++-------------- nametable  选哪一块
    +++----------------- fine Y      tile 内的第几行（0-7）

    v = PPU 当前正在取数据的地址
    t = 下一帧/下一行应该从哪里开始
    x = fine X 滚动（0-7）
    w = 两次写入寄存器中，下一次是第几次
```

### 关键：t 的某些部分会在固定时刻被复制进 v

```cpp
void Ppu::copy_x() noexcept      // dot 257：整个横向部分（含 nametable 位）
{
    v_ = (v_ & 0xFBE0) | (t_ & 0x041F);
}

void Ppu::copy_y() noexcept      // 预渲染行 dot 280-304：整个纵向部分
{
    v_ = (v_ & 0x841F) | (t_ & 0x7BE0);
}
```

**这些复制动作就是滚动本身。**

程序写 `$2005` 设置 `t`，然后 PPU 按固定的节拍把 `t` 的部分搬进 `v`，
`v` 决定了下一行/下一帧从哪里开始取 tile。

> **为什么游戏能在关卡中途改变滚动？**
> 因为在任意时刻重写 `t`，下一次 `copy_x` 就会生效。
> 超级玛丽正是用这个（配合 sprite 0 hit）让顶部状态栏保持不动。

### 横向和纵向的自动步进

```cpp
void Ppu::increment_x() noexcept
{
    if ((v_ & 0x001F) == 31) {       // 走到第 32 个 tile
        v_ &= ~0x001F;
        v_ ^= 0x0400;                // 切换横向 nametable
    } else {
        ++v_;
    }
}

void Ppu::increment_y() noexcept
{
    if ((v_ & 0x7000) != 0x7000) {   // fine Y 还没到 7
        v_ += 0x1000;
        return;
    }
    v_ &= ~0x7000;
    u16 coarse_y = (v_ & 0x03E0) >> 5;
    if (coarse_y == 29) { coarse_y = 0; v_ ^= 0x0800; }   // 切换纵向 nametable
    else if (coarse_y == 31) { coarse_y = 0; }            // 30/31 行不存在
    else ++coarse_y;
    v_ = (v_ & ~0x03E0) | (coarse_y << 5);
}
```

---

## 4. 时序：扫描线和 dot

PPU 一次画一个像素，每秒 5,369,318 个：

```
一帧 = 262 条扫描线 × 每条 341 个 dot = 89342 个 dot
```

| 扫描线 | 内容 |
|--------|------|
| `-1` | 预渲染：清除标志、把 `t` 复制进 `v`、预取下一行的前两个 tile |
| `0-239` | 可见：每条画 256 个像素 |
| `240` | 后渲染：空闲 |
| `241-260` | vblank：通知 CPU，等它干活 |

```cpp
void Ppu::on_new_scanline() noexcept
{
    if (scanline_ == 241) {
        status_ |= 0x80;                              // vblank 标志
        if (ctrl_ & 0x80) nmi_occurred_ = true;       // 并触发 NMI
    }
    if (scanline_ == -1) {
        status_ &= ~0xE0;                             // 清除上一帧的标志
        sprite_zero_hit_ = false;
        sprite_overflow_ = false;
    }
}
```

### 为什么游戏必须在 vblank 干活

**这是 PPU 唯一"闭嘴"的时间。**

渲染期间写 PPU 寄存器会让画面出现撕裂、错位、闪烁。所以 NES 游戏的典型结构是：

```
主循环:
  游戏逻辑
  等 vblank（读 $2002 的 bit 7，或等 NMI）
  做 OAM DMA
  改滚动 / 调色板
```

---

## 5. 取指流水线：每 8 个 dot 一个 tile

```
dot % 8      动作
-------      ----
  0          把上一次取到的 tile 装进移位寄存器，然后取 nametable 字节
  2          取 attribute 字节
  4          取 pattern 的低位平面
  6          取 pattern 的高位平面
  7          coarse X + 1
```

移位寄存器是 16 位，**低 8 位是刚取到的 tile，高 8 位是正在画的 tile。**
这个一个 tile 的延迟就是流水线本身。

```cpp
void Ppu::load_shifters() noexcept
{
    shifter_lo_ = (shifter_lo_ & 0xFF00) | next_tile_lsb_;
    shifter_hi_ = (shifter_hi_ & 0xFF00) | next_tile_msb_;
    // attribute 每个 tile 只有 2 位，要摊到 8 个像素上
    attr_lo_ = (attr_lo_ & 0xFF00) | ((next_tile_attr_ & 1) ? 0x00FF : 0x0000);
    attr_hi_ = (attr_hi_ & 0xFF00) | ((next_tile_attr_ & 2) ? 0x00FF : 0x0000);
}
```

### 取数据

```cpp
void Ppu::fetch_tile_id()   { next_tile_id_ = read_vram(0x2000 | (v_ & 0x0FFF)); }

void Ppu::fetch_tile_attr()
{
    const u16 address = 0x23C0 | (v_ & 0x0C00) | ((v_ >> 4) & 0x0038) | ((v_ >> 2) & 0x0007);
    u8 attr = read_vram(address);
    if (v_ & 0x0040) attr >>= 4;   // 2x2 tile 块的下半
    if (v_ & 0x0002) attr >>= 2;   // 右半
    next_tile_attr_ = attr & 0x03;
}

void Ppu::fetch_tile_lsb()  { next_tile_lsb_ = read_vram(table + next_tile_id_ * 16 + fine_y); }
void Ppu::fetch_tile_msb()  { next_tile_msb_ = read_vram(table + next_tile_id_ * 16 + fine_y + 8); }
```

> **注意 `+ fine_y` 和 `+ fine_y + 8`。**
> 位平面是分离的（见 [ines-format.md](ines-format.md)），不是交错的。
> 这里写错会让图形完全错乱，但看起来又"像是有图案"，极难发现。

---

## 6. 像素合成

```cpp
// 背景：从移位寄存器取一位
const int bit = 15 - fine_x_;
bg_colour = (hi << 1) | lo;
bg_palette = (ahi << 1) | alo;

// 精灵：找到第一个不透明且横向命中的
for (each sprite) {
    const int offset = x - s.x;
    if (offset < 0 || offset > 7) continue;
    const u8 colour = ...;
    if (colour == 0) continue;      // 透明，试下一个
    ...
    break;
}

// 优先级
if (fg_colour != 0 && (bg_colour == 0 || fg_in_front)) {
    palette_addr = 0x10 + (fg_palette << 2) + fg_colour;
} else if (bg_colour != 0) {
    palette_addr = (bg_palette << 2) + bg_colour;
}
// 都是 0 就用 $3F00 的通用背景色
```

**精灵属性字节的 bit 5 是"在背景后面"**，不是"在前面"。这是容易搞反的一位。

---

## 7. Sprite 0 hit：游戏怎么知道光束在哪

**这是 PPU 告诉 CPU "我现在画到这一行了"的唯一办法。**

```
当不透明的精灵 0 像素 覆盖 不透明的背景像素 时，
$2002 的 bit 6 置位（然后只能等到下一帧的预渲染行才清除）。
```

```cpp
if (fg_is_zero && sprite_zero_in_range_ && bg_colour != 0 && fg_colour != 0 &&
    bg_on && sprites_on && x != 255) {
    sprite_zero_hit_ = true;
    status_ |= 0x40;
}
```

### 为什么超级玛丽需要它

```
游戏中：
  背景是滚动的
  但顶部的状态栏不能滚动

做法：
  1. 状态栏区域用背景 tile 画出来
  2. 放一个不可见的精灵 0 在状态栏下面
  3. 游戏等 sprite 0 hit
  4. hit 之后立刻把滚动设成 0
  -> 状态栏不动，下面的世界滚动
```

**没有 sprite 0 hit，这个效果做不出来。**
Phase 4 的测试专门验证了它真的会触发：

```cpp
TEST_F(RenderingTest, SpriteZeroHitIsUsedForTheStatusBar)
```

---

## 8. Sprite 的 8 个限制

```
每条扫描线最多 8 个精灵
超出时 $2002 的 bit 5（overflow）置位
```

```cpp
if (sprite_count_ >= 8) {
    sprite_overflow_ = true;
    status_ |= 0x20;
    break;   // 但仍然会被游戏用闪烁来规避
}
```

### 精灵的 Y 坐标是"上方那一行"

```
OAM 里的 Y = 19  ->  精灵显示在扫描线 20..27
```

```cpp
const int row = line - sprite_y - 1;
```

**这一位偏移是实际硬件行为。** 测试最初写错了，是三条精灵测试同时失败才发现的。

---

## 9. 调色板：32 字节，但每个精灵只有 3 色

```
$3F00        通用背景色
$3F01-$3F03  背景调色板 0
$3F05-$3F07  背景调色板 1
$3F09-$3F0B  背景调色板 2
$3F0D-$3F0F  背景调色板 3

$3F11-$3F13  精灵调色板 0
$3F15-$3F17  精灵调色板 1
$3F19-$3F1B  精灵调色板 2
$3F1D-$3F1F  精灵调色板 3
```

### `$3F10` 不是一个独立的字节

```cpp
u8 Ppu::palette_index(u16 address) const noexcept
{
    u8 index = (address - 0x3F00) & 0x1F;
    if (index == 0x10 || index == 0x14 || index == 0x18 || index == 0x1C) {
        index -= 0x10;   // $3F10 就是 $3F00
    }
    return index;
}
```

**所以每个精灵调色板的"第 0 色"是够不到的，实际只有 3 种可用颜色。**
第 0 色永远表示透明。

### 64 色，但同屏最多 25 色

```cpp
u32 Ppu::colour(u8 palette_index, bool greyscale) noexcept
{
    if (greyscale) palette_index &= 0x30;   // 灰度只保留高两位
    return kPalette[palette_index & 0x3F];
}
```

**注意灰度模式不是"去饱和"，而是把整个调色板塌缩到四行灰色上。**
这是硬件的做法。

---

## 10. 跑一个真实游戏

`demo_ppu` 跑 240 帧之后的输出（ASCII 预览，亮度分级）：

```
  |+++++++++++++++++++++++++++%+%+%+++++#+#+%+%+%+%+%+%+%+%+%+%+%%+|
  |+%+%%++++++++%+%+-++ +++ +%++++% +++ ## ++%++++++++%+++++++++%+%+|
  |+++++%%%%%%%+%+%+++++++++++%+%+%+%+%++++%%++%%+++++%+%%+%%+%+%+%+|
  |+%+%+%+%+%+%+%+%+++++++ +++%+%+%+%+%+++++++%+%+%+%+%+%+%+%+%+%+%|
  ...                                        （中间是大标题）
  |-###+###++##-###-###-###-###-###-###-###-###-###-###-###-###-###|
  |#---+#--+#--#---#---#---#---#---#---#---#---#---#---#---#---#-+#|
  |- ---#++#-##-###-###-###-###-###-###-###-###-###-###-###-###-#--|
```

如果把每个调色板颜色映射成一个字符，可以读出结构：

```
第 8-22 行   状态栏文字（MARIO / WORLD 1-1 / TIME）
第 40-150 行 SUPER MARIO BROS. 大标题
第 136-158 行 ©1985 NINTENDO
第 192-206 行 马里奥本人（红帽红衣 @@ = #b53120，肤色 $$ = #ea9e22）
第 208-238 行 地面砖块（重复图案）
```

调色板统计：

```
  72.7%  #9290ff   天空
   7.8%  #994e00   砖块/地面
   5.0%  #88d800   灌木
   4.6%  #fffeff   文字
   3.9%  #000000   轮廓
   3.2%  #feccc5   高光
```

**12 种颜色，符合 NES 的限制（同屏最多 25 色）。**

### 测试断言的东西

```cpp
TEST_F(RenderingTest, TheFrameIsNotBlank)        // 6 <= colours <= 25
TEST_F(RenderingTest, ThePictureHasTheShapeOfAScreen)
TEST_F(RenderingTest, SpriteZeroHitIsUsedForTheStatusBar)
TEST_F(RenderingTest, ThePpuAddressSpaceIsPopulatedWithRealData)
TEST_F(RenderingTest, NoIllegalOpcodeIsEverReached)
```

最后一条是关于 CPU 的最强断言：

> **跑一个真实商业游戏 60 帧，从不执行一个 6502 未定义的字节。**

---

## 11. 这一阶段修掉的两个真 bug

### 1. 位平面是分离的，不是交错的

`demo_cartridge` 最初按交错格式读 CHR：

```cpp
// 错误
const u8 plane0 = chr[tile * 16 + y * 2];
const u8 plane1 = chr[tile * 16 + y * 2 + 1];
```

**PPU 里的实现反而是对的**（`+ fine_y` / `+ fine_y + 8`）。

发现方式：PPU 渲染测试失败，一路查到测试数据，再查到 demo 的渲染器。
结论：**两个地方对同一份格式有两种理解，其中一个是错的。**

### 2. 精灵的 Y 是"上方那一行"

```cpp
// 错误：精灵会整体下移一行
const int row = line - sprite_y;

// 正确
const int row = line - sprite_y - 1;
```

发现方式：三条精灵测试同时失败。

---

## 12. 代码对应

| 概念 | 文件 |
|------|------|
| PPU 主体 | `custom_nes_core/src/core/nes/ppu.{hpp,cpp}` |
| 帧缓冲 | `custom_nes_core/src/core/nes/framebuffer.hpp` |
| CPU/PPU 3:1 同步 | `custom_nes_core/src/core/nes/machine.{hpp,cpp}` |
| 单元测试 | `custom_nes_core/tests/core/test_ppu.cpp` |
| 真实 ROM 渲染测试 | `custom_nes_core/tests/core/test_real_rom.cpp` |
| 可运行讲解 | `tools/demo_ppu.cpp` |

```bash
./build/demo_ppu                      # 写 frames/frame_N.ppm
./build/tests/fc_tests --gtest_filter='Ppu.*:Machine.*:RenderingTest.*'
```

**PPM 可以用任何图像工具打开，macOS 上：**

```bash
sips -s format png frames/frame_240.ppm --out frame.png
```

---

## 13. 自测

1. CPU 能看到 PPU 的 VRAM 吗？为什么？
2. `$2002` 一次读操作做了哪三件事？
3. `$2007` 的读取为什么要缓冲？谁不缓冲？
4. `v`、`t`、`x`、`w` 分别是什么？
5. `copy_x()` 在什么时候发生？`copy_y()` 呢？
6. 为什么每条扫描线只能画 8 个精灵？
7. `$3F10` 和 `$3F00` 是什么关系？为什么精灵只有 3 种颜色？
8. sprite 0 hit 是用来做什么的？
9. OAM 里 Y = 19 的精灵显示在哪几条扫描线上？
10. PPU 的移位寄存器为什么是 16 位？

<details>
<summary>答案</summary>

1. 不能。PPU 有自己独立的 14 位地址空间。CPU 只能通过 `$2006`/`$2007` 这一对寄存器间接访问
2. 清除 vblank 标志、取消尚未被取走的 NMI、复位两次写入的开关
3. 因为 PPU 的 VRAM 比 CPU 慢，需要在 CPU 读的时候同时预取下一个字节。调色板例外——它太小了，硬件没有为它做缓冲
4. `v` = 当前取数据的 VRAM 地址；`t` = 下一行/下一帧的起始地址；`x` = fine X 滚动（0-7）；`w` = 两次写入寄存器的第几次
5. `copy_x()` 在 dot 257（每行一次）；`copy_y()` 在预渲染行的 dot 280-304（每帧一次）
6. 因为硬件只有 8 组精灵的移位寄存器。超出的会被丢弃，但 overflow 标志会置位
7. `$3F10` 就是 `$3F00`（镜像）。所以精灵调色板的第 0 色和通用背景色是同一个字节，实际只有 3 色可用，第 0 色表示透明
8. 让 CPU 知道 PPU 画到了哪一行。超级玛丽用它把状态栏固定住，同时让下面的世界滚动
9. 扫描线 20 到 27（8x8）
10. 因为要同时容纳"正在画的 tile"（高 8 位）和"刚取到的 tile"（低 8 位）。这一个 tile 的延迟就是流水线

</details>

---

**上一章：** [ines-format.md](ines-format.md)
**下一阶段：** Phase 5 — Controller（按键输入）
