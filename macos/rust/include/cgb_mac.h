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
