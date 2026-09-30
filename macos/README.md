# Classic Game Box — Swift/macOS host (experimental)

A parallel front end where **Swift replaces `winit`** and nothing else. The
whole application — igui UI, wgpu renderer, libretro emulator — stays in Rust;
Swift owns only the window and translates AppKit events.

```
┌──────────────────────────────┐         ┌───────────────────────────────────┐
│  Swift (`macos/`)            │         │  Rust (`macos/rust`, crate        │
│  NSWindow + NSView           │  C ABI  │  `cgb-mac`)                       │
│  CAMetalLayer                │◀───────▶│  igui_app runtime + igui UI       │
│  AppKit events → cgb_mac_*   │         │  wgpu backend + presenter         │
│  a 60 Hz tick                │         │  cgb-app: library + emulator      │
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
```

Or by hand:

```bash
cargo build -p cgb-mac            # target/debug/libcgb_mac.dylib
swift build --package-path macos  # macos/.build/debug/cgb-mac
```

## Layout

| path | what |
|---|---|
| `macos/rust/src/host.rs` | `MacGpuPlugin` (CAMetalLayer → wgpu surface → presenter), `MacTextMeasurePlugin` |
| `macos/rust/src/input.rs` | `MacEvent` → `igui_core::InputEvent`, key/modifier/button mapping |
| `macos/rust/src/ffi.rs` | the `cgb_mac_*` C ABI |
| `macos/rust/include/cgb_mac.h` | the header Swift imports (symlinked into the C target) |
| `macos/Sources/ClassicGameBoxMac/HostView.swift` | the layer + AppKit event forwarding |
| `macos/Sources/ClassicGameBoxMac/AppDelegate.swift` | window, `cgb_mac_start`, the frame timer |

## Status

Working: the library UI renders into Swift's Metal layer; pointer (click,
double-click, drag, wheel) and keyboard (named keys, text, modifiers) are
forwarded; resizing reconfigures the surface; dropped files import.

Known gaps:

- The frame timer presents at 60 Hz unconditionally; `cgb_mac_needs_frame`
  exists but is unused, so an idle app still paints.
- IME is a minimal `NSTextInputClient` (preedit / commit forwarded,
  `hasMarkedText` always false); CJK composition is untested.
- Clipboard (Cmd+C/V) is not wired, so text-field copy/paste does nothing.
- Fullscreen: the Rust app asks a `HostWindow`, but this host publishes none
  yet, so the in-app fullscreen toggle is a no-op.
- Audio goes through Rust `cpal` (as in the main app).
- Packaging: a dev build links `target/debug/deps/libcgb_mac.dylib` by absolute
  path; a release `.app` would copy/`@rpath` it.
