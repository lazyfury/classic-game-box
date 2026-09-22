#ifndef FC_EMULATOR_API_H
#define FC_EMULATOR_API_H

/* ---------------------------------------------------------------------------
 * The emulator's public interface, in plain C.
 *
 * Why C and not C++
 * -----------------
 * Swift cannot call C++ directly. It can call C, through a bridging header,
 * with no runtime support and no name mangling to worry about. So this file is
 * the one place where the two languages meet, and it is deliberately the
 * dumbest file in the project: no classes, no templates, no exceptions, no
 * allocation the caller has to think about beyond create and destroy.
 *
 * This is also the boundary that enforces the architecture rule from
 * AGENTS.md: the core does not know a window exists. Everything below is
 * data going one way and button presses coming back.
 *
 *       Metal view  ---> framebuffer (a pointer to 256x240 uint32)
 *       Audio out   ---> samples (44100 Hz float)
 *       Keyboard    <--- set_button
 *
 * Ownership
 * ---------
 *   fc_create  allocates.  fc_destroy  frees.  fc_save_state allocates a
 *   buffer that fc_free_state releases.  Nothing else allocates anything the
 *   caller has to think about.
 *
 *   fc_framebuffer returns a pointer INTO the machine. It stays valid until
 *   the machine is destroyed, and its contents change every frame.
 * ------------------------------------------------------------------------- */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* An opaque handle. The caller never sees what is inside. */
typedef struct fc_machine fc_machine;

/* The screen is always this size. The NES has no other video mode. */
#define FC_SCREEN_WIDTH 256
#define FC_SCREEN_HEIGHT 240

/* One button. The values match nes::Controller::Button. */
typedef enum fc_button {
    FC_BUTTON_A = 0,
    FC_BUTTON_B = 1,
    FC_BUTTON_SELECT = 2,
    FC_BUTTON_START = 3,
    FC_BUTTON_UP = 4,
    FC_BUTTON_DOWN = 5,
    FC_BUTTON_LEFT = 6,
    FC_BUTTON_RIGHT = 7
} fc_button;

/* -- lifecycle ------------------------------------------------------------ */

/* Create a machine with no cartridge. Never returns NULL. */
fc_machine* fc_create(void);

/* Free a machine. Passing NULL is allowed and does nothing. */
void fc_destroy(fc_machine* machine);

/* -- loading -------------------------------------------------------------- */

/* Load a .nes image. Returns false and sets the error string on failure. */
bool fc_load_rom(fc_machine* machine, const uint8_t* data, size_t size);

/* Why the last operation failed. Never NULL. */
const char* fc_last_error(const fc_machine* machine);

/* A one line description of the loaded cartridge, or "" if there is none. */
const char* fc_rom_summary(const fc_machine* machine);

/* -- running -------------------------------------------------------------- */

void fc_reset(fc_machine* machine);

/* Run until the PPU finishes a frame.
 *
 * Returns false if the CPU halted, which means the emulator hit an opcode it
 * does not implement. That is a bug here, not in the game. */
bool fc_run_frame(fc_machine* machine);

/* True once the CPU has halted. Every call after that does nothing. */
bool fc_is_halted(const fc_machine* machine);

/* Frames completed since power on. */
uint32_t fc_frame_count(const fc_machine* machine);

/* -- video ---------------------------------------------------------------- */

/* 256 * 240 pixels, 0x00RRGGBB, top row first. The pointer is owned by the
 * machine and is rewritten by fc_run_frame. */
const uint32_t* fc_framebuffer(const fc_machine* machine);

/* Pixel at (x, y), or 0 if the coordinates are off screen. */
uint32_t fc_pixel(const fc_machine* machine, int x, int y);

/* -- audio ---------------------------------------------------------------- */

/* The APU produces this many samples per second. */
int fc_sample_rate(void);

/* Copy up to `max_samples` mono samples into `out`, and remove them from the
 * machine's queue. Returns the number written.
 *
 * Written to be safe to call from an audio callback: no allocation, no locks,
 * and it returns quickly even when there is nothing to give. */
size_t fc_take_samples(fc_machine* machine, float* out, size_t max_samples);

/* Samples waiting right now. */
size_t fc_samples_pending(const fc_machine* machine);

/* Drop everything queued. Useful after a pause. */
void fc_clear_samples(fc_machine* machine);

/* -- the lock free sample queue ------------------------------------------- */

/* Moving samples from the thread that runs the machine to the real-time
 * audio callback is the one place a front end must not use a mutex. The
 * callback is not allowed to wait for anything, and the emulator thread can
 * be busy for a whole frame; if they share a lock, the callback misses its
 * deadline and the speaker crackles.
 *
 * This is a single producer / single consumer ring buffer built on
 * release/acquire atomics. The producer fills and the consumer drains; only
 * the indices are shared, and neither side ever blocks. If the producer
 * overruns, the newest samples are dropped rather than corrupting the reads
 * the consumer is in the middle of.
 *
 * Create it once at start up. Push from the emulator thread. Pop from the
 * audio callback. It is not safe to have two producers or two consumers. */
typedef struct fc_audio_queue fc_audio_queue;

