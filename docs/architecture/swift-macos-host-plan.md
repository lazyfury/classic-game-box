# Swift/macOS host — remaining work

Status of the experiment on branch `feat/swift-macos-host`. The architecture is
settled and the vertical slice runs; this is the backlog to reach parity with
the `winit` host and to ship it.

Authority: [`../../macos/README.md`](../../macos/README.md),
[`quill-native-migration.md`](quill-native-migration.md).

## Where it is

Rust owns everything (`crates/cgb-mac`): the igui UI, the wgpu renderer and
`cgb-app` (library + libretro emulator). Swift owns the window only:
`NSWindow` + `CAMetalLayer` + AppKit event forwarding
(`macos/Sources/ClassicGameBoxMac`). `cgb-app` builds with
`--no-default-features`, so no `winit` is compiled on this path; the Rust host
is linked **statically** into the Swift binary.

Working: the library UI renders into Swift's layer; pointer (move/down/up/
double-click/drag/wheel/leave), keyboard (named + character keys, text,
modifiers) and IME preedit/commit are forwarded; the cursor and IME caret come
from the UI; clipboard works; files can be dropped; resizing is coalesced;
fullscreen is applied by Swift; gamepads use Swift's `GameController` (no
`gilrs` in the build); frames are event-driven with a `CADisplayLink` while the
app wants them; audio goes through Rust `cpal`.

Verified on hardware: PSP, N64 and PS1 (the offscreen-CGL path coexists with
Metal/wgpu), the downloadable-core flow, and the IME candidate position.

## Decisions (settled)

1. **Titlebar** — transparent + `fullSizeContentView` (the UI runs under it and
   reserves the traffic-light inset). Implemented.
2. **Link model** — static (`libcgb_mac.a` into the Swift binary). Implemented.
3. **Workspace** — `crates/cgb-mac` stays a workspace member.
4. **Host abstraction** — long-term; started as the `cgb-host` crate
   (`HostWindow` / `GamepadSource`), which `cgb-app` and the hosts share.


## A. Native-API parity (do first)

The `winit` host does a few things this host does not yet.

- **A1 · Cursor.** `igui_winit` calls `App::cursor()` after each frame and sets
  the winit cursor. Here the pointer stays an arrow, so cards/buttons never
  show the hand. Add `cgb_mac_cursor(app) -> u32` (an `igui_core::Cursor`
  discriminant) and have Swift map it to `NSCursor` and `set()` it (only when
  it changed).
- **A2 · IME caret.** `igui_winit`'s `ImePlugin` is a `FrameObserver` that reads
  `App::caret()` and calls `set_ime_cursor_area`, so the candidate window sits
  on the caret. Here `firstRect(forCharacterRange:)` returns `.zero`. Add
  `cgb_mac_caret(app, *x, *y, *w, *h) -> bool` (from `App::caret()`, in logical
  points), cache it in `HostView`, and return that rect from `firstRect`.
- **A3 · Clipboard.** No `Clipboard` service is registered, so Cmd+C/V does
  nothing in text fields. Register one on the Rust side backed by the system
  pasteboard (`arboard`, which `igui_winit` already uses, or `objc2-app-kit`'s
  `NSPasteboard`). The app's Cmd+C/V hotkeys and `insertText:` path already
  work once the service exists.
- **A4 · Drag & drop.** The FFI `cgb_mac_dropped_file` exists but Swift never
  calls it. In `HostView`: `registerForDraggedTypes([.fileURL])` and implement
  `draggingEntered` / `draggingUpdated` / `performDragOperation` to forward the
  URLs.
- **A5 · Key repeat.** `NSEvent.isARepeat` is dropped; forward it so held
  arrows/backspace can repeat if the UI wants that. Low priority. **Deferred** —
  key filtering may need per-emulator config; leave a note and revisit when a
  test hits it. (Repeat itself is already forwarded: `HostView.keyDown` does not
  filter `isARepeat`.)
