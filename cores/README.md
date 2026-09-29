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
| `fbneo` | Arcade | `libretro/FBNeo` | `dist/fbneo_libretro.dylib` | ✅ arm64, loads standard Neo Geo sets (encrypted C-ROMs) |
| `parallel_n64` | Nintendo 64 | `libretro/parallel-n64` | `dist/parallel_n64_libretro.dylib` | ✅ arm64 + dynarec, **hardware-rendered** (OpenGL / GLideN64) |
| `genesis_plus_gx` | MD / Genesis / SMS / GG / SG-1000 | `libretro/Genesis-Plus-GX` | `dist/genesis_plus_gx_libretro.dylib` | 🔧 build.sh added, not yet built/verified |
| `picodrive` | MD / Genesis / SMS / GG / SG-1000 | `libretro/picodrive` | `dist/picodrive_libretro.dylib` | 🔧 build.sh added, not yet built/verified |
| `freej2me_plus` | J2ME (Java ME) | `TASEmulators/freej2me-plus` | `dist/freej2me_plus_libretro.dylib` | ✅ arm64, boots a JVM and returns 240×320 frames |

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
- `system` is `nes`, `gba`, `gb`, `gbc`, `arcade`, `n64`, `j2me` (or one of
  the Sega keys); an unknown system is skipped with a
  warning. A duplicate `(system, key)` keeps the first.
- `option_defaults` (optional) is a `{ "core_option_key": "value" }` map the
  app applies **before loading**, for options the player has not chosen. A core's
  own default can be a poor fit for a desktop frontend: `freej2me_plus` overrides
  `freej2me_backlightcolor` to `Disabled`, because the core otherwise tints every
  frame with a green LCD backlight.

The app reads the packaged `<app data>/cores/cores.json`, else this file when
running from a checkout. `dylib` is a file name resolved in the packaged
`cores/` dir and `dist/`, or an absolute path. The loader is
`cgb-library::load_cores`; `cgb-systems::choose_core` makes the pick.

## Adding a core

Same process for a core you wrote or a third-party project. Two shapes: an
**existing console** (data only) or a **new console** (also a small Rust
change).

**Conventions**

- Directory `cores/<name>/`, where `<name>` is the manifest `key` (lower snake
  case).
- Script `cores/<name>/build.sh`, executable, emits `cores/dist/<dylib>`.
- Third-party source clones into `cores/sources/<name>` (gitignored); a
  `<NAME>_SRC` env var overrides it with a local checkout.
- One row in [`cores.json`](cores.json).

### 1. Build script

Copy [`build.sh.example`](build.sh.example) to `cores/<name>/build.sh`, fill
in the clone URL, source dir and output name, and `chmod +x` it. There are two
shapes, both already in the tree:

- **Makefile** (Mesen, Nestopia, QuickNES, Snes9x, Genesis Plus GX, PicoDrive):
  `make -C <srcdir> -f Makefile platform=osx -j"$JOBS"`.
  *macOS 26+ quirk:* some Makefiles derive their deployment target from the
  macOS *minor* version and fall back to 10.4, which arm64 rejects. Pass
  `MINVERSION=-mmacosx-version-min=11.0` (Nestopia does).
- **CMake** (mGBA is the only one so far): configure with
  `-DBUILD_LIBRETRO=ON`, then `cmake --build … --target <name>_libretro`.

`fbneo` is the arcade core. It matches the *standard* Neo Geo romsets (the
encrypted 4 MiB C-ROMs, e.g. `201-c1.c1` = `72813676`) and reads its ROMs by
CRC, so the BIOS file names do not matter. It is a plain Makefile build from
`libretro/FBNeo` (`src/burner/libretro`, `platform=osx`).

`genesis_plus_gx` is one module for four consoles: Mega Drive / Genesis,
Master System, Game Gear and SG-1000. It builds from `Makefile.libretro` at the
`libretro/Genesis-Plus-GX` repo root with `platform=osx`, and renders **RGB565**
(the host converts it). `cores.json` lists it once per console under the same
key, the way mGBA appears for GBA and GB; the core picks the console from the
loaded ROM.

`picodrive` is the lightweight alternative for the same four consoles (plus 32X
and Sega/Mega CD, which this app does not model yet). It is the first core with
git submodules (`platform/libpicofe`, `cpu/cyclone`, `pico/cd/libchdr`,
`pico/sound/emu2413`, `platform/common/dr_libs`), so its `build.sh` clones with
`--recurse-submodules`. It builds from `Makefile.libretro` at the
`libretro/picodrive` repo root with `platform=osx`, renders **RGB565**, declares
`need_fullpath`, and lists once per console like Genesis Plus GX.

