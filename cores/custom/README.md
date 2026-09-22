# custom cores

Third-party libretro cores you add yourself. Everything here is data plus a
build script — no Rust change, no registry edit.

[Nestopia](nestopia/build.sh) (NES) ships as the worked example: its
`build.sh` clones `libretro/nestopia`, builds it with the project's own
libretro Makefile, and drops `nestopia_libretro.dylib` in `cores/dist/`.
It is already declared in [`cores.json`](cores.json), so
`--core nestopia` works after one build.

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

   `key` is what `--core` and the settings picker use; it must be unique.
   `name` defaults to `key`. `sample_rate` / `fps` are hints only — the real
   values come from the core's own `av_info` after a game loads (`nestopia`
   reports 256×240 @ 60.099 fps / 48000 Hz). `system` is `nes`, `gba`, `gb`
   or `gbc`; an unknown system is skipped with a warning.

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
