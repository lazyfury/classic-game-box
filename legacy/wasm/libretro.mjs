// ---------------------------------------------------------------------------
// The libretro core, wrapped so the front end can drive it.
//
// This is stage L4a/L5 of docs/architecture/libretro-migration.md: the core is
// `fc_libretro.wasm`, a standalone WebAssembly module that exports the libretro
// ABI, and this file is the front end half of that interface. It has the same
// shape as `wasm/emulator.mjs` -- the object `useEmulator.ts` has always
// driven -- so the renderer can swap one for the other without the game loop
// knowing which ABI is underneath.
//
// Why a front end is this small
// -----------------------------
// libretro is callbacks in both directions:
//
//     front end                         core
//     ---------                         ----
//     retro_set_*                ->     where a picture, sound and buttons go
//     retro_init / load_game / run ->   do the work
//
// `addFunction` turns a JavaScript function into a pointer the wasm module can
// call, so the callbacks below *are* the front end. Everything else is
// bookkeeping: recording what the core said, and copying bytes in and out of
// the heap.
//
// A pointer only means something together with the heap it points into. The
// module's heap is a fixed 64MB (`ALLOW_MEMORY_GROWTH=0`), so the views below
// never go stale and caching one is safe -- the whole reason this is a
// standalone module rather than an Emscripten side module.
//
// The two conversions that are not just plumbing
// ----------------------------------------------
//   * audio: the core produces interleaved stereo int16 because that is what
//     libretro asks for; this front end's audio pipeline has always taken mono
//     float. The NES has one speaker, so the two channels are the same sample,
//     and `takeSamples` hands back the left one scaled to [-1, 1].
//   * buttons: the console's eight switches are numbered one way by
//     packages/fc-core/src/ffi/emulator_api.h (A=0) and another by libretro (B=0, A=8). The
//     front end keeps speaking the first; the map lives here.
// ---------------------------------------------------------------------------

// -- the constants a front end needs, named the way libretro.h names them ----

/** One console button, numbered the way packages/fc-core/src/ffi/emulator_api.h numbers them.
 *  This is the numbering the rest of the front end already uses, so it is the
 *  one the CoreHost surface speaks. */
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

/** The same eight buttons, numbered the way libretro numbers them. */
export const Joypad = Object.freeze({
    B: 0,
    Y: 1,
    SELECT: 2,
    START: 3,
    UP: 4,
    DOWN: 5,
    LEFT: 6,
    RIGHT: 7,
    A: 8,
    X: 9,
    L: 10,
    R: 11,
});

/** Console button -> libretro id. The only place the two numbering schemes
 *  meet, which is why it is a table rather than arithmetic. */
const CONSOLE_TO_JOYPAD = [
    Joypad.A, Joypad.B, Joypad.SELECT, Joypad.START,
    Joypad.UP, Joypad.DOWN, Joypad.LEFT, Joypad.RIGHT,
];

export const Memory = Object.freeze({
    SAVE_RAM: 0,
    RTC: 1,
    SYSTEM_RAM: 2,
    VIDEO_RAM: 3,
});

export const Region = Object.freeze({ NTSC: 0, PAL: 1 });

export const PixelFormat = Object.freeze({ XRGB8888: 1 });
export const Device = Object.freeze({ NONE: 0, JOYPAD: 1 });

// -- environment commands, only the ones this front end answers --------------

const ENV_GET_CAN_DUPE = 3;
const ENV_GET_SYSTEM_DIRECTORY = 9;
const ENV_SET_PIXEL_FORMAT = 10;
const ENV_SET_INPUT_DESCRIPTORS = 11;
const ENV_GET_LOG_INTERFACE = 27;
const ENV_GET_SAVE_DIRECTORY = 31;
const ENV_SET_CONTROLLER_INFO = 35;
const ENV_SET_MEMORY_MAPS = 36 | 0x10000;   // EXPERIMENTAL

/** The parts of retro_system_info this front end reads, in bytes. */
const SYSTEM_INFO_SIZE = 16;
const AV_INFO_SIZE = 40;

/**
 * A ring of save states living in the core's own memory.
 *
 * The same idea as `StateBuffer` in wasm/emulator.mjs: rewind takes a snapshot
 * every couple of frames, so a snapshot is a memcpy into a slot rather than a
 * JavaScript allocation. `retro_serialize` writes into a pointer, so a slot is
 * just an offset into one buffer the buffer owns.
 */
