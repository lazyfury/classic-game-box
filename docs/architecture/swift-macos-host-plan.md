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
(polled), gamepad (via `gilrs` inside `cgb-app`), audio (Rust `cpal`).

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

## Open questions

1. **Titlebar** — transparent + full-size content (matching the app's
   traffic-light inset), or a plain native titlebar?
2. **Link model** — static into the Swift binary, or a bundled dylib?
3. **Workspace** — keep `macos/rust` a workspace member (the whole-workspace
   build compiles it), or exclude it and build it only from
   `macos/scripts/*.sh`?
4. **`cgb-app` surface** — is the `HostWindow` trait + `winit-host` feature the
   right permanent shape, or should the host abstraction move to its own crate?
