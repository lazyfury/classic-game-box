// ---------------------------------------------------------------------------
// Feeding the speaker.
//
// The shape of the problem: the emulator produces audio in bursts, one frame
// at a time, whenever a frame happens to finish. The sound card consumes it at
// an absolutely fixed rate and does not tolerate being kept waiting. Something
// has to sit in the middle, and that something is a ring buffer.
//
//   emulator (page thread) --write--> ring buffer --read--> audio thread
//
// Only the ring buffer is shared. The emulator itself is only ever touched by
// the page's thread, which is what keeps this simple.
//
// Two clocks, not one
// -------------------
// The emulator aims at 60.0988 frames a second, which is 44100 samples a
// second, and the sound card plays 44100 samples a second. Those are the same
// number and they are not the same rate: one is counted by the system clock
// and the other by a crystal on the audio chip, and they disagree by tens of
// parts per million. Treated as identical, the ring slowly fills or slowly
// empties, and after half an hour of play the player hears a click or a gap.
//
// The fix is to stop treating them as equal. The fill level is the error
// signal, and the emulator's frame rate is nudged by up to half a percent to
// keep the ring at its target:
//
//   frameSeconds = NES_FRAME_SECONDS * (1 + gain * (fill - target) / target)
//
// This is a proportional controller on an integrator, so the fill converges
// to the target exponentially and does not oscillate. Half a percent is eight
// cents of pitch, which is not audible, and in normal running the correction
// is a fraction of that.
//
// What is NOT here: any measurement of how it sounds. The numbers this class
// exposes -- fill, underruns, dropped -- are what to look at instead.
// ---------------------------------------------------------------------------

import workletUrl from './pcm-worklet.js?url';

/** Samples the ring can hold: about three quarters of a second. The fill
 *  normally sits near the target; this is headroom for a stalled main thread. */
const DEFAULT_CAPACITY = 32768;

/** Where the fill level is kept. About 70ms, which is enough slack for a
 *  garbage collection pause and little enough that a sound effect still lands
 *  near the frame that caused it. */
const DEFAULT_TARGET_FILL = 3072;

/** Controller gain. The fill converges with a time constant of roughly
 *  target / (44100 * gain), so 0.02 is about three and a half seconds. */
const RATE_CONTROL_GAIN = 0.02;

/** The most the emulator's speed may be bent, as a fraction. Half a percent is
 *  eight cents: inaudible, and enough to absorb any real crystal. */
const MAX_RATE_CORRECTION = 0.005;

const WRITE = 0;
const READ = 1;
const UNDERRUNS = 2;
const CAPACITY = 3;
const CONTROL_WORDS = 4;
const CONTROL_BYTES = CONTROL_WORDS * 4;

export class AudioOutput {
    readonly #context: AudioContext;
    readonly #control: Uint32Array;
    readonly #samples: Float32Array;
    readonly #capacity: number;
    readonly #targetFill: number;

    #dropped = 0;
    #silentFrames = 0;

    private constructor(
        context: AudioContext,
        buffer: SharedArrayBuffer,
        capacity: number,
        targetFill: number,
    ) {
        this.#context = context;
        this.#capacity = capacity;
        this.#targetFill = targetFill;
        this.#control = new Uint32Array(buffer, 0, CONTROL_WORDS);
        this.#samples = new Float32Array(buffer, CONTROL_BYTES, capacity);
    }

