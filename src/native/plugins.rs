//! The non-graphics host plugins: the fullscreen request, the gamepad source,
//! the clipboard and the text measurer.
//!
//! All four are platform-neutral — they only depend on `cgb-libretro` types,
//! `igui` services and `arboard` — so both shells use this one implementation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::app::SharedBackend;
use crate::host::{GamepadSource, HostWindow};
use cgb_libretro::{GamepadSnapshot, InputState};
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

/// The shell's gamepad source: a snapshot the shell fills through the C ABI
/// (Apple `GameController` or Win32 `XInput`) and this applies to the shared
/// [`InputState`] once per frame.
struct NativeGamepad {
    snapshot: Rc<RefCell<GamepadSnapshot>>,
}

impl GamepadSource for NativeGamepad {
    fn poll(&mut self, state: &mut InputState) {
        self.snapshot.borrow().apply(state);
    }
}

/// Publishes the shell's gamepad source as the `SharedGamepad` service.
pub struct NativeGamepadPlugin {
    source: Rc<RefCell<dyn GamepadSource>>,
}

impl NativeGamepadPlugin {
    /// Create the plugin and a handle to the snapshot the shell writes.
    pub fn new() -> (Self, Rc<RefCell<GamepadSnapshot>>) {
        let snapshot = Rc::new(RefCell::new(GamepadSnapshot::default()));
        let source: Rc<RefCell<dyn GamepadSource>> = Rc::new(RefCell::new(NativeGamepad {
            snapshot: snapshot.clone(),
        }));
        (Self { source }, snapshot)
    }
}

impl Plugin for NativeGamepadPlugin {
    fn name(&self) -> &'static str {
        "cgb-host-gamepad"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.insert_service(self.source.clone());
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
