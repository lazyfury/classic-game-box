/*
 * cgb_mac.h — C ABI over the embedded macOS host (`cgb-mac`).
 *
 * Swift owns the window and its `CAMetalLayer`; the Rust side owns the UI and
 * the emulator. Swift hands over the layer, asks for frames, and forwards
 * native events. Nothing about the UI crosses this boundary — Swift never
 * paints it.
 */
#ifndef CGB_MAC_H
#define CGB_MAC_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque embedded app. */
typedef struct CgbMacApp CgbMacApp;

/*
 * Start the app rendering into `layer` (a `CAMetalLayer *`), sized in physical
 * pixels. `library_dir` and `rom` may be NULL. Returns NULL only when `layer`
 * is NULL.
 */
CgbMacApp *cgb_mac_start(void *layer, uint32_t width, uint32_t height, double scale,
                         const char *library_dir, const char *rom);

/* Tear the app down. */
void cgb_mac_destroy(CgbMacApp *app);

/* Run one frame: update, lay out, paint and present. */
void cgb_mac_frame(CgbMacApp *app);

/*
 * Whether the app wants another frame (a running game, an animation, a
 * download). A host may skip `cgb_mac_frame` while this is false, but must
 * still present once after any input or resize.
 */
bool cgb_mac_needs_frame(const CgbMacApp *app);

/*
 * The pending fullscreen request: 1 enter, 0 leave, -1 none. The app parks a
 * request; Swift applies it with AppKit's own `toggleFullScreen:` animation.
 */
int32_t cgb_mac_take_fullscreen(CgbMacApp *app);

/*
 * The cursor the UI wants: an igui_core::Cursor discriminant (0 default,
 * 1 pointer, 2 text, 3 col-resize, 4 row-resize, 5 grab, 6 grabbing).
 */
uint32_t cgb_mac_cursor(const CgbMacApp *app);

/*
 * The focused text caret in logical viewport points (origin top-left), for
 * placing the IME candidate window. Returns false when there is none.
 */
bool cgb_mac_caret(const CgbMacApp *app, float *out_x, float *out_y,
                   float *out_width, float *out_height);

/* -------------------------------------------------------------------------
 * Gamepad (Swift's GameController). libretro joypad ids are mirrored here so
 * Swift never hardcodes them.
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

/* Replace one port's gamepad snapshot. `buttons`: bit i = CGB_JOYPAD_* i.
 * Axes are -32768..32767, libretro convention (Y positive is down). */
void cgb_mac_gamepad_state(CgbMacApp *app, uint32_t port, uint32_t buttons,
                           int16_t left_x, int16_t left_y,
                           int16_t right_x, int16_t right_y);

/* Mark a gamepad port connected/disconnected; disconnect clears it. */
void cgb_mac_gamepad_connected(CgbMacApp *app, uint32_t port, bool connected);

/* Resize the drawable (physical pixels) and update the backing scale. */
void cgb_mac_resize(CgbMacApp *app, uint32_t width, uint32_t height, double scale);

/* Queue a file dropped on the window; it is imported on the next frame. */
void cgb_mac_dropped_file(CgbMacApp *app, const char *path);

/* -------------------------------------------------------------------------
 * Native events. Coordinates are logical viewport points, origin top-left.
 * Modifier bits: 1 shift, 2 ctrl, 4 alt, 8 command.
 * Pointer button tags: 0 left, 1 right, 2 middle.
 * ------------------------------------------------------------------------- */

void cgb_mac_pointer_move(CgbMacApp *app, float x, float y);
void cgb_mac_pointer_down(CgbMacApp *app, float x, float y, uint32_t button, uint32_t click_count);
void cgb_mac_pointer_up(CgbMacApp *app, float x, float y, uint32_t button);
void cgb_mac_pointer_leave(CgbMacApp *app);
void cgb_mac_scroll(CgbMacApp *app, float x, float y, float dx, float dy);

void cgb_mac_key_down(CgbMacApp *app, uint32_t key_code, const char *characters, uint32_t modifiers);
void cgb_mac_key_up(CgbMacApp *app, uint32_t key_code, const char *characters);
void cgb_mac_text(CgbMacApp *app, const char *utf8);
void cgb_mac_modifiers(CgbMacApp *app, uint32_t bits);

/* Input-method event. kind: 0 enabled, 1 disabled, 2 preedit, 3 commit.
 * For preedit, sel_start / sel_end are byte offsets, or -1 for none. */
void cgb_mac_ime(CgbMacApp *app, uint32_t kind, const char *text, int32_t sel_start, int32_t sel_end);

#ifdef __cplusplus
}
#endif

#endif /* CGB_MAC_H */
