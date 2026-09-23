# assets

Icons and other bundled, non-code resources.

The native cores are *not* here — they are built into `cores/dist/` and shipped
with the app bundle (see `cores/README.md`). Nothing in this folder is read by
the Rust crates at compile time.

## `roms/<system>/system/`

The arcade core needs a BIOS. Only the two the app targets are bundled —
`neogeo.zip` (Neo Geo) and `pgm.zip` (IGS PGM) — because shipping the whole
MAME/FBNeo BIOS collection added ~44 MB to the bundle for boards the app does
not aim at. They live in `roms/arcade/system/` and are committed on purpose so
the app works out of the box.

They are **not** read in place: at startup `cgb-app` seeds the writable
`<app data>/system` directory the core is actually pointed at
(`GET_SYSTEM_DIRECTORY`) with any file it is missing, so a BIOS the player
drops there always wins. To play another arcade board, drop its BIOS `.zip`
into that writable directory; it is picked up without a rebuild. Game ROMs and
saves are still never committed; the `.gitignore` list of ROM extensions stays
in force.
