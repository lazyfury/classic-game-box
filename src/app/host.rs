//! The native-window contract the app runs against.
//!
//! `cgb-app` never names a windowing library: what it needs from the window
//! (fullscreen today, redraw later) goes through [`HostWindow`]. The default
//! `winit` host adapts a [`winit::window::Window`]; the embedded Swift/macOS
//! host implements it over AppKit, so `cgb-app` can build with
//! `--no-default-features` and no `winit` at all.

use std::rc::Rc;

/// What the app asks of the native window.
pub trait HostWindow {
    /// Enter or leave borderless fullscreen.
    fn set_fullscreen(&self, on: bool);
}

/// The host window a platform plugin publishes as the `SharedHostWindow`
/// service.
pub type SharedHostWindow = Rc<dyn HostWindow>;

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
