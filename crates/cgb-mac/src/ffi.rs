//! The C ABI Swift calls. See `include/cgb_mac.h` for the mirror.

use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr};
use std::path::PathBuf;
use std::rc::Rc;

use cgb_app::app::{App as CgbApp, SharedHostWindow};
use cgb_app::cli::Args;
use cgb_input::GamepadSnapshot;
use igui::igui_app::{App as IguiApp, AppConfig, PlatformEvent};
use igui::igui_core::{Cursor, ImeEvent, Vec2};

use crate::host::{
    MacClipboardPlugin, MacGamepadPlugin, MacGpu, MacGpuPlugin, MacHostWindow, MacSurface,
    MacTextMeasurePlugin,
};
use crate::input::{key_from_code, modifiers_from_bits, pointer_button, MacEvent, MacInputPlugin};

/// A running embedded app. Opaque to C (`CgbMacApp`).
pub struct CgbMacApp {
    app: IguiApp,
    gpu: MacGpu,
    host_window: MacHostWindow,
    gamepad: Rc<RefCell<GamepadSnapshot>>,
    drops: Rc<RefCell<Vec<PathBuf>>>,
}

impl CgbMacApp {
    /// Route one native event through the runtime into the UI.
    fn emit(&mut self, event: &MacEvent) {
        self.app.platform_event(PlatformEvent::new(event));
    }
}

/// Read an optional UTF-8 string.
///
/// # Safety
///
/// `ptr` must be NULL or a valid NUL-terminated string.
unsafe fn opt_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees a valid NUL-terminated string.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// Read an optional UTF-8 path.
///
/// # Safety
///
/// `ptr` must be NULL or a valid NUL-terminated string.
unsafe fn opt_path(ptr: *const c_char) -> Option<PathBuf> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees a valid NUL-terminated string.
    let text = unsafe { CStr::from_ptr(ptr) }.to_str().ok()?;
    if text.is_empty() {
        None
    } else {
        Some(PathBuf::from(text))
    }
}

/// Start the app rendering into `layer` (a `CAMetalLayer*`), sized in physical
/// pixels. `library_dir` and `rom` may be NULL.
///
/// Returns NULL only on a null layer; the runtime is brought up here (the GPU
/// surface is created on resume).
///
/// # Safety
///
/// `layer` must be a live `CAMetalLayer` that outlives the returned app;
/// `library_dir` / `rom` must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_start(
    layer: *mut c_void,
    width: u32,
    height: u32,
    scale: f64,
    library_dir: *const c_char,
    rom: *const c_char,
) -> *mut CgbMacApp {
    if layer.is_null() {
        eprintln!("cgb-mac: layer 为空");
        return std::ptr::null_mut();
    }

    // SAFETY: the caller guarantees the pointers are NULL or valid strings.
    let args = unsafe {
        Args {
            library_dir: opt_path(library_dir),
            rom: opt_path(rom),
            ..Default::default()
        }
    };

    let logic = CgbApp::new(args);
    let drops = logic.drop_sink();
    let (gpu_plugin, gpu) = MacGpuPlugin::new();
    let host_window = MacHostWindow::default();
    let (gamepad_plugin, gamepad) = MacGamepadPlugin::new();

    let mut builder = IguiApp::new(AppConfig {
        title: "Classic Game Box".to_string(),
        size: (1100.0, 760.0),
        ..Default::default()
    })
    .plugin(gpu_plugin)
    .plugin(MacTextMeasurePlugin)
    .plugin(MacClipboardPlugin)
    .plugin(MacInputPlugin)
    .plugin(gamepad_plugin)
    .logic(logic);
    builder.insert_service(MacSurface {
        layer,
        width,
        height,
        scale,
    });
    let host: SharedHostWindow = Rc::new(host_window.clone());
    builder.insert_service(host);

    let mut app = builder.build();
    app.resumed();

    // A missing backend means the Metal surface or wgpu device could not be
    // created; surface it as a start failure so Swift can tell the user.
    if !gpu.is_ready() {
        eprintln!("cgb-mac: GPU 初始化失败（Metal surface / wgpu backend）");
        return std::ptr::null_mut();
    }

    Box::into_raw(Box::new(CgbMacApp {
        app,
        gpu,
        host_window,
        gamepad,
        drops,
    }))
}

/// Tear the app down.
///
/// # Safety
///
/// `app` must come from `cgb_mac_start` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_destroy(app: *mut CgbMacApp) {
    if app.is_null() {
        return;
    }
    // SAFETY: the caller guarantees an owned, not-yet-freed pointer.
    drop(unsafe { Box::from_raw(app) });
}

