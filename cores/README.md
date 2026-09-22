# cores

Native libretro cores, built on demand. Each is a third-party project of its
own, cloned into `sources/` (gitignored) and linked into `dist/` (gitignored):

| Core | Consoles | Source | Output | Status |
|---|---|---|---|---|
| Mesen | NES / FC | `libretro/Mesen` | `dist/mesen_libretro.dylib` | ✅ built & ABI-verified (Q1) |
| mGBA | GB / GBC / GBA | `EmulatorJS/mgba` | `dist/mgba_libretro.dylib` | ⏸ deferred to Q4, unverified |

```bash
./scripts/build-cores.sh              # Mesen (NES) only — the Q1 target
./scripts/build-cores.sh --with-mgba  # also mGBA, when GBA work resumes
./cores/mesen/build.sh                # NES only
```

The app looks for the resulting `.dylib` next to its data, via
`cgb-library::Paths::core_dylib`. `libretro/libretro.h` is the vendored ABI
contract (the same header `crates/cgb-libretro` binds by hand).

## Why these are scripts, not a build target

Mesen and mGBA are tens of megabytes of source each, fetched over the network
and built in minutes. They do not belong in `cargo build`. Once built, the app
runs them exactly like any other libretro core.

## arm64

Everything here targets Apple Silicon. Mesen is the older Mesen 1.x (C++11);
it built cleanly as arm64 and its `retro_*` exports were verified with
`nm -gU`. mGBA is deferred: upstream `libretro/mgba` now builds the core
through CMake (`-DBUILD_LIBRETRO=ON`), and this machine has no cmake, so the
deferred script targets the `EmulatorJS/mgba` fork that still ships
`Makefile.libretro`. See `mgba/build.sh` for the full note.
