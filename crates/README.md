# crates

The feature sub-crates of the workspace. The **app is the root package**
(`cgb-app`, binary `classic-game-box`) and lives in `../src/`; the UI is the app's
`../src/ui` module, not a crate. This folder holds only the engine/device crates.

Dependency direction: `cgb-app → everything`, the device crates and
`cgb-libretro` → `cgb-systems`, and `cgb-systems` → nothing. No crate in this
folder depends on the app or the platform except the device crates, which own
exactly one device each.

| crate | type | responsibility |
|---|---|---|
| `cgb-systems` | lib | system/core registry, joypad ids (dependency-free) |
| `cgb-paths` | lib | file layout (`Paths`) + settings (`Settings`) |
| `cgb-cores` | lib | `cores.json` manifest, buildbot catalog, runtime downloader |
| `cgb-library` | lib | SQLite game library, ROM import, screenshots, save states, `.srm`, cheats |
| `cgb-libretro` | lib | libretro front end: `dlopen`, callbacks, ABI |
| `cgb-audio` | lib | cpal output + SPSC ring buffer (int16 stereo) |
| `cgb-input` | lib | keyboard bindings + gilrs gamepads → button masks |

The app's own layout is documented in [`../src`](../src): `app/` (state, frame
loop, feature handlers), `ui/` (views built from a pure `ViewModel`),
`session.rs` (one running game), `cli.rs` / `cores_cli.rs` / `selfcheck.rs`.

## Frame data flow

```
cgb-libretro::CoreHost::run_frame()
   ├── video callback → Frame { width, height, rgba }  ─→ app updates TextureId
   └── audio callback → Vec<i16> stereo                ─→ cgb-audio ring buffer → cpal
cgb-input::InputState → cgb-libretro::CoreHost::set_buttons(port, mask)
cgb_paths::Paths → CoreHost::{new, load_game}
app::ui::ViewModel ← app::app projects core/library state
```

`cgb-libretro` returns plain values, never UI or device types, so the whole
pipeline above the core can be exercised headlessly.
