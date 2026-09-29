/* The libretro log callback is variadic:
 *
 *     void (*retro_log_printf_t)(enum retro_log_level level,
 *                                const char *fmt, ...);
 *
 * Rust cannot define C-variadic functions on stable, so this shim is registered
 * as the core's log callback directly: it formats the message with `vsnprintf`
 * and hands the finished line back to Rust (`cgb_log_emit`). Without it the
 * front end can only print the raw format string (e.g. `[%s] %s`).
 */
#include <stdarg.h>
#include <stdio.h>

extern void cgb_log_emit(unsigned level, const char *text);

void cgb_core_log(unsigned level, const char *fmt, ...) {
    char buffer[4096];
    va_list args;

    if (!fmt)
        return;

    va_start(args, fmt);
    vsnprintf(buffer, sizeof(buffer), fmt, args);
    va_end(args);

    cgb_log_emit(level, buffer);
}
