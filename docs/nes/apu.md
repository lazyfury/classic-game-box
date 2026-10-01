# APU — Audio Processing Unit

> 目标：理解 NES 的五个声道是什么、它们为什么这么简单，以及一个真实游戏怎么用它们做出音乐。
>
> 本文输出都来自 `tools/demo_apu.cpp`，并可由 `custom_nes_core/tests/core/test_apu.cpp` 与
> `custom_nes_core/tests/core/test_real_rom.cpp` 验证。

---

## 1. 它不是独立芯片

```
         +----------------------------------+
         |        Ricoh 2A03                |
         |                                  |
         |   CPU core  ------+--- APU       |
         +------------------|---------------+
                            |
                   $4000-$4017 (CPU 地址空间)
```

APU 和 CPU **在同一块硅片上**。这就是为什么它共享 CPU 时钟、为什么它的寄存器在 CPU 的地址空间里。

```
CPU  1.789773 MHz      基准
PPU  5.369319 MHz      正好 3 倍
APU  0.894886 MHz      正好 1/2
```

---

## 2. 五个声部

| 声道 | 寄存器 | 是什么 |
|------|--------|--------|
| Pulse 1 | `$4000-$4003` | 方波，四种占空比 |
| Pulse 2 | `$4004-$4007` | 同上，可以和 1 失谐 |
| Triangle | `$4008-$400B` | 三角波，**完全没有音量控制** |
| Noise | `$400C-$400F` | 移位寄存器假装是随机数 |
| DMC | `$4010-$4013` | 从 CPU 内存读采样的差分调制 |

加两个全局寄存器：

```
$4015  写：启用哪些声道      读：哪些在播放
$4017  帧序列器：4 步 / 5 步，以及 IRQ 屏蔽
```

> 注：`$4016`/`$4017` 的**读**是手柄（见 [controllers.md](controllers.md)）。
> `$4017` 的**写**是 APU 帧序列器。这是真实的不对称。

---

## 3. 方波：八位移位寄存器

**方波不是"生成"出来的。** 脉冲声道每计满一个定时器，就从四种 8 位模式里取一位作为输出：

```
duty 0  (12.5%)
   ####                                  #####                    
duty 1  (25%)
   #########                             ##########               
duty 2  (50%)
   ###################                   ####################     
duty 3  (25% negated)
  #         ####################     ####          ###############
```

```cpp
inline constexpr u8 kDutyTable[4][8] = {
    { 0, 1, 0, 0, 0, 0, 0, 0 },   // 12.5%
    { 0, 1, 1, 0, 0, 0, 0, 0 },   // 25%
    { 0, 1, 1, 1, 1, 0, 0, 0 },   // 50%
    { 1, 0, 0, 1, 1, 1, 1, 0 },   // 25% 取反
};
```

**duty 3 就是 duty 1 上下颠倒。** 这就是全部区别——也是大多数 NES 音乐里两个主旋律声部的区别。

### 音高

```
period = timer + 1 个 APU 周期
```

定时器是 11 位（`$4002` 低 8 位 + `$4003` 低 3 位）。

---

## 4. 包络：硬件唯一提供的"乐器"

**没有 attack / decay / sustain / release。** 只有一个从 15 开始、每 (period+1) 个四分帧减 1 的计数器，和一个"到 0 之后回到 15 还是停住"的标志。

```cpp
u8 decay_envelope_clock(u8& divider, u8& decay, bool& start, u8 period, bool loop)
{
    if (start) {
        start = false;
        decay = 15;
        divider = period;
        return decay;
    }
    if (divider == 0) {
        divider = period;
        if (decay > 0) --decay;
        else if (loop) decay = 15;
    } else {
        --divider;
    }
    return decay;
}
```

**每一个铜管重音、每一枚金币、每一个跳跃音效，全都是这个计数器加一个长度计数器的某种组合。**

---

## 5. 长度计数器

一个只会递减到 0 就把声道静音的计数器。装载值来自一张 32 项的表：

```cpp
inline constexpr u8 kLengthTable[32] = {
    10, 254, 20,  2, 40,  4, 80,  6, ...
};
```

**表不是线性的**——它是芯片设计者认为"够用的音符长度"的集合。

> **陷阱：** 声道被禁用时写 `$4003` **不会**装载长度计数器。
> 必须先写 `$4015` 启用，再写音符。这一条骗过了我 Phase 6 的一半测试。

---

## 6. 三角波和噪声

### 三角波没有音量

```
15 14 13 ... 1 0 0 1 ... 14 15    32 步
```

