// ---------------------------------------------------------------------------
// The audio thread's half of the ring buffer.
//
// This file runs in an AudioWorkletGlobalScope: a separate thread, a separate
// JavaScript context, no DOM, and -- importantly -- no access to the
// WebAssembly module, which lives on the page's thread. So it cannot call
// fc_take_samples. The only thing the two threads can share is a
// SharedArrayBuffer, and that is what this uses.
//
//   page thread                                audio thread
//   -----------                                ------------
//   engine.takeSamples()                       process(128 frames)
//        |                                          |
//        v                                          v
//   ring.push()  ---- SharedArrayBuffer ----  ring.read()
//
// Why not just postMessage the samples over?
//
// You can, and it works. But every message is a structured clone and an
// allocation on both sides, sixty times a second, forever, and the audio
// thread would rather not allocate at all. More to the point, a message that
// has not arrived yet is indistinguishable from silence, so the buffer has to
// be deeper to cover the message latency. Shared memory has no latency and no
// copies: the producer writes the samples where the consumer reads them.
//
// The one rule that makes this safe without a lock: the audio thread never
// waits. If the ring is empty it outputs silence and counts the underrun. A
// callback that blocks is a callback that misses its deadline, and the
// speaker clicks. That is also why this is not written as a worker with a
// mutex -- see packages/fc-core/src/ffi/emulator_api.h for the same argument in C.
//
// (Plain JavaScript rather than TypeScript because Vite imports this with
// `?url`, which copies the file as-is instead of transpiling it. addModule
// needs a URL, not a module the bundler has already wrapped.)
// ---------------------------------------------------------------------------

// The control block: four unsigned 32 bit words before the samples.
const WRITE = 0;      // monotonic, written by the page thread
const READ = 1;       // monotonic, written by the audio thread
const UNDERRUNS = 2;  // how many times the ring was short
const CAPACITY = 3;   // samples the ring can hold

const CONTROL_WORDS = 4;
const CONTROL_BYTES = CONTROL_WORDS * 4;

class PcmSink extends AudioWorkletProcessor {
    #control = null;
    #samples = null;
    #capacity = 0;

    constructor() {
        super();

        // The buffer is handed over once, after the context starts.
        this.port.onmessage = (event) => {
            if (event.data?.type !== 'ring') {
                return;
            }
            this.#control = new Uint32Array(event.data.sab, 0, CONTROL_WORDS);
            this.#capacity = Atomics.load(this.#control, CAPACITY);
            this.#samples = new Float32Array(event.data.sab, CONTROL_BYTES, this.#capacity);
        };
    }

    /**
     * Called by the audio thread. Must fill `outputs[0][0]` and return
     * quickly; it runs every 2.9ms at 44.1kHz and 128 frames.
     */
    process(_inputs, outputs) {
        const channel = outputs[0]?.[0];
        if (channel === undefined) {
            return true;
        }

        const frames = channel.length;
        let taken = 0;

        if (this.#control !== null) {
            // Acquire: everything the page thread wrote before it published
            // the new write index is now visible.
            const read = Atomics.load(this.#control, READ);
            const write = Atomics.load(this.#control, WRITE);

            // Unsigned difference, which stays correct across the wrap of the
            // counters: the true value is always far below 2^31.
            const available = (write - read) >>> 0;
            taken = available < frames ? available : frames;

            for (let i = 0; i < taken; i += 1) {
                channel[i] = this.#samples[(read + i) % this.#capacity];
            }

            if (taken > 0) {
                // Release: the samples above are visible before the page
                // thread sees the space as free.
                Atomics.store(this.#control, READ, (read + taken) >>> 0);
            }
        }

        // The tail must be silence, never whatever was in the output buffer
        // last time. An underrun is a short gap, not a burst of noise.
        channel.fill(0, taken);

        if (taken < frames && this.#control !== null) {
            Atomics.add(this.#control, UNDERRUNS, 1);
        }

        // Keep running. Returning false would let the browser shut the node
        // down, and there is always more audio coming.
        return true;
    }
}

registerProcessor('pcm-sink', PcmSink);
