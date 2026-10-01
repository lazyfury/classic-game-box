# macOS 前端 Frontend

> 目标：解释 Core 和界面之间的那条线在哪里、为什么在那里，以及一个前端最少需要做什么。
>
> 相关：[bus.md](bus.md) — 为什么 Core 不依赖 UI

---

## 1. 那条线在哪里

AGENTS.md 里有一条硬性规则：**Core 不得依赖 UI。**

```
+-----------------------------------------------------+
|                     Core (C++)                      |
|   CPU / Bus / Cartridge / PPU / APU / Controller    |
|                                                     |
|   产出的全部东西：                                   |
|     const uint32_t*  256x240 像素, 0x00RRGGBB       |
|     float[]          44100 Hz 采样                  |
|   接受的唯一输入：                                   |
|     set_button(button, pressed)                     |
+--------------------------+--------------------------+
                           |
                    C 接口 (custom_nes_core/src/ffi)
                           |
                  Emscripten (wasm/)
                           |
+--------------------------+--------------------------+
|                Frontend (Electron)                  |
|   WebAssembly 线性内存 / Canvas / Web Audio / 键盘    |
+-----------------------------------------------------+
```

**这条线是物理的**：`custom_nes_core/src/ffi/emulator_api.h` 是纯 C。C++ 的名字修饰、模板、
异常都不跨过它，所以同一份 Core 能被三种完全不同的东西链接：

| 链接者 | 用途 |
|---|---|
| C++ 测试 / 教学 demo | `ctest`、`demo_*` |
| `tools/fc_headless` | 原生无头运行，用来和前端对照 |
| Emscripten → WebAssembly | Electron 渲染进程 |

**没有 UI 的 Core，才有可对照的参照物。** 无头运行和窗口运行如果对同一条
输入画出不同的像素，问题一定在两者之间的某个地方，而不是“模拟器本来就是
这样的吧”。

### 为什么这么划

| 如果 Core 里有 UI 代码 | 后果 |
|---|---|
| `#include <Metal/Metal.h>` | 无头环境下不能跑测试 |
| `NSWindow` / `document` | 换平台要重写核心 |
| 键盘事件处理 | 录像回放和真实按键走两条不同的路 |

**现在，测试和 CI 在没有窗口的机器上跑完整套 400+ 个测试。**

---

## 2. C 接口

```c
typedef struct fc_machine fc_machine;      /* 不透明的句柄 */

fc_machine* fc_create(void);
void        fc_destroy(fc_machine*);

bool        fc_load_rom(fc_machine*, const uint8_t* data, size_t size);
const char* fc_last_error(const fc_machine*);

bool        fc_run_frame(fc_machine*);     /* 跑一帧 */
bool        fc_is_halted(const fc_machine*);

const uint32_t* fc_framebuffer(const fc_machine*);   /* 256x240 */
size_t      fc_take_samples(fc_machine*, float* out, size_t max);
void        fc_set_button(fc_machine*, fc_button, bool pressed, int port);
```

### 三条设计约束

**1. `fc_framebuffer` 返回的指针属于 machine。**

```c
const uint32_t* p = fc_framebuffer(m);   /* 缓存它 */
for (;;) {
    fc_run_frame(m);                     /* 内容变，指针不变 */
    upload(p);                           /* 零拷贝 */
}
```

**前端不需要每帧 memcpy 240KB。** 指针稳定，内容原地更新。
在 WebAssembly 里这条更重要：那个指针就是线性内存里的一个偏移，JS 侧的
`Uint8Array` 是它的视图，`fc_run_frame` 写进去的像素下一行就能读到。

**2. `fc_take_samples` 不分配内存、不加锁。**

它可以在音频回调里调用。Core 侧是 `Apu::drain()`：

```cpp
std::size_t Apu::drain(f32* out, std::size_t max_samples) noexcept
{
    const std::size_t count = std::min(max_samples, samples_.size());
    std::copy_n(samples_.begin(), count, out);
    samples_.erase(samples_.begin(), samples_.begin() + count);
    return count;
}
```

**3. 每个函数都接受 `nullptr`。** 忘记检查的前端得到 0，不是崩溃。

```cpp
TEST(CApi, EveryFunctionSurvivesANullHandle)
```