**要么响，要么不响。** 制造"轻一点"的三角波的唯一办法是快速地开关它——这就是 NES 音乐里三角波底鼓和贝斯音的区别。

它有两个计数器：长度计数器，和**线性计数器**（`$4008`），后者是三角波自己的第二个包络。

### 噪声是移位寄存器

```cpp
const u16 tap = mode_ ? 6 : 1;
const u16 feedback = (lfsr_ & 1u) ^ ((lfsr_ >> tap) & 1u);
lfsr_ = (lfsr_ >> 1) | (feedback << 14);
```

**完全确定性。** 15 位模式和 6 位模式只差一个抽头，听起来却完全不同：

```
  15 bit mode (hiss)
     ######################  #####################   ####################
  6 bit mode (metallic)
      ################  ##### #######  ####  ## ####   ###  ######  ###
```

一个是军鼓，一个是激光。

---

## 7. 帧序列器

一个约 240 Hz 的计数器，驱动所有声道的包络和长度：

```
每个 step     7457 个 CPU 周期（3728.5 个 APU tick）
              ^^^^^^^^^^^^^                      ^^^^^^^^^
              数据手册写的是 CPU 周期   我们的 tick 是半个 CPU 周期

4 步模式：
  step 1  四分帧
  step 2  四分帧 + 半帧
  step 3  四分帧
  step 4  四分帧 + 半帧，并触发帧 IRQ
  然后回到 1

5 步模式：
  step 1  四分帧
  step 2  四分帧 + 半帧
  step 3  四分帧
  step 4  （什么都不做）
  step 5  四分帧 + 半帧
  然后回到 1
```

**5 步模式存在的唯一理由**是让半帧时钟以略不同的频率运行，从而制造一种音高效果——而且它不触发 IRQ。

```cpp
if (++frame_counter_ >= kFrameStepCycles) {
    frame_counter_ = 0;
    if (five_step_) { ... } else { ... }
}
```

> **陷阱（真实踩过）：** 这里有一个很容易犯的单位错误。2A03 的数据手册表
> 是以 **CPU 周期** 写的：
>
> ```
> 帧序列器 step        7457 CPU 周期   -> 四分帧 240 Hz
> 噪声移位寄存器周期    4, 8, 16, ...   -> f = CPU / period
> DMC 位速率            428, ..., 54    -> f = CPU / rate
> 三角波                (t + 1)         -> f = CPU / (32 * (t + 1))
> ```
>
> 但我们的 APU tick 是 **半个** CPU 周期（`tick_cpu` 里 `while (cpu_remainder_ >= 2)`）。
> 如果把这些表直接当成 tick 数用，帧序列器、噪声、DMC、三角波全部
> **慢一半**（低一个八度），只有脉冲波因为公式本来就是 `16 * (t + 1)`
> 而恰好正确。听感上：军鼓变成 "shhh" 而不是 "tss"，贝斯低了一个八度。
>
> 修法：凡是以 CPU 周期为单位的表，用之前都除以 2；三角波定时器改成
> 每个 tick 走两次。`custom_nes_core/tests/core/test_apu.cpp` 的 `ApuRates.*` 三个测试把
> 噪声 55930 Hz、三角波 3494 steps/s、DMC 216 tick/byte 钉死。

---

## 8. 混音不是线性的

真实 2A03 的混音可以用两条公式近似：

```cpp
f32 pulse_out = 0.0f;
if (pulse_sum > 0) {
    pulse_out = 95.88f / ((8128.0f / static_cast<f32>(pulse_sum)) + 100.0f);
}

const f32 tnd = (triangle / 8227.0f) + (noise / 12241.0f) + (dmc / 22638.0f);
f32 tnd_out = 0.0f;
if (tnd > 0.0f) {
    tnd_out = 159.79f / ((1.0f / tnd) + 100.0f);
}

return pulse_out + tnd_out;
```

**这重要**：直接用线性求和会让脉冲声道相对三角波和噪声过响，听起来完全不对。

### 输出是单极性的

**0 = 静音，声音只会往上推。** 真实主板上有一个电容隔掉直流。导出 WAV 时也要做同样的事——一阶高通：

```cpp
const f32 filtered = sample - previous_input + 0.995f * previous_output;
```

**没有它，录音会带一个恒定偏移，只用到一半量程。**

---

## 9. 一个真实游戏的声音

`demo_apu` 第 6 节，跑超级玛丽，按下 Start，录 10 秒：

