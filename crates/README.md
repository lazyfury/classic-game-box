# crates

The Rust packages of the workspace. The root `Cargo.toml` is a **virtual
manifest** (no `[package]`); the product is the Swift/macOS app in
[`../macos`](../macos), and every crate here is a library it embeds (or the
secondary `winit` dev host).

| crate | type | responsibility |
|---|---|---|
| `cgb-app` | lib + bin | the shared app: igui UI, `App`/`AppLogic`, `Session`, feature handlers; the `classic-game-box` bin is a **deprecated** `winit` dev host (not kept in sync) |
| `cgb-host` | lib | the host contract: `HostWindow` / `GamepadSource` traits, shared by the app and the hosts |
| `cgb-mac` | staticlib | the Swift host: a `CAMetalLayer` → wgpu surface, native-event translation, the `cgb_mac_*` C ABI |
| `cgb-systems` | lib | system/core registry, joypad ids (dependency-free) |
| `cgb-paths` | lib | file layout (`Paths`) + settings (`Settings`) |
| `cgb-cores` | lib | `cores.json` manifest, buildbot catalog, runtime downloader |
| `cgb-library` | lib | SQLite game library, ROM import, screenshots, save states, `.srm`, cheats |
| `cgb-libretro` | lib | libretro front end: `dlopen`, callbacks, ABI |
| `cgb-audio` | lib | cpal output + SPSC ring buffer (int16 stereo) |
| `cgb-input` | lib | keyboard bindings + `GamepadSnapshot`; `gilrs` is an optional feature |

Dependency direction:

```
cgb-mac  → cgb-app (default-features = false), cgb-host, cgb-input, igui, arboard
cgb-app  → cgb-host, cgb-libretro, cgb-audio, cgb-input, cgb-paths, cgb-cores, cgb-library, cgb-systems
cgb-host → cgb-input
device crates / cgb-libretro → cgb-systems
cgb-systems → nothing
```

`cgb-app` never names a windowing library: the window is a `HostWindow` trait
and the gamepad a `GamepadSource`, both from `cgb-host`. The default
`winit-host` feature provides the `winit` + `gilrs` implementations; the
embedded host builds with `--no-default-features` and provides its own.

The app's own layout is under [`cgb-app/src`](cgb-app/src): `app/` (state,
frame loop, feature handlers, host traits), `ui/` (views built from a pure
`ViewModel`), `session.rs` (one running game), `cli.rs` / `cores_cli.rs` /
`selfcheck.rs`.

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
