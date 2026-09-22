# cores

Native libretro cores, built on demand. Each is a third-party project of its
own, cloned into `sources/` (gitignored) and linked into `dist/` (gitignored):

| Core | Consoles | Source | Output |
|---|---|---|---|
| Mesen | NES / FC | `libretro/Mesen` | `dist/mesen_libretro.dylib` |
| mGBA | GB / GBC / GBA | `libretro/mgba` | `dist/mgba_libretro.dylib` |

```bash
./scripts/build-cores.sh        # both
./cores/mesen/build.sh          # NES only
./cores/mgba/build.sh           # GB/GBA only
```

The app looks for the resulting `.dylib` next to its data, via
`cgb-library::Paths::core_dylib`. `libretro/libretro.h` is the vendored ABI
contract (the same header `crates/cgb-libretro` binds by hand).

## Why these are scripts, not a build target

Mesen and mGBA are tens of megabytes of source each, fetched over the network
and built in minutes. They do not belong in `cargo build`. Once built, the app
runs them exactly like any other libretro core.

## arm64

Everything here targets Apple Silicon. Mesen is the older Mesen 1.x (C++11)
and is the most likely place an arm64 flag needs fixing; mGBA supports arm64
natively. This is the Q1 spike risk called out in
`docs/architecture/quill-native-migration.md`.
