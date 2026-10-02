# C++/Win32 Windows host — 对标 macOS host 的计划

状态：**W1 完成**（Rust 侧已写完并由 macOS 的 `cargo clippy`/`test` 类型检查）；
**W2–W3 已在 Windows VM 上验证**（MinGW 交叉编译产物 + Parallels Win11 实机运行）；
**W4 待做**。目标是给产品加第二个原生 host：**C++ Win32 壳 + Rust 应用库**，
形态与 Swift/macOS host 一一对应。

权威对照：
[`../../macos/README.md`](../../macos/README.md)、
[`swift-macos-host-plan.md`](swift-macos-host-plan.md)、
[`../../AGENTS.md`](../../AGENTS.md)。

## 0. 为什么是「C++ Win32 壳」而不是 winit

Windows 上不使用 `winit`：它在虚拟机里窗口生命周期/swapchain 重建不稳定，而且本仓库
没有 Windows 真机，必须优先选择**最大控制权、最少抽象层**的原生窗口。

macOS 那条路之所以是「Swift 壳 + Rust 库 + C ABI」，是因为 AppKit 只能用 Swift 写。
Windows 的 Win32 用 C++ 写是同构的：`HWND` + `WndProc` + 消息循环，全部自己掌控。
也评估过让 Rust 直接用 `windows-rs` 写整层（省掉 C ABI），但既然目标是**对标 macOS
host**，就保持同一形状：**C++ 壳只做窗口与事件，UI/渲染/模拟器全在 Rust**。

## 1. 总体形状（与 macOS 对称）

```
┌────────────────────────────────┐         ┌─────────────────────────────────────┐
│  C++ (`windows/`)              │         │  Rust (`src/native/`, in `cgb-app`)     │
│  RegisterClassExW + CreateWindow│  C ABI  │  the app library                     │
│  HWND                          │◀───────▶│  igui_app runtime + igui UI          │
│  WndProc 事件 → cgb_host_*      │         │  wgpu backend + presenter            │
│  XInput 手柄 → cgb_host_gamepad │         │  cgb-app: library + emulator         │
│  IMM32 IME / 光标 / 全屏        │         │  cgb-libretro + cpal audio           │
└────────────────────────────────┘         └─────────────────────────────────────┘
```

**FFI 边界包含 UI**：C++ 从不绘制、不布局、不感知控件。它只创建 `HWND`、把句柄交给
Rust、按需请求帧、转发 pointer/keyboard/IME/drop 事件。

目录（`src/native/` 为两个壳共用，`windows/` 为 C++ 壳独占）：

| path | 角色 |
|---|---|
| `src/native/mod.rs` | 统一 host 模块：输入 / surface / GPU / 插件 + `cgb_host_*` C ABI |
| `src/native/input.rs` | `NativeEvent` → `igui_core::InputEvent`；AppKit keyCode / Win32 VK 两张键码表 |
| `src/native/surface.rs` | `NativeSurface` + `create_surface`（mac `CoreAnimationLayer` / win `RawHandle::Win32`） |
| `src/native/gpu.rs` | `NativeGpuPlugin`（handle → wgpu surface → backend → presenter） |
| `src/native/plugins.rs` | `NativeHostWindow` / `NativeGamepadPlugin` / `NativeClipboardPlugin` / `NativeTextMeasurePlugin` |
| `src/native/ffi.rs` | `cgb_host_*` C ABI |
| `src/native/include/cgb_host.h` | 两个壳共用的头 |
| `windows/src/WinWindow.*` | 窗口类、`WndProc`、消息循环、全屏（C++ 壳独占） |
| `windows/src/Input.*` | `WndProc` 消息 → `cgb_host_*` 的纯 helper（UTF / 修饰键 / 键字符） |
| `windows/src/Gamepads.*` | `XInputGetState` → libretro 快照 |
| `windows/src/LaunchOptions.*` | 命令行 → `cgb_host_start` 参数 |
| `windows/CMakeLists.txt`、`windows/scripts/*` | 构建 / 打包 |

Rust 侧在根包 `cgb-app` 的 `src/native/`，macOS 壳在 `macos/`，Windows 壳在
`windows/`；工作区根即应用包，模拟器边界仍是唯一成员 crate `crates/cgb-libretro`。

## 2. 表面（surface）与渲染

wgpu 24 能从**裸 `HWND`** 建表面，无需窗口库：

