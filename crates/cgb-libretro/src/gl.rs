//! Offscreen OpenGL for hardware-rendering cores, on macOS.
//!
//! libretro's hardware-rendering cores (N64's Mupen64Plus-Next/GLideN64, and
//! later PSP's PPSSPP) do not hand the front end pixels; they ask it for a GL
//! context and a framebuffer object (`RETRO_ENVIRONMENT_SET_HW_RENDER`), render
//! into that FBO themselves, and signal a frame with
//! `video_refresh(RETRO_HW_FRAME_BUFFER_VALID, …)`.
//!
//! This module owns that context and FBO. It uses **CGL** (Core OpenGL), which
//! needs no window and no AppKit: an offscreen 4.1 core context satisfies the
//! 3.3 core context GLideN64 requests. Each frame the app reads the FBO back
//! with `glReadPixels` into RGBA8, exactly like the software path, so the rest
//! of the front end (the `Frame` type, the wgpu texture, the post-process
//! shaders) is unchanged.
//!
//! The context is created and used on the calling thread (the app's main
//! thread, which is also where `retro_run` and `take_frame` run), so no GL
//! object is ever touched cross-thread.
//!
//! `get_proc_address` is served by `dlsym(RTLD_DEFAULT, …)`: the OpenGL
//! framework is linked into the process, so its GL symbols are visible. This is
//! the same lookup the C++ probes in the repo's history used.

use std::ffi::{c_char, c_int, c_uint, c_void, CStr};
use std::ptr;

// --- GL / CGL types -------------------------------------------------------

type GLenum = c_uint;
type GLuint = c_uint;
type GLint = c_int;
type GLsizei = c_int;

// --- GL constants ---------------------------------------------------------

const GL_FRAMEBUFFER: GLenum = 0x8d40;
const GL_RENDERBUFFER: GLenum = 0x8d41;
const GL_COLOR_ATTACHMENT0: GLenum = 0x8ce0;
const GL_DEPTH_ATTACHMENT: GLenum = 0x8d00;
const GL_DEPTH_STENCIL_ATTACHMENT: GLenum = 0x821a;

const GL_TEXTURE_2D: GLenum = 0x0de1;
const GL_TEXTURE_MIN_FILTER: GLenum = 0x2801;
const GL_TEXTURE_MAG_FILTER: GLenum = 0x2800;
const GL_TEXTURE_WRAP_S: GLenum = 0x2802;
const GL_TEXTURE_WRAP_T: GLenum = 0x2803;
const GL_NEAREST: GLint = 0x2600;
const GL_CLAMP_TO_EDGE: GLint = 0x812f;

const GL_RGBA8: GLint = 0x8058 as GLint;
const GL_RGBA: GLenum = 0x1908;
const GL_UNSIGNED_BYTE: GLenum = 0x1401;
const GL_DEPTH_COMPONENT24: GLenum = 0x81a6;
const GL_DEPTH24_STENCIL8: GLenum = 0x88f0;

const GL_READ_FRAMEBUFFER: GLenum = 0x8ca8;
const GL_FRAMEBUFFER_COMPLETE: GLenum = 0x8cd5;
const GL_PACK_ALIGNMENT: GLenum = 0x0d05;

// CGL pixel-format attributes (`CGLTypes.h`).
const K_CGL_PFA_COLOR_SIZE: u32 = 8;
const K_CGL_PFA_ALPHA_SIZE: u32 = 11;
const K_CGL_PFA_DEPTH_SIZE: u32 = 12;
const K_CGL_PFA_STENCIL_SIZE: u32 = 13;
const K_CGL_PFA_ACCELERATED: u32 = 73;
const K_CGL_PFA_CLOSEST_POLICY: u32 = 74;
const K_CGL_PFA_OPENGL_PROFILE: u32 = 99;
const K_CGL_OGL_VERSION_GL4_CORE: u32 = 0x4100;
const K_CGL_NO_ERROR: c_int = 0;

// `RTLD_DEFAULT` on macOS is `(void *)-2`.
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

