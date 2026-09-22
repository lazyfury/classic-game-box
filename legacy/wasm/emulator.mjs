// ---------------------------------------------------------------------------
// A NES, wrapped.
//
// The JavaScript twin of frontend/Sources/Emulator.swift. Below this file
// there is nothing but C: packages/fc-core/src/ffi/emulator_api.h, compiled to WebAssembly.
// Nothing here knows how a PPU works, and nothing below it knows that
// JavaScript exists. Change the language above the C interface and not one
// line of the emulator has to move.
//
// How a pointer crosses the border
// --------------------------------
// The C interface hands back bare pointers. On the wasm side a pointer is just
// an offset into linear memory, and linear memory is exposed to JavaScript as
// one big ArrayBuffer. So `fc_framebuffer()` becomes a *view* onto the
// emulator's own pixels: `HEAPU8.subarray(ptr, ptr + 245760)`. Nothing is
// copied and nothing is serialised, on either side of the boundary.
//
// That is the whole reason this is wasm and not a helper process. A native
// helper has to turn 240KB of pixels into bytes in a pipe sixty times a
// second and something has to unpack it again at the other end. Here the
// renderer reads the emulator's memory directly.
//
// One rule worth stating out loud: the heap does not move. The build fixes
// INITIAL_MEMORY and turns ALLOW_MEMORY_GROWTH off, because growing the heap
// replaces the ArrayBuffer and every view taken before the growth silently
// goes stale. Caching a view is then safe, which is what makes the code below
// simple.
//
// Loading the module is the caller's job
// --------------------------------------
// `Emulator.create()` takes an already instantiated module rather than
// importing one itself, because "where is fc_core.wasm" has a different answer
// everywhere this file runs:
//
//   node wasm/headless.mjs   ->  ./dist/fc_core.mjs, next to this file
//   Electron renderer        ->  /fc_core.mjs, off the page's own origin
//   a web page               ->  wherever the site put it
//
// A static import would pick one of those and break the other two. Two lines
// at each call site is cheaper than a build step that rewrites the import.
// ---------------------------------------------------------------------------

/// One button. These values are the contract with packages/fc-core/src/ffi/emulator_api.h,
/// which in turn matches nes::Controller::Button. They are not arbitrary and
/// must not be reordered.
export const Button = Object.freeze({
    A: 0,
    B: 1,
    SELECT: 2,
    START: 3,
    UP: 4,
    DOWN: 5,
    LEFT: 6,
    RIGHT: 7,
});

/// The ABI the JavaScript below expects. src/wasm/glue.cpp returns this, and
/// the check in `create` is what stops a stale fc_core.wasm from being paired
/// with a newer wrapper. Without it the mismatch shows up as a garbled picture
/// rather than as an error. Exported so the test can compare the two rather
/// than hardcode the number in a third place.
const EXPECTED_ABI = 2;
export { EXPECTED_ABI };

/**
 * A block of the emulator's own memory, holding save states.
 *
 * This exists for rewind, and for one reason: rewinding needs a snapshot every
 * couple of frames, which is thirty a second, and thirty Uint8Array allocations
 * a second is thirty garbage collections a minute. In a frame loop that is not
 * a cost, it is a stutter.
 *
 * So the storage lives on the wasm side, where nothing has to be collected, and
 * a snapshot is a memcpy into a slot the buffer already owns. The caller never
 * sees a pointer; it sees slots.
 */
export class StateBuffer {
    #api;
    #machine;
    #pointer;
    #capacity;
    #slots;
    #sizes;

    /** Not `new StateBuffer()`. See Emulator.createStateBuffer(). */
    constructor(api, machine, pointer, capacity, slots) {
        this.#api = api;
        this.#machine = machine;
        this.#pointer = pointer;
        this.#capacity = capacity;
        this.#slots = slots;
        this.#sizes = new Uint32Array(slots);
    }

    get slots() {
        return this.#slots;
    }

    /** Bytes per slot, which is the size of one save state. */
    get capacity() {
        return this.#capacity;
    }

    /** Snapshot into this slot, overwriting whatever was there. */
    save(slot) {
        if (this.#pointer === 0 || slot < 0 || slot >= this.#slots) {
            return false;
        }
        const size = this.#api._fc_save_state_into(
            this.#machine, this.#pointer + slot * this.#capacity, this.#capacity);
        this.#sizes[slot] = size;
        return size > 0;
    }