```rust
use igui::igui_backend_wgpu::wgpu::rwh;

let target = wgpu::SurfaceTargetUnsafe::RawHandle {
    raw_display_handle: rwh::RawDisplayHandle::Windows(rwh::WindowsDisplayHandle::new()),
    raw_window_handle: rwh::RawWindowHandle::Win32(rwh::Win32WindowHandle::new(
        NonZeroIsize::new(hwnd as isize).unwrap(),
    )),
};
let surface = unsafe { instance.create_surface_unsafe(target) }?;
```

这与 macOS 的 `SurfaceTargetUnsafe::CoreAnimationLayer(layer)` 完全对称。其余
（`WgpuBackend::from_instance`、格式选择、`NativePresenter` 的帧呈现逻辑）原样照搬成
`NativePresenter`。

**VM 稳定性（关键）**：

- 显式选择后端，可在设备创建前用 `WGPU_BACKEND=dx12|vulkan|gl` 切换。
- 允许强制 **WARP**（DX12 软件光栅器，任何 VM 都能跑）。注意上游
  `igui_backend_wgpu::from_instance` 里写死了 `force_fallback_adapter: false`
  （`crates/platform/wgpu/igui_backend_wgpu/src/backend/init.rs`）。需要二选一：
  1. 给 igui 上游加参数（推荐，最小改动）；或
  2. `cgb-app` 自建 `Instance` + adapter + device，绕过 `from_instance`。
  在 v1 先按「环境变量 + 文档」，把 WARP 通道留作 phase 4 的显式项。
- `WM_SIZE` / `WM_DPICHANGED` 里**只记录**尺寸/缩放，`surface.configure` 留到帧边界
  （抄 `NativePresenter` 里对 `Lost`/`Outdated` 返回 `PresentOutcome::Reconfigured` 的做法）。
- `PresentMode::Fifo` + `desired_maximum_frame_latency = 2`（与 mac 一致）。
- **DX12 resize 的坑（已修在 igui 上游）**：DX12 的 `ResizeBuffers` 在后备缓冲还被引用时
  会失败，而 igui `v0.3.0` 的 `igui_backend_wgpu::end_frame` **不释放**上一帧的 surface
  `TextureView`（它一直留在 backend 的 `frame` 里），于是 `surface.configure` 报
  `Invalid surface`。已在 igui 侧修复（`end_frame` 现在丢弃 frame；`is_offscreen_frame`
  改用 `last_frame_offscreen`），cgb 已 bump 到 `v0.3.1`。Metal 不需要这个，无需额外处理。
- **不因 wgpu 校验错误而 abort**：`extern "C"` 函数里的 panic 会 `__fastfail`
  （Windows 事件日志里的 `0xc0000409`）。host 用
  `device.on_uncaptured_error` 把错误改成打印；否则任何一次驱动拒绝都会把进程打死。

## 3. 帧驱动（对标 mac 的 display link + 事件 monitor）

macOS：任何输入排一帧；`cgb_host_needs_frame` 为真时 `CADisplayLink` 持续出帧。

Windows：一个**自己掌控的消息循环**：

- 主循环用 `MsgWaitForMultipleObjects`（或 `PeekMessageW` + `WaitMessage`），
  空闲时阻塞，避免忙等。
- 收到任意消息 → 排一帧（`cgb_host_frame`）。
- `cgb_host_needs_frame` 为真 → 用 `timeBeginPeriod(1)` + 高频 `WM_TIMER`（或自带
  `WaitForSingleObject` 定时）以接近核心帧率驱动；为假则回到纯事件驱动。
- 尺寸变更/DPI 变更在帧边界统一 `cgb_host_resize`。

## 4. 能力对照表（mac → win）

