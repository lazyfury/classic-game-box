//! The native macOS host: a wgpu surface over a Swift-provided `CAMetalLayer`,
//! and the `igui_app` plugins the embedded runtime needs.
//!
//! This is the piece that replaces `igui_winit` for the Swift path. The window
//! (and its `CAMetalLayer`) is Swift's; here we only turn that layer into a
//! `wgpu::Surface`, build the backend, and present each frame's `DrawList`.
//!
//! There is no `winit` here: Swift drives the clock and forwards events (see
//! `ffi.rs`).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;

use cgb_app::app::{GamepadSource, HostWindow, SharedBackend};
use cgb_input::{GamepadSnapshot, InputState};
use igui::igui_app::{App, AppBuilder, LifecycleObserver, Plugin, PresentOutcome, Presenter};
use igui::igui_backend_wgpu::wgpu;
use igui::igui_backend_wgpu::{FontConfig, FontMetrics, FontMode, WgpuBackend};
use igui::igui_core::{Color, FontWeight, Size, ViewportSize};
use igui::igui_render::{DrawList, RenderBackend};
use igui::igui_ui::{Clipboard, MemoryClipboard, TextMeasurer};

/// The `CAMetalLayer` Swift hands over, with its pixel geometry.
///
/// Inserted as a service before the runtime is built, so the graphics
/// lifecycle can find it. `layer` is borrowed: Swift owns it for the app's
/// lifetime (the same contract `wgpu`'s `CoreAnimationLayer` target states).
#[derive(Clone, Copy)]
pub struct MacSurface {
    pub layer: *mut c_void,
    /// Drawable size in physical pixels.
    pub width: u32,
    pub height: u32,
    /// Backing scale (`2.0` on Retina).
    pub scale: f64,
}

/// The live surface + backend, created on the first resume.
struct GpuState {
    #[allow(dead_code)]
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    backend: SharedBackend,
    scale: f64,
}

type SharedGpuState = Rc<RefCell<Option<GpuState>>>;

/// A handle to the graphics state, kept by the FFI so Swift can resize.
#[derive(Clone, Default)]
pub struct MacGpu {
    state: SharedGpuState,
}

impl MacGpu {
    /// Whether the surface/backend came up (the lifecycle ran successfully).
    pub fn is_ready(&self) -> bool {
        self.state.borrow().is_some()
    }

    /// Reconfigure the surface for a new drawable size / scale.
    pub fn resize(&self, width: u32, height: u32, scale: f64) {
        let mut guard = self.state.borrow_mut();
        let Some(state) = guard.as_mut() else {
            return;
        };
        if width > 0 && height > 0 {
            state.config.width = width;
            state.config.height = height;
            state
                .surface
                .configure(state.backend.borrow().device(), &state.config);
        }
        state.scale = scale;
        let scale = if scale > 0.0 { scale as f32 } else { 1.0 };
        state.backend.borrow_mut().set_scale_factor(scale);
    }
}

/// The host window the app asks for fullscreen.
///
/// The app calls [`HostWindow::set_fullscreen`]; the request is parked here and
/// Swift polls it with `cgb_mac_take_fullscreen` (so Swift keeps the AppKit
/// `toggleFullScreen:` animation and no callback crosses the boundary).
#[derive(Clone, Default)]
pub struct MacHostWindow {
    request: Rc<Cell<Option<bool>>>,
}

impl MacHostWindow {
    /// Take the pending request, if any.
    pub fn take_request(&self) -> Option<bool> {
        self.request.take()
    }
}

impl HostWindow for MacHostWindow {
    fn set_fullscreen(&self, on: bool) {
        self.request.set(Some(on));
    }
}

/// The Swift gamepad source: a snapshot Swift fills through the C ABI and this
/// applies to the shared [`InputState`] once per frame.
struct MacGamepad {
    snapshot: Rc<RefCell<GamepadSnapshot>>,
}