---

## 3. 三个时钟

```
    显示器的 requestAnimationFrame (~60 Hz)
            |
            v
    useEmulator 的 tick()
            |
            +--> 累加器：该到期的帧都跑掉（通常 1，偶尔 2，有时 0）
            |        |
            |        +--> fc_run_frame()
            |        +--> fc_take_samples()  -> 环形缓冲
            |        +--> fc_framebuffer()   -> canvas 位图
            |
            +--> 纹理上传 + 绘制
```

**NES 不是 60 Hz。** PPU 的 dot 时钟把 NTSC 彩色副载波那样分频，得到
60.0988 Hz。这 0.16% 不是噪声：十分钟就是一整秒，音乐会慢慢和画面对不上。

**而显示器恰好是 60.000 Hz。** 所以“一次 requestAnimationFrame 跑一帧”
会让游戏慢 0.16%。正确做法是一个累加器：记住下一帧**应该**在什么时候跑，
每次动画回调把已经到期的帧都跑掉。

```ts
const NES_FRAME_SECONDS = 1 / 60.0988;
nextFrameTime += NES_FRAME_SECONDS;      // 注意：+=，不是 = now
while (now >= nextFrameTime) { runFrame(); nextFrameTime += NES_FRAME_SECONDS; }
```

> **为什么必须写 `+=` 而不是 `= now`：** 用 `= now` 就把每次都舍入到显示器
> 的节拍上，累加器反而变回“一帧一次”。`+=` 保留了余数，长期平均才正确。

**窗口本身就是模拟器的时钟。** 没有后台 Timer，没有第二个线程跑 CPU，
画面和模拟天然同步。

---

## 4. 音频：环形缓冲

```
    页面线程  --写-->  环形缓冲  --读-->  音频线程 (AudioWorklet)
```

模拟器一帧一帧地**突发**产生音频；声卡以**绝对固定**的速率消费它。
中间必须有个缓冲。

```ts
push(samples)              // 页面线程
process(inputs, outputs)   // 音频线程，见 pcm-worklet.js
```

**只有环形缓冲是共享的。** 模拟器本身只被页面线程碰，这让问题简单一个数量级。

**用 SharedArrayBuffer 而不是 postMessage**：每条消息都是两边各一次结构化
克隆和分配，一秒 60 次，永远如此；更重要的是“还没到的消息”和“静音”无法
区分，缓冲必须做得很深才能覆盖消息延迟。共享内存没有延迟、没有拷贝。

### 锁和等待都不允许

音频回调里加锁是实时性违规——如果页面线程持锁，回调会阻塞，扬声器爆音。
所以这里没有任何锁：**音频线程永远不等**，环形缓冲空了就输出静音并记一次
underrun。

```js
const available = (write - read) >>> 0;   // 无符号差，跨越回绕也正确
taken = Math.min(available, frames);
channel.fill(0, taken);                   // 尾部必须是静音，不是上一帧的残留
```

### 两个时钟，不是一个

模拟器瞄准 60.0988 Hz（= 44100 样本/秒），声卡也播 44100 样本/秒。
这两个数相同，**速率并不相同**：一个由系统时钟数，一个由音频芯片的石英晶体
数，差几十 ppm。当成相等处理，环形缓冲会慢慢填满或慢慢排空，半小时后玩家
听到一次爆音或断音。

修法是停止把两者当成相等：**把填充度当作误差信号**，反过来微调模拟器的帧率
（最多 ±0.5%，八个音分，听不出来）。

```
frameSeconds = NES_FRAME_SECONDS * (1 + gain * (fill - target) / target)
```

这是一个作用在积分器上的比例控制器，填充度指数收敛到目标且不震荡。

### 诚实说明：丢的是最旧的

环形缓冲满了的时候丢**最旧**的采样而不是最新的——小幅跳一下比越落越远好。
`underruns` 和 `dropped` 两个计数会显示在状态栏里。

---

## 5. 渲染：一个位图

```ts
context.putImageData(image, 0, 0);
```

**没有转换。** Core 给的是 `uint32` 的 `0x00RRGGBB`，在小端机器内存里就是
`B, G, R, 0`；canvas 的 `ImageData` 要的是 `R, G, B, A`。中间那次逐字节
重排是 61440 次迭代，大约五分之一毫秒：