| 能力 | macOS 实现 | Windows 实现 | 状态 |
|---|---|---|---|
| 窗口 | `NSWindow`（透明标题栏 + `fullSizeContentView`） | `WS_OVERLAPPEDWINDOW` 普通标题栏 | 计划 |
| 表面 | `CAMetalLayer` → wgpu | `HWND` → wgpu（`RawHandle::Win32`） | 计划 |
| 指针 move/down/up/leave | `NSEvent` 各类 mouse* | `WM_MOUSEMOVE` / `WM_*BUTTONDOWN/UP` / `WM_MOUSELEAVE` + `TrackMouseEvent` | 计划 |
| 双击 | `event.clickCount` | `WM_LBUTTONDBLCLK`（需 `CS_DBLCLKS`），或自算间隔 | 计划 |
| 滚轮 | `scrollWheel` | `WM_MOUSEWHEEL` / `WM_MOUSEHWHEEL`（`GET_WHEEL_DELTA_WPARAM`） | 计划 |
| 键盘 | `keyDown/Up`（keyCode + chars） | `WM_KEYDOWN/UP` + `MapVirtualKey`/`ToUnicode`（`WM_CHAR` 只取文本） | 计划 |
| 修饰键 | `flagsChanged` | `WM_KEYDOWN` 里 `GetKeyState`；或 `GetKeyboardState` | 计划 |
| 文本/IME | `NSTextInputClient` | IMM32：`WM_IME_STARTCOMPOSITION/COMPOSITION/ENDCOMPOSITION` + `ImmGetCompositionStringW` | 计划 |
| IME 候选位 | `firstRect(forCharacterRange:)` | `ImmSetCandidateWindow` / `ImmSetCompositionWindow`（由 `cgb_host_caret` 驱动） | 计划 |
| 剪贴板 | `arboard`（Rust） | **同一份 `NativeClipboardPlugin`，`arboard` 跨平台** | 计划 |
| 拖放 | `registerForDraggedTypes` + `performDragOperation` | `DragAcceptFiles` + `WM_DROPFILES` | 计划 |
| 光标 | `NSCursor`（帧后 `cgb_host_cursor`） | `LoadCursorW`/`SetCursor`（帧后 `cgb_host_cursor`） | 计划 |
| 全屏 | `NSWindow.toggleFullScreen` | `WS_POPUP` ↔ `WS_OVERLAPPEDWINDOW` + `SetWindowPos` 到显示器矩形 | 计划 |
| 手柄 | Swift `GameController` → 快照 | C++ `XInputGetState` → 快照（同样的 `cgb_host_gamepad_*`） | 计划 |
| 音频 | Rust `cpal` | Rust `cpal`（WASAPI，同代码） | 已可复用 |
| 高 DPI | `backingScaleFactor` | `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` + `WM_DPICHANGED` | 计划 |
| 硬件 GL 核心（N64/PSP/PS1） | 离屏 CGL + 回读 | 离屏 **WGL** + FBO（Phase 4，暂缓） | 计划 |

## 5. C ABI（`cgb_host.h`）

与 `cgb_host.h` 同名同形，`layer` 换成 `hwnd`（`void*`）。

```c
typedef struct CgbHostApp CgbHostApp;

/* hwnd 是活的窗口句柄；library_dir / rom 可为 NULL。失败返回 NULL。 */
CgbHostApp *cgb_host_start(void *hwnd, uint32_t width, uint32_t height, double scale,
                         const char *library_dir, const char *rom);
void cgb_host_destroy(CgbHostApp *app);

void cgb_host_frame(CgbHostApp *app);
bool cgb_host_needs_frame(const CgbHostApp *app);
void cgb_host_resize(CgbHostApp *app, uint32_t width, uint32_t height, double scale);

/* -1 无请求，1 进全屏，0 退全屏 */
int32_t cgb_host_take_fullscreen(CgbHostApp *app);
uint32_t cgb_host_cursor(const CgbHostApp *app);
bool cgb_host_caret(const CgbHostApp *app, float *x, float *y, float *w, float *h);

void cgb_host_pointer_move(CgbHostApp *app, float x, float y);
void cgb_host_pointer_down(CgbHostApp *app, float x, float y, uint32_t button, uint32_t click_count);
void cgb_host_pointer_up(CgbHostApp *app, float x, float y, uint32_t button);
void cgb_host_pointer_leave(CgbHostApp *app);
void cgb_host_scroll(CgbHostApp *app, float x, float y, float dx, float dy);

void cgb_host_key_down(CgbHostApp *app, uint32_t vk, const char *characters, uint32_t modifiers);
void cgb_host_key_up(CgbHostApp *app, uint32_t vk, const char *characters);
void cgb_host_text(CgbHostApp *app, const char *utf8);
void cgb_host_modifiers(CgbHostApp *app, uint32_t bits);
void cgb_host_ime(CgbHostApp *app, uint32_t kind, const char *text, int32_t sel_start, int32_t sel_end);
void cgb_host_dropped_file(CgbHostApp *app, const char *path);

enum { CGB_JOYPAD_B = 0, /* …与 cgb_host.h 相同… */ CGB_JOYPAD_R3 = 15 };
void cgb_host_gamepad_state(CgbHostApp *app, uint32_t port, uint32_t buttons,
                           int16_t lx, int16_t ly, int16_t rx, int16_t ry);
void cgb_host_gamepad_connected(CgbHostApp *app, uint32_t port, bool connected);
```