`freej2me_plus` is the odd one: the libretro module is only a shim that
`fork/exec`s a Java VM (`freej2me_plus-lr.jar`) and talks to it over
stdin/stdout. Its `build.sh` therefore also compiles the jar and builds a
`jlink`-trimmed JRE, and needs a JDK 9+ on the machine (upstream's own Ant
build instead wants JDK 8; the script compiles with `javac --release 8`, which
works on any modern JDK). Before compiling it patches `Libretro.java` from a
pristine copy: decode the piped game/save paths as UTF-8 (non-ASCII names would
otherwise be "not found" and the JVM exits to a black frame), and rate-limit the
synthetic key repeat (upstream fires `keyRepeated` every frame, i.e. ~60/s, so a
held direction races). The outputs land in `cores/dist/freej2me_plus/`
(`freej2me_plus-lr.jar` + `runtime/`); the app seeds the jar into the writable
system dir and puts `runtime/bin` on `PATH` so the core finds `java`. See
`crates/cgb-app/src/app.rs` (`j2me_dir`, `prepend_path`).

`./scripts/build-cores.sh` runs every `cores/*/build.sh` in name order;
`--skip-mgba` skips the cmake build.

### 2. Manifest row

Add the entry to [`cores.json`](cores.json) (see the format above). The console
name in `system` is what ties the core to a loader — get it right and nothing
else is needed.

### 3. New console (only if `system` is not already supported)

A small, mechanical Rust change; the core itself is still just data.

- `crates/cgb-systems/src/system.rs`: add the `SystemId` variant, then update
  `SYSTEMS`, `name`, `short`, `extensions`, `key`, `parse_key` and
  `system_for_path`. Add a case to the `system_for_path` test. `from_key`
  delegates to `parse_key`, and the manifest loader uses `parse_key` too, so
  the key → console map lives in exactly one place.
- `crates/cgb-library/src/settings.rs`: add the `<system>_core` field and the
  `core_key` / `set_core_key` match arms, so the pick persists.

No UI change: the library badge and play page read `SystemId`.

### 4. Verify

```bash
./scripts/build-cores.sh --skip-mgba
file cores/dist/<name>_libretro.dylib                  # arm64
nm -gU cores/dist/<name>_libretro.dylib | grep -c ' _retro_'
cargo run -p cgb-app -- --rom game.<ext> --core <name>
```

Then add the core to `crates/cgb-libretro/tests/cores_run_through_the_host.rs`:
a synthetic ROM plus its expected geometry and sample rate. That test is the
gate — it drives the core through the real `CoreHost`, including the pixel
format conversion (see below). A core can also be tried with no row at all,
straight from a path: `--core ./cores/dist/<dylib>`.

### 5. Runtime notes (why most cores need no patch)

- **Pixel format:** the host accepts `XRGB8888` and `RGB565` and converts both
  to RGBA8; anything else is refused and the core keeps its `0RGB1555` default.
  So a core that hardcodes RGB565 (mGBA, Snes9x) needs no patch.
- **`need_fullpath`:** the app always passes the real ROM path *and* the bytes,
  so cores that read the file (Mesen) and cores that read the buffer both work.
- **`pitch`** is bytes per row, not pixels; the host slices each row by the
  format's bytes-per-pixel, so a padded pitch is fine.

`custom_nes_core` is the one core built without cmake or a third-party
Makefile: it compiles the read-only `legacy/packages/fc-*` sources directly
with `clang++`. The front end still uses only the standard libretro ABI — that
core's private `fc_*` extension is ignored.

## Why scripts, not a build target

Mesen, mGBA and FBNeo are tens to hundreds of megabytes of source, fetched
over the network and built in minutes (mGBA needs cmake; FBNeo is the biggest).
They do not belong in `cargo build`. Once built, the app runs them exactly like
any other libretro core.

## arm64

Everything here targets Apple Silicon. Each built core was verified arm64 with
its `retro_*` exports (`nm -gU`). mGBA (`libretro/mgba`, CMake
`-DBUILD_LIBRETRO=ON`) renders **RGB565**; the host accepts and converts both
XRGB8888 and RGB565. Every built core is exercised by
`crates/cgb-libretro/tests/cores_run_through_the_host.rs`.
