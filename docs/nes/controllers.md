# 手柄 Controller

> 目标：理解 NES 手柄的串行协议，以及为什么"模拟输入"和"真实按键"在模拟器里是同一件事。
>
> 本文输出都来自 `tools/demo_input.cpp`，并可由 `custom_nes_core/tests/core/test_controller.cpp`
> 与 `custom_nes_core/tests/core/test_real_rom.cpp` 验证。

---

## 1. 只有几根线，所以必须串行

主机到手柄的线很少。所以 8 个按键**不是并行读取的**。

手柄里有一个 **4021 移位寄存器**，它同时保存 8 个按键的状态；CPU 先锁存，
然后一次一个把位移出来：

```
write $4016 = 1     锁存：把按键状态装进移位寄存器
write $4016 = 0     停止锁存，开始移位

read  $4016 bit 0   A
read  $4016 bit 0   B
read  $4016 bit 0   Select
read  $4016 bit 0   Start
read  $4016 bit 0   Up
read  $4016 bit 0   Down
read  $4016 bit 0   Left
read  $4016 bit 0   Right
read  $4016 bit 0   1，以及之后每一次
```

`demo_input` 第 1 节的实测输出（按住 A 和 Right）：

```
      clock  button   bit
      -----  -------  ---
        1    A        1
        2    B        0
        3    Select   0
        4    Start    0
        5    Up       0
        6    Down     0
        7    Left     0
        8    Right    1
        9    (nothing)  1
        10    (nothing)  1
        11    (nothing)  1
```

### 顺序由接线决定，不能改

按键是**焊死**在移位寄存器的固定输入上的。没有任何程序能改变"第几个时钟出哪个键"。

**所有读手柄的 NES 程序都依赖这个确切的顺序。** 顺序错了，游戏里的按键会全部错位
（按 A 变成按 Right 之类）。

### 最后那个 1 不是填充

**程序读 9 次，检查第 9 次是不是 1**，以此判断端口上插的是标准手柄还是别的东西
（光枪、扩展设备、或者什么都没插）。

打鸭子（Duck Hunt）就是用这个来区分光枪和手柄的。

---

## 2. 锁存：按住时读多少次都一样

```cpp
void Controller::strobe(bool high) noexcept
{
    strobing_ = high;
    if (high) {
        shift_ = buttons_;   // 锁存期间持续重装
    }
}

u8 Controller::read() noexcept
{
    if (strobing_) {
        return buttons_ & 0x01;   // 锁存期间永远返回 A
    }
    const u8 value = shift_ & 0x01;
    shift_ = (shift_ >> 1) | 0x80;   // 从高位补 1
    return value;
}
```

实测：

```
  While the latch is held the register reloads every time, so the
  line never advances:

      0 0 0 0 0 0    <- A is not pressed, so all zero, and it never moves on
```

### 锁存是"快照"，不是"跟随"

```cpp
TEST(Controller, TheStrobeTakesASnapshot)
```

按下 A → 锁存 → 释放 A、按下 Right → 读出来的 8 位**仍然是锁存时的状态**。

**这一点欺骗了很多初学者**：如果程序只在每帧开始时锁存一次，那么帧中途的按键
变化要等下一帧才被看到。

---

## 3. 两个端口共用一根锁存线

```
$4016 写   -> 锁存两个端口
$4016 读   -> 手柄 1
$4017 读   -> 手柄 2
$4017 写   -> APU 的帧计数器，不是锁存
```

**这个不对称是真实的**：两个端口共用一根 strobe 线。

```cpp
// custom_nes_core/src/core/nes/bus.cpp
if (address == kController1) {          // $4016 写
    const bool high = (value & 0x01u) != 0;
    controllers_[0].strobe(high);
    controllers_[1].strobe(high);       // 两个一起
    return;
}
// $4017 的写落到 APU
```

测试：

```cpp
TEST(ControllerBus, TheStrobeReachesBothPorts)
TEST(ControllerBus, PortTwoWritesDoNotStrobe)
```

---

## 4. 只有 bit 0 是接着的

```
read $4016 的 bit 0     手柄
read $4016 的 bit 1-7   open bus
```