/// Run one frame: update, lay out, paint and present.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_frame(app: *mut CgbMacApp) {
    if let Some(app) = unsafe { app.as_mut() } {
        app.app.frame();
    }
}

/// Whether the app wants another frame (a running game, an animation, a
/// download). A host may skip `cgb_mac_frame` while this is false — but must
/// still present once after any input or resize.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_needs_frame(app: *const CgbMacApp) -> bool {
    unsafe { app.as_ref() }.is_some_and(|app| app.app.needs_frame())
}

/// The pending fullscreen request: `1` enter, `0` leave, `-1` none.
///
/// The app parks a request in `HostWindow::set_fullscreen`; Swift applies it
/// with AppKit's own `toggleFullScreen:` animation.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_take_fullscreen(app: *mut CgbMacApp) -> i32 {
    match unsafe { app.as_ref() }.and_then(|app| app.host_window.take_request()) {
        Some(true) => 1,
        Some(false) => 0,
        None => -1,
    }
}

/// The cursor the UI wants (an `igui_core::Cursor` discriminant), or `0`
/// (default arrow) when the app is not running.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_cursor(app: *const CgbMacApp) -> u32 {
    unsafe { app.as_ref() }
        .and_then(|app| app.app.cursor())
        .map_or(0, cursor_code)
}

/// Map a core cursor to its ABI discriminant (the order of `Cursor`'s
/// variants).
fn cursor_code(cursor: Cursor) -> u32 {
    match cursor {
        Cursor::Default => 0,
        Cursor::Pointer => 1,
        Cursor::Text => 2,
        Cursor::ColResize => 3,
        Cursor::RowResize => 4,
        Cursor::Grab => 5,
        Cursor::Grabbing => 6,
    }
}

/// The focused text caret in logical viewport points (origin top-left), for
/// placing the IME candidate window. Returns false when there is none.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`; the out pointers must be
/// writable `f32`s or NULL.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_caret(
    app: *const CgbMacApp,
    out_x: *mut f32,
    out_y: *mut f32,
    out_width: *mut f32,
    out_height: *mut f32,
) -> bool {
    let Some(rect) = (unsafe { app.as_ref() }).and_then(|app| app.app.caret()) else {
        return false;
    };
    if !out_x.is_null() {
        unsafe { *out_x = rect.left() };
    }
    if !out_y.is_null() {
        unsafe { *out_y = rect.top() };
    }
    if !out_width.is_null() {
        unsafe { *out_width = rect.size.width };
    }
    if !out_height.is_null() {
        unsafe { *out_height = rect.size.height };
    }
    true
}

// ---------------------------------------------------------------------------
// Gamepad (Swift's GameController)
// ---------------------------------------------------------------------------

/// Replace one port's gamepad snapshot. `buttons`: bit i = `CGB_JOYPAD_*` i.
/// Axes are `-32768..32767`, libretro's convention (Y positive is down).
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_gamepad_state(
    app: *mut CgbMacApp,
    port: u32,
    buttons: u32,
    left_x: i16,
    left_y: i16,
    right_x: i16,
    right_y: i16,
) {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return;
    };
    let mut snapshot = app.gamepad.borrow_mut();
    if let Some(slot) = snapshot.buttons.get_mut(port as usize) {
        *slot = buttons as u16;
    }
    if let Some(sticks) = snapshot.analog.get_mut(port as usize) {
        sticks[0] = [left_x, left_y];
        sticks[1] = [right_x, right_y];
    }
}

/// Mark a gamepad port connected or disconnected; disconnecting clears it.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_gamepad_connected(
    app: *mut CgbMacApp,
    port: u32,
    connected: bool,
) {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return;
    };
    app.gamepad.borrow_mut().connect(port as usize, connected);
}

/// Resize the drawable (physical pixels) and update the backing scale.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_resize(app: *mut CgbMacApp, width: u32, height: u32, scale: f64) {
    if let Some(app) = unsafe { app.as_mut() } {
        app.gpu.resize(width, height, scale);
    }
}

/// Queue a file dropped on the window; it is imported on the next frame.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`; `path` a valid C string.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_dropped_file(app: *mut CgbMacApp, path: *const c_char) {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return;
    };
    // SAFETY: the caller guarantees a valid NUL-terminated string.
    if let Some(path) = unsafe { opt_path(path) } {
        app.drops.borrow_mut().push(path);
    }
}

// ---------------------------------------------------------------------------
// Pointer / wheel
// ---------------------------------------------------------------------------