/* Capacity in samples. Returns NULL on allocation failure. */
fc_audio_queue* fc_audio_queue_create(uint32_t capacity);
void fc_audio_queue_destroy(fc_audio_queue* queue);

/* Copy `count` samples in; returns how many fit. Producer only. */
uint32_t fc_audio_queue_push(fc_audio_queue* queue, const float* samples, uint32_t count);

/* Copy up to `count` samples out and zero-fill the rest of `out` so the
 * callback never plays stale memory; returns how many were real. Consumer
 * only. */
uint32_t fc_audio_queue_pop(fc_audio_queue* queue, float* out, uint32_t count);

/* Samples waiting right now. */
uint32_t fc_audio_queue_fill(const fc_audio_queue* queue);

/* How many times the consumer ran dry since the queue was created. A front
 * end can show this; it is the first thing to look at when sound crackles. */
uint64_t fc_audio_queue_underruns(const fc_audio_queue* queue);

/* -- input ---------------------------------------------------------------- */

/* `port` is 0 for controller 1, 1 for controller 2. */
void fc_set_button(fc_machine* machine, fc_button button, bool pressed, int port);

/* Release every button on both ports, for when the window loses focus. */
void fc_release_all_buttons(fc_machine* machine);

/* -- diagnostics ---------------------------------------------------------- */


/* -- save states ---------------------------------------------------------- */

/* Write the whole machine into a newly allocated buffer and put its length in
 * `size`. Release the buffer with fc_free_state.
 *
 * Returns NULL when there is nothing to save -- no cartridge is loaded -- or
 * when the allocation fails. A state is a few tens of kilobytes: console RAM,
 * the PPU's memory, every APU channel's phase, and the mapper's bank registers.
 *
 * Not in it, on purpose: the framebuffer and the queued audio samples. They are
 * output, they are recomputed, and carrying them would make every state twenty
 * times larger for no benefit. */
uint8_t* fc_save_state(const fc_machine* machine, size_t* size);

/* Put the machine back into the state `data` describes.
 *
 * Returns false, and changes nothing at all, if the bytes are not a state, are
 * from a version this build does not know, or were written for a different
 * cartridge. A save from another game cannot be applied: the ROM behind every
 * bank number is different. */
bool fc_load_state(fc_machine* machine, const uint8_t* data, size_t size);

/* Release a buffer from fc_save_state. Passing NULL is allowed. */
void fc_free_state(uint8_t* data);

/* How many bytes one state takes for this machine, exactly.
 *
 * Constant for a given cartridge, so a front end can allocate a rewind buffer
 * once rather than guessing. Call it after loading a ROM. */
size_t fc_state_size(const fc_machine* machine);

/* Write a state into a buffer the caller owns, and return how many bytes were
 * written. Zero means `capacity` was too small and nothing was written.
 *
 * This exists next to fc_save_state for one reason: rewind saves thirty times
 * a second, and thirty allocations a second is thirty garbage collections a
 * minute. With a buffer the front end already owns there is nothing to
 * allocate and nothing to release. */
size_t fc_save_state_into(const fc_machine* machine, uint8_t* out, size_t capacity);

/* Whether the loaded cartridge's mapper saves its bank registers.
 *
 * False means save states are incomplete for this game: the machine comes back
 * at the right moment in time, with the cartridge re-pointed by whatever the
 * game has since written to it. Worth telling the player rather than letting
 * them discover it. */
bool fc_mapper_saves_state(const fc_machine* machine);

/* Total CPU cycles since power on. */
uint64_t fc_total_cycles(const fc_machine* machine);

/* The CPU's program counter, for a status line. */
uint16_t fc_cpu_pc(const fc_machine* machine);

/* -- debugging and cheats ------------------------------------------------- */

/* Read one byte of the CPU's address space without changing the machine.
 *
 * Console RAM and cartridge RAM only; 0 for everything else. `fc_peek` is not
 * a bus cycle: reading a PPU register has side effects (reading $2002 clears
 * the vblank flag), and reading a ROM byte can bank-switch on a few mappers,
 * so neither is safe for a debugger or a cheat search. RAM and only RAM is
 * what those actually need. */
uint8_t fc_peek(fc_machine* machine, uint16_t address);

/* Write one byte into the CPU's address space, now.
 *
 * The write goes through the bus, so it decodes the same way a CPU write
 * would: $075A and $0F5A are the same byte, and a cheat written against one
 * of them has to land on the other. */
void fc_poke(fc_machine* machine, uint16_t address, uint8_t value);

/* Replace the whole cheat list.
 *
 * `data` is `count` entries of four bytes each, little endian:
 *
 *     [0] address low   [1] address high   [2] value   [3] flags
 *
 * flags bit 0 = freeze (rewrite it at the start of every frame),
 * flags bit 1 = enabled. Anything else in the flags byte is ignored, and a
 * null pointer with a count of zero is how the list is emptied.
 *
 * One call with the whole list rather than add/remove/set verbs: the list is
 * the front end's, it is tiny, and a state that can be half-updated is a state
 * that can be wrong. */
void fc_set_cheats(fc_machine* machine, const uint8_t* data, int count);

/* How many cheats the core is holding. */
int fc_cheat_count(const fc_machine* machine);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* FC_EMULATOR_API_H */
