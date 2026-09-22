# assets

Icons and other bundled, non-code resources.

The native cores are *not* here — they are built into `cores/dist/` and shipped
with the app bundle (see `cores/README.md`). Nothing in this folder is read by
the Rust crates at compile time.

## `roms/<system>/system/`

The arcade core needs a BIOS (`neogeo.zip` and friends). Those `.zip` files
live in `roms/arcade/system/` and are committed on purpose: they are small and
let the app work out of the box. They are **not** read in place — at startup
`cgb-app` seeds the writable `<app data>/system` directory the core is actually
pointed at (`GET_SYSTEM_DIRECTORY`) with any file it is missing, so a BIOS the
player drops there always wins. Game ROMs and saves are still never committed;
the `.gitignore` list of ROM extensions stays in force.

This is a development convenience, not a distribution plan: a release build
should trim the set to what the shipped cores need.
