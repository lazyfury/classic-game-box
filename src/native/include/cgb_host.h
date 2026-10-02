/*
 * cgb_host.h — C ABI over the embedded native host (`cgb-app`).
 *
 * Both shells speak this one ABI. The shell owns the window and translates its
 * native events; Rust owns the UI, the renderer and the emulator. Nothing about
 * the UI crosses this boundary — the shell never paints it.
 *
 * Platform handles: `cgb_host_start` takes a `CAMetalLayer *` on macOS and an
 * `HWND` on Windows. Key codes are the shell's own (`NSEvent.keyCode` /
 * Win32 virtual key).
 */
#ifndef CGB_HOST_H
#define CGB_HOST_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque embedded app. */
typedef struct CgbHostApp CgbHostApp;

/*
 * Start the app rendering into `handle`, sized in physical pixels.
 * `library_dir` and `rom` may be NULL. Returns NULL when `handle` is NULL or
 * the GPU backend could not be created.
 */
CgbHostApp *cgb_host_start(void *handle, uint32_t width, uint32_t height, double scale,
                           const char *library_dir, const char *rom);

/* Tear the app down. */
void cgb_host_destroy(CgbHostApp *app);

/* Run one frame: update, lay out, paint and present. */
void cgb_host_frame(CgbHostApp *app);

/*
 * Whether the app wants another frame (a running game, an animation, a
 * download). A host may skip `cgb_host_frame` while this is false, but must
 * still present once after any input or resize.
 */
bool cgb_host_needs_frame(const CgbHostApp *app);

/*
 * The pending fullscreen request: 1 enter, 0 leave, -1 none. The app parks a
 * request; the shell applies it with its platform's own fullscreen transition.
 */
int32_t cgb_host_take_fullscreen(CgbHostApp *app);

/*
 * The cursor the UI wants: an igui_core::Cursor discriminant (0 default,
 * 1 pointer, 2 text, 3 col-resize, 4 row-resize, 5 grab, 6 grabbing).
 */
uint32_t cgb_host_cursor(const CgbHostApp *app);

/*
 * The focused text caret in logical viewport points (origin top-left), for
 * placing the IME candidate window. Returns false when there is none.
 */
bool cgb_host_caret(const CgbHostApp *app, float *out_x, float *out_y,
                    float *out_width, float *out_height);

/* -------------------------------------------------------------------------
 * Gamepad. libretro joypad ids are mirrored here so the shell never hardcodes
 * them. The shell reports each device's raw state by its own slot index; the
 * app maps slots to libretro ports.
 * ------------------------------------------------------------------------- */

enum {
    CGB_JOYPAD_B = 0,
    CGB_JOYPAD_Y = 1,
    CGB_JOYPAD_SELECT = 2,
    CGB_JOYPAD_START = 3,
    CGB_JOYPAD_UP = 4,
    CGB_JOYPAD_DOWN = 5,
    CGB_JOYPAD_LEFT = 6,
    CGB_JOYPAD_RIGHT = 7,
    CGB_JOYPAD_A = 8,
    CGB_JOYPAD_X = 9,
    CGB_JOYPAD_L = 10,
    CGB_JOYPAD_R = 11,
    CGB_JOYPAD_L2 = 12,
    CGB_JOYPAD_R2 = 13,
    CGB_JOYPAD_L3 = 14,
    CGB_JOYPAD_R3 = 15
};

/* Declare or update a gamepad device slot. `slot` is the shell's own index
 * (connection order); `name` is the display label; `connected` false clears it. */
void cgb_host_gamepad_device(CgbHostApp *app, uint32_t slot, const char *name, bool connected);

/* Replace one device slot's state. `buttons`: bit i = CGB_JOYPAD_* i.
 * Axes are -32768..32767, libretro convention (Y positive is down). */
void cgb_host_gamepad_state(CgbHostApp *app, uint32_t slot, uint32_t buttons,
                            int16_t left_x, int16_t left_y,
                            int16_t right_x, int16_t right_y);

/* Resize the drawable (physical pixels) and update the backing scale. */
void cgb_host_resize(CgbHostApp *app, uint32_t width, uint32_t height, double scale);

/* Queue a file dropped on the window; it is imported on the next frame. */
void cgb_host_dropped_file(CgbHostApp *app, const char *path);

/* -------------------------------------------------------------------------
 * Native events. Coordinates are logical viewport points, origin top-left.
 * Modifier bits: 1 shift, 2 ctrl, 4 alt, 8 command/meta.
 * Pointer button tags: 0 left, 1 right, 2 middle.
 * ------------------------------------------------------------------------- */

void cgb_host_pointer_move(CgbHostApp *app, float x, float y);
void cgb_host_pointer_down(CgbHostApp *app, float x, float y, uint32_t button, uint32_t click_count);
void cgb_host_pointer_up(CgbHostApp *app, float x, float y, uint32_t button);
void cgb_host_pointer_leave(CgbHostApp *app);
void cgb_host_scroll(CgbHostApp *app, float x, float y, float dx, float dy);

/* `key_code` is the shell's own key code (AppKit keyCode / Win32 virtual key);
 * `characters` is the text the key produced with modifiers ignored (may be
 * NULL). */
void cgb_host_key_down(CgbHostApp *app, uint32_t key_code, const char *characters, uint32_t modifiers);
void cgb_host_key_up(CgbHostApp *app, uint32_t key_code, const char *characters);
void cgb_host_text(CgbHostApp *app, const char *utf8);
void cgb_host_modifiers(CgbHostApp *app, uint32_t bits);

/* Input-method event. kind: 0 enabled, 1 disabled, 2 preedit, 3 commit.
 * For preedit, sel_start / sel_end are byte offsets, or -1 for none. */
void cgb_host_ime(CgbHostApp *app, uint32_t kind, const char *text, int32_t sel_start, int32_t sel_end);

#ifdef __cplusplus
}
#endif

#endif /* CGB_HOST_H */
