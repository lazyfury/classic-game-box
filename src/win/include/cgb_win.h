/*
 * cgb_win.h — C ABI over the embedded Windows host (`cgb-win`).
 *
 * The C++ Win32 shell owns the window and its `HWND`; the Rust side owns the
 * UI and the emulator. C++ hands over the window, asks for frames, and
 * forwards native messages. Nothing about the UI crosses this boundary — C++
 * never paints it.
 */
#ifndef CGB_WIN_H
#define CGB_WIN_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque embedded app. */
typedef struct CgbWinApp CgbWinApp;

/*
 * Start the app rendering into `hwnd` (a `HWND`), sized in physical pixels.
 * `library_dir` and `rom` may be NULL. Returns NULL only when `hwnd` is NULL
 * or the GPU backend could not be created.
 */
CgbWinApp *cgb_win_start(void *hwnd, uint32_t width, uint32_t height, double scale,
                         const char *library_dir, const char *rom);

/* Tear the app down. */
void cgb_win_destroy(CgbWinApp *app);

/* Run one frame: update, lay out, paint and present. */
void cgb_win_frame(CgbWinApp *app);

/*
 * Whether the app wants another frame (a running game, an animation, a
 * download). A host may skip `cgb_win_frame` while this is false, but must
 * still present once after any input or resize.
 */
bool cgb_win_needs_frame(const CgbWinApp *app);

/*
 * The pending fullscreen request: 1 enter, 0 leave, -1 none. The app parks a
 * request; the shell applies it by switching to borderless fullscreen on the
 * monitor rect.
 */
int32_t cgb_win_take_fullscreen(CgbWinApp *app);

/*
 * The cursor the UI wants: an igui_core::Cursor discriminant (0 default,
 * 1 pointer, 2 text, 3 col-resize, 4 row-resize, 5 grab, 6 grabbing).
 */
uint32_t cgb_win_cursor(const CgbWinApp *app);

/*
 * The focused text caret in logical viewport points (origin top-left), for
 * placing the IME candidate window. Returns false when there is none.
 */
bool cgb_win_caret(const CgbWinApp *app, float *out_x, float *out_y,
                   float *out_width, float *out_height);

/* -------------------------------------------------------------------------
 * Gamepad (the shell's XInput). libretro joypad ids are mirrored here so C++
 * never hardcodes them.
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
void cgb_win_gamepad_state(CgbWinApp *app, uint32_t port, uint32_t buttons,
                           int16_t left_x, int16_t left_y,
                           int16_t right_x, int16_t right_y);

/* Mark a gamepad port connected/disconnected; disconnect clears it. */
void cgb_win_gamepad_connected(CgbWinApp *app, uint32_t port, bool connected);

/* Resize the drawable (physical pixels) and update the backing scale. */
void cgb_win_resize(CgbWinApp *app, uint32_t width, uint32_t height, double scale);

/* Queue a file dropped on the window; it is imported on the next frame. */
void cgb_win_dropped_file(CgbWinApp *app, const char *path);

/* -------------------------------------------------------------------------
 * Native events. Coordinates are logical viewport points, origin top-left.
 * Modifier bits: 1 shift, 2 ctrl, 4 alt, 8 meta (the Windows key).
 * Pointer button tags: 0 left, 1 right, 2 middle.
 * ------------------------------------------------------------------------- */

void cgb_win_pointer_move(CgbWinApp *app, float x, float y);
void cgb_win_pointer_down(CgbWinApp *app, float x, float y, uint32_t button, uint32_t click_count);
void cgb_win_pointer_up(CgbWinApp *app, float x, float y, uint32_t button);
void cgb_win_pointer_leave(CgbWinApp *app);
void cgb_win_scroll(CgbWinApp *app, float x, float y, float dx, float dy);

/* `vk` is a Win32 virtual-key code; `characters` is the `ToUnicode` text the
 * key produced at WM_KEYDOWN time (may be NULL). */
void cgb_win_key_down(CgbWinApp *app, uint32_t vk, const char *characters, uint32_t modifiers);
void cgb_win_key_up(CgbWinApp *app, uint32_t vk, const char *characters);
void cgb_win_text(CgbWinApp *app, const char *utf8);
void cgb_win_modifiers(CgbWinApp *app, uint32_t bits);

/* Input-method event. kind: 0 enabled, 1 disabled, 2 preedit, 3 commit.
 * For preedit, sel_start / sel_end are byte offsets, or -1 for none. */
void cgb_win_ime(CgbWinApp *app, uint32_t kind, const char *text, int32_t sel_start, int32_t sel_end);

#ifdef __cplusplus
}
#endif

#endif /* CGB_WIN_H */
