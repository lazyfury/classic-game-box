//! The wgpu surface + presenter the embedded runtime renders through.
//!
//! Shared by both shells: they hand Rust a window handle (see
//! [`NativeSurface`]) and Rust owns the surface, the backend and the presenter.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::SharedBackend;
use igui::igui_app::{App, AppBuilder, LifecycleObserver, Plugin, PresentOutcome, Presenter};
use igui::igui_backend_wgpu::wgpu;
use igui::igui_backend_wgpu::{FontConfig, FontMode, WgpuBackend};
use igui::igui_core::{Color, Size, ViewportSize};
use igui::igui_render::{DrawList, RenderBackend};

use super::surface::{create_surface, NativeSurface};

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
pub struct NativeGpu {
    state: SharedGpuState,
}

impl NativeGpu {
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

/// The graphics plugin: builds the surface/backend on first resume and installs
/// the presenter.
pub struct NativeGpuPlugin {
    gpu: NativeGpu,
}

impl NativeGpuPlugin {
    /// Create the plugin and a handle to the graphics state it will fill in.
    pub fn new() -> (Self, NativeGpu) {
        let gpu = NativeGpu::default();
        (Self { gpu: gpu.clone() }, gpu)
    }
}

impl Plugin for NativeGpuPlugin {
    fn name(&self) -> &'static str {
        "cgb-host-gpu"
    }

    fn build(&self, app: &mut AppBuilder) {
        app.add_lifecycle_observer(NativeGpuLifecycle {
            state: self.gpu.state.clone(),
        });
    }
}

struct NativeGpuLifecycle {
    state: SharedGpuState,
}

impl LifecycleObserver for NativeGpuLifecycle {
    fn resumed(&mut self, app: &mut App) {
        if self.state.borrow().is_some() {
            return;
        }
        let Some(desc) = app.services().get::<NativeSurface>().copied() else {
            eprintln!("cgb-host: 没有 NativeSurface 服务，跳过 GPU 初始化");
            return;
        };
        if desc.handle.is_null() {
            eprintln!("cgb-host: 窗口句柄为空，跳过 GPU 初始化");
            return;
        }

        let instance = wgpu::Instance::default();
        // SAFETY: the shell owns the window for at least as long as the
        // surface; the handle is valid for the app's lifetime.
        let surface = match unsafe { create_surface(&instance, desc.handle) } {
            Ok(surface) => surface,
            Err(error) => {
                eprintln!("cgb-host: 创建 wgpu surface 失败：{error}");
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
                eprintln!("cgb-host: 创建 wgpu backend 失败：{error}");
                return;
            }
        };

        let info = backend.adapter().get_info();
        eprintln!(
            "cgb-host: wgpu backend={:?} adapter={:?} driver={:?}",
            info.backend, info.name, info.driver
        );
        // wgpu's default handler panics on an uncaptured validation error (for
        // example a `Surface` resize the driver rejects). A panic crossing this
        // `extern "C"` boundary aborts the process, and a VM's virtual GPU
        // rejects some swap-chain resizes, so log and carry on instead.
        backend.device().on_uncaptured_error(Box::new(|error| {
            eprintln!("cgb-host: wgpu 错误（已忽略，避免崩溃）：{error}");
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
            eprintln!("cgb-host: 字体设置失败，使用回退：{error}");
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
        app.set_presenter(NativePresenter {
            state: self.state.clone(),
        });
    }
}

/// Presents the app's `DrawList` to the window surface.
struct NativePresenter {
    state: SharedGpuState,
}

impl NativePresenter {
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

impl Presenter for NativePresenter {
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
                eprintln!("cgb-host: surface 错误：{error}");
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