    /** Restore from this slot. False if nothing was ever saved there. */
    load(slot) {
        if (this.#pointer === 0 || slot < 0 || slot >= this.#slots) {
            return false;
        }
        const size = this.#sizes[slot];
        if (size === 0) {
            return false;
        }
        return this.#api._fc_load_state(
            this.#machine, this.#pointer + slot * this.#capacity, size) !== 0;
    }

    /** Forget every slot, without freeing anything. */
    clear() {
        this.#sizes.fill(0);
    }

    /** Free the memory. The buffer is unusable afterwards. */
    destroy() {
        if (this.#pointer !== 0) {
            this.#api._free(this.#pointer);
            this.#pointer = 0;
        }
    }
}

export class Emulator {
    #api = null;
    #handle = 0;

    #width = 0;
    #height = 0;
    #sampleRate = 0;

    // Scratch space for draining the APU, allocated once. `takeSamples` is
    // called once per frame forever, so allocating per call would hand the
    // garbage collector 60 short lived buffers a second for no reason.
    #samplePtr = 0;
    #sampleCapacity = 0;

    #isLoaded = false;

    /** Not `new Emulator()`: a module has to be compiled and instantiated
     *  first, and that is asynchronous. Pass the object the emscripten
     *  factory resolves to. */
    static async create({ module, sampleCapacity = 8192 } = {}) {
        if (module === undefined || module === null) {
            throw new Error('Emulator.create needs an instantiated wasm module');
        }
        const emulator = new Emulator();
        await emulator.#initialise(module, sampleCapacity);
        return emulator;
    }

    async #initialise(module, sampleCapacity) {
        this.#api = module;

        const abi = this.#api._fc_wasm_abi();
        if (abi !== EXPECTED_ABI) {
            throw new Error(
                `fc_core.wasm speaks ABI ${abi}, this wrapper speaks ${EXPECTED_ABI}. ` +
                `Run ./wasm/build.sh.`);
        }

        this.#width = this.#api._fc_wasm_screen_width();
        this.#height = this.#api._fc_wasm_screen_height();
        this.#sampleRate = this.#api._fc_wasm_sample_rate();

        this.#handle = this.#api._fc_create();
        if (this.#handle === 0) {
            // fc_create only fails when the process is out of memory, in which
            // case there is nothing useful left to do.
            throw new Error('could not create the emulator');
        }

