//! The native Windows host: a wgpu surface over a C++-provided `HWND`, and the
//! `igui_app` plugins the embedded runtime needs.
//!
//! This mirrors `src/mac/host.rs`. The window (and its `HWND`) is the C++ Win32
//! shell's; here we only turn that handle into a `wgpu::Surface`, build the
//! backend, and present each frame's `DrawList`. There is no `winit` here: the
//! C++ message loop drives the clock and forwards events (see `ffi.rs`).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::num::NonZeroIsize;
use std::rc::Rc;

use crate::app::SharedBackend;
use crate::host::{GamepadSource, HostWindow};
use cgb_libretro::{GamepadSnapshot, InputState};
use igui::igui_app::{App, AppBuilder, LifecycleObserver, Plugin, PresentOutcome, Presenter};
use igui::igui_backend_wgpu::wgpu;
use igui::igui_backend_wgpu::wgpu::rwh;
use igui::igui_backend_wgpu::{FontConfig, FontMetrics, FontMode, WgpuBackend};
use igui::igui_core::{Color, FontWeight, Size, ViewportSize};
use igui::igui_render::{DrawList, RenderBackend};
use igui::igui_ui::{Clipboard, MemoryClipboard, TextMeasurer};

/// The `HWND` the C++ shell hands over, with its pixel geometry.
///
/// Inserted as a service before the runtime is built, so the graphics
/// lifecycle can find it. `hwnd` is borrowed: the shell owns the window for the
/// app's lifetime.
#[derive(Clone, Copy)]
pub struct WinSurface {
    pub hwnd: *mut c_void,
    /// Drawable size in physical pixels.
    pub width: u32,
    pub height: u32,
    /// Backing scale (`dpi / 96`).
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

/// A handle to the graphics state, kept by the FFI so the shell can resize.
#[derive(Clone, Default)]
pub struct WinGpu {
    state: SharedGpuState,
}

impl WinGpu {
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
            // DX12's `ResizeBuffers` fails while a reference to a swap-chain
            // back buffer is alive, and igui `v0.3.0` keeps the previous
            // frame's surface texture view in its frame state. Render one
            // offscreen frame first so that view is dropped before we
            // reconfigure. Upstream now drops the frame in `end_frame`, so
            // this can go once cgb's igui dependency is bumped past `v0.3.0`.
            // (Metal reconfigures fine without it either way.)
            let viewport = WinPresenter::viewport_of(state);
            {
                let mut backend = state.backend.borrow_mut();
                if backend.begin_frame(viewport).is_ok() {
                    let _ = backend.end_frame();
                }
            }
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
/// the C++ shell polls it with `cgb_win_take_fullscreen` (so the shell owns the
/// `WS_POPUP` ↔ `WS_OVERLAPPEDWINDOW` switch and no callback crosses the ABI).
#[derive(Clone, Default)]
pub struct WinHostWindow {
    request: Rc<Cell<Option<bool>>>,
}

impl WinHostWindow {
    /// Take the pending request, if any.
    pub fn take_request(&self) -> Option<bool> {
        self.request.take()
    }
}

impl HostWindow for WinHostWindow {
    fn set_fullscreen(&self, on: bool) {
        self.request.set(Some(on));
    }
}

/// The C++ gamepad source: a snapshot the shell fills through the C ABI
/// (XInput) and this applies to the shared [`InputState`] once per frame.
struct WinGamepad {
    snapshot: Rc<RefCell<GamepadSnapshot>>,
}

impl GamepadSource for WinGamepad {
    fn poll(&mut self, state: &mut InputState) {
        self.snapshot.borrow().apply(state);
    }
}

/// Publishes the C++ gamepad source as the `SharedGamepad` service.
pub struct WinGamepadPlugin {
    source: Rc<RefCell<dyn GamepadSource>>,
}

impl WinGamepadPlugin {
    /// Create the plugin and a handle to the snapshot the shell writes.
    pub fn new() -> (Self, Rc<RefCell<GamepadSnapshot>>) {
        let snapshot = Rc::new(RefCell::new(GamepadSnapshot::default()));
        let source: Rc<RefCell<dyn GamepadSource>> = Rc::new(RefCell::new(WinGamepad {
            snapshot: snapshot.clone(),
        }));
        (Self { source }, snapshot)
    }
}

impl Plugin for WinGamepadPlugin {
    fn name(&self) -> &'static str {
        "cgb-win-gamepad"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.insert_service(self.source.clone());
    }
}

/// The graphics plugin: builds the surface/backend on first resume and
/// installs the presenter.
pub struct WinGpuPlugin {
    gpu: WinGpu,
}

impl WinGpuPlugin {
    /// Create the plugin and a handle to the graphics state it will fill in.
    pub fn new() -> (Self, WinGpu) {
        let gpu = WinGpu::default();
        (Self { gpu: gpu.clone() }, gpu)
    }
}

impl Plugin for WinGpuPlugin {
    fn name(&self) -> &'static str {
        "cgb-win-gpu"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_lifecycle_observer(WinGpuLifecycle {
            state: self.gpu.state.clone(),
        });
    }
}

struct WinGpuLifecycle {
    state: SharedGpuState,
}

