# custom cores

Third-party libretro cores you add yourself. Everything here is data plus a
build script — no Rust change, no registry edit.

Two cores ship as examples, both already declared in
[`cores.json`](cores.json) so they work after one build:

- [Nestopia](nestopia/build.sh) (NES) — clones `libretro/nestopia` and builds
  it with the project's own libretro Makefile.
- [custom_nes_core](custom_nes_core/build.sh) (NES) — the legacy hand-written
  FC / NES core from the read-only `legacy/` archive. That tree is a CMake
  project, but this machine has no cmake, so the script drives `clang++`
  directly over the same sources. Only the standard libretro ABI is used: the
  core also exports a private `fc_*` extension (`fc_libretro_get_ext`) which
  the host ignores.

## Add another

1. Create the build script `cores/custom/<name>/build.sh`. It clones/builds
   whatever it needs and **must** drop the finished module in `cores/dist/`.
   Copy [`nestopia/build.sh`](nestopia/build.sh) and change the clone URL,
   source dir and final `cp`. Make it executable (`chmod +x`).
   `scripts/build-cores.sh` runs every `cores/custom/*/build.sh` after the
   built-in cores.

2. Declare it in [`cores.json`](cores.json), next to the Nestopia entry:

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

   `key` is what `--core` and the settings picker use; it must be unique
   **per console** (one module can serve two consoles, like mGBA). `name`
   defaults to `key`. `sample_rate` / `fps` are hints only — the real values
   come from the core's own `av_info` after a game loads (`nestopia` reports
   256×240 @ 60.099 fps / 48000 Hz). `system` is `nes`, `gba`, `gb` or `gbc`;
   an unknown system is skipped with a warning. A `(system, key)` that the
   built-in [`../cores.json`](../cores.json) already declares is skipped: a
   custom core cannot silently shadow Mesen/mGBA.

3. Build and run:

   ```bash
   ./scripts/build-cores.sh
   cargo run -p cgb-app -- --rom mario.nes --core nestopia
   ```

   Or skip the manifest entirely and point straight at a module:
   `cargo run -p cgb-app -- --rom mario.nes --core ./cores/dist/nestopia_libretro.dylib`.

## Where the app looks

Two manifests are merged at startup: the built-in [`../cores.json`](../cores.json)
(Mesen, mGBA), then this one. Each is read from the packaged data dir first
(`<app data>/cores/cores.json` and `<app data>/cores/custom/cores.json`), then
from the checkout (`cores/cores.json`, `cores/custom/cores.json`) when running
development. `dylib` is a file name resolved in the packaged `cores/` dir and
`cores/dist/`, or an absolute path.