        this.#sampleCapacity = sampleCapacity;
        this.#samplePtr = this.#api._malloc(sampleCapacity * 4);
    }

    // -- the machine's shape ------------------------------------------------

    get width() { return this.#width; }
    get height() { return this.#height; }
    get sampleRate() { return this.#sampleRate; }

    // -- loading ------------------------------------------------------------

    /**
     * Load a .nes image. `bytes` is a Uint8Array holding the whole file.
     *
     * The ROM has to be copied into the emulator's linear memory because that
     * is the only memory the C side can address, and the copy is released
     * again immediately: the cartridge keeps its own copy, so there is no
     * reason to hold two.
     */
    loadRom(bytes) {
        this.#assertAlive();

        if (!(bytes instanceof Uint8Array) || bytes.length === 0) {
            throw new Error('loadRom needs a non-empty Uint8Array');
        }

        const ptr = this.#api._malloc(bytes.length);
        if (ptr === 0) {
            throw new Error(`could not allocate ${bytes.length} bytes for the ROM`);
        }

        try {
            this.#api.HEAPU8.set(bytes, ptr);
            this.#isLoaded = this.#api._fc_load_rom(this.#handle, ptr, bytes.length) !== 0;
        } finally {
            this.#api._free(ptr);
        }

        return this.#isLoaded;
    }

    get isLoaded() { return this.#isLoaded; }

    /// A one line description of the cartridge, or "" if there is none.
    get romSummary() {
        return this.#api.UTF8ToString(this.#api._fc_rom_summary(this.#handle));
    }

    /// Why the last operation failed. Never empty when something returned false.
    get lastError() {
        return this.#api.UTF8ToString(this.#api._fc_last_error(this.#handle));
    }

    // -- running ------------------------------------------------------------

    reset() {
        this.#assertAlive();
        this.#api._fc_reset(this.#handle);
    }

    /**
     * Run until the PPU finishes a frame.
     *
     * Returns false if the CPU halted, which means the emulator met an opcode
     * it does not implement. That is a bug in this project, not in the game,
     * and callers should say so rather than show a frozen picture.
     */
    runFrame() {
        this.#assertAlive();
        return this.#api._fc_run_frame(this.#handle) !== 0;
    }

    get isHalted() { return this.#api._fc_is_halted(this.#handle) !== 0; }
    get frameCount() { return this.#api._fc_frame_count(this.#handle); }

    /**
     * Total CPU cycles since power on.
     *
     * `fc_total_cycles` returns uint64_t. On wasm32 that is an i64, and
     * Emscripten hands an i64 back as a BigInt -- so without this conversion
     * the value looks like a number and behaves like one right up until
     * somebody compares it with a plain number, at which point JavaScript
     * throws "Cannot mix BigInt and other types" rather than quietly doing
     * the wrong thing. Converting at the boundary means no caller has to
     * know.
     *
     * Number is exact to 2^53. At 1.79MHz that is a hundred and sixty years
     * of continuous play.
     */
    get totalCycles() { return Number(this.#api._fc_total_cycles(this.#handle)); }
    get cpuPc() { return this.#api._fc_cpu_pc(this.#handle); }

    // -- video --------------------------------------------------------------

    /**
     * The framebuffer, as bytes, top row first: 4 bytes per pixel.
     *
     * The view is into the emulator's own memory, so it is free to take and it
     * is rewritten by every `runFrame()`. It is not an image yet: the pixels
     * are 0x00RRGGBB stored little endian, so the four bytes are actually
     * B, G, R, 0. Turning that into whatever the display wants is the
     * renderer's job, not the emulator's -- a PPM needs three bytes, a canvas
     * needs four in a different order, and neither question belongs in the
     * core.
     */
    framebufferBytes() {
        this.#assertAlive();
        const ptr = this.#api._fc_framebuffer(this.#handle);
        const length = this.#width * this.#height * 4;
        return this.#api.HEAPU8.subarray(ptr, ptr + length);
    }

    /// Pixel at (x, y) as 0x00RRGGBB, or 0 if the coordinates are off screen.
    pixel(x, y) {
        this.#assertAlive();
        return this.#api._fc_pixel(this.#handle, x, y) >>> 0;
    }

    // -- audio --------------------------------------------------------------

    /**
     * Copy up to `count` mono samples out of the machine and return them.
     *
     * The returned array is a view onto the scratch buffer, valid until the
     * next call. Callers that keep the samples must copy them.
     */
    takeSamples(count = this.#sampleCapacity) {
        this.#assertAlive();
        const wanted = Math.min(count, this.#sampleCapacity);
        const taken = this.#api._fc_take_samples(this.#handle, this.#samplePtr, wanted);
        const base = this.#samplePtr >> 2;
        return this.#api.HEAPF32.subarray(base, base + taken);
    }

    get samplesPending() {
        this.#assertAlive();
        return this.#api._fc_samples_pending(this.#handle);
    }

    clearSamples() {
        this.#assertAlive();
        this.#api._fc_clear_samples(this.#handle);
    }

    // -- input --------------------------------------------------------------

    /** `port` is 0 for controller 1, 1 for controller 2. */
    setButton(button, pressed, port = 0) {
        this.#assertAlive();
        this.#api._fc_set_button(this.#handle, button, pressed ? 1 : 0, port);
    }

    /** Release every button on both ports, for when the window loses focus. */
    releaseAllButtons() {
        this.#assertAlive();
        this.#api._fc_release_all_buttons(this.#handle);
    }

    // -- cheats and debugging -----------------------------------------------

    /**
     * Read one byte of the CPU's address space without changing the machine.
     *
     * Console RAM and cartridge RAM; 0 elsewhere. Meant for a cheat search and
     * for showing a value in the panel, so it deliberately does not go through
     * the bus: reading a PPU register has side effects.
     */
    peek(address) {
        this.#assertAlive();
        return this.#api._fc_peek(this.#handle, address & 0xFFFF);
    }

    /** Write one byte into the CPU's address space, now. */
    poke(address, value) {
        this.#assertAlive();
        this.#api._fc_poke(this.#handle, address & 0xFFFF, value & 0xFF);
    }

    /**
     * Replace the cheat list.
     *
     * `cheats` is an array of objects shaped `{ address, value, freeze,
     * enabled }`. They are packed into the four-bytes-per-entry form the C
     * side reads, which is the one place the layout is written down on this
     * side of the boundary; `fc_set_cheats` in packages/fc-core/src/ffi/emulator_api.h is the
     * other.
     */
    setCheats(cheats = []) {
        this.#assertAlive();

        // At least one byte, so that an empty list still has a valid pointer
        // to pass: `_malloc(0)` may hand back 0, which reads as "no list"
        // rather than "an empty list" -- the same thing here, but only by
        // accident.
        const bytes = new Uint8Array(Math.max(cheats.length * 4, 1));
        cheats.forEach((cheat, position) => {
            const at = position * 4;
            bytes[at] = cheat.address & 0xFF;
            bytes[at + 1] = (cheat.address >> 8) & 0xFF;
            bytes[at + 2] = cheat.value & 0xFF;
            bytes[at + 3] = (cheat.freeze ? 0x01 : 0) | (cheat.enabled ? 0x02 : 0);
        });

        const pointer = this.#api._malloc(bytes.length);
        try {
            this.#api.HEAPU8.set(bytes, pointer);
            this.#api._fc_set_cheats(this.#handle, pointer, cheats.length);
        } finally {
            this.#api._free(pointer);
        }
    }

    /** How many cheats the core is holding. */
    get cheatCount() {
        this.#assertAlive();
        return this.#api._fc_cheat_count(this.#handle);
    }

    // -- save states --------------------------------------------------------

    /**
     * Serialize the machine. Returns a copy of the bytes, or null if there is
     * nothing to save.
     *
     * The copy is not optional. `fc_save_state` hands back a malloc'd buffer
     * inside the emulator's own memory, and `slice` is what takes it out of
     * there before the buffer is released.
     */
    saveState() {
        this.#assertAlive();

        const sizePointer = this.#api._malloc(4);
        try {
            const pointer = this.#api._fc_save_state(this.#handle, sizePointer);
            if (pointer === 0) {
                return null;
            }

            // size_t, little endian, which is what wasm32 is.
            const size = new DataView(this.#api.HEAPU8.buffer).getUint32(sizePointer, true);
            const bytes = this.#api.HEAPU8.slice(pointer, pointer + size);
            this.#api._fc_free_state(pointer);
            return bytes;
        } finally {
            this.#api._free(sizePointer);
        }
    }

    /**
     * Reserve room for `slots` save states, in the emulator's own memory.
     *
     * Returns null if there is no cartridge, or if the memory could not be
     * had. One slot costs about twenty kilobytes.
     */
    createStateBuffer(slots) {
        this.#assertAlive();
        if (!Number.isInteger(slots) || slots < 1) {
            throw new Error('a state buffer needs at least one slot');
        }

        const capacity = this.#api._fc_state_size(this.#handle);
        if (capacity === 0) {
            return null;
        }

        const pointer = this.#api._malloc(capacity * slots);
        if (pointer === 0) {
            return null;
        }

        return new StateBuffer(this.#api, this.#handle, pointer, capacity, slots);
    }

    /**
     * Put the machine back. Returns false, having changed nothing, if the
     * bytes are not a state or are for a different cartridge.
     */
    loadState(bytes) {
        this.#assertAlive();
        if (!(bytes instanceof Uint8Array) || bytes.length === 0) {
            return false;
        }

        const pointer = this.#api._malloc(bytes.length);
        if (pointer === 0) {
            return false;
        }

        try {
            this.#api.HEAPU8.set(bytes, pointer);
            return this.#api._fc_load_state(this.#handle, pointer, bytes.length) !== 0;
        } finally {
            this.#api._free(pointer);
        }
    }

    /**
     * Whether the loaded cartridge's mapper saves its bank registers.
     *
     * False means save states are incomplete for this game: the machine comes
     * back at the right moment with the cartridge wired the way it was at
     * power on.
     */
    get mapperSavesState() {
        this.#assertAlive();
        return this.#api._fc_mapper_saves_state(this.#handle) !== 0;
    }

    // -- lifecycle ----------------------------------------------------------

    destroy() {
        if (this.#handle === 0) {
            return;
        }
        this.#api._fc_destroy(this.#handle);
        this.#api._free(this.#samplePtr);
        this.#handle = 0;
        this.#samplePtr = 0;
        this.#isLoaded = false;
    }

    #assertAlive() {
        if (this.#handle === 0) {
            throw new Error('the emulator has been destroyed');
        }
    }
}