#[link(name = "OpenGL", kind = "framework")]
extern "C" {
    fn CGLChoosePixelFormat(attribs: *const u32, pix: *mut *mut c_void, npix: *mut c_int) -> c_int;
    fn CGLCreateContext(pix: *mut c_void, share: *mut c_void, ctx: *mut *mut c_void) -> c_int;
    fn CGLSetCurrentContext(ctx: *mut c_void) -> c_int;
    fn CGLGetCurrentContext() -> *mut c_void;
    fn CGLDestroyContext(ctx: *mut c_void) -> c_int;
    fn CGLDestroyPixelFormat(pix: *mut c_void) -> c_int;

    fn glGenFramebuffers(n: GLsizei, ids: *mut GLuint);
    fn glBindFramebuffer(target: GLenum, framebuffer: GLuint);
    fn glDeleteFramebuffers(n: GLsizei, ids: *const GLuint);
    fn glFramebufferTexture2D(
        target: GLenum,
        attachment: GLenum,
        textarget: GLenum,
        texture: GLuint,
        level: GLint,
    );
    fn glGenRenderbuffers(n: GLsizei, ids: *mut GLuint);
    fn glBindRenderbuffer(target: GLenum, renderbuffer: GLuint);
    fn glDeleteRenderbuffers(n: GLsizei, ids: *const GLuint);
    fn glRenderbufferStorage(target: GLenum, internalformat: GLenum, w: GLsizei, h: GLsizei);
    fn glFramebufferRenderbuffer(
        target: GLenum,
        attachment: GLenum,
        rbtarget: GLenum,
        renderbuffer: GLuint,
    );
    fn glCheckFramebufferStatus(target: GLenum) -> GLenum;
    fn glGenTextures(n: GLsizei, ids: *mut GLuint);
    fn glBindTexture(target: GLenum, texture: GLuint);
    fn glDeleteTextures(n: GLsizei, ids: *const GLuint);
    fn glTexImage2D(
        target: GLenum,
        level: GLint,
        internalformat: GLint,
        width: GLsizei,
        height: GLsizei,
        border: GLint,
        format: GLenum,
        type_: GLenum,
        pixels: *const c_void,
    );
    fn glTexParameteri(target: GLenum, pname: GLenum, param: GLint);
    fn glReadBuffer(mode: GLenum);
    fn glPixelStorei(pname: GLenum, param: GLint);
    fn glReadPixels(
        x: GLint,
        y: GLint,
        width: GLsizei,
        height: GLsizei,
        format: GLenum,
        type_: GLenum,
        pixels: *mut c_void,
    );
    fn glFinish();

    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// The default framebuffer size until the core reports its own. N64 cores
/// default to 640×480 (`43screensize`), and the `video_refresh` size is used to
/// rebuild the FBO if a game disagrees.
pub const DEFAULT_WIDTH: u32 = 640;
pub const DEFAULT_HEIGHT: u32 = 480;

/// An offscreen CGL context plus the framebuffer object a core renders into.
///
/// Not `Send`: it is created on the app's thread and only ever used there.
pub struct GlContext {
    context: *mut c_void,
    pixel_format: *mut c_void,
    fbo: GLuint,
    color: GLuint,
    depth: GLuint,
    width: u32,
    height: u32,
    /// Whether the core's framebuffer is bottom-left origin (GL-native), which
    /// means a read-back must be flipped to reach the top-down `Frame` layout.
    flip: bool,
}

impl GlContext {
    /// Create the context and a complete FBO of `width`×`height`.
    ///
    /// `depth`/`stencil` attach renderbuffers (Mupen asks for depth, not
    /// stencil). `flip` records the core's `bottom_left_origin`.
    pub fn new(
        width: u32,
        height: u32,
        depth: bool,
        stencil: bool,
        flip: bool,
    ) -> Result<Self, String> {
        let width = width.max(1);
        let height = height.max(1);

        let mut attrs: Vec<u32> = vec![
            K_CGL_PFA_OPENGL_PROFILE,
            K_CGL_OGL_VERSION_GL4_CORE,
            K_CGL_PFA_COLOR_SIZE,
            24,
            K_CGL_PFA_ALPHA_SIZE,
            8,
        ];
        if depth || stencil {
            attrs.push(K_CGL_PFA_DEPTH_SIZE);
            attrs.push(24);
        }
        if stencil {
            attrs.push(K_CGL_PFA_STENCIL_SIZE);
            attrs.push(8);
        }
        attrs.push(K_CGL_PFA_ACCELERATED);
        attrs.push(K_CGL_PFA_CLOSEST_POLICY);
        attrs.push(0); // terminator

        let mut pixel_format: *mut c_void = ptr::null_mut();
        let mut npix: c_int = 0;
        let err = unsafe { CGLChoosePixelFormat(attrs.as_ptr(), &mut pixel_format, &mut npix) };
        if err != K_CGL_NO_ERROR || pixel_format.is_null() {
            return Err(format!("CGLChoosePixelFormat failed ({err})"));
        }

        let mut context: *mut c_void = ptr::null_mut();
        let err = unsafe { CGLCreateContext(pixel_format, ptr::null_mut(), &mut context) };
        if err != K_CGL_NO_ERROR || context.is_null() {
            unsafe { CGLDestroyPixelFormat(pixel_format) };
            return Err(format!("CGLCreateContext failed ({err})"));
        }

        unsafe { CGLSetCurrentContext(context) };

        let mut gl = GlContext {
            context,
            pixel_format,
            fbo: 0,
            color: 0,
            depth: 0,
            width,
            height,
            flip,
        };
        gl.build_fbo(depth, stencil)?;
        Ok(gl)
    }

    /// Build (or rebuild) the FBO and its attachments at the current size.
    fn build_fbo(&mut self, depth: bool, stencil: bool) -> Result<(), String> {
        unsafe {
            glGenFramebuffers(1, &mut self.fbo);
            glGenTextures(1, &mut self.color);
            glBindFramebuffer(GL_FRAMEBUFFER, self.fbo);

            glBindTexture(GL_TEXTURE_2D, self.color);
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGBA8,
                self.width as GLsizei,
                self.height as GLsizei,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                ptr::null(),
            );
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
            glFramebufferTexture2D(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                self.color,
                0,
            );

            if depth || stencil {
                glGenRenderbuffers(1, &mut self.depth);
                glBindRenderbuffer(GL_RENDERBUFFER, self.depth);
                if stencil {
                    glRenderbufferStorage(
                        GL_RENDERBUFFER,
                        GL_DEPTH24_STENCIL8,
                        self.width as GLsizei,
                        self.height as GLsizei,
                    );
                    glFramebufferRenderbuffer(
                        GL_FRAMEBUFFER,
                        GL_DEPTH_STENCIL_ATTACHMENT,
                        GL_RENDERBUFFER,
                        self.depth,
                    );
                } else {
                    glRenderbufferStorage(
                        GL_RENDERBUFFER,
                        GL_DEPTH_COMPONENT24,
                        self.width as GLsizei,
                        self.height as GLsizei,
                    );
                    glFramebufferRenderbuffer(
                        GL_FRAMEBUFFER,
                        GL_DEPTH_ATTACHMENT,
                        GL_RENDERBUFFER,
                        self.depth,
                    );
                }
            }

            let status = glCheckFramebufferStatus(GL_FRAMEBUFFER);
            if status != GL_FRAMEBUFFER_COMPLETE {
                return Err(format!("incomplete GL framebuffer (0x{status:x})"));
            }
        }
        Ok(())
    }

    /// Rebuild the FBO if the core reports a different frame size.
    fn resize(
        &mut self,
        width: u32,
        height: u32,
        depth: bool,
        stencil: bool,
    ) -> Result<(), String> {
        if self.width == width && self.height == height {
            return Ok(());
        }
        unsafe {
            if self.fbo != 0 {
                glDeleteFramebuffers(1, &self.fbo);
                self.fbo = 0;
            }
            if self.color != 0 {
                glDeleteTextures(1, &self.color);
                self.color = 0;
            }
            if self.depth != 0 {
                glDeleteRenderbuffers(1, &self.depth);
                self.depth = 0;
            }
        }
        self.width = width.max(1);
        self.height = height.max(1);
        self.build_fbo(depth, stencil)
    }

    /// The GL framebuffer object a core should render into.
    pub fn framebuffer(&self) -> u32 {
        self.fbo
    }

    /// Make this context current on the calling thread.
    pub fn make_current(&self) -> Result<(), String> {
        let err = unsafe { CGLSetCurrentContext(self.context) };
        if err != K_CGL_NO_ERROR {
            return Err(format!("CGLSetCurrentContext failed ({err})"));
        }
        Ok(())
    }

    /// Read the framebuffer back as tightly packed, top-down RGBA8.
    ///
    /// `width`/`height` are what the core reported this frame; a mismatch
    /// rebuilds the FBO first (the frame in flight is lost, which only happens
    /// if a game changes resolution). Alpha is forced opaque: the N64 has no
    /// alpha channel and a cleared FBO would otherwise composite as
    /// transparent.
    pub fn read_frame(
        &mut self,
        width: u32,
        height: u32,
        depth: bool,
        stencil: bool,
    ) -> Result<Vec<u8>, String> {
        self.resize(width, height, depth, stencil)?;
        let w = self.width as usize;
        let h = self.height as usize;
        let mut rgba = vec![0u8; w * h * 4];
        unsafe {
            glBindFramebuffer(GL_READ_FRAMEBUFFER, self.fbo);
            glReadBuffer(GL_COLOR_ATTACHMENT0);
            glPixelStorei(GL_PACK_ALIGNMENT, 1);
            glReadPixels(
                0,
                0,
                self.width as GLsizei,
                self.height as GLsizei,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                rgba.as_mut_ptr() as *mut c_void,
            );
            glFinish();
        }
        if self.flip {
            flip_rows(&mut rgba, w, h);
        }
        force_opaque_alpha(&mut rgba);
        Ok(rgba)
    }
}

