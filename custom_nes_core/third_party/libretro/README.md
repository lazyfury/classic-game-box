# libretro-common / libretro.h

The official libretro API header, vendored so the build does not depend on
whatever a machine happens to have installed.

| | |
|---|---|
| source | `https://github.com/libretro/libretro-common/blob/master/include/libretro.h` |
| fetched | 2026-09-15, via `https://cdn.jsdelivr.net/gh/libretro/libretro-common@master/include/libretro.h` |
| lines | 8716 |
| sha256 | `362f210f9a15f155fcaf3b2aa4d519e2f6fbe8f83a71e31701a6ff475dbf9ef8` |

## Why it is committed

It is the contract between a core and a front end. Pinning a copy means the
ABI this project builds against is a fact in the repository rather than a
property of the machine that compiled it, and it means a fresh clone builds
with no network access.

## Updating it

Re-fetch the same URL, verify `RETRO_API_VERSION` is still `1` (it is the
incompatible-change counter, and a bump is a reason to read this file again
before updating), replace this file, and update the two lines above. Nothing
in `src/libretro/` needs to change for a header update that only adds
symbols.
