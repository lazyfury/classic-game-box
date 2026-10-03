# cores

Native libretro cores, built on demand. Each core is a directory with its own
build script (`cores/<name>/build.sh`); the third-party source is cloned into
`sources/` (gitignored) and the module linked into `dist/` (gitignored).

| Core | Consoles | Source | Output | Status |
|---|---|---|---|---|
| `mesen` | NES / FC | `libretro/Mesen` | `dist/mesen_libretro.dylib` | ✅ arm64, synthetic NROM |
| `mgba` | GB / GBC / GBA | `libretro/mgba` | `dist/mgba_libretro.dylib` | ✅ arm64, RGB565, synthetic GBA ROM |
| `snes9x` | SNES / SFC | `libretro/snes9x` | `dist/snes9x_libretro.dylib` | ✅ arm64, RGB565, synthetic LoROM |
| `nestopia` | NES / FC | `libretro/nestopia` | `dist/nestopia_libretro.dylib` | ✅ arm64, synthetic NROM |
| `custom_nes_core` | NES / FC | `custom_nes_core/`（单 CMake 项目） | `dist/custom_nes_core_libretro.dylib` | ✅ arm64 (cmake), synthetic NROM |
| `fbneo` | Arcade | `libretro/FBNeo` | `dist/fbneo_libretro.dylib` | ✅ arm64, loads standard Neo Geo sets (encrypted C-ROMs) |
| `parallel_n64` | Nintendo 64 | `libretro/parallel-n64` | `dist/parallel_n64_libretro.dylib` | ✅ arm64 + dynarec, **hardware-rendered** (OpenGL / GLideN64) |
| `ppsspp` | PlayStation Portable | `hrydgard/ppsspp` (buildbot dylib) | `dist/ppsspp_libretro.dylib` | ✅ arm64 (buildbot dylib), **hardware-rendered** (OpenGL) |
| `mednafen_psx_hw` | Sony PlayStation | `libretro/beetle-psx-libretro` | `dist/mednafen_psx_hw_libretro.dylib` | ✅ arm64, **hardware-rendered** (OpenGL) |
| `pcsx2` | PlayStation 2 | `PCSX2/pcsx2` (buildbot dylib) | `dist/pcsx2_libretro.dylib` | ✅ arm64 (buildbot dylib), **hardware-rendered** (OpenGL core ≥ 3.3) |
| `genesis_plus_gx` | MD / Genesis / SMS / GG / SG-1000 | `libretro/Genesis-Plus-GX` | `dist/genesis_plus_gx_libretro.dylib` | 🔧 build.sh added, not yet built/verified |
| `picodrive` | MD / Genesis / SMS / GG / SG-1000 | `libretro/picodrive` | `dist/picodrive_libretro.dylib` | 🔧 build.sh added, not yet built/verified |
| `freej2me_plus` | J2ME (Java ME) | `TASEmulators/freej2me-plus` | `dist/freej2me_plus_libretro.dylib` | ✅ arm64, boots a JVM and returns 240×320 frames |

```bash
./scripts/build-cores.sh                    # every cores/*/build.sh, in name order
./scripts/build-cores.sh --minimal          # only the redistributable set
./scripts/build-cores.sh --only mesen,mgba  # an explicit list
./scripts/build-cores.sh --skip-mgba        # skip mGBA when cmake is unavailable
./cores/mesen/build.sh                      # build one core at a time
```

## Bundled set vs. downloaded cores

The released app ships **no core dylibs** — only the full `cores/cores.json`.
Cores are fetched at runtime from the libretro buildbot (settings page →
下载核心, or `--download-core`); the downloaded rows land in
`<app data>/cores/downloaded.json` and are merged with the bundled manifest at
startup. At load the app drops manifest rows whose module is absent
(`resolve_module` in `src/app/helpers.rs`), so it never offers a core it cannot
run, and keeps the rest so the library page can recommend a download. When the
library has games for a console with no available core, the library page shows
a “缺少核心” card (and adding such games prompts once) with a one-click
download of the recommended core.

