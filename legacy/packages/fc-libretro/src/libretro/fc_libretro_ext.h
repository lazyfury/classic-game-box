#ifndef FC_LIBRETRO_EXT_H
#define FC_LIBRETRO_EXT_H

/* ---------------------------------------------------------------------------
 * The core's optional extension to the libretro ABI.
 *
 * libretro is the contract, and everything that fits in it goes there. This
 * header is for the things that do not:
 *
 *   * `retro_cheat_set` speaks Game Genie and Pro Action Replay strings, so
 *     there is nowhere to put the address/value pair this project's own front
 *     end works in
 *   * `retro_get_memory_data` hands over memory, but not a machine's program
 *     counter or its total cycle count
 *   * libretro has no way for a front end to ask a *core* a question -- the
 *     `environment` callback only goes the other way
 *
 * The last one is why this is an extra exported symbol rather than another
 * `RETRO_ENVIRONMENT_*`. A standard front end never looks for it, so it costs
 * nothing there; a front end that knows about it can ask, and one that does
 * not still gets a complete emulator.
 *
 * How to use it
 * -------------
 *     const fc_libretro_ext_v1* ext = fc_libretro_get_ext();
 *     if (ext != NULL && ext->abi_version == FC_LIBRETRO_EXT_VERSION) { ... }
 *
 * A front end must treat every field as optional and must check `struct_size`
 * before reading one, because a newer core may have appended fields to a
 * structure an older front end still has a smaller definition of.
 *
 * Rules for changing this ABI
 * ---------------------------
 * Fields are only ever appended, never removed, reordered or re-typed.
 * Appending bumps `FC_LIBRETRO_EXT_VERSION`, and a reader uses the smaller of
 * its own `sizeof` and the core's `struct_size` as the fields it may touch.
 * The point is that an old front end keeps working against a new core, which
 * is the same promise libretro itself makes.
 * ------------------------------------------------------------------------- */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define FC_LIBRETRO_EXT_VERSION 3u

typedef struct fc_libretro_ext_v1 {
    /** Equal to FC_LIBRETRO_EXT_VERSION at the time the core was built. */
    uint32_t abi_version;

    /** `sizeof(fc_libretro_ext_v1)` in the core. See the rules above. */
    uint32_t struct_size;

    /* -- debugging ----------------------------------------------------------
     *
     * `peek` reads console RAM and nothing else, and `poke` writes through
     * the bus so address decoding is not duplicated. Both are the same
     * operations src/ffi/emulator_api.h calls fc_peek and fc_poke, which is
     * deliberate: two front ends and one rule.
     */

    /** One byte of CPU address space, or 0. Never has a side effect. */
    int (*peek)(uint16_t address);

    /** Write one byte through the bus, now. */
    void (*poke)(uint16_t address, uint8_t value);

    /* -- cheats, in this project's own language ----------------------------
     *
     * `data` is `count` entries of four bytes each, little endian:
     *
     *     [0] address low   [1] address high   [2] value   [3] flags
     *
     * flags bit 0 = freeze (rewrite every frame), bit 1 = enabled. The whole
     * list replaces whatever was there, and a null pointer with a count of
     * zero empties it.
     *
     * Returns the number of cheats now held, or -1 if no cartridge is loaded.
     */

    int (*set_raw_cheats)(const uint8_t* data, int count);

    /** How many cheats the core is holding. */
    int (*raw_cheat_count)(void);

    /* -- what is in the slot ----------------------------------------------- */

    /** False when the loaded mapper does not save its bank registers. */
    bool (*mapper_saves_state)(void);

    /** A one line description of the cartridge, or "" when there is none. */
    const char* (*rom_summary)(void);

    /* -- diagnostics ------------------------------------------------------- */

    /** CPU cycles since power on. */
    uint64_t (*total_cycles)(void);

    /** The CPU's program counter, for a status line. */
    uint16_t (*cpu_pc)(void);

    /* -- audio, without the conversion libretro asks for --------------------
     *
     * libretro carries sound as interleaved signed 16-bit stereo, and this
     * core obeys that: `retro_run` calls the front end's audio callback with
     * exactly that. But the APU computes in 32-bit float, and a front end that
     * has always compared its output sample for sample -- this project's own,
     * through electron/verify.sh -- cannot round trip through int16 without
     * every byte changing.
     *
     * So the frame's samples are kept in their original form and handed out
     * here as well. It is the same audio, at full precision, for a front end
     * that asked for it by name. A standard front end never calls this and
     * still gets correct int16 through the normal callback.
     *
     * Copies up to `max` mono samples into `out` and returns how many. The
     * samples are drained: the next frame's call returns the next frame's.
     */
    size_t (*take_samples)(float* out, size_t max);

    /* -- why a load failed ------------------------------------------------
     *
     * libretro reports a failed retro_load_game with a bare false. A libretro
     * logger would carry the reason -- "mapper 176 is not implemented yet" --
     * but the log callback is a C variadic function and JavaScript cannot
     * build one, so on the wasm side the reason was lost and the player saw
     * only "the core did not accept this cartridge". This carries the text out
     * instead.
     *
     * Empty when the last load succeeded, or when nothing has been attempted.
     * A core without the extension falls back to the generic message.
     */
    const char* (*last_error)(void);
} fc_libretro_ext_v1;

/**
 * The core's extension table, or NULL for a core that does not have one.
 *
 * Always present in this project's core, and never looked for by RetroArch.
 */
const fc_libretro_ext_v1* fc_libretro_get_ext(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* FC_LIBRETRO_EXT_H */