// SAFETY: `GlContext` holds raw CGL/GL pointers, which make it `!Send` by
// default. Every GL call is issued from the app's main thread — the one that
// creates the context, runs `retro_run` and calls `read_frame` — and the value
// is otherwise only touched under `HostShared::gl`'s mutex. Declaring it `Send`
// keeps `HostShared` in an `Arc` without changing any access pattern.
unsafe impl Send for GlContext {}

impl Drop for GlContext {
    fn drop(&mut self) {
        unsafe {
            // Only touch GL if this context is current on this thread.
            if CGLGetCurrentContext() == self.context {
                if self.fbo != 0 {
                    glDeleteFramebuffers(1, &self.fbo);
                }
                if self.color != 0 {
                    glDeleteTextures(1, &self.color);
                }
                if self.depth != 0 {
                    glDeleteRenderbuffers(1, &self.depth);
                }
                CGLSetCurrentContext(ptr::null_mut());
            }
            CGLDestroyContext(self.context);
            CGLDestroyPixelFormat(self.pixel_format);
        }
    }
}

/// Resolve a GL symbol by name for a core's `get_proc_address`.
pub fn proc_address(name: &CStr) -> *mut c_void {
    unsafe { dlsym(RTLD_DEFAULT, name.as_ptr()) }
}