- **A6 · Scroll sign.** Verify wheel/trackpad direction against `winit`'s
  `wheel_pixels` convention (`y > 0` scrolls down); tune the line→point factor.
  **Leave as is** (implemented, not to be changed).
- **A7 · Titlebar / safe area.** **Done** — transparent title bar +
  `.fullSizeContentView`, matching `safe_area()` in `cgb-app`.

## B. Frame pacing

**Status:** implemented — the always-on timer is gone; a local `NSEvent`
monitor schedules a frame for any input, and `tick` reschedules at 60 Hz only
while `cgb_mac_needs_frame` is true.

- **B1 · Event-driven ticking.** Today the timer presents at 60 Hz
  unconditionally. Use `cgb_mac_needs_frame`: keep a fast timer only while it
  is true (a running game / animation / download), otherwise present once after
  each input event and on resize, and stop ticking when idle.
- **B2 · Repaint after input.** When idle, an event must trigger exactly one
  `cgb_mac_frame`.
- **B3 · vsync.** **Done** — `NSView.displayLink` (`CADisplayLink`, macOS 14+)
  drives frames while `cgb_mac_needs_frame` is true; input still schedules a
  one-shot frame. The core's frame rate is not the display rate; `cgb-app`
  accumulates `dt`.

## C. Packaging

**Status:** implemented — `macos/scripts/package.sh` builds a release Rust
dylib + Swift binary and assembles `dist/Classic Game Box (Swift).app` with the
dylib under `Contents/Frameworks` (`@rpath`) and cores/assets under
`Contents/Resources`. B3 (CVDisplayLink) is still open.

- **C1 · Link model.** **Done** — static: `crates/cgb-mac` builds only a
  `staticlib`, SwiftPM links `libcgb_mac.a`, no dylib is bundled.
- **C2 · `macos/scripts/package.sh`.** Release build → `Classic Game Box.app`
  with `Info.plist`, the Rust lib (if dynamic), and `cores/dist` + `assets`
  under `Contents/Resources`. Ad-hoc codesign.
- **C3 · Resource resolution.** `cgb-app::resource_dir()` already looks in
  `Contents/Resources`; confirm `cores/cores.json`, the minimal core set and
  the arcade/J2ME/PPSSPP assets resolve from the bundle.
- **C4 · Downloadable cores.** **Verified** — the settings page's download flow
  works through this host.

## D. Tests & docs

- **D1 · Unit tests.** `GamepadSnapshot::apply` is tested. `crates/cgb-mac/src/input.rs`
  is pure mapping — still to add tests for `key_from_code`, `modifiers_from_bits`,
  `pointer_button` and `MacEvent::to_input` (double-click, wheel).
- **D2 · Manual checklist.** A short visual acceptance list (library renders,
  click opens a game, keys play, resize, fullscreen, drop a ROM, save/load).
- **D3 · Conventions.** Add `macos/` and `crates/cgb-mac` to `AGENTS.md`'s layout
  and to `.pi/skills/cgb-rust/SKILL.md`; document the `winit-host` feature and
  the `cgb-app --no-default-features` rule.
- **D4 · README.** Mention the Swift host as the experimental alternative front
  end.

## E. Hardware-GL cores

- **E1.** **Verified** — PSP, N64 and PS1 all render through `cgb-libretro`'s
  offscreen CGL context on the main thread, shared with Metal/wgpu.

## F. Robustness

- **F1 · Surface lifetime.** **Done** — `applicationWillTerminate` destroys the
  Rust app while `view` (and its layer) is still alive; `view` is released only
  after.
- **F2 · Startup errors.** **Done** — a missing backend returns NULL from
  `cgb_mac_start` and Swift shows an `NSAlert`.
- **F3 · Event coalescing.** **Done** — `applyResizeIfNeeded` reconfigures the
  surface at most once a frame.

## G. Swift-native gamepad (decided interface)