To bundle cores into a local build, build them into `cores/dist` first:
`macos/scripts/package.sh` copies every `cores/dist/*.dylib` into
`Contents/Resources/cores`. `./scripts/build-cores.sh --minimal` builds the
redistributable set `mesen mgba custom_nes_core freej2me_plus` (see
`scripts/core-profiles.sh`), `--only a,b,c` picks an explicit list, and no flag
builds every `cores/*/build.sh`. The non-commercial cores (`snes9x`,
`genesis_plus_gx`, `picodrive`, `fbneo`) cannot be sold, so build them only
locally and never redistribute them.

### No buildbot build — compile these yourself

Two cores have no `apple/osx/arm64` buildbot artifact, so neither the release
nor the runtime download can provide them. Build from source and put the
result where the app looks:

- **`freej2me_plus`** (Java ME) — `./cores/freej2me_plus/build.sh` (needs a
  JDK). Copy `cores/dist/freej2me_plus_libretro.dylib` and the
  `cores/dist/freej2me_plus/` bundle (jar + trimmed JRE) into a packaged app's
  `Contents/Resources/cores/` and `Contents/Resources/freej2me_plus/`; in a
  source checkout, leaving them in `cores/dist/` is enough. On startup the app
  seeds the jar into `<app data>/system/` and prepends the JRE to `PATH`.
- **`custom_nes_core`** (the self-authored NES core) —
  `./cores/custom_nes_core/build.sh`, then drop the dylib into
  `Contents/Resources/cores/` (packaged) or `cores/dist/` (checkout).

## Blocked cores

Some buildbot cores download and load fine but still cannot run a game here.
`BLOCKED_CORES` in `crates/cgb-cores/src/catalog.rs` hides those from the whole
app: they are not listed in the download card (`--search-core`), never
recommended for their console, and `--download-core` / registration refuse
them.

- **`squirreljme`** (Java ME): its libretro glue passes `-jar <path>` as the
  whole `argv`, but `sjme_nvm_parseCommandLine` starts at `argv[1]` (it treats
  `argv[0]` as the program name), so it falls through to
  `SJME_ERROR_INVALID_ARGUMENT` and `retro_load_game` returns `false` — the app
  reports that as “the core refused to load `<jar>`”. It additionally needs a
  SquirrelJME boot-suite jar in the system directory, which the buildbot does
  not ship. Use the bundled FreeJ2ME-Plus for J2ME instead.

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
- `system` is `nes`, `gba`, `gb`, `gbc`, `snes`, `arcade`, `n64`, `psp`, `ps1`, `ps2`, `j2me` (or one of
  the Sega keys); an unknown system is skipped with a
  warning. A duplicate `(system, key)` keeps the first.
- `option_defaults` (optional) is a `{ "core_option_key": "value" }` map the
  app applies **before loading**, for options the player has not chosen. A core's
  own default can be a poor fit for a desktop frontend: `freej2me_plus` overrides
  `freej2me_backlightcolor` to `Disabled`, because the core otherwise tints every
  frame with a green LCD backlight. `nestopia` overrides
  `nestopia_blargg_ntsc_filter` to `disabled`: its Blargg NTSC filter reads a
  wild pointer (SIGSEGV) when Nestopia is loaded in-process after Mesen has run
  a frame, and the crisp 256×224 output suits the nearest-neighbour scaler.

The app reads the packaged `<app data>/cores/cores.json`, else this file when
running from a checkout. `dylib` is a file name resolved in the packaged
`cores/` dir and `dist/`, or an absolute path. The loader is
`cgb-library::load_cores`; `cgb-systems::choose_core` makes the pick.

## Downloading cores at runtime

The app can fetch a core from the libretro buildbot instead of shipping every
`.dylib`. The **catalog** ("下载源") is a local copy of the buildbot's core list:

