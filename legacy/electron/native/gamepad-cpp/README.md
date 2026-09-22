# fc-gamepad (C++)

A small helper that reads real game controllers and writes what it finds, one
JSON object per line, to standard output. This is the **Windows** build of the
helper; macOS builds the Swift one in `../gamepad` instead. Both speak the same
protocol, byte for byte, so the main process and the renderer cannot tell which
one is on the other end of the pipe.

```
pnpm run build:native                 # from electron/, builds native/bin/fc-gamepad.exe
node scripts/build-native.mjs --cpp   # force this build on any platform
```

On macOS, `--cpp` compiles and tests the portable half but does **not**
install it over the Swift helper; the platform's own reader keeps working.

The binary is what the Electron main process spawns. It is not needed to run
the emulator or the tests.

## Why a second helper

The macOS helper exists because the browser's Gamepad API makes Chromium hold a
HID connection that stops the application quitting (the whole story is in
`../gamepad/README.md`). Windows does not have that problem in the same way,
but it does need a reader, and the reader should not be the page:

- **One protocol, one place to test it.** `src/main/gamepad.ts` parses JSON
  lines and `test/gamepad.test.mjs` covers the parser. A Windows-specific
  browser path would be a second mapping to keep in step.
- **The page should not know about platforms.** The renderer's
  `NativeGamepadSource` already consumes the helper's reading; nothing above
  the helper changes for Windows except the file name.

So Windows gets a helper of its own, in C++, using the system's XInput API.
Nothing has to be installed with it: `xinput1_4.dll` (or `xinput1_3.dll` on
older systems) ships with Windows.

## The protocol

One JSON object per line on **stdout**, flushed per line. Two message kinds.

### `hello`

```json
{"pid":84213,"type":"hello","version":2}
```

Once, at startup. Its arrival is the only proof the helper is alive, which is
worth having when a pad "does not work".

### `pads` — the whole list, whenever it changes

```json
{"type":"pads","pads":[
  {"index":0,"id":"Xbox Controller",
   "buttons":{"A":false,"B":true,"SELECT":false,"START":false,
              "UP":false,"DOWN":false,"LEFT":false,"RIGHT":false}}]}
```

On connect, and then **only when something changes**. The helper polls at 60Hz,
but a steady stream of "nothing changed" would be noise for the reader to
parse, so it is filtered here. The reader (`src/main/gamepad.ts`) filters
against the last list once more, so the renderer hears each change exactly
once.

`index` is a **slot**, not a device: it is what the settings screen offers as
"player 1" or "player 2", and it is the only name a pad has, because two
identical controllers report the same `id`. A pad keeps its slot while it stays
connected, and the lowest free slot is reused after it goes.

## The mapping

The console's mapping, and the same one the Swift helper and the renderer's
browser `GamepadSource` apply, so a pad behaves the same on any path:

| NES switch | XInput input |
|---|---|
| A | `XINPUT_GAMEPAD_A` (the bottom face button, where A is printed) |
| B | `XINPUT_GAMEPAD_B` (the right face button) |
| Start | `XINPUT_GAMEPAD_START` |
| Select | `XINPUT_GAMEPAD_BACK` |
| D-pad | `XINPUT_GAMEPAD_DPAD_*`, with the left stick as a duplicate |

The stick duplicates the d-pad because most people reach for the stick. The
deadzone is `0.5`, because a worn stick drifts and a drifting stick walks the
player into a wall. XInput's Y axis is +1 up and -1 down, which matches the
GameController framework and is the opposite of the browser's Gamepad API; the
sign is handled in `backend_windows.cpp` and nowhere else.

XInput is read every poll for connected slots and every half second for empty
ones, because `XInputGetState` on an empty slot is documented to be slow.

### What XInput does not cover

A DualShock or DualSense in its native mode speaks HID, not XInput; on Windows
it may show up as a nameless device with no input. `Windows.Gaming.Input`
would cover those (and Switch Pro controllers), but it is WinRT and several
times the surface of this helper. The honest position is a note rather than a
guess; pads with an XInput mode, which is most of them, work today.

## stdin

stdin is a watchdog, not input. The parent keeps it open for as long as it
wants the helper to live; when the parent exits the pipe closes, the read
returns EOF, and this exits. That is what stops a killed Electron from leaving
an orphan holding a controller — the exact failure the macOS helper was written
to avoid, and one worth avoiding on Windows too.

## Building

Requires CMake and a C++20 compiler. On Windows, Visual Studio's build tools
are enough; the CMake generator is chosen by CMake.

```bash
cmake -S native/gamepad-cpp -B native/gamepad-cpp/build
cmake --build native/gamepad-cpp/build --config Release
```

The output is `native/gamepad-cpp/build/out/fc-gamepad.exe`, which
`scripts/build-native.mjs` copies to `native/bin/fc-gamepad.exe` — the fixed
path the main process spawns.

On macOS and Linux the same build produces a helper with the empty backend in
`backend_stub.cpp`. That build cannot read a pad; it exists so the portable
half (the protocol, the slot bookkeeping, the change detection) can be compiled
and tested anywhere.

## Testing

The reporter and the protocol are the parts with decisions in them and they
have a test that needs no pad and no Windows:

```bash
pnpm run test:native
# or, by hand:
cmake -S native/gamepad-cpp -B native/gamepad-cpp/build -DCMAKE_BUILD_TYPE=Release
cmake --build native/gamepad-cpp/build --config Release
ctest --test-dir native/gamepad-cpp/build -C Release --output-on-failure
```

Reading a real pad is what a pad on the desk is for. On Windows:

```powershell
# run for two seconds, press a button, watch a line appear
& { Start-Sleep -Seconds 2 } | native\bin\fc-gamepad.exe
```

If nothing appears, check that the controller is on and recognised by
**Settings → Bluetooth & devices → Controllers** (`joy.cpl`), and that it
appears in the Game Controllers test screen. XInput pads appear there; a pad
that only enumerates as a HID device may not.
