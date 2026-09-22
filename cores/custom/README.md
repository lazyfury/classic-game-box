# custom cores

Third-party libretro cores you add yourself. Everything here is data plus a
build script — no Rust change, no registry edit.

## Add one

1. Create the build script `cores/custom/<name>/build.sh`. It clones/builds
   whatever it needs and **must** drop the finished module in `cores/dist/`:

   ```bash
   #!/bin/bash
   set -euo pipefail
   ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
   SRC="$ROOT/cores/sources/nestopia"
   [ -d "$SRC" ] || git clone --depth 1 https://github.com/libretro/nestopia "$SRC"
   make -C "$SRC" -f Makefile platform=osx -j"$(sysctl -n hw.ncpu)"
   mkdir -p "$ROOT/cores/dist"
   cp "$SRC"/nestopia_libretro.dylib "$ROOT/cores/dist/"
   ```

   Make it executable (`chmod +x`). `scripts/build-cores.sh` runs every
   `cores/custom/*/build.sh` after the built-in cores.

2. Declare it in [`cores.json`](cores.json):

   ```json
   {
     "cores": [
       {
         "key": "nestopia",
         "name": "Nestopia",
         "system": "nes",
         "dylib": "nestopia_libretro.dylib",
         "sample_rate": 48000,
         "fps": 60.098
       }
     ]
   }
   ```

   `key` is what `--core` and the settings picker use; it must be unique.
   `name` defaults to `key`. `sample_rate` / `fps` are hints only — the real
   values come from the core's own `av_info` after a game loads. `system` is
   `nes`, `gba`, `gb` or `gbc`; an unknown system is skipped with a warning.

3. Build and run:

   ```bash
   ./scripts/build-cores.sh
   cargo run -p cgb-app -- --rom mario.nes --core nestopia
   ```

   Or skip the manifest entirely and point straight at a module:
   `cargo run -p cgb-app -- --rom mario.nes --core ./cores/dist/nestopia_libretro.dylib`.

## Where the app looks

`cores/custom/cores.json` is read at startup: the packaged
`<app data>/cores/cores.json` first, then this repo's
`cores/custom/cores.json` when running from a checkout. `dylib` is a file name
resolved in the packaged `cores/` dir and `cores/dist/`, or an absolute path.
