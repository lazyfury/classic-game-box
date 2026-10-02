//! The host contract between the app and whatever owns the window and devices.
//!
//! `cgb-app` never names a windowing library or a gamepad backend: the window
//! is a [`HostWindow`], the gamepad a [`GamepadSource`]. A platform host — the
//! embedded Swift host (`cgb-mac`) publishes its implementations as services.
//!
//! Keeping the traits here (rather than in `cgb-app`) is what lets a host
//! depend on them without depending on the whole app.

use std::cell::RefCell;
use std::rc::Rc;

use cgb_libretro::InputState;

/// What the app asks of the native window.
pub trait HostWindow {
    /// Enter or leave borderless fullscreen.
    fn set_fullscreen(&self, on: bool);
}

/// The host window a platform plugin publishes as the `SharedHostWindow`
/// service.
pub type SharedHostWindow = Rc<dyn HostWindow>;

/// A source of gamepad state, polled once per frame.
///
/// The macOS host uses Swift's `GameController` and writes a
/// `cgb_libretro::GamepadSnapshot`. The default backend is `gilrs`
/// (`src/app/gamepads.rs`).
pub trait GamepadSource {
    /// Merge the current gamepad state into `state`.
    fn poll(&mut self, state: &mut InputState);

    /// The connected pads and the port each currently drives, for the input
    /// assignment UI. A source that cannot enumerate its devices returns an
    /// empty list (the macOS `GameController` path).
    fn devices(&self) -> Vec<GamepadDevice> {
        Vec::new()
    }

    /// Move the pad with `id` to `port`, or off every port when `port` is
    /// `None`.
    fn assign(&mut self, _id: &str, _port: Option<usize>) {}

    /// Arm the port a pad can claim by pressing its Start button ("press to
    /// claim"), or `None` to stop waiting. The source clears the armed port
    /// once a pad claims it.
    fn claim(&mut self, _port: Option<usize>) {}

    /// Take a pending reset request: the **main** controller (the pad on port
    /// `0`) held Select for the long-press interval. Cleared once read, so the
    /// host acts on each press once.
    fn take_reset_request(&mut self) -> bool {
        false
    }
}

/// One connected gamepad, as the assignment UI sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GamepadDevice {
    /// An id stable for the session (the source chooses its format).
    pub id: String,
    /// A human label (`"Xbox Wireless Controller"`).
    pub name: String,
    /// The port this pad currently drives, if any.
    pub port: Option<usize>,
}

/// The gamepad source a platform plugin publishes as the `SharedGamepad`
/// service.
pub type SharedGamepad = Rc<RefCell<dyn GamepadSource>>;