```
  samples        : 440277  (9.98 seconds)
  peak           : 0.68
  rms            : 0.38
  frames audible : 459 of 600
  channels on    : $0f   (bit 0 pulse 1, 1 pulse 2, 2 triangle, 3 noise, 4 DMC)

  What each voice is doing right now (0-15, DMC is 0-127):
    Pulse 1     0
    Pulse 2     0
    Triangle    8
    Noise      12
    DMC        48
```

导出的 WAV 的时间结构：

```
  t=   0s rms=     0 
  t=   1s rms=     0 
  t=   2s rms=  2270 #####
  t=   3s rms=  2238 #####
  ...
  t=   9s rms=  2380 #####
```

**前 2 秒是静音**（因为按下 Start 之后游戏还没开始放音乐），然后音乐持续不断。

### 标题画面是静音的

```cpp
TEST_F(InputTest, TheTitleScreenIsSilent)
{
    // Super Mario Bros does not start its music until the game does, so a
    // silent title screen is correct, not a broken APU.
    EXPECT_EQ(machine_.apu().enabled_channels() & 0x0F, 0x00);
}
```

**这是个重要的对照：在按下 Start 之前，`$4015` 的 enable 位是 0。**
如果不做这个对照，很容易以为 APU 坏了。

```bash
afplay frames/game_audio.wav
```

---

## 10. 代码对应

| 概念 | 文件 |
|------|------|
| 五个声道 + 帧序列器 + 混音 | `custom_nes_core/src/core/nes/apu.{hpp,cpp}` |
| 接在 `$4000-$4017` | `custom_nes_core/src/core/nes/bus.cpp` |
| 时钟同步（CPU/2） | `custom_nes_core/src/core/nes/machine.cpp` `apu_.tick_cpu()` |
| 单元测试 | `custom_nes_core/tests/core/test_apu.cpp` |
| 真实游戏声音测试 | `custom_nes_core/tests/core/test_real_rom.cpp` `InputTest` |
| 可运行讲解 + WAV 导出 | `tools/demo_apu.cpp` |

```bash
./build/demo_apu                        # 写 frames/game_audio.wav
./build/tests/fc_tests --gtest_filter='Apu*:InputTest.*'
```

---

## 11. 诚实的边界

| 项目 | 状态 |
|------|------|
| 五个声道的波形 | ✅ |
| 包络 / 长度 / 线性计数器 | ✅ |
| 扫频（含 pulse 1/2 的取反差异） | ✅ |
| 帧序列器 4/5 步、帧 IRQ | ✅ |
| 非线性混音 | ✅（标准近似） |
| DMC 采样播放与 IRQ | ✅ |
| **DMC 的"读内存会拖住 CPU 4 个周期"** | ❌ 立即读取 |
| **DMC 的 IRQ 在真实硬件上有很多坑** | ❌ 只实现基本行为 |
| **精确的混音曲线** | ❌ 用公式近似，真机是非线性的 |

**"能听出正确的音"这个目标达到了。** 要做到波形级一致需要查表和逐周期对齐 CPU 停顿。

---

## 12. 自测

1. APU 为什么和 CPU 在同一个地址空间里？
2. 方波是怎么"生成"的？四种占空比分别是什么？
3. 写 `$4003` 时如果声道没被 `$4015` 启用，会发生什么？
4. 三角波怎么调节音量？
5. 噪声的两个模式差在哪里？听起来有什么不同？
6. 5 步模式和 4 步模式的唯一功能区别是什么？
7. 为什么混音不能用简单的线性求和？
8. 为什么导出的 WAV 要做高通滤波？

<details>
<summary>答案</summary>

1. APU 和 CPU 在**同一块芯片**上（Ricoh 2A03）。它不是一个独立部件，所以共享地址空间
2. 不是生成的，是从四种 8 位模式里按定时器逐位取出：12.5%、25%、50%、25% 取反
3. **长度计数器不会被装载**，音符保持静音。必须先写 `$4015` 启用
4. **不能。** 三角波没有音量控制，只能靠快速开关来"变轻"
5. 反馈抽头：15 位模式用 bit 1，6 位模式用 bit 6。前者是嘶声（军鼓），后者是金属声（激光）
6. 5 步模式**不触发帧 IRQ**，而且半帧时钟的频率略有不同，产生音高效果
7. 因为真实 2A03 的混音是非线性的。线性求和会让脉冲声道过响，听起来完全不对
8. 因为 APU 的输出是单极性的（0 = 静音，声音只往上）。真实主板上有电容隔直，WAV 里也要做同样的事，否则只有一半量程可用且有恒定偏移

</details>

---

**上一章：** [controllers.md](controllers.md)
**下一阶段：** Phase 7 — macOS 前端（Electron）
