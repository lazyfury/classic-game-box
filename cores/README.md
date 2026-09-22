# cores

Native libretro cores, built on demand. Each is a third-party project of its
own, cloned into `sources/` (gitignored) and linked into `dist/` (gitignored).
Which cores exist is **data, not code**: the app loads them from `cores.json`.

| Core | Consoles | Source | Output | Status |
|---|---|---|---|---|
| Mesen | NES / FC | `libretro/Mesen` | `dist/mesen_libretro.dylib` | ✅ arm64, runs a synthetic NROM |
| mGBA | GB / GBC / GBA | `libretro/mgba` | `dist/mgba_libretro.dylib` | ✅ arm64, RGB565, runs a synthetic GBA ROM |
| Nestopia (custom) | NES / FC | `libretro/nestopia` | `dist/nestopia_libretro.dylib` | ✅ arm64, runs a synthetic NROM |
| custom_nes_core (custom) | NES / FC | `legacy/packages/fc-{core,libretro}` | `dist/custom_nes_core_libretro.dylib` | ✅ arm64 (clang++ direct), runs a synthetic NROM |

```bash
./scripts/build-cores.sh              # Mesen + mGBA + every custom core
./scripts/build-cores.sh --skip-mgba  # skip mGBA when cmake is unavailable
./cores/mesen/build.sh                # build one core at a time
```

## The core list is data: `cores.json`

Two manifests are merged at startup, in this order:

- [`cores.json`](cores.json) — the built-in cores (Mesen, mGBA ×2).
- [`custom/cores.json`](custom/cores.json) — cores you add.

Each entry is `key` / `name` / `system` / `dylib` (+ optional `sample_rate`,
`fps`). `key` is unique **per console**, so one module can serve two: mGBA
appears once for GBA and once for GB. A built-in `(system, key)` wins a
collision, so a custom core cannot silently shadow Mesen/mGBA. A key a settings
file remembers but no manifest declares just falls back to the console default.

The loader is `cgb-library::load_cores`; `cgb-systems::choose_core` makes the
pick; `App::find_module` resolves a `dylib` (packaged `cores/`, then this
`dist/`) or uses a path as given. `libretro/libretro.h` is the vendored ABI
contract (the same header `crates/cgb-libretro` binds by hand).

## Custom / third-party cores

See [`custom/README.md`](custom/README.md). In short: a
`cores/custom/<name>/build.sh` that emits into `dist/`, plus a row in
`custom/cores.json`. `./scripts/build-cores.sh` runs every such script, so
adding a core changes no Rust. A core can also be tried without any manifest,
straight from a path:

```bash
cargo run -p cgb-app -- --rom mario.nes --core ./cores/dist/mesen_libretro.dylib
```

## Why scripts, not a build target

Mesen and mGBA are tens of megabytes of source each, fetched over the network
and built in minutes (mGBA needs cmake). They do not belong in `cargo build`.
Once built, the app runs them exactly like any other libretro core.

## arm64

Everything here targets Apple Silicon. Each built core was verified arm64 with
its `retro_*` exports (`nm -gU`). mGBA (`libretro/mgba`, CMake
`-DBUILD_LIBRETRO=ON`) renders **RGB565**; the host accepts and converts both
XRGB8888 and RGB565.