impl GamepadSource for MacGamepad {
    fn poll(&mut self, state: &mut InputState) {
        self.snapshot.borrow().apply(state);
    }
}

/// Publishes the Swift gamepad source as the `SharedGamepad` service.
pub struct MacGamepadPlugin {
    source: Rc<RefCell<dyn GamepadSource>>,
}

impl MacGamepadPlugin {
    /// Create the plugin and a handle to the snapshot Swift writes.
    pub fn new() -> (Self, Rc<RefCell<GamepadSnapshot>>) {
        let snapshot = Rc::new(RefCell::new(GamepadSnapshot::default()));
        let source: Rc<RefCell<dyn GamepadSource>> = Rc::new(RefCell::new(MacGamepad {
            snapshot: snapshot.clone(),
        }));
        (Self { source }, snapshot)
    }
}

impl Plugin for MacGamepadPlugin {
    fn name(&self) -> &'static str {
        "cgb-mac-gamepad"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.insert_service(self.source.clone());
    }
}

/// The graphics plugin: builds the surface/backend on first resume and
/// installs the presenter.
pub struct MacGpuPlugin {
    gpu: MacGpu,
}

impl MacGpuPlugin {
    /// Create the plugin and a handle to the graphics state it will fill in.
    pub fn new() -> (Self, MacGpu) {
        let gpu = MacGpu::default();
        (Self { gpu: gpu.clone() }, gpu)
    }
}

impl Plugin for MacGpuPlugin {
    fn name(&self) -> &'static str {
        "cgb-mac-gpu"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_lifecycle_observer(MacGpuLifecycle {
            state: self.gpu.state.clone(),
        });
    }
}

struct MacGpuLifecycle {
    state: SharedGpuState,
}

impl LifecycleObserver for MacGpuLifecycle {
    fn resumed(&mut self, app: &mut App) {
        if self.state.borrow().is_some() {
            return;
        }
        let Some(desc) = app.services().get::<MacSurface>().copied() else {
            eprintln!("cgb-mac: 没有 MacSurface 服务，跳过 GPU 初始化");
            return;
        };

        let instance = wgpu::Instance::default();
        // SAFETY: Swift owns the CAMetalLayer for at least as long as the
        // surface; the pointer is valid for the app's lifetime.
        let surface = match unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(desc.layer))
        } {
            Ok(surface) => surface,
            Err(error) => {
                eprintln!("cgb-mac: 创建 Metal surface 失败：{error}");
                return;
            }
        };

        let mut backend = match WgpuBackend::from_instance(
            &instance,
            Some(&surface),
            wgpu::PowerPreference::HighPerformance,
        ) {
            Ok(backend) => backend,
            Err(error) => {
                eprintln!("cgb-mac: 创建 wgpu backend 失败：{error}");
                return;
            }
        };

        // Prefer a non-sRGB format so the shader's unorm colors match the
        // Canvas backend; fall back to whatever the layer offers.
        let capabilities = surface.get_capabilities(backend.adapter());
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .unwrap_or(capabilities.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: desc.width.max(1),
            height: desc.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: Vec::new(),
        };
        surface.configure(backend.device(), &config);

        let scale = if desc.scale > 0.0 { desc.scale } else { 1.0 };
        backend.set_scale_factor(scale as f32);
        backend.set_clear_color(Color::new(0.039, 0.039, 0.039, 1.0));
        let font = FontConfig {
            mode: FontMode::System,
            device_pixel_rasterization: true,
            ..Default::default()
        };
        if let Err(error) = backend.set_font_config(font) {
            eprintln!("cgb-mac: 字体设置失败，使用回退：{error}");
        }

        let backend: SharedBackend = Rc::new(RefCell::new(backend));
        app.services_mut().insert(backend.clone());
        *self.state.borrow_mut() = Some(GpuState {
            instance,
            surface,
            config,
            backend,
            scale,
        });
        app.set_presenter(MacPresenter {
            state: self.state.clone(),
        });
    }
}

