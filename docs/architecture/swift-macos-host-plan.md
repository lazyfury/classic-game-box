# Swift/macOS host — remaining work

Status of the experiment on branch `feat/swift-macos-host`. The architecture is
settled and the vertical slice runs; this is the backlog to reach parity with
the `winit` host and to ship it.

Authority: [`../../macos/README.md`](../../macos/README.md),
[`quill-native-migration.md`](quill-native-migration.md).

## Where it is

Rust owns everything (`macos/rust`, crate `cgb-mac`): the igui UI, the wgpu
renderer and `cgb-app` (library + libretro emulator). Swift owns the window
only: `NSWindow` + `CAMetalLayer` + a 60 Hz tick + AppKit event forwarding
(`macos/Sources/ClassicGameBoxMac`). `cgb-app` builds with
`--no-default-features`, so no `winit` is compiled on this path.

Working: library UI renders into Swift's layer; pointer (move/down/up/double-
click/drag/wheel/leave), keyboard (named + character keys, text, modifiers),
basic IME preedit/commit, dropped files (FFI only), resize, fullscreen
(polled), gamepad (currently `gilrs` inside `cgb-app` — moving to Swift, see
§G), audio (Rust `cpal`).

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
  arrows/backspace can repeat if the UI wants that. Low priority.
- **A6 · Scroll sign.** Verify wheel/trackpad direction against `winit`'s
  `wheel_pixels` convention (`y > 0` scrolls down); tune the line→point factor.
- **A7 · Titlebar / safe area.** The app reserves space for the macOS chrome
  (`safe_area()` in `cgb-app`). Decide the Swift window chrome — the latest
  commit set the `winit` path to `TitlebarMode::Native`; match it here
  (`titlebarAppearsTransparent`, `fullSizeContentView`, or a normal titlebar)
  and confirm the header clears the traffic lights.

## B. Frame pacing

- **B1 · Event-driven ticking.** Today the timer presents at 60 Hz
  unconditionally. Use `cgb_mac_needs_frame`: keep a fast timer only while it
  is true (a running game / animation / download), otherwise present once after
  each input event and on resize, and stop ticking when idle.
- **B2 · Repaint after input.** When idle, an event must trigger exactly one
  `cgb_mac_frame`.
- **B3 · vsync (optional).** Replace the `Timer` with a `CVDisplayLink` (or
  `CADisplayLink`) for steadier pacing. Note the core's frame rate is not the
  display rate; `cgb-app` already accumulates `dt`.

## C. Packaging

- **C1 · Link model.** Either statically link `libcgb_mac.a` (the crate already
  builds a `staticlib`) so no dylib needs bundling, or bundle
  `libcgb_mac.dylib` into `Contents/Frameworks` with an `@rpath` and an
  `install_name`. Static is simpler for an app that is one process.
- **C2 · `macos/scripts/package.sh`.** Release build → `Classic Game Box.app`
  with `Info.plist`, the Rust lib (if dynamic), and `cores/dist` + `assets`
  under `Contents/Resources`. Ad-hoc codesign.
- **C3 · Resource resolution.** `cgb-app::resource_dir()` already looks in
  `Contents/Resources`; confirm `cores/cores.json`, the minimal core set and
  the arcade/J2ME/PPSSPP assets resolve from the bundle.
- **C4 · Downloadable cores.** Downloaded cores land in app data, as in the
  main app; verify the settings page's download flow works through this host.

## D. Tests & docs

- **D1 · Unit tests.** `macos/rust/src/input.rs` is pure mapping — add tests for
  `key_from_code`, `modifiers_from_bits`, `pointer_button` and
  `MacEvent::to_input` (double-click, wheel).
- **D2 · Manual checklist.** A short visual acceptance list (library renders,
  click opens a game, keys play, resize, fullscreen, drop a ROM, save/load).
- **D3 · Conventions.** Add `macos/` and `macos/rust` to `AGENTS.md`'s layout
  and to `.pi/skills/cgb-rust/SKILL.md`; document the `winit-host` feature and
  the `cgb-app --no-default-features` rule.
- **D4 · README.** Mention the Swift host as the experimental alternative front
  end.

## E. Hardware-GL cores

- **E1.** N64 / PSP / PS1 render through `cgb-libretro`'s offscreen CGL context
  on the thread running `retro_run`. Here that is the main thread, shared with
  Metal/wgpu. Verify one of them end to end (and that `CoreHost::drop` ordering
  still holds).

## F. Robustness

- **F1 · Surface lifetime.** The `CAMetalLayer` must outlive the wgpu surface.
  Swift keeps `HostView` alive until `applicationWillTerminate` destroys the
  Rust app; make this explicit and assert it (destroy the app before releasing
  the view).
- **F2 · Startup errors.** `cgb_mac_start` returns null only on a null layer;
  surface/backend failures are only `eprintln!` today. Surface them to Swift
  (an `NSAlert`) so a GPU failure is visible.
- **F3 · Event coalescing.** A live resize fires many `cgb_mac_resize` calls;
  coalesce to one surface reconfigure per tick if it shows up as jank.

## G. Swift-native gamepad (decided interface)

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
- **G5.** Manual: hot-plug an Xbox pad, check buttons / sticks / triggers;
  unplug and confirm the port clears; confirm `gilrs` is absent from the
  release binary (`nm` / `otool`).

## Open questions

1. **Titlebar** — transparent + full-size content (matching the app's
   traffic-light inset), or a plain native titlebar?
2. **Link model** — static into the Swift binary, or a bundled dylib?
3. **Workspace** — keep `macos/rust` a workspace member (the whole-workspace
   build compiles it), or exclude it and build it only from
   `macos/scripts/*.sh`?
4. **`cgb-app` surface** — is the `HostWindow` trait + `winit-host` feature the
   right permanent shape, or should the host abstraction move to its own crate?