```ts
for (let source = 0, out = 0; out < destination.length; source += 4, out += 4) {
    destination[out]     = framebuffer[source + 2];   // R
    destination[out + 1] = framebuffer[source + 1];   // G
    destination[out + 2] = framebuffer[source];       // B
    destination[out + 3] = 255;                       // A
}
```

> 以后可以变成 WebGL 纹理，用 BGRA 格式，代价为零。现在还不值得为它增加
> 复杂度——**先测量，再优化**。

### 两个细节

**缩放必须是 nearest。**

```css
image-rendering: pixelated;
image-rendering: crisp-edges;
```

这里的每个像素都是刻意的。线性过滤会把像素艺术变成糊。

**长宽比**

NES 的像素**不是方的**：256 个像素铺在 4:3 屏幕上，每个像素是 8:7。
画面按比例塞进视图而不变形，留出的黑边由 CSS 负责，而不是把画布拉成窗口
的形状。

---

## 6. 输入

**所有输入源汇入同一个 `InputManager`。**

```
键盘  --+
        +--> InputManager --(取 OR)--> fc_set_button()
手柄  --+
```

关键在 **OR**：键盘和手柄各自记自己的状态，console 只看到合并后的结果。
这样“按住手柄 A 的同时松开键盘 X”不会把 A 也一起松开。

### 键盘

| 按键 | NES |
|------|-----|
| 方向键 或 `W` `A` `S` `D` | 十字键 |
| `Z` 或 `J` | B（手柄左边的键） |
| `X` 或 `K` | A（手柄右边的键） |
| `Return` 或 `Space` | Start |
| `Tab` 或 右 Shift | Select |
| `R` | 复位 |
| `F12` | 截图 |
| `F` | 快进 |

用的是 `event.code`（物理键位）而不是 `event.key`（字符）——前端决定的是
“键在哪里”，不是“键说什么”，这样 AZERTY 键盘上 Z 和 X 还在同样两根手指下。

**失去焦点时必须松开所有键**，否则你按住右键切到别的窗口，马里奥会一直往右跑。

### 手柄：两条路

| 路径 | 读法 | 备注 |
|---|---|---|
| 浏览器 Gamepad API | 每个动画帧轮询一次快照 | 需要窗口聚焦；macOS 上会让进程无法退出 |
| 原生助手 `electron/native/gamepad` | 独立进程，GameController 框架，按键变化时通过管道推送 | macOS 默认走这条 |

原生助手存在的唯一原因是那个 macOS bug：页面只要**监听**了
`gamepadconnected`，Chromium 就会在浏览器进程里启动自己的手柄服务，它握着
一个 HID 连接，让整个进程无法关闭——`app.quit()`、`app.exit()`、
`process.exit()` 都停在不可中断等待里，只能 SIGKILL。独立进程可以杀。

手柄映射（两条路一致）：左摇杆复制十字键，死区 0.5（磨损的摇杆会漂移，
漂移的摇杆会把玩家走进墙里），A 是右手边的面键。

---

## 7. 无头模式：让前端可测试

```bash
# 原生构建跑同一个 ROM，写出 PPM
./build/fc_headless game.nes --frames 700 --start --dump native.ppm

# 前端无头跑，写出 PNG
cd electron && pnpm run selftest
```

**它存在的理由有两个：**

1. 让整个链路（C 接口、输入映射、帧循环）**在没有显示器的环境下可验证**
2. 出问题时第一件事就是跑它——**如果无头跑出正确的画面，问题就在窗口里，
   不在 Core 里**

`electron/verify.sh` 更进一步：把每个 ROM 分别喂给原生构建和这个应用，
任何像素、任何 APU 采样不同就失败。**这是把“能玩”和“正确”分开的工具。**

---

## 8. 构建

```bash
# 1. 把 Core 编译成 WebAssembly（改 src/ 后重跑）
./wasm/build.sh

# 2. 前端
cd electron
pnpm install
pnpm run dev          # 开发窗口
pnpm start            # 生产构建
pnpm run dist         # 打一个 macOS .dmg 到 release/
```

