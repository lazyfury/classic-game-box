# fc-gamepad

A small macOS helper that reads real game controllers through Apple's
**GameController** framework and writes what it finds, one JSON object per
line, to standard output. It exists because of a specific bug in the Electron
front end; the short version is at the bottom of this file.

> Windows does not use this helper. It builds `../gamepad-cpp`, a C++ program
> that reads XInput and writes the same protocol; see that README for the
> mapping and the platform's caveats. The two exist side by side because the
> GameController framework is the right reader on macOS and XInput is the
> right one on Windows, and the pipe between helper and application hides the
> difference.

```
pnpm run build:native        # from electron/, builds native/bin/fc-gamepad
```

The binary is what the Electron main process spawns. It is not needed to run
the emulator, the tests, or the native build.

## Why it exists

The front end used to read pads through the browser's Gamepad API. That API
works — but on macOS, a page that so much as adds a `gamepadconnected`
listener starts Chromium's own gamepad service in the browser process, and
that service holds a HID connection which makes the application impossible to
shut down. `app.quit()`, `app.exit()` and `process.exit()` all leave the
process in an uninterruptible wait (`ps` state `U`) until somebody sends
`SIGKILL`.

A separate process has none of that problem: it can be killed. So the reading
moved out of Chromium, into this program, and the main process talks to it over
a pipe. Chromium never touches HID, and closing the window works.

## The protocol

One JSON object per line on **stdout**, flushed per line. Two message kinds.

### `hello`

```json
{"type":"hello","version":2,"pid":84213}
```

Once, at startup. Its arrival is the only proof the helper is alive, which is
worth having when a pad "does not work".

### `pads` — the whole list, whenever it changes

```json
{"type":"pads","pads":[
  {"index":0,"id":"Xbox Wireless Controller Xbox One",
   "buttons":{"A":false,"B":true,"SELECT":false,"START":false,
              "UP":false,"DOWN":false,"LEFT":false,"RIGHT":false}},
  {"index":1,"id":"DualSense Wireless Controller","buttons":{...}}]}
```

On connect, and then **only when something changes**. The helper polls at 60Hz,
but a steady stream of "nothing changed" would be noise for the reader to
parse, so it is filtered here. The reader (`src/main/gamepad.ts`) filters
against the last list once more, so the renderer hears each change exactly
once.

The whole list is sent rather than one pad at a time, because the list is what
the renderer draws and what a disconnection is measured against: "pad 1 is
gone" is only meaningful next to "pads 0 and 2 are still here". A pad that
disappears has to be reported, or its buttons stay held forever -- there is
nobody left to release them.

`index` is a **slot**, not a device: it is what the settings screen offers as
"player 1" or "player 2", and it is the only name a pad has, because two
identical controllers report the same `id`. A pad keeps its slot while it stays
connected, and the lowest free slot is reused after it goes.

## The mapping

The console's mapping, and the same one the renderer's browser `GamepadSource`
applies, so a pad behaves the same on either path:

| NES switch | GameController input |
|---|---|
| A | `buttonA` (the right-hand face button, where the letter A is printed) |
| B | `buttonB` |
| Start | `buttonMenu` |
| Select | `buttonOptions` (micro pads have none) |
| D-pad | `dpad`, with the left stick as a duplicate |

The stick duplicates the d-pad because most people reach for the stick. The
deadzone is `0.5`, because a worn stick drifts and a drifting stick walks the
player into a wall. The stick's Y axis is +1 up and -1 down, the opposite of
the browser's Gamepad API; the sign flip is in `main.swift` and nowhere else.

## stdin

stdin is a watchdog, not input. The parent keeps it open for as long as it
wants the helper to live; when the parent exits the pipe closes, the read
returns nil, and this exits. That is what stops a killed Electron from leaving
an orphan holding the HID connection — which is the exact failure this whole
program was written to avoid.

## Reading it yourself

```bash
# prints "hello" and one "pads" line, then exits when the 3 seconds are up
(sleep 3) | native/bin/fc-gamepad
```

Press a button while it runs and a line appears. If nothing appears, the pad is
not paired, or macOS is withholding it: **System Settings → Privacy &
Security → Input Monitoring**, tick Electron (or this application), then quit
and start it again — macOS reads that list at launch.