/// A pointer move, in logical viewport points (origin top-left).
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_pointer_move(app: *mut CgbMacApp, x: f32, y: f32) {
    if let Some(app) = unsafe { app.as_mut() } {
        app.emit(&MacEvent::PointerMove(Vec2::new(x, y)));
    }
}

/// A pointer button press (`button`: 0 left, 1 right, 2 middle).
/// `click_count >= 2` also reports a double click.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_pointer_down(
    app: *mut CgbMacApp,
    x: f32,
    y: f32,
    button: u32,
    click_count: u32,
) {
    if let Some(app) = unsafe { app.as_mut() } {
        app.emit(&MacEvent::PointerDown {
            position: Vec2::new(x, y),
            button: pointer_button(button),
            click_count,
        });
    }
}

/// A pointer button release.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_pointer_up(app: *mut CgbMacApp, x: f32, y: f32, button: u32) {
    if let Some(app) = unsafe { app.as_mut() } {
        app.emit(&MacEvent::PointerUp {
            position: Vec2::new(x, y),
            button: pointer_button(button),
        });
    }
}

/// The pointer left the window.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_pointer_leave(app: *mut CgbMacApp) {
    if let Some(app) = unsafe { app.as_mut() } {
        app.emit(&MacEvent::PointerLeave);
    }
}

/// A scroll wheel / trackpad event. `dx` / `dy` are in logical pixels, `y > 0`
/// scrolls down.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_scroll(app: *mut CgbMacApp, x: f32, y: f32, dx: f32, dy: f32) {
    if let Some(app) = unsafe { app.as_mut() } {
        app.emit(&MacEvent::Wheel {
            position: Vec2::new(x, y),
            delta: Vec2::new(dx, dy),
        });
    }
}

// ---------------------------------------------------------------------------
// Keyboard / text / IME
// ---------------------------------------------------------------------------

/// A key press. `characters` is `charactersIgnoringModifiers` (may be NULL).
/// `modifiers`: 1 shift, 2 ctrl, 4 alt, 8 command.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`; `characters` a valid C
/// string or NULL.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_key_down(
    app: *mut CgbMacApp,
    key_code: u32,
    characters: *const c_char,
    modifiers: u32,
) {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return;
    };
    // Modifiers may change without a `flagsChanged` on the way in.
    app.emit(&MacEvent::Modifiers(modifiers_from_bits(modifiers)));
    let characters = unsafe { opt_string(characters) };
    if let Some(key) = key_from_code(key_code, characters.as_deref()) {
        app.emit(&MacEvent::KeyDown(key));
    }
}

/// A key release.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`; `characters` a valid C
/// string or NULL.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_key_up(
    app: *mut CgbMacApp,
    key_code: u32,
    characters: *const c_char,
) {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return;
    };
    let characters = unsafe { opt_string(characters) };
    if let Some(key) = key_from_code(key_code, characters.as_deref()) {
        app.emit(&MacEvent::KeyUp(key));
    }
}

/// Committed text (from `insertText:`).
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`; `utf8` a valid C string.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_text(app: *mut CgbMacApp, utf8: *const c_char) {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return;
    };
    if let Some(text) = unsafe { opt_string(utf8) } {
        if !text.is_empty() {
            app.emit(&MacEvent::Text(text));
        }
    }
}

/// A modifier-state change (`flagsChanged`). Same bits as `cgb_mac_key_down`.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_modifiers(app: *mut CgbMacApp, bits: u32) {
    if let Some(app) = unsafe { app.as_mut() } {
        app.emit(&MacEvent::Modifiers(modifiers_from_bits(bits)));
    }
}

/// An input-method event. `kind`: 0 enabled, 1 disabled, 2 preedit, 3 commit.
/// For preedit, `sel_start` / `sel_end` are byte offsets, or `-1` for none.
///
/// # Safety
///
/// `app` must be a live pointer from `cgb_mac_start`; `text` a valid C string
/// or NULL.
#[no_mangle]
pub unsafe extern "C" fn cgb_mac_ime(
    app: *mut CgbMacApp,
    kind: u32,
    text: *const c_char,
    sel_start: i32,
    sel_end: i32,
) {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return;
    };
    let text = unsafe { opt_string(text) };
    let event = match kind {
        0 => ImeEvent::Enabled,
        1 => ImeEvent::Disabled,
        2 => ImeEvent::Preedit {
            text: text.unwrap_or_default(),
            cursor: (sel_start >= 0).then(|| (sel_start as usize, sel_end.max(sel_start) as usize)),
        },
        3 => ImeEvent::Commit(text.unwrap_or_default()),
        _ => return,
    };
    app.emit(&MacEvent::Ime(event));
}
