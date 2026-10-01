//! The bundled `winit` / `gilrs` host adapters.
//!
//! The host traits themselves live in `cgb-host`; this module adapts the
//! bundled `winit` window and `gilrs` gamepads to them for the default host.

#[cfg(feature = "gilrs")]
use cgb_host::GamepadSource;
#[cfg(feature = "winit-host")]
use cgb_host::HostWindow;
#[cfg(feature = "gilrs")]
use cgb_input::Gamepads;
#[cfg(feature = "gilrs")]
use cgb_input::InputState;

/// Adapts a `winit` window to [`HostWindow`].
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

/// The default `gilrs` gamepad source.
#[cfg(feature = "gilrs")]
pub(crate) struct GilrsGamepads(pub(crate) Gamepads);

#[cfg(feature = "gilrs")]
impl GamepadSource for GilrsGamepads {
    fn poll(&mut self, state: &mut InputState) {
        self.0.poll(state);
    }
}