/// Presents the app's `DrawList` to the Metal layer.
struct MacPresenter {
    state: SharedGpuState,
}

impl MacPresenter {
    fn viewport_of(state: &GpuState) -> ViewportSize {
        let scale = if state.scale > 0.0 {
            state.scale as f32
        } else {
            1.0
        };
        ViewportSize::new(Size::new(
            state.config.width as f32 / scale,
            state.config.height as f32 / scale,
        ))
    }
}

impl Presenter for MacPresenter {
    fn viewport(&self) -> ViewportSize {
        self.state
            .borrow()
            .as_ref()
            .map_or(ViewportSize::default(), Self::viewport_of)
    }

    fn present(&mut self, list: &DrawList) -> PresentOutcome {
        let mut guard = self.state.borrow_mut();
        let Some(state) = guard.as_mut() else {
            return PresentOutcome::Skipped;
        };
        let viewport = Self::viewport_of(state);

        let texture = match state.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                state
                    .surface
                    .configure(state.backend.borrow().device(), &state.config);
                return PresentOutcome::Reconfigured;
            }
            Err(wgpu::SurfaceError::Timeout) => return PresentOutcome::Skipped,
            Err(error) => {
                eprintln!("cgb-mac: surface 错误：{error}");
                return PresentOutcome::Skipped;
            }
        };

        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut backend = state.backend.borrow_mut();
        if backend
            .begin_frame_with_view(
                view,
                state.config.width,
                state.config.height,
                state.config.format,
                viewport,
            )
            .is_ok()
        {
            let _ = backend.submit(list);
            let _ = backend.end_frame();
        }
        drop(backend);
        texture.present();
        PresentOutcome::Presented
    }
}

/// Registers the system clipboard (with an in-process fallback) for the text
/// fields' copy / cut / paste.
#[derive(Default)]
pub struct MacClipboardPlugin;

impl Plugin for MacClipboardPlugin {
    fn name(&self) -> &'static str {
        "cgb-mac-clipboard"
    }

    fn build(&self, app: &mut AppBuilder) {
        let clipboard: Rc<RefCell<dyn Clipboard>> = Rc::new(RefCell::new(MacClipboard::new()));
        app.insert_service(clipboard);
    }
}

/// The macOS pasteboard, with an in-process fallback when it cannot be opened.
struct MacClipboard {
    inner: RefCell<Option<arboard::Clipboard>>,
    fallback: MemoryClipboard,
}

impl MacClipboard {
    fn new() -> Self {
        Self {
            inner: RefCell::new(arboard::Clipboard::new().ok()),
            fallback: MemoryClipboard::default(),
        }
    }
}

impl Clipboard for MacClipboard {
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
///
/// The `winit` host gets this from `igui_winit::TextMeasurePlugin`; the
/// embedded host copies the ~20 lines so it needs no `igui_winit`.
#[derive(Default)]
pub struct MacTextMeasurePlugin;

impl Plugin for MacTextMeasurePlugin {
    fn name(&self) -> &'static str {
        "cgb-mac-text-measure"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_lifecycle_observer(MacTextMeasureLifecycle);
    }
}

struct MacTextMeasureLifecycle;

impl LifecycleObserver for MacTextMeasureLifecycle {
    fn resumed(&mut self, app: &mut App) {
        if app.services().get::<Rc<dyn TextMeasurer>>().is_some() {
            return;
        }
        let Some(backend) = app.services().get::<SharedBackend>().cloned() else {
            return;
        };
        let metrics = backend.borrow().text_metrics();
        let measurer: Rc<dyn TextMeasurer> = Rc::new(MacTextMeasurer { metrics });
        app.services_mut().insert(measurer);
    }
}

struct MacTextMeasurer {
    metrics: FontMetrics,
}

impl TextMeasurer for MacTextMeasurer {
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
