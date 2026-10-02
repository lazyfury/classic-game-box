//! The native window surface the shell hands over.
//!
//! Both shells give Rust an opaque window handle plus its pixel geometry; only
//! the *kind* of handle differs. macOS hands over a `CAMetalLayer*`, Windows an
//! `HWND`. [`create_surface`] is the single place that knows the difference.

use std::ffi::c_void;
use std::num::NonZeroIsize;

use igui::igui_backend_wgpu::wgpu;
use igui::igui_backend_wgpu::wgpu::rwh;

/// The window handle the shell hands over, with its physical pixel geometry.
///
/// Inserted as a service before the runtime is built, so the graphics lifecycle
/// can find it. `handle` is borrowed: the shell owns the window for the app's
/// lifetime.
#[derive(Clone, Copy)]
pub struct NativeSurface {
    /// `CAMetalLayer*` on macOS, `HWND` on Windows.
    pub handle: *mut c_void,
    /// Drawable size in physical pixels.
    pub width: u32,
    pub height: u32,
    /// Backing scale (Retina `2.0`, Windows `dpi / 96`).
    pub scale: f64,
}

/// Create a `wgpu::Surface` from the shell's handle.
///
/// The macOS target is the only one that cannot compile elsewhere
/// (`SurfaceTargetUnsafe::CoreAnimationLayer` is `#[cfg(metal)]`), so it is
/// the branch that is gated. The Windows branch is written against
/// `raw-window-handle`, which is cross-platform, and is deliberately left
/// ungated: that keeps it type-checked by the macOS dev machine's
/// `cargo clippy` / `cargo test` gate.
///
/// # Safety
///
/// `handle` must be a live window handle that outlives the returned surface.
#[allow(unreachable_code)]
pub(super) unsafe fn create_surface(
    instance: &wgpu::Instance,
    handle: *mut c_void,
) -> Result<wgpu::Surface<'static>, wgpu::CreateSurfaceError> {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: the caller guarantees `handle` is a live `CAMetalLayer`.
        return unsafe { mac_surface(instance, handle) };
    }
    // SAFETY: the caller guarantees `handle` is a live window.
    unsafe { win_surface(instance, handle) }
}

/// The macOS path: a `CAMetalLayer*` straight into wgpu.
///
/// # Safety
///
/// `layer` must be a live `CAMetalLayer`.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
unsafe fn mac_surface(
    instance: &wgpu::Instance,
    layer: *mut c_void,
) -> Result<wgpu::Surface<'static>, wgpu::CreateSurfaceError> {
    // SAFETY: the caller guarantees `layer` is a live `CAMetalLayer`.
    unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer)) }
}

/// The Windows path: an `HWND` via `raw-window-handle`.
///
/// # Safety
///
/// `hwnd` must be a live window handle.
unsafe fn win_surface(
    instance: &wgpu::Instance,
    hwnd: *mut c_void,
) -> Result<wgpu::Surface<'static>, wgpu::CreateSurfaceError> {
    // The caller checked for null; a null handle has no `NonZeroIsize`.
    let window = NonZeroIsize::new(hwnd as isize).expect("window handle must be non-null");
    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: rwh::RawDisplayHandle::Windows(rwh::WindowsDisplayHandle::new()),
        raw_window_handle: rwh::RawWindowHandle::Win32(rwh::Win32WindowHandle::new(window)),
    };
    // SAFETY: the caller guarantees `hwnd` is live for the surface's lifetime.
    unsafe { instance.create_surface_unsafe(target) }
}
