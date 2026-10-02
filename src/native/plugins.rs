//! The non-graphics host plugins: the fullscreen request, the gamepad source,
//! the clipboard and the text measurer.
//!
//! All four are platform-neutral — they only depend on `cgb-libretro` types,
//! `igui` services and `arboard` — so both shells use this one implementation.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::app::SharedBackend;
use crate::host::{GamepadDevice, GamepadSource, HostWindow};
use cgb_libretro::InputState;
use igui::igui_app::{App, AppBuilder, LifecycleObserver, Plugin};
use igui::igui_backend_wgpu::FontMetrics;
use igui::igui_core::FontWeight;
use igui::igui_ui::{Clipboard, MemoryClipboard, TextMeasurer};

/// The host window the app asks for fullscreen.
///
/// The app calls [`HostWindow::set_fullscreen`]; the request is parked here and
/// the shell polls it with `cgb_host_take_fullscreen` (so the shell owns its
/// platform's fullscreen transition and no callback crosses the ABI).
#[derive(Clone, Default)]
pub struct NativeHostWindow {
    request: Rc<Cell<Option<bool>>>,
}

impl NativeHostWindow {
    /// Take the pending request, if any.
    pub fn take_request(&self) -> Option<bool> {
        self.request.take()
    }
}

impl HostWindow for NativeHostWindow {
    fn set_fullscreen(&self, on: bool) {
        self.request.set(Some(on));
    }
}

/// One device the shell reports, in the shell's connection order.
#[derive(Clone, Default)]
pub struct DeviceState {
    pub name: String,
    pub connected: bool,
    pub buttons: u16,
    /// `[stick][axis]`, libretro's convention (Y positive down).
    pub analog: [[i16; 2]; 2],
}

/// The device list the shell fills through the C ABI; shared with the source
/// that reads it once per frame.
pub type SharedDevices = Rc<RefCell<Vec<DeviceState>>>;

/// The shell's gamepad source: it holds only raw per-device state, and Rust
/// maps device slots to libretro ports, so one assignment UI works for every
/// backend.
struct NativeGamepad {
    devices: SharedDevices,
    /// Device slot → the port it drives.
    ports: HashMap<usize, usize>,
}

impl GamepadSource for NativeGamepad {
    fn poll(&mut self, state: &mut InputState) {
        let devices = self.devices.borrow();
        for (slot, device) in devices.iter().enumerate() {
            if !device.connected {
                continue;
            }
            let Some(&port) = self.ports.get(&slot) else {
                continue;
            };
            state.set_gamepad_mask(port, device.buttons);
            for (stick, axes) in device.analog.iter().enumerate() {
                for (axis, value) in axes.iter().enumerate() {
                    state.set_analog(port, stick, axis, *value);
                }
            }
        }
    }

    fn devices(&self) -> Vec<GamepadDevice> {
        self.devices
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, device)| device.connected)
            .map(|(slot, device)| GamepadDevice {
                id: slot.to_string(),
                name: device.name.clone(),
                port: self.ports.get(&slot).copied(),
            })
            .collect()
    }

    fn assign(&mut self, id: &str, port: Option<usize>) {
        let Ok(slot) = id.parse::<usize>() else {
            return;
        };
        if let Some(port) = port {
            self.ports.retain(|_, assigned| *assigned != port);
            self.ports.insert(slot, port);
        } else {
            self.ports.remove(&slot);
        }
    }
}

/// Publishes the shell's gamepad source as the `SharedGamepad` service.
pub struct NativeGamepadPlugin {
    devices: SharedDevices,
}

impl NativeGamepadPlugin {
    /// Create the plugin and the device list the shell fills through the FFI.
    pub fn new() -> (Self, SharedDevices) {
        let devices: SharedDevices = Rc::new(RefCell::new(Vec::new()));
        (
            Self {
                devices: devices.clone(),
            },
            devices,
        )
    }
}

impl Plugin for NativeGamepadPlugin {
    fn name(&self) -> &'static str {
        "cgb-host-gamepad"
    }

    fn build(&self, app: &mut AppBuilder) {
        let source: crate::host::SharedGamepad = Rc::new(RefCell::new(NativeGamepad {
            devices: self.devices.clone(),
            ports: HashMap::new(),
        }));
        app.insert_service(source);
    }
}

/// Registers the system clipboard (with an in-process fallback) for the text
/// fields' copy / cut / paste. `arboard` is cross-platform.
#[derive(Default)]
pub struct NativeClipboardPlugin;

impl Plugin for NativeClipboardPlugin {
    fn name(&self) -> &'static str {
        "cgb-host-clipboard"
    }

    fn build(&self, app: &mut AppBuilder) {
        let clipboard: Rc<RefCell<dyn Clipboard>> = Rc::new(RefCell::new(NativeClipboard::new()));
        app.insert_service(clipboard);
    }
}

/// The system clipboard, with an in-process fallback when it cannot be opened.
struct NativeClipboard {
    inner: RefCell<Option<arboard::Clipboard>>,
    fallback: MemoryClipboard,
}

impl NativeClipboard {
    fn new() -> Self {
        Self {
            inner: RefCell::new(arboard::Clipboard::new().ok()),
            fallback: MemoryClipboard::default(),
        }
    }
}

impl Clipboard for NativeClipboard {
    fn get(&self) -> Option<String> {
        if let Some(clipboard) = self.inner.borrow_mut().as_mut() {
            if let Ok(text) = clipboard.get_text() {
                return Some(text);
            }
        }
        self.fallback.get()
    }

    fn set(&mut self, text: &str) {
        self.fallback.set(text);
        if let Some(clipboard) = self.inner.borrow_mut().as_mut() {
            let _ = clipboard.set_text(text.to_string());
        }
    }
}

/// Registers the backend's real font metrics as the layout measurer.
#[derive(Default)]
pub struct NativeTextMeasurePlugin;

impl Plugin for NativeTextMeasurePlugin {
    fn name(&self) -> &'static str {
        "cgb-host-text-measure"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_lifecycle_observer(NativeTextMeasureLifecycle);
    }
}

struct NativeTextMeasureLifecycle;

impl LifecycleObserver for NativeTextMeasureLifecycle {
    fn resumed(&mut self, app: &mut App) {
        if app.services().get::<Rc<dyn TextMeasurer>>().is_some() {
            return;
        }
        let Some(backend) = app.services().get::<SharedBackend>().cloned() else {
            return;
        };
        let metrics = backend.borrow().text_metrics();
        let measurer: Rc<dyn TextMeasurer> = Rc::new(NativeTextMeasurer { metrics });
        app.services_mut().insert(measurer);
    }
}

struct NativeTextMeasurer {
    metrics: FontMetrics,
}

impl TextMeasurer for NativeTextMeasurer {
    fn advance(&self, ch: char, font_size: f32) -> f32 {
        self.metrics.advance(ch, font_size)
    }

    fn advance_weighted(&self, ch: char, font_size: f32, weight: FontWeight) -> f32 {
        self.metrics.advance_weighted(ch, font_size, weight)
    }

    fn line_height(&self, font_size: f32) -> f32 {
        self.metrics.line_height(font_size)
    }

    fn ascent(&self, font_size: f32) -> f32 {
        self.metrics.ascent(font_size)
    }

    fn measure_run(&self, text: &str, font_size: f32) -> f32 {
        self.metrics.measure_run(text, font_size)
    }

    fn measure_run_weighted(&self, text: &str, font_size: f32, weight: FontWeight) -> f32 {
        self.metrics.measure_run_weighted(text, font_size, weight)
    }
}