**Status:** implemented — `cgb-input::GamepadSnapshot`, `cgb-app`'s
`GamepadSource`/`SharedGamepad`, `MacGamepadPlugin` + `cgb_mac_gamepad_*`, and
Swift `Gamepads.swift`. `gilrs` is off the embedded build.

**Decision.** On this host the gamepad source is Swift, using Apple's
`GameController` framework (`GCController`), not `gilrs`. Apple's own mapping
is correct for the Xbox Wireless Controller over Bluetooth, which `gilrs`
mislabels here; hot-plug and per-controller profiles also come for free. The
embedded build must not construct `gilrs` at all.

The seam mirrors the window: `cgb-app` takes gamepad state from a host-provided
source when one is registered, and falls back to `gilrs` otherwise.

### Rust

- **G1 · `cgb-input`: a snapshot type.** Add

  ```rust
  pub struct GamepadSnapshot {
      pub buttons: [u16; 2],           // per port, libretro joypad bitmask
      pub analog: [[[i16; 2]; 2]; 2],  // [port][stick][axis]
      pub connected: [bool; 2],
  }
  ```

  with `set_button(port, JoypadButton, down)`, `set_analog(port, stick, axis,
  i16)`, `connect(port, bool)` and `apply(&self, &mut InputState)` (sets the
  gamepad mask and the sticks, leaving the keyboard source untouched). Add
  `InputState::set_gamepad_mask(port, u16)` for the mask write.
- **G2 · `cgb-app`: a gamepad source.** Add

  ```rust
  pub trait GamepadSource { fn poll(&mut self, state: &mut InputState); }
  pub type SharedGamepad = Rc<RefCell<dyn GamepadSource>>;
  ```

  `App.gamepads` becomes `Option<Rc<RefCell<dyn GamepadSource>>>`. In
  `App::init`, prefer `ctx.service::<SharedGamepad>()`; else (with the `gilrs`
  feature) wrap `Gamepads`. Move `Gamepads::new()` out of `App::new` into
  `init`. `step_gamepad` calls `poll` as today.
- **G3 · gate `gilrs`.** Make `gilrs` optional in `cgb-input` (feature
  `gilrs`), enabled by `cgb-app`'s `winit-host`. The embedded build then links
  no `gilrs`.

### C ABI (`cgb_mac.h`)

```c
/* libretro joypad ids, so Swift never hardcodes them. */
enum { CGB_JOYPAD_B = 0, CGB_JOYPAD_Y = 1, CGB_JOYPAD_SELECT = 2,
       CGB_JOYPAD_START = 3, CGB_JOYPAD_UP = 4, CGB_JOYPAD_DOWN = 5,
       CGB_JOYPAD_LEFT = 6, CGB_JOYPAD_RIGHT = 7, CGB_JOYPAD_A = 8,
       CGB_JOYPAD_X = 9, CGB_JOYPAD_L = 10, CGB_JOYPAD_R = 11,
       CGB_JOYPAD_L2 = 12, CGB_JOYPAD_R2 = 13, CGB_JOYPAD_L3 = 14,
       CGB_JOYPAD_R3 = 15 };

/* Replace one port's snapshot. buttons: bit i = CGB_JOYPAD_* i.
 * Axes are -32768..32767, libretro convention (Y positive is down). */
void cgb_mac_gamepad_state(CgbMacApp *app, uint32_t port, uint32_t buttons,
                           int16_t left_x, int16_t left_y,
                           int16_t right_x, int16_t right_y);

/* Mark a port connected/disconnected; disconnect clears it. */
void cgb_mac_gamepad_connected(CgbMacApp *app, uint32_t port, bool connected);
```

`cgb_mac_gamepad_state` writes into the same `Rc<RefCell<GamepadSnapshot>>`
the source applies; `connected` sets/clears the flag and zeroes that port.

### Swift (`Gamepads.swift`)

- `GCController.startWirelessControllerDiscovery(...)` at start; observe
  `.GCControllerDidConnect` / `.GCControllerDidDisconnect`.