- `cores/catalog.json` — the **built-in snapshot**, embedded at compile time,
  so search works offline. Regenerate it with `./scripts/update-core-catalog.sh`.
- `<app data>/cores/catalog.json` — the **user cache**, written by
  `--force-update`; the app prefers it over the snapshot when it parses.

CLI:

```bash
classic-game-box --search-core snes        # search the catalog (offline)
classic-game-box --force-update            # refresh the cache from the buildbot
classic-game-box --download-core mame      # fetch a core, then open it to prove it loads
```

The download lands in `<app data>/cores/<name>_libretro.dylib` (which
`find_module` already searches first) and is recorded in
`<app data>/cores/downloaded.json`, so it shows up in the settings page's
per-console core picker. A core whose console this app does not model is not
registered — it loads only via `--core <path>`. The download URL is built from
the current platform (`apple/osx/arm64`, `linux/x86_64`, …) and the buildbot's
`latest/` directory; `--core-base-url` overrides the base.

The settings page's **下载核心** card is the same thing with a search box: type a
name, click 下载, and the download runs on a background thread while the status
line shows progress. `刷新下载源` rewrites the cache.

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

### 1. Build it first

Get the module building before anything else: a core that does not emit a
loadable `cores/dist/<dylib>` cannot be taken further.

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

`snes9x` is the Super Nintendo / SFC core. It is software-rendered: it asks
for **RGB565** (the host converts it) and never calls `SET_HW_RENDER`, so it
rides the plain frame path with no OpenGL. SuperFX / SA-1 / CX4 / SDD-1 /
SPC7110 / MSU-1 and the DSP-1..4 mixers are emulated in-tree (the DSP firmware
table is compiled in), so a regular SNES game needs **no BIOS or firmware
ROM**; only BS-X (`BS-X.bin`) and Sufami Turbo (`STBIOS.bin`) want one, and the
manifest does not expose `.bs`/`.st`. It is a Makefile core, but the file is
`libretro/Makefile` (not the repo root), has no submodules, and needs no
deployment-target override on arm64. `.bin` stays with Genesis, so a SNES `.bin`
is fixed per game from the card's “选择机种…” menu.

`ppsspp` is the PlayStation Portable core. Its `build.sh` installs the libretro
buildbot's `apple/osx/arm64` dylib instead of compiling: the upstream libretro
Makefile coerces every `TARGET_ARCH` containing "64" to `x86_64`, and its macOS
ffmpeg bundle (`hrydgard/ppsspp-ffmpeg`) ships only `macosx/universal` with no
`arm64` slice, so the buildbot artifact is the maintained arm64 build (override
with `PPSSPP_URL`). It links only desktop OpenGL, so it rides the same offscreen
GL path as ParaLLEl-N64.

`mednafen_psx_hw` is Beetle PSX HW, the PlayStation core. It builds from source
(no submodules) with `HAVE_OPENGL=1` — that flag selects the hardware renderer
and renames the output to `mednafen_psx_hw`; without it the same Makefile emits
the software `mednafen_psx`. It links desktop OpenGL and rides the offscreen GL
path like N64/PSP. A real PlayStation BIOS is optional (Beetle runs HLE /
OpenBIOS); drop `scph5500.bin` / `scph5501.bin` / `scph5502.bin` into the app's
system directory and the firmware scan finds it. `.cue`/`.ccd`/`.toc`/`.m3u`/`.img`
select PS1; `.iso`/`.chd`/`.pbp` stay with PSP, and a card's “选择机种…” menu
overrides the extension per game. The script also fetches the upstream `assets/` tree
into `cores/dist/ppsspp/`; the app seeds that into `<system dir>/PPSSPP/` at
startup (without `compat.ini` the core warns at init), and `package.sh`
ships it in `Resources/ppsspp/`.

