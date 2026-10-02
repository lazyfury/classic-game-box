//! The non-graphics host plugins: the fullscreen request, the gamepad source,
//! the clipboard and the text measurer.
//!
//! All four are platform-neutral — they only depend on `cgb-libretro` types,
//! `igui` services and `arboard` — so both shells use this one implementation.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::app::SharedBackend;
use crate::host::{GamepadDevice, GamepadSource, HostWindow};
use cgb_libretro::{InputState, JoypadButton, MAX_PORTS};
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

/// How long the main pad must hold Select to request a full reset.
const RESET_HOLD: Duration = Duration::from_secs(1);

/// The shell's gamepad source: it holds only raw per-device state, and Rust
/// maps device slots to libretro ports, so one assignment UI works for every
/// backend.
struct NativeGamepad {
    devices: SharedDevices,
    /// Device slot → the port it drives.
    ports: HashMap<usize, usize>,
    /// Slots already auto-assigned, so a manual unassign is not undone on the
    /// next frame.
    seen: HashSet<usize>,
    /// The port a "press Start to claim" pad takes next, if armed.
    claim: Option<usize>,
    /// Slots currently holding Start, for rising-edge detection while a claim
    /// is armed.
    start_down: HashSet<usize>,
    /// When the main pad's Select started being held, for the reset long-press.
    select_since: Option<Instant>,
    /// Whether the current Select hold already fired a reset request.
    reset_latched: bool,
    /// A pending reset request, taken by the app.
    reset_requested: bool,
}

impl NativeGamepad {
    /// A long Select press on the main pad (the one on port `0`) requests a
    /// full reset, once per press (a held button fires only once). `now` is
    /// passed in so the long-press is testable.
    fn detect_reset(&mut self, devices: &[DeviceState], now: Instant) {
        let select_bit = 1u16 << JoypadButton::Select.id();
        let main = self
            .ports
            .iter()
            .find(|(_, port)| **port == 0)
            .map(|(slot, _)| *slot);
        let held = main.is_some_and(|slot| {
            devices
                .get(slot)
                .is_some_and(|device| device.connected && device.buttons & select_bit != 0)
        });
        if !held {
            self.select_since = None;
            self.reset_latched = false;
            return;
        }
        let since = *self.select_since.get_or_insert(now);
        if !self.reset_latched && now.duration_since(since) >= RESET_HOLD {
            self.reset_requested = true;
            self.reset_latched = true;
        }
    }
}