```cpp
u8 NesBus::read_controller(int index) noexcept
{
    const u8 bit = controllers_[index].read();
    return (open_bus_ & 0xFEu) | (bit & 0x01u);
}
```

**所以程序必须掩码。** 不掩码的程序会读到总线上残留的东西。

```cpp
TEST(ControllerBus, TheUpperBitsAreOpenBus)
```

---

## 5. 一个真实的读取程序

`demo_input` 用的是超级玛丽同款写法（`custom_nes_core/tests/core/test_controller.cpp` 里也有）：

```
      LDA #$01
      STA $4016     ; 锁存
      LDA #$00
      STA $4016     ; 开始移位
      LDX #$08
loop  LDA $4016
      LSR A         ; bit 0 进 carry
      ROL $10       ; carry 进 $10 的最低位，其余左移
      DEX
      BNE loop
```

**8 次之后，第一个读到的键在 bit 7，最后一个在 bit 0：**

```
$10 = A<<7 | B<<6 | Select<<5 | Start<<4 | Up<<3 | Down<<2 | Left<<1 | Right
```

测试验证了这个位布局：

```cpp
TEST(ControllerCpu, AProgramCanReadAllEightButtonsIntoAZeroPageByte)
    // 按 Start -> $10 == 0x10
TEST(ControllerCpu, TheDirectionPadBitsAreInTheRightOrder)
    // 按 Up + Right -> $10 == 0x09
```

---

## 6. 用真实游戏验证：按下 Start

这是 Phase 5 的验收标准。`demo_input` 第 2、3 节：

### 不按任何键

```
  Ran 120 frames with no buttons pressed.
  Pixels changed: 218 of 61440  (0.35%)
```

**120 帧只有 218 个像素变化** —— 标题画面基本静止，只有一个光标在闪。游戏在等按键。

### 按下 Start 5 帧

```
  Pixels changed: 57207 of 61440  (93.11%)
```

**93% 的像素变了。** 画面完全换掉了——游戏开始了。

```
  The screen is gone. Five frames of input, sampled once by the
  game's vblank routine, was enough to start it.
```

**按住 5 帧就够了。** 因为游戏每帧在自己的 vblank 例程里采样一次手柄，
所以按键只要"活过"一次 vblank 就会被注意到。测试确认 1 帧也够：

```cpp
TEST_F(InputTest, AFiveFramePressIsEnough)
TEST_F(InputTest, AOneFramePressIsEnough)
```

### 反复按会怎样

```
按住  5 帧:  变化像素 = 57207
按住 10 帧:  变化像素 = 57207
按住 20 帧:  变化像素 = 57207
按住 40 帧:  变化像素 = 57207
```

**完全一样。** 因为游戏只在第一次采样时响应 Start，之后的按住没有额外效果。

**这就是"手柄没有任何去抖、连发、长按逻辑"的直接证据** —— 那些全部在游戏软件里。
所以同一个硬件在不同游戏里手感不同。

---

## 7. 按住 Right

`demo_input` 第 4 节：

```
  Holding Right for 180 frames...

  Pixels changed: 14763
  The view has scrolled: Mario walked and the world moved past him.
```

---

## 8. 怎么确认游戏真的在读手柄

**不要靠看画面猜。** 我加了一个读计数：

```cpp
[[nodiscard]] u64 Controller::read_count() const noexcept;
```

`demo_input` 第 5 节：

```
    frame   total reads
    -----   -----------
        0          6216
        1          6224
        2          6232
        3          6240
        4          6248
        5          6256
```

**每帧精确 +8。** 说明游戏每帧完整轮询 8 个按键。

> **一个停止读取手柄的游戏会立刻在这里暴露。**
> 这比"看着画面觉得马里奥没动"要可靠得多——毕竟马里奥也可能是被墙挡住了。

---

## 9. 为什么脚本输入和真实键盘是同一件事

```cpp
// custom_nes_core/src/core/nes/machine.hpp
void set_button(Controller::Button button, bool pressed, int index = 0) noexcept
{
    bus_.controller(index).set_button(button, pressed);
}
```