export class StateBuffer {
    #mod;
    #pointer;
    #capacity;
    #slots;
    #sizes;

    constructor(mod, pointer, capacity, slots) {
        this.#mod = mod;
        this.#pointer = pointer;
        this.#capacity = capacity;
        this.#slots = slots;
        this.#sizes = new Uint32Array(slots);
    }

    get slots() { return this.#slots; }
    get capacity() { return this.#capacity; }

    save(slot) {
        if (this.#pointer === 0 || slot < 0 || slot >= this.#slots) {
            return false;
        }
        const at = this.#pointer + slot * this.#capacity;
        const ok = this.#mod._retro_serialize(at, this.#capacity) !== 0;
        this.#sizes[slot] = ok ? this.#capacity : 0;
        return ok;
    }

    load(slot) {
        if (this.#pointer === 0 || slot < 0 || slot >= this.#slots) {
            return false;
        }
        const size = this.#sizes[slot];
        if (size === 0) {
            return false;
        }
        return this.#mod._retro_unserialize(this.#pointer + slot * this.#capacity, size) !== 0;
    }

    clear() { this.#sizes.fill(0); }

    destroy() {
        if (this.#pointer !== 0) {
            this.#mod._free(this.#pointer);
            this.#pointer = 0;
        }
    }
}

const HEX = '0123456789ABCDEF';

/** A raw `{address, value}` cheat as a Pro Action Replay code, which is the
 *  language `retro_cheat_set` speaks. See packages/fc-libretro/src/libretro/cheat_codes.cpp. */
function proActionReplayCode(address, value)
{
    const byte = (n) => HEX[(n >> 4) & 0xF] + HEX[n & 0xF];
    return '00' + byte((address >> 8) & 0xFF) + byte(address & 0xFF) + byte(value & 0xFF);
}

/**
 * Load a core and wire a front end to it.
 *
 * `module` is an instantiated `fc_libretro.mjs`, passed in rather than imported
 * because "where the .wasm lives" has a different answer in Node, in the
 * Electron renderer, and on a web page. See the note at the top of
 * wasm/emulator.mjs for the same reasoning.
 */
export async function createCoreHost(module)
{
    if (!module) {
        throw new Error('createCoreHost needs an instantiated wasm module');
    }
    const mod = module;

    // A directory the core can name, even though there is no filesystem.
    // mGBA does not ask. Mesen refuses to load a cartridge unless a system
    // directory comes back non-NULL, and treats the empty string as "the
    // front end never set one" and throws the first time it wants to open
    // MesenDB.txt. The root is a directory as far as the ABI is concerned;
    // with no filesystem every open below it fails cleanly. The bytes live for
    // the lifetime of the host.
    const rootDirectory = mod._malloc(2);
    mod.HEAPU8[rootDirectory] = 0x2f;   // '/'
    mod.HEAPU8[rootDirectory + 1] = 0;

    // -- what the callbacks record ------------------------------------------

    let pixelFormat = null;
    let memoryMapPublished = 0;
    let inputDescriptorsPublished = false;
    let controllerInfoPublished = false;
    let inputPolls = 0;
    let inputQueries = 0;

    const video = { pointer: 0, width: 0, height: 0, pitch: 0 };

    // One frame is about 735 stereo frames. Bigger than any frame; a core that
    // produced more would be dropping audio, which is the same thing the front
    // end's own ring buffer does when it is full.
    const audio = new Int16Array(16384 * 2);
    let audioLength = 0;

    // The mono float view the rest of the front end expects, filled lazily by
    // takeSamples and reused so a frame allocates nothing.
    const audioFloat = new Float32Array(16384);

    // Where the extension writes the APU's own float samples, so takeSamples
    // can hand back exactly what the machine produced. Allocated once.
    const samplePointer = mod._malloc(audioFloat.length * 4);

    const buttons = [new Uint8Array(16), new Uint8Array(16)];

    let frameCount = 0;
    let isLoaded = false;
    let lastError = '';
    let cheatCount = 0;

    function pushSample(left, right)
    {
        if (audioLength + 2 > audio.length) {
            return;
        }
        audio[audioLength++] = left;
        audio[audioLength++] = right;
    }

    // -- the environment callback -------------------------------------------

    const environment = mod.addFunction((command, data) =>
    {
        switch (command)
        {
        case ENV_SET_PIXEL_FORMAT: {
            const format = mod.HEAPU32[data >> 2];
            pixelFormat = format;
            return format === PixelFormat.XRGB8888 ? 1 : 0;
        }

        case ENV_GET_CAN_DUPE:
            mod.HEAPU8[data] = 1;
            return 1;

        case ENV_GET_SYSTEM_DIRECTORY:
        case ENV_GET_SAVE_DIRECTORY:
            // The core writes a `const char*` out through the pointer it is
            // handed. One shared root, allocated once; nothing is ever written
            // under it, because there is no filesystem to write to.
            mod.HEAPU32[data >> 2] = rootDirectory;
            return 1;

        case ENV_SET_INPUT_DESCRIPTORS:
            inputDescriptorsPublished = true;
            return 1;

        case ENV_SET_CONTROLLER_INFO:
            controllerInfoPublished = true;
            return 1;

        case ENV_SET_MEMORY_MAPS:
            memoryMapPublished = mod.HEAPU32[(data + 4) >> 2];
            return 1;

        case ENV_GET_LOG_INTERFACE:
            // A logger would have to be a C variadic function, and addFunction
            // cannot build one. Saying no is allowed, and the core stays quiet
            // when nobody is listening.
            return 0;

        default:
            return 0;
        }
    }, 'iii');

    // -- video, audio and input ---------------------------------------------

    const videoRefresh = mod.addFunction((pointer, width, height, pitch) =>
    {
        video.pointer = pointer;
        video.width = width;
        video.height = height;
        video.pitch = pitch;
    }, 'viiii');

    const audioSample = mod.addFunction((left, right) => pushSample(left, right), 'vii');

    const audioSampleBatch = mod.addFunction((pointer, frames) =>
    {
        const base = pointer >> 1;   // HEAP16 indices are half the byte address
        for (let i = 0; i < frames; ++i) {
            pushSample(mod.HEAP16[base + i * 2], mod.HEAP16[base + i * 2 + 1]);
        }
        return frames;
    }, 'iii');

    const inputPoll = mod.addFunction(() => { ++inputPolls; }, 'v');

    const inputState = mod.addFunction((port, device, index, id) =>
    {
        ++inputQueries;
        if (port > 1 || id > 15) {
            return 0;
        }
        return buttons[port][id] ? 1 : 0;
    }, 'iiiii');

    // -- registration, in the order libretro requires -----------------------
    //
    // The environment goes first, then retro_init, then the four callbacks.
    // Mesen creates its Console inside retro_init, and retro_set_video_refresh
    // stores the callback on that Console -- registering first is silently
    // forgotten when init replaces it, and the core then runs frames nobody
    // sees. The other cores here tolerate either order, and this is the order
    // the ABI describes.

    mod._retro_set_environment(environment);
    mod._retro_init();
    mod._retro_set_video_refresh(videoRefresh);
    mod._retro_set_audio_sample(audioSample);
    mod._retro_set_audio_sample_batch(audioSampleBatch);
    mod._retro_set_input_poll(inputPoll);
    mod._retro_set_input_state(inputState);

    // -- a scratch struct, reused so a call allocates nothing ---------------

    const scratch = mod._malloc(AV_INFO_SIZE);

    function readSystemInfo()
    {
        mod._retro_get_system_info(scratch);
        return {
            libraryName: mod.UTF8ToString(mod.HEAPU32[scratch >> 2]),
            libraryVersion: mod.UTF8ToString(mod.HEAPU32[(scratch >> 2) + 1]),
            validExtensions: mod.UTF8ToString(mod.HEAPU32[(scratch >> 2) + 2]),
            needFullpath: mod.HEAPU8[scratch + 12] !== 0,
            blockExtract: mod.HEAPU8[scratch + 13] !== 0,
        };
    }

    function readAvInfo()
    {
        mod._retro_get_system_av_info(scratch);
        // The struct pads before its doubles, so they are read through a
        // DataView rather than pretending HEAPF64 is aligned to it.
        const bytes = new DataView(mod.HEAPU8.buffer);
        return {
            baseWidth: mod.HEAPU32[scratch >> 2],
            baseHeight: mod.HEAPU32[(scratch >> 2) + 1],
            maxWidth: mod.HEAPU32[(scratch >> 2) + 2],
            maxHeight: mod.HEAPU32[(scratch >> 2) + 3],
            fps: bytes.getFloat64(scratch + 24, true),
            sampleRate: bytes.getFloat64(scratch + 32, true),
        };
    }

    // Some cores (mGBA) dereference their machine inside
    // retro_get_system_av_info and are only safe to ask once a cartridge is in;
    // others (this project's own) answer whenever. So the call is attempted and,
    // if it faults, the geometry is filled in at load instead. The defaults are
    // the NES, this front end's home console.
    let av = {
        baseWidth: 256, baseHeight: 240, maxWidth: 256, maxHeight: 240,
        fps: 60.0988, sampleRate: 44100.0,
    };
    try {
        av = readAvInfo();
    } catch (ignored) {
        // Left at the defaults; loadRom refreshes it once the core has a game.
    }

    // -- the CoreHost surface ------------------------------------------------

    const host = {
        // -- the machine's shape (the Emulator surface) -----------------------

        get width() { return av.baseWidth; },
        get height() { return av.baseHeight; },
        get sampleRate() { return av.sampleRate; },
        get frameSeconds() { return 1 / av.fps; },

        /** Bytes between the starts of two rows. Not always width * 4: mGBA
         *  renders into a 256-pixel-wide buffer and reports 240 visible
         *  pixels with a 1024 byte pitch. Reading it as 240 * 4 shears every
         *  row by 64 bytes, which is what diagonal stripes are. */
        get pitch() { return video.pitch || av.baseWidth * 4; },

        // -- loading ----------------------------------------------------------

        loadRom(bytes)
        {
            if (!(bytes instanceof Uint8Array) || bytes.length === 0) {
                lastError = 'loadRom needs a non-empty Uint8Array';
                return false;
            }

            // A libretro core holds one game at a time. Loading a second
            // without taking the first out is undefined for the core: mGBA
            // keeps the machine it already has and refuses the new cartridge,
            // which is a Game Boy game that "does not start" when the player
            // picks another one. The unload is what a front end owes the ABI
            // between two games.
            if (isLoaded) {
                mod._retro_unload_game();
                isLoaded = false;
            }

            const data = mod._malloc(bytes.length);
            if (data === 0) {
                lastError = `could not allocate ${bytes.length} bytes for the ROM`;
                return false;
            }

            // retro_game_info { path, data, size, meta }
            const info = mod._malloc(16);
            mod.HEAPU32[(info >> 2) + 0] = 0;
            mod.HEAPU32[(info >> 2) + 1] = data;
            mod.HEAPU32[(info >> 2) + 2] = bytes.length;
            mod.HEAPU32[(info >> 2) + 3] = 0;

            mod.HEAPU8.set(bytes, data);

            // A core that answers a cartridge it cannot parse with a C++
            // exception rather than `false` would otherwise take the renderer
            // down with it: wasm exceptions surface in JavaScript. Mesen
            // catches its own, and this is the net under the ones that do not.
            let thrownError = null;
            try {
                isLoaded = mod._retro_load_game(info) !== 0;
            } catch (thrown) {
                isLoaded = false;
                thrownError = thrown instanceof Error ? thrown.message : String(thrown);
            }
            if (isLoaded) {
                frameCount = 0;
                lastError = '';
                try {
                    av = readAvInfo();
                } catch (ignored) {
                    // A core that still will not answer keeps the defaults.
                }
            } else {
                // The core's own reason for refusing, when it has one to give:
                // the NES core carries "mapper 176 is not implemented yet" in
                // the extension, and without it the player only sees the
                // generic line. A core from another project has no extension
                // and keeps the generic message.
                lastError = thrownError ?? 'the core did not accept this cartridge';
                if (mod._fc_ext_last_error !== undefined) {
                    const reason = mod.UTF8ToString(mod._fc_ext_last_error());
                    if (reason) {
                        lastError = reason;
                    }
                }
            }

            // The core copies what it needs during the call (need_fullpath is
            // false), so both buffers can go now.
            mod._free(info);
            mod._free(data);
            return isLoaded;
        },

        get isLoaded() { return isLoaded; },
        get romSummary()
        {
            // The extension has the one line summary; a core without it gets
            // its library name, which is the next best thing.
            return mod._fc_ext_rom_summary !== undefined
                ? mod.UTF8ToString(mod._fc_ext_rom_summary())
                : readSystemInfo().libraryName;
        },
        get lastError() { return lastError; },

        // -- running ----------------------------------------------------------

        reset()
        {
            mod._retro_reset();
        },

        runFrame()
        {
            audioLength = 0;
            mod._retro_run();
            ++frameCount;
            // libretro has no notion of a halted core; a frame that produced no
            // picture is the closest signal, and the front end treats false as
            // "stop and say so".
            return true;
        },

        get isHalted() { return false; },
        get frameCount() { return frameCount; },
        get totalCycles()
        {
            return mod._fc_ext_total_cycles !== undefined
                ? Number(mod._fc_ext_total_cycles()) : 0;
        },
        get cpuPc()
        {
            return mod._fc_ext_cpu_pc !== undefined ? mod._fc_ext_cpu_pc() : 0;
        },

        // -- video ------------------------------------------------------------

        /** The framebuffer as bytes, top row first, four bytes per pixel.
         *  XRGB8888 in memory is the same 0x00RRGGBB the C interface hands
         *  over, so the renderer does not change. The length is the pitch
         *  times the rows, because the pitch is what the core actually wrote;
         *  the geometry gives the visible size, which is not always the same
         *  as the stride. */
        framebufferBytes()
        {
            const stride = host.pitch;
            const rows = video.height || av.baseHeight;
            return mod.HEAPU8.subarray(video.pointer, video.pointer + stride * rows);
        },

        pixel(x, y)
        {
            if (x < 0 || y < 0 || x >= video.width || y >= video.height) {
                return 0;
            }
            return mod.HEAPU32[(video.pointer >> 2) + y * (host.pitch >> 2) + x] >>> 0;
        },

        // -- audio ------------------------------------------------------------

        /**
         * The last frame's sound, as mono float in [-1, 1].
         *
         * The extension carries the APU's own float samples, and this prefers
         * them: libretro's int16 is the correct ABI and a standard front end
         * gets it, but it is lossy, and this project's parity test compares
         * the bytes. A core without the extension falls back to converting
         * the int16 the callback already received -- the NES has one speaker,
         * so the left channel is the sample.
         */
        takeSamples(count = audioFloat.length)
        {
            const wanted = Math.min(count, audioFloat.length);

            if (mod._fc_ext_take_samples !== undefined) {
                const got = mod._fc_ext_take_samples(samplePointer, wanted);
                if (got > 0) {
                    const base = samplePointer >> 2;
                    return mod.HEAPF32.subarray(base, base + got);
                }
                return audioFloat.subarray(0, 0);
            }

            const frames = Math.min(audioLength >> 1, wanted);
            for (let i = 0; i < frames; ++i) {
                audioFloat[i] = audio[i * 2] / 32768;
            }
            return audioFloat.subarray(0, frames);
        },

        get samplesPending() { return audioLength >> 1; },
        clearSamples() { audioLength = 0; },

        // -- input ------------------------------------------------------------

        /** `button` is a console button (Button.*); `port` is 0 or 1. */
        setButton(button, pressed, port = 0)
        {
            const id = CONSOLE_TO_JOYPAD[button];
            if (id === undefined || port < 0 || port > 1) {
                return;
            }
            buttons[port][id] = pressed ? 1 : 0;
        },

        releaseAllButtons()
        {
            buttons[0].fill(0);
            buttons[1].fill(0);
        },

        setControllerPortDevice(port, device)
        {
            mod._retro_set_controller_port_device(port, device);
        },

        // -- cheats and debugging ---------------------------------------------

        peek(address)
        {
            return mod._fc_ext_peek !== undefined
                ? mod._fc_ext_peek(address & 0xFFFF) : 0;
        },

        poke(address, value)
        {
            if (mod._fc_ext_poke !== undefined) {
                mod._fc_ext_poke(address & 0xFFFF, value & 0xFF);
            }
        },

        /**
         * Replace the cheat list.
         *
         * A freeze cheat is a Pro Action Replay code the core puts back every
         * frame, which is the same mechanism as the C interface's
         * address/value/freeze; a one-shot is a poke, which is what it is.
         */
        setCheats(cheats = [])
        {
            // Before a cartridge is in, there is nothing to cheat on, and a
            // core is not obliged to survive being asked: mGBA's
            // retro_cheat_reset dereferences its machine, which does not exist
            // until retro_load_game. The front end asks at machine build (for
            // cheats that may already be set) and again at load, so doing
            // nothing here costs nothing.
            if (!isLoaded) {
                return;
            }

            mod._retro_cheat_reset();
            cheatCount = 0;

            let index = 0;
            for (const cheat of cheats) {
                if (!cheat.enabled) {
                    continue;
                }
                if (cheat.freeze) {
                    const code = proActionReplayCode(cheat.address, cheat.value);
                    const text = mod._malloc(mod.lengthBytesUTF8(code) + 1);
                    mod.stringToUTF8(code, text, mod.lengthBytesUTF8(code) + 1);
                    mod._retro_cheat_set(index++, 1, text);
                    mod._free(text);
                } else {
                    host.poke(cheat.address, cheat.value);
                }
                ++cheatCount;
            }
        },

        get cheatCount() { return cheatCount; },

        // -- save states ------------------------------------------------------

        serializeSize() { return mod._retro_serialize_size(); },

        saveState()
        {
            const size = mod._retro_serialize_size();
            if (!size) {
                return null;
            }
            const buffer = mod._malloc(size);
            const ok = mod._retro_serialize(buffer, size) !== 0;
            const bytes = ok ? mod.HEAPU8.slice(buffer, buffer + size) : null;
            mod._free(buffer);
            return bytes;
        },

        createStateBuffer(slots)
        {
            if (!Number.isInteger(slots) || slots < 1) {
                throw new Error('a state buffer needs at least one slot');
            }
            const capacity = mod._retro_serialize_size();
            if (!capacity) {
                return null;
            }
            const pointer = mod._malloc(capacity * slots);
            if (pointer === 0) {
                return null;
            }
            return new StateBuffer(mod, pointer, capacity, slots);
        },

        loadState(bytes)
        {
            if (!(bytes instanceof Uint8Array) || bytes.length === 0) {
                return false;
            }
            const buffer = mod._malloc(bytes.length);
            if (buffer === 0) {
                return false;
            }
            mod.HEAPU8.set(bytes, buffer);
            const ok = mod._retro_unserialize(buffer, bytes.length) !== 0;
            mod._free(buffer);
            return ok;
        },

        get mapperSavesState()
        {
            // A core without the extension is assumed whole; the flag exists to
            // warn about the ones that are not.
            return mod._fc_ext_mapper_saves_state !== undefined
                ? mod._fc_ext_mapper_saves_state() !== 0 : true;
        },

        // -- lifecycle --------------------------------------------------------

        destroy()
        {
            mod._free(samplePointer);
            mod._free(scratch);
            mod._retro_unload_game();
            mod._retro_deinit();
            isLoaded = false;
        },

        // -- the libretro-shaped names, for the probe and the tests -----------

        apiVersion() { return mod._retro_api_version(); },
        systemInfo() { return readSystemInfo(); },
        avInfo() { return readAvInfo(); },

        /** The picture and the last frame's sound, in libretro's own terms. */
        framebuffer() { return { ...video }; },
        audio() { return audio.subarray(0, audioLength); },
        run() { host.runFrame(); },
        loadGame(bytes) { return host.loadRom(bytes); },
        memory(id)
        {
            const pointer = mod._retro_get_memory_data(id);
            const size = mod._retro_get_memory_size(id);
            if (!pointer || !size) {
                return null;
            }
            return mod.HEAPU8.subarray(pointer, pointer + size);
        },
        serialize() { return host.saveState(); },
        unserialize(bytes) { return host.loadState(bytes); },
        cheatSet(index, enabled, code)
        {
            if (!isLoaded) {
                return;
            }
            const text = mod._malloc(mod.lengthBytesUTF8(code) + 1);
            mod.stringToUTF8(code, text, mod.lengthBytesUTF8(code) + 1);
            mod._retro_cheat_set(index, enabled ? 1 : 0, text);
            mod._free(text);
        },
        cheatReset()
        {
            if (isLoaded) {
                mod._retro_cheat_reset();
            }
            cheatCount = 0;
        },
        extension()
        {
            const pointer = mod._fc_libretro_get_ext();
            if (!pointer) {
                return null;
            }
            return {
                pointer,
                abiVersion: mod.HEAPU32[pointer >> 2],
                structSize: mod.HEAPU32[(pointer >> 2) + 1],
            };
        },
        observations()
        {
            return {
                pixelFormat,
                memoryMapPublished,
                inputDescriptorsPublished,
                controllerInfoPublished,
                inputPolls,
                inputQueries,
            };
        },

        module: mod,
        scratch,
    };

    return host;
}
