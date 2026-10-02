# crates

Two Rust packages. The **root `Cargo.toml` is the app** (`cgb-app`, `src/`),
and the one member package is the emulator boundary:

| package | type | responsibility |
|---|---|---|
| `cgb-app` (root `src/`) | lib + staticlib | the app: igui UI, frame loop, wiring, game library, settings, audio device, core manifest/catalog/download, and the Swift host C ABI. `libcgb_app.a` is what the Swift binary links |
| `crates/cgb-libretro` | lib | the libretro front end (`dlopen`, callbacks, ABI), plus the system registry, joypad ids and the input model it shares with the app |

Dependency direction (one way only):

```
cgb-app (root)  → cgb-libretro, igui
cgb-libretro    → nothing app-shaped (libloading only)
```

`cgb-libretro` never depends on the UI, the audio device or the game database:
it exposes plain values (`Frame`, `Vec<i16>` samples) that the app drains each
frame, and the pure domain types (`SystemId`, `CoreSpec`, `InputState`,
`KeyboardBindings`) the app and the input layer agree on.

## The app's layout (`src/`)

```
src/ui/         igui views, built from a pure ViewModel
src/app/        app state, frame loop, feature handlers (library/cores/saves/…)
src/session.rs  one running game (libretro session + audio)
src/library/    SQLite game library, ROM import, screenshots, saves, cheats
src/paths/      file layout (Paths) + settings (Settings)
src/cores/      cores.json manifest, buildbot catalog, runtime downloader
src/audio/      cpal output + SPSC ring buffer (int16 stereo)
src/host.rs     the HostWindow / GamepadSource contract
src/native/     the native host: CAMetalLayer / HWND → wgpu surface + native
                events, the cgb_host_* C ABI (Swift/macOS + C++/Win32)
src/cli.rs  src/cores_cli.rs  src/selfcheck.rs   headless surfaces
```

## Frame data flow

```
cgb_libretro::CoreHost::run_frame()
   ├── video callback → Frame { width, height, rgba }  ─→ app updates TextureId
   └── audio callback → Vec<i16> stereo                ─→ src/audio ring buffer → cpal
cgb_libretro::InputState → cgb_libretro::CoreHost::set_buttons(port, mask)
crate::paths::Paths → CoreHost::{new, load_game}
app::ui::ViewModel ← app::app projects core/library state
```

## The old crates

`cgb-host`, `cgb-systems`, `cgb-input`, `cgb-paths`, `cgb-cores`,
`cgb-library`, `cgb-audio` and `cgb-mac` no longer exist as separate packages;
their code moved into the two packages above (git history is preserved under
the new paths). `cgb-systems` + `cgb-input` became flat modules of
`cgb-libretro` (`system.rs`, `joypad.rs`, `core_choice.rs`, `input.rs`).
