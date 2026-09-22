# crates

The Rust workspace. One concern per crate; the dependency direction is
`cgb-app → everything`, `cgb-ui`/`cgb-libretro`/`cgb-input` → `cgb-systems`, and
`cgb-systems` → nothing. No crate in this folder depends on the platform except
`cgb-app` (and the device crates, which own exactly one device each).

| crate | type | responsibility |
|---|---|---|
| `cgb-app` | bin `classic-game-box` | winit + wgpu host, frame loop, wiring |
| `cgb-ui` | lib | quill views built from a pure `ViewModel` |
| `cgb-libretro` | lib | libretro front end: `dlopen`, callbacks, ABI |
| `cgb-systems` | lib | system/core registry, joypad ids (dependency-free) |
| `cgb-audio` | lib | cpal output + SPSC ring buffer (int16 stereo) |
| `cgb-input` | lib | keyboard bindings + gilrs gamepads → button masks |
| `cgb-library` | lib | SQLite library, settings, save states, `.srm` |

## Frame data flow

```
cgb-libretro::CoreHost::run_frame()
   ├── video callback → Frame { width, height, rgba }  ─→ cgb-app updates TextureId
   └── audio callback → Vec<i16> stereo                ─→ cgb-audio ring buffer → cpal
cgb-input::InputState → cgb-libretro::CoreHost::set_buttons(port, mask)
cgb-library::Paths → CoreHost::{new, load_game}
cgb-ui::ViewModel ← cgb-app projects core/library state
```

`cgb-libretro` returns plain values, never UI or device types, so the whole
pipeline above the core can be exercised headlessly.