    /**
     * Build the pipeline and hand the shared buffer to the audio thread.
     *
     * Throws if SharedArrayBuffer is missing, which means the page is not
     * cross origin isolated: COOP and COEP have to be on the response, both
     * for the dev server and for app:// in the main process.
     */
    static async create(options: {
        capacity?: number;
        targetFill?: number;
        /** The core's rate: 44100 for the NES, 65536 for a GBA, 131072 for a
         *  Game Boy. The context runs at it, so nothing resamples on the way
         *  in; Chromium resamples to the device at the far end. */
        sampleRate?: number;
    } = {}): Promise<AudioOutput> {
        const sampleRate = options.sampleRate ?? 44100;
        // The ring's sizes are times, not sample counts: three quarters of a
        // second of headroom and a seventy millisecond target. A core that
        // runs faster gets proportionally more samples to hold the same time.
        const scale = sampleRate / 44100;
        const capacity = options.capacity ?? Math.round(DEFAULT_CAPACITY * scale);
        const targetFill = options.targetFill ?? Math.round(DEFAULT_TARGET_FILL * scale);

        // SharedArrayBuffer is the actual requirement. Cross origin isolation
        // is the usual way a browser grants it, but it is not the only way:
        // Electron exposes SharedArrayBuffer to renderers when the
        // SharedArrayBuffer feature is enabled, and then it works while
        // `crossOriginIsolated` is still false. Requiring isolation as well
        // meant audio worked in the packaged app (app:// is isolated) and
        // silently did not from the dev server -- the worst way for the two to
        // differ.
        //
        // So: ask for the capability, not for the mechanism.
        if (typeof SharedArrayBuffer !== 'function') {
            throw new Error(
                'audio needs shared memory, and SharedArrayBuffer is missing. '
                + 'The main process has to enable the SharedArrayBuffer feature, '
                + 'and the page has to be served with Cross-Origin-Opener-Policy: '
                + 'same-origin and Cross-Origin-Embedder-Policy: require-corp.',
            );
        }

        // Ask for the emulator's own rate, so there is no resampling between
        // the APU and the ring. Chromium still resamples to the device at the
        // far end if the hardware runs at 48kHz.
        const context = new AudioContext({ sampleRate });

        const url = new URL(workletUrl, document.baseURI).href;
        await context.audioWorklet.addModule(url);

        const buffer = new SharedArrayBuffer(CONTROL_BYTES + capacity * 4);
        const output = new AudioOutput(context, buffer, capacity, targetFill);

        const node = new AudioWorkletNode(context, 'pcm-sink', {
            numberOfInputs: 0,
            numberOfOutputs: 1,
            outputChannelCount: [1],
        });
        node.connect(context.destination);

        new Uint32Array(buffer, 0, CONTROL_WORDS)[CAPACITY] = capacity;
        node.port.postMessage({ type: 'ring', sab: buffer });

        return output;
    }

    /**
     * Copy samples into the ring. Extra samples are dropped and counted: a
     * full ring means the emulator is producing faster than the speaker is
     * consuming, and the rate control above is what is supposed to stop that
     * from ever happening.
     */
    push(source: Float32Array): void {
        if (source.length === 0) {
            return;
        }

        const control = this.#control;
        const write = Atomics.load(control, WRITE);
        const read = Atomics.load(control, READ);
        const free = this.#capacity - ((write - read) >>> 0);

        const count = Math.min(source.length, free);
        for (let i = 0; i < count; i += 1) {
            this.#samples[(write + i) % this.#capacity] = source[i];
        }

        if (count < source.length) {
            this.#dropped += source.length - count;
        }

        // Release: the samples are visible before the audio thread sees the
        // new write index.
        Atomics.store(control, WRITE, (write + count) >>> 0);
    }

    /**
     * How much of the emulator's speed to give up, as a fraction.
     *
     * Positive means "run slower": the ring holds more than it should, so the
     * emulator is producing too fast. Negative means the opposite. Multiply
     * the frame duration by 1 + this.
     */
    get rateCorrection(): number {
        const error = (this.fill - this.#targetFill) / this.#targetFill;
        const clamped = Math.max(-1, Math.min(1, error));
        return Math.max(
            -MAX_RATE_CORRECTION,
            Math.min(MAX_RATE_CORRECTION, clamped * RATE_CONTROL_GAIN),
        );
    }

    get fill(): number {
        const write = Atomics.load(this.#control, WRITE);
        const read = Atomics.load(this.#control, READ);
        return (write - read) >>> 0;
    }

    get targetFill(): number {
        return this.#targetFill;
    }

    get capacity(): number {
        return this.#capacity;
    }

    /**
     * Throw away everything queued, without touching where the audio thread is
     * reading.
     *
     * Used when the machine stops or jumps. Without it, pausing for a moment
     * and resuming plays a second of audio from before the pause, and loading
     * a save state plays a second of the state that was just abandoned.
     */
    clear(): void {
        const write = Atomics.load(this.#control, WRITE);
        Atomics.store(this.#control, READ, write);
        this.#dropped = 0;
    }

    /** Times the audio thread ran out. The first thing to look at when the
     *  sound crackles. */
    get underruns(): number {
        return Atomics.load(this.#control, UNDERRUNS);
    }

    /** Samples the ring had no room for. Should stay at zero; if it does not,
     *  the rate control is losing. */
    get dropped(): number {
        return this.#dropped;
    }

    /** Frames during which the APU produced nothing at all. */
    noteSilentFrame(): void {
        this.#silentFrames += 1;
    }

    get silentFrames(): number {
        return this.#silentFrames;
    }

    get state(): string {
        return this.#context.state;
    }

    /**
     * Chromium in Electron is configured to allow audio without a user
     * gesture, but a development build run in a browser is not, and a
     * suspended context is silent with no visible reason. Resuming is cheap
     * and idempotent, so it happens on start and on the first key press.
     */
    async resume(): Promise<void> {
        if (this.#context.state === 'suspended') {
            await this.#context.resume();
        }
    }

    async close(): Promise<void> {
        if (this.#context.state !== 'closed') {
            await this.#context.close();
        }
    }
}