impl LifecycleObserver for WinGpuLifecycle {
    fn resumed(&mut self, app: &mut App) {
        if self.state.borrow().is_some() {
            return;
        }
        let Some(desc) = app.services().get::<WinSurface>().copied() else {
            eprintln!("cgb-win: 没有 WinSurface 服务，跳过 GPU 初始化");
            return;
        };
        if desc.hwnd.is_null() {
            eprintln!("cgb-win: HWND 为空，跳过 GPU 初始化");
            return;
        }

        let instance = wgpu::Instance::default();
        // SAFETY: the C++ shell owns the window for at least as long as the
        // surface; the handle is valid for the app's lifetime.
        let surface = match unsafe { create_surface(&instance, desc.hwnd) } {
            Ok(surface) => surface,
            Err(error) => {
                eprintln!("cgb-win: 创建 HWND surface 失败：{error}");
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
                eprintln!("cgb-win: 创建 wgpu backend 失败：{error}");
                return;
            }
        };

        let info = backend.adapter().get_info();
        eprintln!(
            "cgb-win: wgpu backend={:?} adapter={:?} driver={:?}",
            info.backend, info.name, info.driver
        );
        // wgpu's default handler panics on a validation error (e.g. a `Surface`
        // resize that the driver rejects). A panic crossing this `extern "C"`
        // boundary aborts the process, and a VM's virtual GPU rejects some
        // swap-chain resizes, so log and carry on instead of dying.
        backend.device().on_uncaptured_error(Box::new(|error| {
            eprintln!("cgb-win: wgpu 错误（已忽略，避免崩溃）：{error}");
        }));

        // Prefer a non-sRGB format so the shader's unorm colors match the
        // Canvas backend; fall back to whatever the surface offers.
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
            eprintln!("cgb-win: 字体设置失败，使用回退：{error}");
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
        app.set_presenter(WinPresenter {
            state: self.state.clone(),
        });
    }
}

/// Create a `wgpu::Surface` straight from an `HWND` (raw-window-handle), the
/// Windows counterpart of macOS's `SurfaceTargetUnsafe::CoreAnimationLayer`.
///
/// # Safety
///
/// `hwnd` must be a live window handle that outlives the returned surface.
unsafe fn create_surface(
    instance: &wgpu::Instance,
    hwnd: *mut c_void,
) -> Result<wgpu::Surface<'static>, wgpu::CreateSurfaceError> {
    // The caller checked for null; a null HWND has no `NonZeroIsize`.
    let window = NonZeroIsize::new(hwnd as isize).expect("HWND 必须非零");
    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: rwh::RawDisplayHandle::Windows(rwh::WindowsDisplayHandle::new()),
        raw_window_handle: rwh::RawWindowHandle::Win32(rwh::Win32WindowHandle::new(window)),
    };
    // SAFETY: the caller guarantees `hwnd` is live for the surface's lifetime.
    unsafe { instance.create_surface_unsafe(target) }
}

/// Presents the app's `DrawList` to the window surface.
struct WinPresenter {
    state: SharedGpuState,
}

impl WinPresenter {
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

impl Presenter for WinPresenter {
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
                eprintln!("cgb-win: surface 错误：{error}");
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
/// fields' copy / cut / paste. `arboard` is cross-platform, so this is shared
/// with the macOS host verbatim.
#[derive(Default)]
pub struct WinClipboardPlugin;

impl Plugin for WinClipboardPlugin {
    fn name(&self) -> &'static str {
        "cgb-win-clipboard"
    }

    fn build(&self, app: &mut AppBuilder) {
        let clipboard: Rc<RefCell<dyn Clipboard>> = Rc::new(RefCell::new(WinClipboard::new()));
        app.insert_service(clipboard);
    }
}

/// The system clipboard, with an in-process fallback when it cannot be opened.
struct WinClipboard {
    inner: RefCell<Option<arboard::Clipboard>>,
    fallback: MemoryClipboard,
}

impl WinClipboard {
    fn new() -> Self {
        Self {
            inner: RefCell::new(arboard::Clipboard::new().ok()),
            fallback: MemoryClipboard::default(),
        }
    }
}

impl Clipboard for WinClipboard {
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
pub struct WinTextMeasurePlugin;

impl Plugin for WinTextMeasurePlugin {
    fn name(&self) -> &'static str {
        "cgb-win-text-measure"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_lifecycle_observer(WinTextMeasureLifecycle);
    }
}

struct WinTextMeasureLifecycle;

impl LifecycleObserver for WinTextMeasureLifecycle {
    fn resumed(&mut self, app: &mut App) {
        if app.services().get::<Rc<dyn TextMeasurer>>().is_some() {
            return;
        }
        let Some(backend) = app.services().get::<SharedBackend>().cloned() else {
            return;
        };
        let metrics = backend.borrow().text_metrics();
        let measurer: Rc<dyn TextMeasurer> = Rc::new(WinTextMeasurer { metrics });
        app.services_mut().insert(measurer);
    }
}

struct WinTextMeasurer {
    metrics: FontMetrics,
}

impl TextMeasurer for WinTextMeasurer {
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
