# Classic Game Box — Swift/macOS host (experimental)

A parallel front end where **Swift replaces `winit`** and nothing else. The
whole application — igui UI, wgpu renderer, libretro emulator — stays in Rust;
Swift owns only the window and translates AppKit events.

```
┌──────────────────────────────┐         ┌───────────────────────────────────┐
│  Swift (`macos/`)            │         │  Rust (`crates/cgb-mac`, crate     │
│  NSWindow + NSView           │  C ABI  │  `cgb-mac`)                       │
│  CAMetalLayer                │◀───────▶│  igui_app runtime + igui UI       │
│  AppKit events → cgb_mac_*   │         │  wgpu backend + presenter         │
│  an event-driven tick        │         │  cgb-app: library + emulator      │
└──────────────────────────────┘         └───────────────────────────────────┘
```

The FFI boundary **includes the UI**: Swift never paints, lays out or reasons
about a widget. It creates a `CAMetalLayer`, hands the pointer to Rust, asks for
frames, and forwards pointer / keyboard / IME / drop events.

## Why this shape

- `igui_app` is already backend-neutral (`Presenter` / `Runner` /
  `PlatformObserver`). `igui_winit` is only one assembly of it.
- `cgb-app` was refactored so its one window dependency
  (`HostWindow::set_fullscreen`) is a trait, and `igui_winit` / `winit` are an
  optional `winit-host` feature. The embedded host builds `cgb-app` with
  `--no-default-features` and no `winit`.
- `wgpu` 24 can create a surface straight from a `CAMetalLayer`
  (`SurfaceTargetUnsafe::CoreAnimationLayer`), so no windowing library is
  needed between Swift and the GPU.

## Build & run

Requires a macOS SDK with Swift and the native libretro cores
(`./scripts/build-cores.sh`).

```bash
# from the repo root
macos/scripts/build.sh                              # cgb-mac + the Swift app
macos/scripts/run.sh                                # open the library UI
macos/scripts/run.sh /path/to/game.nes              # start a game
macos/scripts/run.sh --library-dir ~/Games          # choose a library
macos/scripts/package.sh                            # → dist/Classic Game Box (Swift).app
```

Or by hand:

```bash
cargo build -p cgb-mac            # target/debug/libcgb_mac.dylib
swift build --package-path macos  # macos/.build/debug/cgb-mac
```

## Layout

| path | what |
|---|---|
| `crates/cgb-mac/src/host.rs` | `MacGpuPlugin` (CAMetalLayer → wgpu surface → presenter), `MacTextMeasurePlugin`, `MacClipboardPlugin`, `MacGamepadPlugin`, `MacHostWindow` |
| `crates/cgb-mac/src/input.rs` | `MacEvent` → `igui_core::InputEvent`, key/modifier/button mapping |
| `crates/cgb-mac/src/ffi.rs` | the `cgb_mac_*` C ABI |
| `crates/cgb-mac/include/cgb_mac.h` | the header Swift imports (symlinked into the C target) |
| `macos/Sources/ClassicGameBoxMac/HostView.swift` | the layer + AppKit event forwarding + drag & drop |
| `macos/Sources/ClassicGameBoxMac/Gamepads.swift` | `GCController` → libretro snapshot |
| `macos/Sources/ClassicGameBoxMac/AppDelegate.swift` | window, `cgb_mac_start`, event-driven frame scheduling |
| `macos/packaging/Info.plist`, `macos/scripts/package.sh` | the `.app` bundle |

The Rust host lives at **`crates/cgb-mac`**: the workspace is a virtual manifest
and every Rust package is a library the Swift app embeds.

## Status

Working: the library UI renders into Swift's Metal layer; pointer (click,
double-click, drag, wheel), keyboard (named keys, text, modifiers) and IME
preedit/commit are forwarded; the cursor and IME caret come from the UI;
clipboard (Cmd+C/V) works; files can be dropped; resizing is coalesced; the
in-app fullscreen toggle is applied by Swift; gamepads are read through Apple's
`GameController` framework (no `gilrs` in the build). Frames are event-driven:
any input schedules one, and a `CADisplayLink` drives them while the app wants
more. The Rust host is linked **statically**, so the app is self-contained.
The title bar is transparent + full-size (the UI runs under it).

Verified on hardware: PSP, N64 and PS1 (the offscreen-CGL path coexists with
Metal/wgpu), the downloadable-core flow, and the IME candidate position.

Known gaps:

- IME is a minimal `NSTextInputClient` (`hasMarkedText` always false); CJK
  composition is untested.
- Audio goes through Rust `cpal` (as in the main app).
- Double-pressing the pad's Select can trip macOS's screen-recording shortcut;
  not remapped yet.
- Key filtering may need per-emulator config (deferred).