/// Reverse the order of the `h` rows of `w`-wide RGBA8 `pixels`.
fn flip_rows(pixels: &mut [u8], w: usize, h: usize) {
    let stride = w * 4;
    for y in 0..h.div_ceil(2) {
        let a = y * stride;
        let b = (h - 1 - y) * stride;
        if a == b {
            break;
        }
        let (head, tail) = pixels.split_at_mut(b);
        let top = &mut head[a..a + stride];
        let bottom = &mut tail[..stride];
        for (a, b) in top.iter_mut().zip(bottom.iter_mut()) {
            std::mem::swap(a, b);
        }
    }
}

/// Set every pixel's alpha byte to `255` (in place).
fn force_opaque_alpha(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        px[3] = 255;
    }
}

/// The real `glBindFramebuffer`, for the front end's interposer.
///
/// # Safety
///
/// Must be called with this context current, like any GL entry point.
pub unsafe fn bind_framebuffer(target: u32, framebuffer: u32) {
    glBindFramebuffer(target, framebuffer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flipping_reverses_the_row_order() {
        // A 2×2 RGBA block; rows are [1,2] then [3,4].
        let mut pixels = vec![
            1u8, 1, 1, 1, 2, 2, 2, 2, //
            3, 3, 3, 3, 4, 4, 4, 4,
        ];
        flip_rows(&mut pixels, 2, 2);
        assert_eq!(&pixels[0..4], &[3, 3, 3, 3]);
        assert_eq!(&pixels[4..8], &[4, 4, 4, 4]);
        assert_eq!(&pixels[8..12], &[1, 1, 1, 1]);
        assert_eq!(&pixels[12..16], &[2, 2, 2, 2]);
    }

    #[test]
    fn alpha_is_forced_opaque() {
        let mut pixels = vec![10, 20, 30, 0, 40, 50, 60, 7];
        force_opaque_alpha(&mut pixels);
        assert_eq!(pixels, [10, 20, 30, 255, 40, 50, 60, 255]);
    }

    /// A real offscreen CGL context and FBO, read back once. This is the GL
    /// wiring itself, not a screenshot: it asserts sizes and alpha only.
    #[test]
    fn an_offscreen_context_reads_a_frame_back() {
        let mut context = GlContext::new(64, 48, true, false, true).expect("CGL context");
        assert_ne!(context.framebuffer(), 0);
        let rgba = context.read_frame(64, 48, true, false).expect("read back");
        assert_eq!(rgba.len(), 64 * 48 * 4);
        // A blank frame is black and our read-back makes it opaque.
        assert!(rgba.chunks_exact(4).all(|px| px[3] == 255));
    }
}