`pcsx2` is **LRPS2**, the PlayStation 2 core. Its `build.sh` installs the
libretro buildbot's `apple/osx/arm64` dylib (override with `PCSX2_URL`). It is
the **unstable** PS2 core (see Status below); the recommended/down-loadable
default for PS2 is **`play`** (Play!), which `cores.json` lists first.

**macOS can only run LRPS2's software renderer.** Its OpenGL renderer hard-fails
on any context without `GL_ARB_shading_language_420pack` (`GSDeviceOGL.cpp`:
"this is required for the OpenGL renderer"), and macOS caps out at OpenGL 4.1
(verified on this machine: the CGL 4.1 core context the front end creates
exposes neither 420pack nor `GL_ARB_shading_language_packing`). The hardware
renderers (Vulkan, paraLLEl-GS) need a Vulkan context, which the front end does
not offer. `cores.json` therefore defaults `pcsx2_renderer` to `Software (SW)`
so the core rides the plain software frame path; a player can override it in the
core-options UI, but the hardware choices need the Vulkan host work described in
`docs/architecture/`.

LRPS2 has **no HLE BIOS**: a real PS2 BIOS dump is required. Drop `scph*.bin` /
`rom1.bin` / `erom.bin` into `<app data>/system/pcsx2/bios/` (the core-info's
`firmware0_path` is `pcsx2/bios`, relative to the system dir). `.iso` / `.chd` /
`.bin` / `.cue` stay with PSP / PS1 / Genesis, so only the unambiguous PS2
extensions (`elf`, `ciso`, `zso`, `mdf`, `nrg`, `dump`) select PS2; a `.iso`
guessed as PSP is fixed per game from the card's “选择机种…” menu. PS2 disc
images are large, so rewind is disabled for the console like N64 / PSP / PS1.

**Status: unstable.** LRPS2 produces picture and sound on the software path.
Headless (release) the load, the frame loop, `serialize` (a 50.6 MB save state),
unload and switching games all work; the crashes are **`retro_reset` and
`retro_unserialize` → SIGBUS** — the fault is right after “Resetting host memory
for virtual systems…” and in the mVU reset, so it is PCSX2's host-memory / state
rebuild on macOS, **not CHD decompression or file I/O**. Disabling MTVU does not
help. `armsx2` / `pcee2` (the same PCSX2 family) are tagged too. Such cores stay
**downloadable and usable**, but `UNSTABLE_CORES` in `src/cores/catalog.rs` tags
them so the download list shows a yellow “不稳定” and they are not the
recommended core. `play` (Play!) is the recommended PS2 core: it runs, just
slowly, and has lower compatibility. Do not treat PS2 as a supported console
yet.

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
`src/app/mod.rs` (`j2me_dir`, `prepend_path`).

`./scripts/build-cores.sh` runs every `cores/*/build.sh` in name order;
`--skip-mgba` skips the cmake build.

### 2. Align the Rust side

The front end drives every core through one libretro host
(`crates/cgb-libretro/src/host.rs`). A new core may call an environment
command the host does not answer yet, so run it through the real host (`--core
<path>`, or the test in step 5) and watch the log, then close the gaps. What
the host already covers:

- **Pixel format** — `XRGB8888` and `RGB565` are accepted and converted to
  RGBA8; anything else is refused and the core keeps its `0RGB1555` default. A
  core that hardcodes RGB565 (mGBA, Snes9x, Genesis Plus GX, PicoDrive) needs
  nothing.
- **`need_fullpath`** — the app always passes the real path *and* the bytes, so
  a core that reads the file (Mesen) and one that reads the buffer both work.
- **Hardware render** — `SET_HW_RENDER` with `OPENGL_CORE` / `OPENGL` rides the
  offscreen CGL GL path (`crates/cgb-libretro/src/gl.rs`); every other context
  type (Vulkan included) is refused so the core can fall back. A software core
  needs nothing.