impl GamepadSource for NativeGamepad {
    fn poll(&mut self, state: &mut InputState) {
        // Clone the shared handle so `detect_reset(&mut self, ...)` can run while
        // the device list is borrowed (borrowing `self.devices` directly would
        // conflict with the `&mut self` call).
        let shared = self.devices.clone();
        let devices = shared.borrow();
        // A newly connected pad takes the first free port, like `gilrs`, so a
        // lone controller works as 1P without a manual assignment. A slot is
        // remembered while connected, so an explicit unassign sticks; a
        // disconnect frees the port for a reconnect to claim again. While a
        // claim is waiting a fresh pad takes no port: the user claims it with
        // Start.
        for (slot, device) in devices.iter().enumerate() {
            if !device.connected {
                if self.seen.remove(&slot) {
                    self.start_down.remove(&slot);
                    if let Some(port) = self.ports.remove(&slot) {
                        state.clear(port);
                    }
                }
                continue;
            }
            let newly_seen = self.seen.insert(slot);
            if self.claim.is_none() && newly_seen && !self.ports.contains_key(&slot) {
                if let Some(port) = (0..MAX_PORTS)
                    .find(|port| !self.ports.values().any(|assigned| assigned == port))
                {
                    self.ports.insert(slot, port);
                }
            }
        }
        // "Press Start to claim": the armed port goes to the first connected
        // pad whose Start just went down.
        let start_bit = 1u16 << JoypadButton::Start.id();
        let mut claimed = None;
        for (slot, device) in devices.iter().enumerate() {
            if !device.connected {
                self.start_down.remove(&slot);
                continue;
            }
            let down = device.buttons & start_bit != 0;
            let edge = down && self.start_down.insert(slot);
            if !down {
                self.start_down.remove(&slot);
            }
            if edge && claimed.is_none() {
                claimed = Some(slot);
            }
        }
        if let Some(slot) = claimed {
            // Only an unassigned pad claims: a pad already on a port ignores
            // Start, so a lone pad cannot hop between ports on repeated presses.
            if !self.ports.contains_key(&slot) {
                if let Some(port) = self.claim.take() {
                    self.ports.retain(|_, assigned| *assigned != port);
                    self.ports.insert(slot, port);
                }
            }
        }
        self.detect_reset(&devices, Instant::now());
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
    fn claim(&mut self, port: Option<usize>) {
        self.claim = port;
    }

    fn take_reset_request(&mut self) -> bool {
        std::mem::take(&mut self.reset_requested)
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
            seen: HashSet::new(),
            claim: None,
            start_down: HashSet::new(),
            select_since: None,
            reset_latched: false,
            reset_requested: false,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::GamepadSource;
    use cgb_libretro::InputState;

    fn device(name: &str, connected: bool) -> DeviceState {
        DeviceState {
            name: name.to_string(),
            connected,
            ..DeviceState::default()
        }
    }

    fn source(devices: SharedDevices) -> NativeGamepad {
        NativeGamepad {
            devices,
            ports: HashMap::new(),
            seen: HashSet::new(),
            claim: None,
            start_down: HashSet::new(),
            select_since: None,
            reset_latched: false,
            reset_requested: false,
        }
    }

    #[test]
    fn a_lone_pad_defaults_to_1p_and_an_unassign_sticks() {
        let devices = Rc::new(RefCell::new(vec![device("Pad", true)]));
        let mut gamepad = source(devices.clone());
        let mut state = InputState::default();

        gamepad.poll(&mut state);
        assert_eq!(gamepad.devices()[0].port, Some(0), "a lone pad takes 1P");

        // The user unassigns it; the next frame must not re-assign it.
        gamepad.assign("0", None);
        gamepad.poll(&mut state);
        assert_eq!(gamepad.devices()[0].port, None, "an unassign sticks");

        // A disconnect frees the slot; a reconnect claims a port again.
        devices.borrow_mut()[0].connected = false;
        gamepad.poll(&mut state);
        devices.borrow_mut()[0].connected = true;
        gamepad.poll(&mut state);
        assert_eq!(
            gamepad.devices()[0].port,
            Some(0),
            "a reconnect claims a port again"
        );
    }

    #[test]
    fn pressing_start_claims_the_armed_port() {
        let devices = Rc::new(RefCell::new(vec![device("A", true), device("B", true)]));
        let mut gamepad = source(devices.clone());
        let mut state = InputState::default();
        // Both pads auto-assign (1P and 2P) before any claim.
        gamepad.poll(&mut state);
        assert_eq!(gamepad.devices()[0].port, Some(0));
        assert_eq!(gamepad.devices()[1].port, Some(1));

        // Free both, arm 1P, then press Start on the second pad.
        gamepad.assign("0", None);
        gamepad.assign("1", None);
        gamepad.claim(Some(0));
        devices.borrow_mut()[1].buttons = 1u16 << JoypadButton::Start.id();
        gamepad.poll(&mut state);

        assert_eq!(
            gamepad.devices()[1].port,
            Some(0),
            "B claimed 1P with Start"
        );
        assert_eq!(gamepad.devices()[0].port, None, "A did not claim");
    }

    #[test]
    fn an_assigned_pad_ignores_start_but_an_unassigned_one_claims() {
        let devices = Rc::new(RefCell::new(vec![device("A", true), device("B", true)]));
        let mut gamepad = source(devices.clone());
        let mut state = InputState::default();
        gamepad.poll(&mut state);
        assert_eq!(gamepad.devices()[0].port, Some(0));
        assert_eq!(gamepad.devices()[1].port, Some(1));

        let start = 1u16 << JoypadButton::Start.id();
        // B is unassigned and 2P is armed; A (already 1P) presses Start first.
        gamepad.assign("1", None);
        gamepad.claim(Some(1));
        devices.borrow_mut()[0].buttons = start;
        gamepad.poll(&mut state);
        assert_eq!(gamepad.devices()[0].port, Some(0), "A ignores Start");

        // Release A, then B presses Start and claims 2P.
        devices.borrow_mut()[0].buttons = 0;
        gamepad.poll(&mut state);
        devices.borrow_mut()[1].buttons = start;
        gamepad.poll(&mut state);
        assert_eq!(gamepad.devices()[1].port, Some(1), "B claims 2P");
    }

    #[test]
    fn holding_select_on_the_main_pad_requests_reset_once_per_press() {
        let devices = Rc::new(RefCell::new(vec![device("A", true)]));
        let mut gamepad = source(devices.clone());
        let mut state = InputState::default();
        gamepad.poll(&mut state);
        assert_eq!(gamepad.devices()[0].port, Some(0), "A is the main pad");

        let select = 1u16 << JoypadButton::Select.id();
        let t0 = Instant::now();
        devices.borrow_mut()[0].buttons = select;
        gamepad.detect_reset(&devices.borrow(), t0);
        assert!(
            !gamepad.take_reset_request(),
            "not yet: the hold just began"
        );

        gamepad.detect_reset(&devices.borrow(), t0 + RESET_HOLD);
        assert!(gamepad.take_reset_request(), "the long hold fires");
        assert!(!gamepad.take_reset_request(), "and only once");

        // Release, then hold again: a second press fires again.
        devices.borrow_mut()[0].buttons = 0;
        gamepad.detect_reset(&devices.borrow(), t0 + RESET_HOLD * 2);
        devices.borrow_mut()[0].buttons = select;
        gamepad.detect_reset(&devices.borrow(), t0 + RESET_HOLD * 3);
        gamepad.detect_reset(&devices.borrow(), t0 + RESET_HOLD * 4);
        assert!(gamepad.take_reset_request(), "a new hold fires again");
    }
}
