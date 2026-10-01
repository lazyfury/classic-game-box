//! The native-window contract the app runs against.
//!
//! `cgb-app` never names a windowing library: what it needs from the window
//! (fullscreen today, redraw later) goes through [`HostWindow`]. The default
//! `winit` host adapts a [`winit::window::Window`]; the embedded Swift/macOS
//! host implements it over AppKit, so `cgb-app` can build with
//! `--no-default-features` and no `winit` at all.

use std::cell::RefCell;
use std::rc::Rc;

#[cfg(feature = "gilrs")]
use cgb_input::Gamepads;
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
/// The default source is `gilrs` ([`GilrsGamepads`]); a host that owns its own
/// gamepad API (the Swift `GameController` host) publishes its own through the
/// `SharedGamepad` service instead.
pub trait GamepadSource {
    /// Merge the current gamepad state into `state`.
    fn poll(&mut self, state: &mut InputState);
}

/// The gamepad source a platform plugin publishes as the `SharedGamepad`
/// service.
pub type SharedGamepad = Rc<RefCell<dyn GamepadSource>>;

/// The default `gilrs` gamepad source.
#[cfg(feature = "gilrs")]
pub(crate) struct GilrsGamepads(pub(crate) Gamepads);

#[cfg(feature = "gilrs")]
impl GamepadSource for GilrsGamepads {
    fn poll(&mut self, state: &mut InputState) {
        self.0.poll(state);
    }
}

/// Adapts a `winit` window to [`HostWindow`] (the default host).
#[cfg(feature = "winit-host")]
pub(crate) struct WinitWindow(pub(crate) std::sync::Arc<winit::window::Window>);

#[cfg(feature = "winit-host")]
impl HostWindow for WinitWindow {
    fn set_fullscreen(&self, on: bool) {
        self.0.set_fullscreen(if on {
            Some(winit::window::Fullscreen::Borderless(None))
        } else {
            None
        });
    }
}
