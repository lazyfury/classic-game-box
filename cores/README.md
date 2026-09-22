# cores

Native libretro cores, built on demand. Each core is a directory with its own
build script (`cores/<name>/build.sh`); the third-party source is cloned into
`sources/` (gitignored) and the module linked into `dist/` (gitignored).

| Core | Consoles | Source | Output | Status |
|---|---|---|---|---|
| `mesen` | NES / FC | `libretro/Mesen` | `dist/mesen_libretro.dylib` | ✅ arm64, synthetic NROM |
| `mgba` | GB / GBC / GBA | `libretro/mgba` | `dist/mgba_libretro.dylib` | ✅ arm64, RGB565, synthetic GBA ROM |
| `nestopia` | NES / FC | `libretro/nestopia` | `dist/nestopia_libretro.dylib` | ✅ arm64, synthetic NROM |
| `custom_nes_core` | NES / FC | `legacy/packages/fc-{core,libretro}` | `dist/custom_nes_core_libretro.dylib` | ✅ arm64 (clang++ direct), synthetic NROM |

```bash
./scripts/build-cores.sh              # every cores/*/build.sh, in name order
./scripts/build-cores.sh --skip-mgba  # skip mGBA when cmake is unavailable
./cores/mesen/build.sh                # build one core at a time
```

`libretro/libretro.h` is the vendored ABI contract (the same header
`crates/cgb-libretro` binds by hand).

## The core list is `cores.json`

[`cores.json`](cores.json) is the single source of what exists:

```json
{
  "key": "nestopia",
  "name": "Nestopia",
  "system": "nes",
  "dylib": "nestopia_libretro.dylib",
  "sample_rate": 48000,
  "fps": 60.098
}
```

- `key` is what `--core` and the settings picker use; it is unique **per
  console**, so one module can serve two — mGBA appears once for GBA and once
  for GB.
- `name` defaults to `key`.
- `sample_rate` / `fps` are hints only: the real values come from the core's
  own `av_info` after a game loads.
- `system` is `nes`, `gba`, `gb` or `gbc`; an unknown system is skipped with a
  warning. A duplicate `(system, key)` keeps the first.

The app reads the packaged `<app data>/cores/cores.json`, else this file when
running from a checkout. `dylib` is a file name resolved in the packaged
`cores/` dir and `dist/`, or an absolute path. The loader is
`cgb-library::load_cores`; `cgb-systems::choose_core` makes the pick.

## Add a core

1. `cores/<name>/build.sh` — clone/build, drop the module in `cores/dist/`.
   Copy an existing one (e.g. [`nestopia/build.sh`](nestopia/build.sh)) and
   change the clone URL, source dir and final `cp`. Make it executable.
2. Add a row to [`cores.json`](cores.json).

`./scripts/build-cores.sh` runs every `cores/<name>/build.sh`, so adding a core
changes no Rust. A core can also be tried without any row, straight from a path:

```bash
cargo run -p cgb-app -- --rom mario.nes --core ./cores/dist/mesen_libretro.dylib
```

If it targets a new console, add the `SystemId` variant and extensions in
`crates/cgb-systems/src/system.rs`.

`custom_nes_core` is the one core built without cmake or a third-party
Makefile: it compiles the read-only `legacy/packages/fc-*` sources directly
with `clang++`. The front end still uses only the standard libretro ABI — that
core's private `fc_*` extension is ignored.

## Why scripts, not a build target

Mesen and mGBA are tens of megabytes of source each, fetched over the network
and built in minutes (mGBA needs cmake). They do not belong in `cargo build`.
Once built, the app runs them exactly like any other libretro core.

## arm64

Everything here targets Apple Silicon. Each built core was verified arm64 with
its `retro_*` exports (`nm -gU`). mGBA (`libretro/mgba`, CMake
`-DBUILD_LIBRETRO=ON`) renders **RGB565**; the host accepts and converts both
XRGB8888 and RGB565. All four cores are exercised by
`crates/cgb-libretro/tests/cores_run_through_the_host.rs`.
