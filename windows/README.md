# Classic Game Box — C++/Win32 host (experimental)

A parallel front end where **C++ replaces `winit`** and nothing else. The whole
application — igui UI, wgpu renderer, libretro emulator — stays in Rust; C++
owns only the window and translates Win32 messages.

```
┌──────────────────────────────┐         ┌───────────────────────────────────┐
│  C++ (`windows/`)            │         │  Rust (`src/win/`, in `cgb-app`)   │
│  RegisterClassExW + HWND     │  C ABI  │  the app library                   │
│  WndProc                     │◀───────▶│  igui_app runtime + igui UI       │
│  Win32 messages → cgb_win_*  │         │  wgpu backend + presenter         │
│  XInput + a message loop     │         │  cgb-app: library + emulator      │
└──────────────────────────────┘         └───────────────────────────────────┘
```

The FFI boundary **includes the UI**: C++ never paints, lays out or reasons
about a widget. It creates an `HWND`, hands the handle to Rust, asks for
frames, and forwards pointer / keyboard / IME / drop events.

## Why this shape

- It mirrors the Swift/macOS host (`macos/`) exactly. `igui_app` is already
  backend-neutral (`Presenter` / `Runner` / `PlatformObserver`);
  `igui_winit` is only one assembly of it.
- `win` **is not used**: it is unstable in virtual machines and there is no
  Windows machine to test on, so the shell keeps full control of the window
  and the message loop.
- `wgpu` 24 can create a surface straight from an `HWND`
  (`SurfaceTargetUnsafe::RawHandle` with `Win32WindowHandle`), so no windowing
  library is needed between C++ and the GPU.
- The Rust host (`src/win/`) is **not** `cfg`-gated: raw-window-handle exposes
  the Windows variants on every target, so the macOS dev machine's
  `cargo clippy` / `cargo test` gate type-checks it.

## Build & run

Requires Windows 10+, MSVC (Visual Studio 2022), CMake, a Rust toolchain, and
native libretro cores (`cores/dist/*.dll`).

```bat
rem from the repo root
windows\scripts\build.bat                          rem cgb-win.exe
windows\scripts\run.bat                            rem open the library UI
windows\scripts\run.bat path\to\game.nes           rem start a game
windows\scripts\run.bat --library-dir C:\Games     rem choose a library
```

Or by hand:

```bat
cargo build                                  rem target\debug\cgb_app.lib
cmake -S windows -B windows\build -A x64
cmake --build windows\build --config Debug
windows\build\Debug\cgb-win.exe
```

On a machine without Windows, `windows/scripts/syntax-check.sh` runs a
mingw-w64 `-fsyntax-only -Wall -Wextra` pass over the shell (it type-checks but
does not link or run):

```bash
brew install mingw-w64        # once
windows/scripts/syntax-check.sh
```

## Layout

| path | what |
|---|---|
| `src/win/host.rs` | `WinGpuPlugin` (HWND → wgpu surface → presenter), `WinTextMeasurePlugin`, `WinClipboardPlugin`, `WinGamepadPlugin`, `WinHostWindow` |
| `src/win/input.rs` | `WinEvent` → `igui_core::InputEvent`, VK/modifier/button mapping |
| `src/win/ffi.rs` | the `cgb_win_*` C ABI |
| `src/win/include/cgb_win.h` | the header the C++ shell imports |
| `windows/src/WinWindow.cpp` | window class, `WndProc`, the message loop, fullscreen, cursor, IME caret |
| `windows/src/Input.cpp` | `WndProc` message → `cgb_win_*` helpers (UTF, modifiers, key char) |
| `windows/src/Gamepads.cpp` | `XInput` → libretro snapshot |
| `windows/src/LaunchOptions.cpp` | command line → `cgb_win_start` arguments |
| `windows/CMakeLists.txt`, `windows/scripts/*` | build |

The Rust side lives at **`src/win/`** inside the root `cgb-app` package; the
workspace root is also the app package, and the emulator boundary is the one
member crate (`crates/cgb-libretro`).

## Status

**W1–W3 verified on a real Windows VM via a MinGW cross-build.** The Rust host
is type-checked by the macOS `cargo clippy`/`test` gate; `windows/scripts/
cross-build-mingw.sh` cross-builds a self-contained `cgb-win.exe` from macOS and
it runs in a Parallels Windows 11 VM (DX12 on the "Parallels Display Adapter"):
the window opens, the library UI renders, and the message loop is stable across
repeated resizes.

One Windows-specific fix came out of it: wgpu's DX12 swap-chain resize
(`ResizeBuffers`) fails while a back-buffer reference is alive, and
`igui_backend_wgpu` keeps the previous frame's surface texture view. `src/win/
host.rs` renders one offscreen frame before reconfiguring the surface to release
it (see the `WinGpu::resize` comment; macOS/Metal needs no such workaround). The
host also installs a non-panicking wgpu error handler: a validation error
crossing the `extern "C"` boundary would otherwise abort the process.

Still to verify on Windows: keyboard / pointer / IME / drag-drop / fullscreen,
XInput, then the W4 items (forcing WARP, and a WGL offscreen context for the
hardware-GL cores). The MSVC build via `windows/scripts/build.bat` has not been
run yet.