- **Core options / input** — core options v1 and v2,
  `SET_CONTROLLER_PORT_DEVICE`, input descriptors, and a no-op rumble interface
  are handled. A core that also wants the keyboard, mouse or pointer callbacks
  is a host change (`host.rs::environment`), as is any new environment command.
- **System / save directories** — `GET_SYSTEM_DIRECTORY` (`<app data>/system`)
  and `GET_SAVE_DIRECTORY` are answered. A core that reads its own assets or
  firmware from the system dir (PPSSPP, FreeJ2ME-Plus) also needs a seed step in
  `src/app/mod.rs` and a copy in `macos/scripts/package.sh`; a BIOS is just a
  file the user drops in, no code.

Touch `ffi.rs` / `host.rs` only when the ABI genuinely needs it, keep the
answers honest, and add a test for anything with a side effect. A private,
non-libretro extension is ignored (`custom_nes_core`'s `fc_*` is never used).

### 3. Manifest row

Add the entry to [`cores.json`](cores.json) (see the format above). The console
name in `system` is what ties the core to a loader — get it right and nothing
else is needed.

### 4. New console (only if `system` is not already supported)

A small, mechanical Rust change; the core itself is still just data.

- `crates/cgb-systems/src/system.rs`: add the `SystemId` variant, then update
  `SYSTEMS`, `name`, `short`, `extensions`, `key`, `parse_key` and
  `system_for_path`. Add a case to the `system_for_path` test. `from_key`
  delegates to `parse_key`, and the manifest loader uses `parse_key` too, so
  the key → console map lives in exactly one place.
- `crates/cgb-library/src/settings.rs`: add the `<system>_core` field and the
  `core_key` / `set_core_key` match arms, so the pick persists.

No UI change: the library badge and play page read `SystemId`.

### 5. Verify

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

### 6. Decide how it ships

Once the core runs, pick how it reaches the player: not every core is bundled.

- **Bundled** — add the key to `CGB_MINIMAL_CORES` in
  `scripts/core-profiles.sh` when the core is small, clean to redistribute and
  has no buildbot build (or must work offline out of the box), the way
  `custom_nes_core` and `freej2me_plus` are. `macos/scripts/package.sh` then
  copies its dylib into the app.
- **Download-only** — leave it out and let the library page's “缺少核心” card /
  the settings 下载核心 card offer it at runtime. Check the libretro buildbot
  actually has it for the target platforms (`apple/osx/arm64`, `linux/x86_64`,
  …); the catalog is a snapshot, so run `./scripts/update-core-catalog.sh` when
  the core is new. The non-commercial cores (`snes9x`, `genesis_plus_gx`,
  `picodrive`, `fbneo`) ship this way — they cannot be redistributed for sale.
- **Blocked** — if the buildbot has it but it cannot run a game here, add the
  key to `BLOCKED_CORES` (`crates/cgb-cores/src/catalog.rs`) so it is never
  listed, recommended or registered. `squirreljme` is the current example; see
  “Blocked cores”.

### 7. Runtime notes (why most cores need no patch)

- **Pixel format:** the host accepts `XRGB8888` and `RGB565` and converts both
  to RGBA8; anything else is refused and the core keeps its `0RGB1555` default.
  So a core that hardcodes RGB565 (mGBA, Snes9x) needs no patch.
- **`need_fullpath`:** the app always passes the real ROM path *and* the bytes,
  so cores that read the file (Mesen) and cores that read the buffer both work.
- **`pitch`** is bytes per row, not pixels; the host slices each row by the
  format's bytes-per-pixel, so a padded pitch is fine.

`custom_nes_core` is built from an in-repo CMake project (`custom_nes_core/`,
a standard `src/` layout) rather than a third-party Makefile.
`cores/custom_nes_core/build.sh` drives cmake and falls back to a direct
`clang++` compile on machines without it. The front end still uses only the
standard libretro ABI — that core's private `fc_*` extension is ignored.

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
