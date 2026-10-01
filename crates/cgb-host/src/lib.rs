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

use cgb_input::InputState;

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
/// `cgb_input::GamepadSnapshot`.
pub trait GamepadSource {
    /// Merge the current gamepad state into `state`.
    fn poll(&mut self, state: &mut InputState);
}

/// The gamepad source a platform plugin publishes as the `SharedGamepad`
/// service.
pub type SharedGamepad = Rc<RefCell<dyn GamepadSource>>;