`input.rs` 里 `NativeEvent` 与 `NativeEvent` 同形，`modifiers_from_bits` / `pointer_button`
直接复用同样的语义（1 shift、2 ctrl、4 alt、8 meta）。**修饰键映射要在
`src/native/input.rs` 做纯函数 + 单测**，与 mac 一致。

## 6. 构建与打包

- Rust：`cargo build` 产出 `target/<profile>/cgb_app.lib`（MSVC 静态库；
  `crate-type` 已含 `staticlib`，Windows 下即 `.lib`）。
- C++：CMake，链接 `cgb_app.lib` + `d3d12/vulkan` 由 wgpu 动态加载；显式链接
  `xinput1_4`、`imm32`、`user32`、`gdi32`、`shell32`、`ole32`、`comctl32`。
- 核心：`cores/dist/*.dll`（`catalog.rs` 已支持 `windows`/`x86_64` 的下载源），
  `assets/` 与 `cores/cores.json` 随包。
- 出物：一个 `.exe` + `cores/` + `assets/`。
- 脚本：`windows/scripts/build.sh`（在 Windows 上跑，或交叉配置）、
  `windows/scripts/run.bat`。

## 7. 分阶段

- **W1 · Rust 侧镜像**：`src/native/{mod,host,input,ffi}.rs` + `include/cgb_host.h`。
  **`win` 不做 `cfg` 门控**：`raw-window-handle` 在所有目标都暴露 `Win32`/`Windows`
  变体，所以整块 host 能在 macOS 上编译，默认的 `cargo clippy -D warnings` /
  `cargo test` 门就能替我们类型检查（开发机没有 Windows）。运行时只有 C++ 壳调
  `cgb_host_start` 才会激活，macOS 构建不会调。`input.rs` 是平台无关的纯映射 + 单测。
  `src/lib.rs` 加 `pub mod win;`，`mac` 保留 `#[cfg(target_os = "macos")]`。
- **W2 · C++ 骨架**：窗口类 + `WndProc` + 消息循环 + `cgb_host_start/frame/destroy`，
  软件核心出画面、键鼠可用。**已用 MinGW 交叉编译产物在 Parallels Win11 VM 上跑通**
  （窗口打开、库界面渲染、反复 resize 稳定）；MSVC 的 `windows/scripts/build.bat`
  尚未跑。
- **W3 · 输入与原生能力对齐**：IME（IMM32）、光标、拖放、全屏、双击、滚轮、
  高 DPI、`XInput` 手柄。代码已写，**待 VM 上逐项人眼验收**。
- **W4 · VM 稳健性 + 硬件核心**：WARP 通道、`WM_SIZE` 帧边界重配；评估 WGL 离屏上下文
  （N64/PSP/PS1），以及打包。

## 8. 风险与开放问题

- **无 Windows 真机**：开发机是 macOS。`src/native/` 在 macOS 上就能编译（mac
  surface 分支 `#[cfg(target_os="macos")]`，win 分支用跨平台的 `raw-window-handle`），
  所以靠 macOS 的 `cargo clippy -D warnings` / `cargo test` 就能类型检查两侧；
  C++ 侧有 `windows/scripts/syntax-check.sh`（mingw-w64 `-fsyntax-only`）与
  `windows/scripts/cross-build-mingw.sh`（完整交叉编译，产出可在 VM 里跑的自包含
  `.exe`）。**已在一台 Parallels Win11 VM 上跑通窗口 + resize**；MSVC 构建与其余
  交互能力仍需验收。
- **上游 `force_fallback_adapter`**：见 §2，需要改 igui 或自建 adapter 路径。
- **标题栏模型**：mac 用透明标题栏 + `fullSizeContentView`，Windows v1 决定用**普通标题栏**
  （VM 最稳）；如要自绘 chrome，另开一期。
- **全屏语义**：无边框 + 显示器矩形，`Esc` 退出由 UI 负责（与 mac 一致）。
- **软件核心 vs 硬件核心**：v1 只承诺软件核心（NES/SNES/GB/Genesis/FBNeo/J2ME）。
  硬件 GL 核心要 WGL，视为独立里程碑。