- Assign controllers to ports in connection order (first two → 0/1); set
  `controller.playerIndex`. On disconnect call
  `cgb_mac_gamepad_connected(port, false)` and free the port.
- Register `extendedGamepad.valueChangedHandler` (falling back to `gamepad` /
  `microGamepad`); rebuild the bitmask + axes and call
  `cgb_mac_gamepad_state(port, ...)` on every change.

### Mapping (`GCController` → libretro)

| `GCController` | libretro |
|---|---|
| `buttonA` | `B` (bottom / confirm) |
| `buttonB` | `A` (right) |
| `buttonX` | `Y` (left) |
| `buttonY` | `X` (top) |
| `leftShoulder` / `rightShoulder` | `L` / `R` |
| `leftTrigger` / `rightTrigger` (> 0.5) | `L2` / `R2` |
| `leftThumbstickButton` / `rightThumbstickButton` | `L3` / `R3` |
| `dpad.up` / `.down` / `.left` / `.right` | `UP` / `DOWN` / `LEFT` / `RIGHT` |
| `buttonMenu` | `START` |
| `buttonOptions` | `SELECT` |
| `leftThumbstick` | analog stick 0 (`x`, `y`) |
| `rightThumbstick` | analog stick 1 (`x`, `y`) |

- **Stick → D-pad parity.** As `gilrs` does, when a stick axis passes ±0.5
  also set the matching D-pad bit, so a core that reads only buttons still
  moves.
- **Axis sign.** `GCController` Y is positive up; libretro wants positive down,
  so negate Y.
- **Two-port cap.** Controllers beyond the first two are ignored (two joypad
  ports).

### Testing

- **G4.** Unit-test `GamepadSnapshot::apply` and the mask write in `cgb-input`.
- **G5.** **Verified** — hot-plug, buttons / sticks / triggers, and the port
  clears on unplug; `gilrs` is absent from the embedded binary.
  **Known quirk:** double-pressing the pad's Select (`buttonOptions`) can trip
  macOS's screen-recording shortcut; not remapped yet.

## H. Experimental: a game in its own window

**Goal.** Run a game in a **separate `NSWindow`** instead of the play column
inside the library window, plus a **separate-window OpenGL mode** that shows
the core's own GL output directly.

**Why.** The library shell is heavy; a dedicated game window is the natural
"player" surface (and a path to a borderless / fullscreen game window that is
independent of the UI). Hardware cores (N64/PSP/PS1) already render into an
offscreen CGL FBO; an OpenGL mode could present that FBO directly instead of
the readback → wgpu-texture path.

**Design sketch.**

- The runtime has one `Presenter`; a second window needs a second surface. Two
  shapes:
  1. **Companion runtime for the game** — a second `cgb-mac` instance that owns
     the game surface and drives `Session`, while the main instance keeps the
     UI. Needs the app state split (UI vs session).
  2. **Multi-surface host** — extend `cgb-mac` to register a second
     `CAMetalLayer` + presenter for the game and have `cgb-app` render the play
     view to it. One app instance; more host plumbing.
- Swift creates the second window/layer and calls
  `cgb_mac_open_game_window(layer, w, h, scale)` / `cgb_mac_close_game_window()`.
- **OpenGL mode**: `cgb-libretro` already owns an offscreen CGL context for
  `SET_HW_RENDER`. A dedicated window could either keep the readback path but
  present to the second surface (cheap to try), or create the GL context on the
  game window's `NSOpenGLView` / `CAOpenGLLayer` and let the core render there
  (no readback, no wgpu) — the real "OpenGL mode".

**Open questions.**

- Which shape (companion runtime vs multi-surface)?
- Does OpenGL mode bypass `igui`/wgpu for the game view only, keeping the UI in
  wgpu?
- Window chrome for the game window (borderless? title bar? controls overlay?).

**Status:** planned, not started.

## Open questions

All four are now settled (see **Decisions** above).