Core 侧依赖：

```bash
brew install cmake ninja googletest
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build build
ctest --test-dir build --output-on-failure
```

`electron/native/build.sh` 会把 Swift 手柄助手编译到固定路径
`native/bin/fc-gamepad`。没编译也能跑，只是手柄来源不启动并且在日志里说明。

---

## 9. 代码对应

| 概念 | 文件 |
|------|------|
| C 接口 | `custom_nes_core/src/ffi/emulator_api.h` / `.cpp` |
| C 接口测试 | `custom_nes_core/tests/core/test_ffi.cpp` |
| WebAssembly 构建 | `wasm/build.sh`、`wasm/emulator.mjs` |
| 帧循环 / 时钟 / 输入装配 | `electron/src/renderer/useEmulator.ts` |
| Canvas 上传 | `electron/src/renderer/useEmulator.ts` 的 `blit()` |
| 音频环形缓冲 | `electron/src/renderer/audio/output.ts` |
| 音频线程 | `electron/src/renderer/audio/pcm-worklet.js` |
| 键盘 / 输入合并 | `electron/src/renderer/input.ts` |
| 浏览器手柄 | `electron/src/renderer/gamepad.ts` |
| 原生手柄助手 | `electron/native/gamepad/Sources/fc-gamepad/main.swift` |
| 主进程 / IPC | `electron/src/main/index.ts`、`electron/src/preload/index.ts` |
| 游戏库 (SQLite) | `electron/src/main/library.ts` |
| 无头 / 对照 | `electron/verify.sh`、`tools/fc_headless.cpp` |

```bash
./wasm/build.sh
cd electron && pnpm start -- --rom /path/to/game.nes
```

---

## 10. 还没做的

| 项目 | 说明 |
|------|------|
| **WebGL 纹理上传** | 现在用 canvas 2D 逐字节重排，够快但可以零成本 |
| **Metal 渲染** | Electron 用 canvas；原生 Metal 前端已移除 |
| **存档 / 读档** | Core 有 serialize，前端还没有 UI |
| **录像回放** | 接口已经支持（`set_button`），前端没有 UI |
| **手柄连发** | 故意不做——那属于游戏/前端，不属于 Core |
| **精确 60.0988 Hz** | 累加器已经做到了；进一步要靠音频驱动节流 |

---

## 11. 自测

1. 为什么 Core 和前端之间是 C 接口而不是 C++？
2. `fc_framebuffer` 返回的指针能缓存吗？为什么？
3. 为什么 `fc_take_samples` 不能分配内存？
4. 为什么需要累加器，而不是“一次 requestAnimationFrame 跑一帧”？
5. 为什么采样器必须是 nearest 而不是 linear？
6. 为什么音频环形缓冲里不能加锁、也不能等待？
7. 键盘和手柄为什么要各自记状态再取 OR？
8. 为什么原生手柄助手要是一个独立进程？
9. 无头模式除了测试还有什么用？

<details>
<summary>答案</summary>

1. C 的名字修饰、模板、异常都不跨边界；同一份 Core 才能同时被 C++ 测试、原生无头和 WebAssembly 链接
2. 能。指针属于 machine，machine 活着它就有效，内容原地更新。这正是零拷贝上传的前提
3. 因为它要在音频回调里被调用，而实时线程不能分配内存（会阻塞、可能触发 GC/锁）
4. NES 是 60.0988 Hz，显示器是 60.000 Hz。一帧一次会让游戏慢 0.16%，十分钟错一秒。累加器保留余数，长期平均才正确
5. 因为 NES 的每个像素都是刻意的。线性过滤会把像素艺术变成糊
6. 音频回调必须赶在 deadline 前返回。加锁会等页面线程；等待会把“静音”和“还没到”混为一谈。所以永远不等，空了就静音并计数
7. 否则松开键盘上的键会把还按着的手柄键一起松开——两个 source 会互相覆盖
8. 因为 macOS 上页面只要监听 Gamepad API，Chromium 的手柄服务就会让整个进程无法退出。独立进程可以杀
9. 定位问题在 Core 还是在窗口。如果无头跑出正确画面，问题就在前端

</details>

---

**上一章：** [bus.md](bus.md)
