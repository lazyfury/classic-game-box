// ---------------------------------------------------------------------------
// Rewind.
//
// The mechanism is just save states, repeatedly. What makes it work is
// deciding *when* to take them and *how many* to keep, and the two numbers
// trade against each other:
//
//   every frame      rewinding is frame perfect, and the buffer holds a
//                    second and a half
//   every 2 frames   rewinding moves in two frame steps, and holds three
//                    seconds
//   every 10 frames  a whole second of memory, and the picture jumps
//
// Two frames is the compromise this uses. It costs about twenty kilobytes
// thirty times a second -- six hundred kilobytes a second, which is nothing --
// and it holds ten seconds, which is longer than anybody holds the key.
//
// Where the memory lives
// ----------------------
// In the emulator, not in JavaScript. See StateBuffer in wasm/emulator.mjs:
// thirty Uint8Array allocations a second is thirty garbage collections a
// minute, and this project has already had one audio glitch caused by
// forgetting that a per-frame allocation is a per-frame allocation.
//
// The playhead
// ------------
// Snapshots go into a ring. `head` is the one the machine is currently at, and
// `count` is how many are still behind it. Both shrink when rewinding and grow
// again when playing forward -- which is what makes "rewind, then carry on"
// overwrite the future rather than the past, exactly like a tape.
// ---------------------------------------------------------------------------

import type { Emulator, StateBuffer } from '@wasm';

/** The console's real rate, for turning a snapshot count into a duration. */
const NES_FRAME_SECONDS = 1 / 60.0988;

export interface RewindOptions {
    /** How many snapshots the ring holds. 300 at two frames each is ten
     *  seconds. */
    slots?: number;
    /** Frames between snapshots. */
    everyFrames?: number;
}

export class Rewind {
    readonly #buffer: StateBuffer | null;
    readonly #everyFrames: number;

    #framesSinceSnapshot = 0;
    #head = -1;
    #count = 0;

    constructor(engine: Emulator, options: RewindOptions = {}) {
        this.#everyFrames = Math.max(1, options.everyFrames ?? 2);

        // Null when there is no cartridge, or when the memory could not be
        // had. Rewind then simply does nothing, and the front end says so
        // rather than offering a key that does not work.
        this.#buffer = engine.createStateBuffer(options.slots ?? 300);
    }

    /** Frames of game time between snapshots. */
    get everyFrames(): number {
        return this.#everyFrames;
    }

    /** False when this machine cannot be rewound. */
    get available(): boolean {
        return this.#buffer !== null;
    }

    /** How many snapshots are behind the playhead. */
    get depth(): number {
        return this.#count;
    }

    /** How far back the buffer can go, as a duration. */
    get seconds(): number {
        return (this.#count * this.#everyFrames) * NES_FRAME_SECONDS;
    }

    /** How much memory the ring is using, for a status line. */
    get bytes(): number {
        return (this.#buffer?.slots ?? 0) * (this.#buffer?.capacity ?? 0);
    }

    /**
     * Record where the machine is, if it is due.
     *
     * Called after every frame that ran, so that `everyFrames` counts frames
     * rather than animation frames -- a slow machine rewinds in the same steps
     * as a fast one.
     */
    record(): void {
        const buffer = this.#buffer;
        if (buffer === null) {
            return;
        }

        this.#framesSinceSnapshot += 1;
        if (this.#framesSinceSnapshot < this.#everyFrames) {
            return;
        }
        this.#framesSinceSnapshot = 0;

        // The slot after the playhead. If the ring is full this is the oldest
        // snapshot, which is the one to lose.
        const slot = (this.#head + 1) % buffer.slots;
        if (!buffer.save(slot)) {
            return;
        }

        this.#head = slot;
        this.#count = Math.min(this.#count + 1, buffer.slots);
    }

    /**
     * Move back one snapshot. False at the beginning of the ring, or when
     * there is nothing to rewind.
     */
    stepBack(): boolean {
        const buffer = this.#buffer;
        if (buffer === null || this.#count === 0) {
            return false;
        }

        const slot = (this.#head - 1 + buffer.slots) % buffer.slots;
        if (!buffer.load(slot)) {
            // A slot that will not load means the ring is not what it thinks
            // it is. Stop rather than walk into it.
            this.#count = 0;
            return false;
        }

        this.#head = slot;
        this.#count -= 1;
        return true;
    }

    /**
     * Throw the history away.
     *
     * Called whenever the machine jumps somewhere the ring does not describe:
     * a different cartridge, a reset, or a loaded save state. Keeping the old
     * snapshots would let the player rewind into a machine that never existed.
     */
    reset(): void {
        this.#buffer?.clear();
        this.#head = -1;
        this.#count = 0;
        this.#framesSinceSnapshot = 0;
    }

    /** The cartridge is going away, and the memory with it. */
    destroy(): void {
        this.#buffer?.destroy();
        this.reset();
    }
}