**Machine 不关心按键从哪来。**

```
    键盘 / 触屏 / 录像回放 / 测试脚本
                   |
                   v
        Controller::set_button(A, true)
                   |
                   v
              Controller            <- 到这里为止都一样
                   |
                   v
               $4016 协议
                   |
                   v
                 游戏
```

**收益：**

| 用途 | 做法 |
|------|------|
| 测试 | `machine.set_button(Start, true)` |
| 录像回放 | 每帧把记录下来的按键状态喂进去 |
| 自动化测试（TAS） | 脚本输入 + 逐帧控制 |
| 真实前端 | 键盘事件映射到同一个调用 |

**Phase 7 接 Electron 前端时，键盘处理只需要调用这一个函数。**
Core 不需要任何改动——这是"Core 不依赖 UI"这条规则换来的。

> 甚至可以说：**一个能回放录像的模拟器，和一个人在对局中操作的模拟器，
> 在 Core 看来没有任何区别。** 这不是巧合，是接口设计的目标。

---

## 10. Controller 里**没有**什么

这一节和"有什么"一样重要：

| 没有 | 在哪里 |
|------|--------|
| 中断 | 没有。只能轮询 |
| 时序 / 帧的概念 | 没有。读多少次都行 |
| 去抖（debounce） | 在游戏里 |
| 连发（turbo） | 在硬件外设或游戏里 |
| 长按判定 | 在游戏里 |
| 组合键 | 在游戏里 |
| 模拟摇杆 / 模拟量 | **没有**。只有 8 个开关 |

**它是一个带锁存的移位寄存器，仅此而已。**
所有"手感"都是软件造出来的。

---

## 11. 代码对应

| 概念 | 文件 |
|------|------|
| 手柄与协议 | `custom_nes_core/src/core/nes/controller.hpp` |
| 两个端口与锁存线 | `custom_nes_core/src/core/nes/bus.{hpp,cpp}` |
| 脚本输入接口 | `custom_nes_core/src/core/nes/machine.hpp` `set_button()` |
| 协议单元测试 | `custom_nes_core/tests/core/test_controller.cpp` |
| 真实游戏输入测试 | `custom_nes_core/tests/core/test_real_rom.cpp` `InputTest` |
| 可运行讲解 | `tools/demo_input.cpp` |

```bash
./build/demo_input                    # 写 frames/input_*.ppm
./build/tests/fc_tests --gtest_filter='Controller*:InputTest.*'
```

---

## 12. 自测

1. 为什么手柄是串行读取而不是并行？
2. 8 个按键移出的顺序是什么？为什么不能改？
3. 第 9 次读取为什么是 1？这有什么用？
4. 锁存期间读 5 次会得到什么？
5. 释放 A 并按下 Right 之后，之前锁存的 8 位会变吗？
6. 写 `$4017` 会锁存手柄吗？
7. `read $4016` 的高 7 位是什么？
8. 为什么"按住 Start 5 帧"和"按住 40 帧"效果完全一样？
9. 如果游戏停止读取手柄，怎么最快发现？

<details>
<summary>答案</summary>

1. 因为主机到手柄的线很少，装不下 8 根并行信号线。用移位寄存器把 8 位串行送出
2. A B Select Start Up Down Left Right。按键焊死在移位寄存器的固定输入上，改不了
3. 移位寄存器空位补 1。程序用它区分标准手柄和光枪/扩展设备/空端口
4. 5 次都返回 A 的状态——锁存期间寄存器持续重装，不前进
5. 不会。锁存是快照，不是跟随
6. 不会。`$4017` 的写是 APU 的帧计数器。只有 `$4016` 的写是锁存（但会同时锁存两个端口）
7. open bus —— 总线上残留的值。所以程序必须掩码只用 bit 0
8. 因为游戏每帧只在 vblank 里采样一次。第一次采样就已经响应了 Start，后面按住没有新信息
9. 看 `Controller::read_count()`。每帧应该稳定增加 8；停止读取会立刻停止增长

</details>

---

**上一章：** [ppu.md](ppu.md)
**下一阶段：** Phase 6 — APU（声音）
