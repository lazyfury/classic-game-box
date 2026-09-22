// ---------------------------------------------------------------------------
// The engine loop.
//
// Everything that has to happen once per frame, once per second, and once per
// lifetime lives here, so the component below it can be a canvas and nothing
// else.
//
// Who owns the clock
// ------------------
// The NES does not run at 60Hz. It runs at 60.0988, because the PPU's dot
// clock divides the NTSC colour burst that way, and that 0.16% is not noise:
// over ten minutes it is a whole second of drift, and the music will not line
// up with the picture.
//
// A display, on the other hand, refreshes at exactly 60.000Hz, and the only
// event a web page gets is requestAnimationFrame, which fires once per
// refresh. So the renderer cannot run "one frame per animation frame" -- that
// is the subtle bug the earlier frame-per-refresh loops shipped with, and it
// makes the game run 0.16% slow.
//
// What works instead is an accumulator. Keep the time the next emulated frame
// is *due*, and on every animation frame run however many frames have come
// due since last time -- usually one, occasionally two, sometimes none. The
// game then runs at the right speed, and the cost is that the picture judders
// by up to one frame, which is the correct trade.
//
// The alternative, running frames in a Web Worker on its own clock, is more
// accurate still and will be worth doing when audio needs it. It is not
// needed to draw a correct picture.
// ---------------------------------------------------------------------------

import { useEffect, useRef, useState, type RefObject } from 'react';
import { Button } from '@wasm';
import { createCoreHost } from '@libretro';
import { chooseCore, sameCore, type CoreChoice } from './systems';

import { AudioOutput } from './audio/output';
import { bootLog } from '../shared/boot';
import {
    DEFAULT_INPUT_SETTINGS, type Cheat, type CoreSelection, type InputSettings,
} from '../shared/api';
import { resolveBindings, padPort } from './bindings';
import { commandMaps } from './commands';
import { INITIAL_STATUS, unloaded, type EngineStatus } from './engineStatus';
import {
    GamepadSource, NativeGamepadSource, NO_PADS, samePads,
    type GamepadInput,
} from './gamepad';
import { attachKeyboard, InputManager, type ButtonName, type CommandName } from './input';
import { Rewind } from './rewind';

/**
 * How many frames one animation frame is allowed to catch up on.
 *
 * Without a cap, one long stall -- a breakpoint, a laptop waking up, a virus
 * scanner -- would leave the accumulator permanently behind, and the emulator
 * would spend the next minute running at double speed trying to catch up. Past
 * this many, the backlog is abandoned instead.
 */
const MAX_CATCHUP_FRAMES = 4;

/** How often the status line is refreshed. Sixty React renders a second to
 *  update a text field is waste, and it shows up as jitter. */
const STATUS_INTERVAL_MS = 250;

/**
 * How much play time is collected before it is written down.
 *
 * The write is a SQLite UPDATE in another process, and doing it every frame
 * would be sixty of them a second to add sixteen milliseconds each. A few
 * seconds of remainder lost to a power cut or a kill -9 is the price, and it
 * is a small one: what has already been flushed can never be lost, because
 * every write adds rather than replaces.
 */
const PLAY_FLUSH_SECONDS = 5;

export interface EmulatorHandle {
    status: EngineStatus;
    /** Load a game by path. False if it could not be read or is not a ROM. */
    loadRom(path: string): Promise<boolean>;
    /** Take the cartridge out and stop running. */
    unload(): void;
    /**
     * Power the console back on with the cartridge already in the slot.
     *
     * This is how a core change reaches the machine: `loadRom` would count a
     * play, but swapping the emulator is not playing the game again. With no
     * cartridge in the slot it does nothing.
     */
    reload(): void;
    /**
     * The same commands the keyboard sends: pause, reset, screenshot, save
     * and load.
     *
     * The toolbar buttons are not a second implementation of any of them --
     * they go through the one command handler, so a button and F1 cannot
     * disagree about what "save" means.
     */
    command(command: CommandName): void;
    /**
     * Say that the picture is hidden -- the screenshot preview is over it.
     *
     * The machine pauses while it is, and lets go of everything the player was
     * holding. It is not a courtesy: a game that kept running behind a picture
     * nobody can see is a game playing itself, and a preview is exactly when a
     * player stops watching. The player's *own* pause is separate and comes
     * back the way it was.
     */
    setPictureHidden(hidden: boolean): void;
    /** Write one byte into the console's memory, now. */
    poke(address: number, value: number): void;
    /** Read one byte of console or cartridge RAM, or null with no machine. */
    peek(address: number): number | null;
}

/** Things the loop has to report to somebody else. */
export interface EmulatorHandlers {
    /**
     * The cheats in force for the cartridge in the slot.
     *
     * Read from a ref rather than captured, for the same reason the input
     * settings are: the machine is built once and the list may change an hour
     * later, and rebuilding it to change a byte would power the console off.
     */
    cheats?: Cheat[];
    /**
     * The input settings in force: keyboard mode, bindings, pad assignments.
     *
     * Read at every key event and every frame rather than captured, so a
     * rebinding made in the settings screen takes effect without rebuilding
     * the listeners or the machine.
     */
    input?: InputSettings;
    /**
     * Whether commands are being accepted at all.
     *
     * False while the console is not the thing on screen -- a settings group
     * in its place, or a screenshot over it -- and asked on every command
     * rather than remembered, because the answer changes as the player moves
     * around the window.
     *
     * It is not the same question as "is the machine paused". Pausing stops
     * the *game*; this stops the *application*, and the difference is a
     * screenshot preview where the arrows are walking through pictures: the
     * pad button bound to 存档 must not save while somebody is binding it in
     * the settings screen, and it must not save while they are looking at a
     * picture either. Nobody is playing, so nothing is commanded.
     */
    commandsAllowed?: () => boolean;
    /**
     * The core the player chose for each console.
     *
     * Read through the ref at load time, like the input settings, so that a
     * change made on the settings screen is picked up by the next load -- and
     * by `reload`, which is what a change made mid-game goes through. The
     * machine is not rebuilt just because this object changed; the caller
     * calls reload() when it wants that.
     */
    cores?: CoreSelection;
    /**
     * A PNG of the picture, when the player takes a screenshot.
     *
     * `asCover` is true for Shift+F12 and the 更新封面 button: the picture is
     * not only kept, it becomes the one on the game's card.
     *
     * Handed up as bytes rather than written here, because the canvas is in
     * this process and the disk is in the other one.
     */
    onScreenshot?: (png: Uint8Array, asCover: boolean) => void;
}

export function useEmulator(
    canvasRef: RefObject<HTMLCanvasElement | null>,
    handlers: EmulatorHandlers = {},
): EmulatorHandle {
    const [status, setStatus] = useState<EngineStatus>(INITIAL_STATUS);

    // Kept in a ref so that the effect below can be set up once. A caller
    // passing a new object every render -- which is what an inline object does
    // -- would otherwise tear down and rebuild the whole machine.
    const outward = useRef(handlers);
    useEffect(() => {
        outward.current = handlers;
    });

    // The two things a caller can ask for, held in a ref so that the functions
    // handed out never change identity. A component that put them in a
    // dependency array would otherwise reload the emulator on every render.
    const actions = useRef<{
        loadRom: (path: string) => Promise<boolean>;
        loadInto: (path: string, countPlay: boolean) => Promise<boolean>;
        reload: () => Promise<boolean>;
        unload: () => void;
        command: (command: CommandName) => void;
        poke: (address: number, value: number) => void;
        peek: (address: number) => number | null;
        applyCheats: () => void;
        setPictureHidden: (hidden: boolean) => void;
    }>({
        loadRom: async () => false,
        loadInto: async () => false,
        reload: async () => false,
        unload: () => undefined,
        command: () => undefined,
        poke: () => undefined,
        peek: () => null,
        applyCheats: () => undefined,
        setPictureHidden: () => undefined,
    });

    useEffect(() => {
        const canvas = canvasRef.current;
        if (canvas === null) {
            return;
        }
        bootLog('renderer', 'emulator effect start');

        // alpha:false because every pixel is opaque and the compositor should
        // not have to blend the whole screen every frame.
        const context = canvas.getContext('2d', { alpha: false });
        if (context === null) {
            setStatus({ ...INITIAL_STATUS, state: 'error', error: 'the canvas has no 2d context' });
            return;
        }

        let disposed = false;
        let animationFrame = 0;
        let emulator: CoreHost | null = null;
        let detachKeyboard: (() => void) | null = null;
        let commandHandler: ((command: CommandName) => void) | null = null;
        let gamepad: GamepadInput | null = null;
        let rewind: Rewind | null = null;
        // The flash message's timeout lives out here because the cleanup has
        // to be able to cancel it.
        let flashTimer = 0;
        let audio: AudioOutput | null = null;
        let audioError: string | null = null;

        // Input does not depend on the machine, so it is set up here rather
        // than inside start(). The keyboard and the pad exist while the player
        // is still browsing the library, which is what lets the footer say
        // whether a pad is connected; presses made before a cartridge exists
        // are dropped by the null check in the apply callback below.
        //
        // `nextFrameTime` and `rewinding` are the two pieces of loop state the
        // keyboard touches, so they are declared out here too. The rest of the
        // loop state still lives in start().
        let nextFrameTime = performance.now() / 1000;
        let rewinding = false;
        /**
         * Whether the picture is hidden -- the screenshot preview is over it.
         *
         * Out here, with the two above, because it is a fact about the window
         * and not about the console: a preview can be opened before any game
         * is loaded, and a game loaded while it is open has to come up paused
         * rather than running for as long as it takes something to notice.
         * `start()` reads it when it is called; the machine's own handler keeps
         * it up to date.
         */
        let pictureHidden = false;
        let engineApply: ((port: number, button: ButtonName, pressed: boolean) => void) | null = null;

        // The settings in force, read afresh each time they are needed. The
        // listeners and the machine are built once; what a key does may change
        // an hour later.
        const inputSettings = (): InputSettings =>
            outward.current.input ?? DEFAULT_INPUT_SETTINGS;

        // One manager for every input source and both ports. The keyboard
        // reports into it, and so does every pad, and the console will only
        // ever see the combined state. See input.ts for why that indirection
        // is not optional.
        const manager = new InputManager((port, button, pressed) => {
            engineApply?.(port, button, pressed);
        });

        detachKeyboard = attachKeyboard(manager, {
            bindings: () => resolveBindings(inputSettings()),
            commands: () => commandMaps(inputSettings()),
            onCommand: (command) => commandHandler?.(command),
            onCommandState: (command, held) => {
                if (command === 'rewind') {
                    rewinding = held;
                    if (!held) {
                        // Coming out of a rewind, do not run a burst of frames
                        // to make up for the time it took.
                        nextFrameTime = performance.now() / 1000;
                    }
                }
            },
        });

        // The second input source. It reports into the same manager, so a
        // button held on a pad is not dropped by letting go of a key.
        //
        // Which source depends on the bridge, and the bridge decides: the
        // native helper is a separate process using GameController, so
        // Chromium never starts its own HID service and the application can
        // still quit. The browser source is the fallback -- and on macOS the
        // reason it is only a fallback. See FcBridge.
        //
        // Every branch says so in the log, because the ways this can be silent
        // look identical from the outside: a source that was never started,
        // and a source that started and found nothing, both produce no
        // `gamepad:` lines at all.
        if (window.fc.gamepadNative) {
            gamepad = new NativeGamepadSource(
                manager,
                (index) => padPort(inputSettings(), index),
                () => commandMaps(inputSettings()),
                (command) => commandHandler?.(command),
            );
            console.log('gamepad: native source started, watching for a pad');
        } else if (window.fc.gamepadEnabled) {
            gamepad = new GamepadSource(
                manager,
                (index) => padPort(inputSettings(), index),
                () => commandMaps(inputSettings()),
                (command) => commandHandler?.(command),
            );
            console.log('gamepad: browser source started, watching for a pad');
        } else {
            console.log('gamepad: not enabled -- see the main process log for why');
        }

        // Pads are polled from the moment the effect runs, not from the moment
        // a cartridge is loaded.
        //
        // This used to live in the frame loop, and the frame loop does not
        // start until a game does -- so the pad list stayed empty while the
        // library was on screen, which is exactly when somebody tries to
        // assign a pad to a player. The pad is on the desk whether or not
        // there is a game in the slot, and a reading has to reach the screen
        // to be assignable.
        //
        // A loop of its own rather than part of `tick`, because the two have
        // nothing to do with each other: this one runs at the display's rate
        // for as long as the page is open, and does nothing but ask the pad
        // source and publish a changed answer.
        let padFrame = 0;
        const pumpPads = (): void => {
            padFrame = requestAnimationFrame(pumpPads);

            // The browser's Gamepad API has no change events, so this is a
            // poll; the native source's poll() is empty because its helper
            // pushes. Both are called the same way so the loop does not have
            // to know which one it has.
            gamepad?.poll();

            const pad = gamepad?.report ?? NO_PADS;

            // Compared against the status itself rather than a local copy.
            // Anything that replaces the status wholesale -- building the
            // machine does -- would otherwise leave the screen showing no pads
            // while the pads were still on the desk, because a local copy
            // would still say "nothing changed".
            setStatus((s) => (samePads(s.gamepad, pad) ? s : { ...s, gamepad: pad }));
        };

        // A scripted run has no gamepad source and must stay deterministic, so
        // there is nothing to pump.
        if (!window.fc.selftestOnly) {
            padFrame = requestAnimationFrame(pumpPads);
        }

        const fail = (error: string): void => {
            setStatus({ ...INITIAL_STATUS, state: 'error', error });
        };

        const start = async (): Promise<void> => {
            bootLog('renderer', 'machine build begin');
            const buildStarted = Date.now();
            setStatus((s) => ({ ...s, state: 'loading' }));

            // Which console. A game the player picked names its own core by
            // its extension; a game on the command line is the only other
            // thing that can, so it is fetched here, before anything is built.
            const bootRomStarted = Date.now();
            const boot = await window.fc.getBootRom();
            bootLog(
                'renderer',
                'getBootRom',
                boot === null
                    ? `${Date.now() - bootRomStarted}ms, no --rom`
                    : `${Date.now() - bootRomStarted}ms, ${boot.path}`,
            );
            const core = pendingCore ?? chooseCore(boot?.path ?? '', outward.current.cores);
            coreInUse = core;
            bootLog('renderer', 'core', `${core.id} (${core.module}), ${core.sampleRate} Hz`);

            // Where the core's JavaScript lives depends on whether this page
            // came from the Vite dev server or the app:// protocol, so ask the
            // document instead of hardcoding. `@vite-ignore` stops Vite trying
            // to bundle a path it cannot resolve at build time.
            //
            // This is a libretro core. wasm/libretro.mjs is the front end half
            // of the ABI and does not care which core it is given; the file
            // name is the whole of the difference between the NES and a Game
            // Boy.
            const moduleUrl = new URL(core.module, document.baseURI).href;
            bootLog('renderer', `import ${core.module} begin`, moduleUrl);
            const importStarted = Date.now();
            const factory = (await import(/* @vite-ignore */ moduleUrl)) as {
                default: () => Promise<unknown>;
            };
            bootLog('renderer', `import ${core.module} done`, `${Date.now() - importStarted}ms`);

            // Instantiating the module is where the .wasm is compiled and the
            // linear memory is set aside, and it is usually the single largest
            // cost in a cold start -- so it gets its own line rather than being
            // folded into "machine build".
            const wasmStarted = Date.now();
            const wasm = await factory.default();
            bootLog('renderer', 'instantiate wasm', `${Date.now() - wasmStarted}ms`);
            if (disposed) {
                return;
            }

            const createStarted = Date.now();
            emulator = await createCoreHost(wasm);
            bootLog('renderer', 'createCoreHost', `${Date.now() - createStarted}ms`);
            const engine = emulator;
            if (disposed) {
                engine.destroy();
                emulator = null;
                return;
            }

            // The cheat list and the debugging window belong to the machine,
            // so they are wired the moment there is one. `applyCheats` reads
            // the list from the ref, so a change made while the library is on
            // screen is picked up by the first game loaded.
            actions.current.poke = (address, value) => engine.poke(address, value);
            actions.current.peek = (address) => (disposed ? null : engine.peek(address));
            actions.current.applyCheats = () => engine.setCheats(outward.current.cheats ?? []);
            actions.current.applyCheats();

            // A machine with no cartridge in it yet. The library screen runs
            // over the top of this, and a game is loaded when the player picks
            // one -- or straight away, if the command line named one.
            let loaded = false;
            // The cartridge in the slot, so a rebuild can put it back. Only
            // meaningful while `loaded` is true.
            let loadedPath: string | null = null;

            // Now that there is a machine, point the input manager at it. The
            // keyboard and the pad were set up when the effect ran; they start
            // reaching the console the moment this is assigned.
            engineApply = (port, button, pressed) =>
                engine.setButton(Button[button], pressed, port);

            // Audio. This can fail -- SharedArrayBuffer needs the page to be
            // cross origin isolated -- and when it does the game still runs,
            // silently. Reporting that in the status line is better than
            // throwing away a working picture because the speaker was busy.
            const audioStarted = Date.now();
            try {
                // The core's own rate, so nothing resamples on the way in.
                // 44100 for the NES, 65536 for a GBA, 131072 for a Game Boy;
                // Chromium resamples to the device at the far end.
                audio = await AudioOutput.create({ sampleRate: core.sampleRate });
                audio.resume().catch(() => undefined);
                bootLog(
                    'renderer',
                    'AudioOutput.create',
                    `${Date.now() - audioStarted}ms state=${audio.state}`,
                );
            } catch (error) {
                audioError = error instanceof Error ? error.message : String(error);
                audio = null;
                bootLog(
                    'renderer',
                    'AudioOutput.create FAILED',
                    `${Date.now() - audioStarted}ms  ${audioError}`,
                );
            }
            if (disposed) {
                void audio?.close();
                audio = null;
                engine.destroy();
                emulator = null;
                return;
            }

            // The picture is sized when a cartridge goes in, not now: a
            // libretro core reports its geometry only once it has a game in it
            // (mGBA) or whenever it is asked (the NES core), and sizing before
            // the former would leave a canvas of the wrong shape. Until then a
            // one pixel placeholder.
            let image = context.createImageData(1, 1);
            const sizeCanvas = (): void => {
                canvas.width = engine.width;
                canvas.height = engine.height;
                image = context.createImageData(engine.width, engine.height);
            };
            sizeCanvas();
            let firstBlitLogged = false;

            // The framebuffer view is taken inside blit(), not here. The fc_*
            // host has a picture pointer as soon as the cartridge is in, but
            // the libretro host only learns where the picture is when the core
            // first hands one over -- and a view taken before that is empty.
            // Taking it again is a subarray, so it costs nothing either way.

            /**
             * Turn the emulator's pixels into an image on the canvas.
             *
             * The emulator stores 0x00RRGGBB, so little endian in memory the
             * bytes run B, G, R, 0. A canvas wants R, G, B, A. This loop is
             * the whole of the conversion, 61440 iterations, roughly a fifth
             * of a millisecond. Later this can become a WebGL texture with a
             * BGRA format and cost nothing at all; it is not yet worth the
             * complexity, and it is worth measuring before optimising.
             */
            const blit = (): void => {
                // The wasm heap never grows (see wasm/CMakeLists.txt), so this
                // view into it cannot be detached, and re-taking it each frame
                // is what lets a host whose pointer arrives late still work.
                const framebuffer = engine.framebufferBytes();
                const destination = image.data;
                // The rows are stepped by the core's pitch, not by width * 4.
                // mGBA renders 240 visible pixels into a 256 pixel wide buffer
                // and says so; treating the stride as 960 shears every row by
                // 64 bytes, which is diagonal stripes.
                const stride = engine.pitch ?? engine.width * 4;
                let out = 0;
                for (let y = 0; y < image.height; ++y) {
                    let source = y * stride;
                    for (let x = 0; x < image.width; ++x, source += 4, out += 4) {
                        destination[out] = framebuffer[source + 2];
                        destination[out + 1] = framebuffer[source + 1];
                        destination[out + 2] = framebuffer[source];
                        destination[out + 3] = 255;
                    }
                }
                context.putImageData(image, 0, 0);
                if (!firstBlitLogged) {
                    firstBlitLogged = true;
                    bootLog('renderer', 'first frame on screen', `${Date.now() - buildStarted}ms`);
                }
            };

            /**
             * Empty the APU's queue: measure it, and hand it to the speaker.
             *
             * This is not free to skip. The APU queues samples until somebody
             * takes them, and with a fixed 64MB heap a front end that never
             * drains would run the emulator out of memory in about a quarter
             * of an hour.
             */
            const drainAudio = (): number => {
                const samples = engine.takeSamples();
                if (samples.length === 0) {
                    return 0;
                }

                let peak = 0;
                for (let i = 0; i < samples.length; i += 1) {
                    const sample = samples[i];
                    if (sample > peak) {
                        peak = sample;
                    }
                }

                audio?.push(samples);
                return peak;
            };

            /**
             * A fingerprint of the picture on screen, so a test can compare
             * this window against another build without anybody having to look
             * at it.
             *
             * The bytes hashed are the R,G,B triples, which is exactly what a
             * PPM holds after its 15 byte header -- so a hash taken here can be
             * compared with `tail -c +16 frame.ppm | shasum -a 256`.
             *
             * A screenshot would not do. It has the browser's scaling in it,
             * and a picture that is subtly wrong in a way nobody notices is
             * precisely what these tests exist to catch.
             */
            const hashPixels = async (): Promise<string> => {
                const picture = engine.framebufferBytes();
                const rgb = new Uint8Array(engine.width * engine.height * 3);
                // Same stride rule as blit: the rows are pitch apart, not
                // width * 4 apart, or a hash of a Game Boy screen would be a
                // hash of sheared rows.
                const stride = engine.pitch ?? engine.width * 4;
                let out = 0;
                for (let y = 0; y < engine.height; ++y) {
                    let source = y * stride;
                    for (let x = 0; x < engine.width; ++x, source += 4, out += 3) {
                        rgb[out] = picture[source + 2];
                        rgb[out + 1] = picture[source + 1];
                        rgb[out + 2] = picture[source];
                    }
                }
                const digest = await crypto.subtle.digest('SHA-256', rgb);
                return Array.from(new Uint8Array(digest))
                    .map((byte) => byte.toString(16).padStart(2, '0'))
                    .join('');
            };

            let paused = false;
            /**
             * The same fact, as this machine sees it: the picture is not on
             * screen, so the loop stops.
             *
             * Kept apart from `paused`, which is the player's own switch:
             * closing the preview has to put the game back the way the player
             * left it, and one flag could not remember that. It starts from
             * `pictureHidden` rather than from false, because the preview may
             * already be open.
             */
            let covered = pictureHidden;
            let windowFrames = 0;
            let windowStartedAt = performance.now();
            let windowPeak = 0;

            // Kept for the whole run, not just the current status window, so
            // `--audiotest` can ask whether the game ever made a sound.
            let peakSeen = 0;

            /**
             * Play time, counted in emulated frames.
             *
             * Counting frames rather than reading a clock is the whole
             * design: a frame is a known number of seconds (see systems.ts),
             * and frames are the only thing that actually happened. A paused
             * game, a minimised window and a lunch break with the game on the
             * title screen all add exactly zero.
             *
             * `playPath` is the game the seconds belong to, and it is set by
             * applyRom; the remainder is flushed whenever a cartridge comes
             * out *or* goes in, so the tail of one game is never credited to
             * the next. Only the whole seconds are sent: the column is an
             * integer and the fraction belongs to the game still running.
             */
            let playSeconds = 0;
            let playPath: string | null = null;

            const flushPlaytime = (): void => {
                if (playPath === null || playSeconds < 1) {
                    return;
                }
                const whole = Math.floor(playSeconds);
                playSeconds -= whole;
                void window.fc.notePlaytime(playPath, whole);
            };

            const tick = (nowMs: number): void => {
                animationFrame = requestAnimationFrame(tick);
                const now = nowMs / 1000;

                // The pads are not polled here. See pumpPads above: a pad has
                // to be visible before a game is loaded, so its loop is its
                // own.

                // Rewinding: one snapshot per animation frame. A snapshot is
                // everyFrames frames apart, so this walks backwards at about
                // the speed the console ran forwards.
                if (rewinding && loaded) {
                    nextFrameTime = now;
                    if (rewind === null || !rewind.stepBack()) {
                        // At the beginning of the ring. Stop, rather than
                        // holding the key and doing nothing.
                        rewinding = false;
                    } else {
                        audio?.clear();
                        blit();
                        return;
                    }
                }

                // Paused: keep the clock pinned to now rather than letting the
                // backlog grow, so resuming runs the next frame immediately
                // instead of running four frames to catch up.
                if (paused || covered || !loaded) {
                    nextFrameTime = now;
                    return;
                }

                if (now < nextFrameTime) {
                    return;
                }

                let ran = 0;
                while (now >= nextFrameTime && ran < MAX_CATCHUP_FRAMES) {
                    if (!engine.runFrame()) {
                        // The loop is ending, so this is the last chance to
                        // write down what this game has been played for.
                        flushPlaytime();
                        setStatus((s) => ({
                            ...s,
                            state: 'halted',
                            error: 'the CPU halted: the emulator met an opcode it does not implement',
                        }));
                        cancelAnimationFrame(animationFrame);
                        animationFrame = 0;
                        return;
                    }
                    const peak = drainAudio();
                    if (peak > windowPeak) {
                        windowPeak = peak;
                    }
                    if (peak > peakSeen) {
                        peakSeen = peak;
                    }

                    // The emulator's frame rate is nudged by up to half a
                    // percent so the audio ring stays at its target. See
                    // audio/output.ts: the system clock and the sound card's
                    // crystal disagree, and without this the ring slowly
                    // fills or empties until the player hears a gap.
                    rewind?.record();

                    nextFrameTime += core.frameSeconds * (1 + (audio?.rateCorrection ?? 0));
                    ran += 1;
                }

                if (ran === MAX_CATCHUP_FRAMES) {
                    nextFrameTime = now;
                }

                if (ran > 0) {
                    playSeconds += core.frameSeconds * ran;
                    if (playSeconds >= PLAY_FLUSH_SECONDS) {
                        flushPlaytime();
                    }
                }

                blit();

                windowFrames += ran;
                const elapsed = nowMs - windowStartedAt;
                if (elapsed >= STATUS_INTERVAL_MS) {
                    const fps = (windowFrames * 1000) / elapsed;
                    setStatus((s) => ({
                        ...s,
                        state: 'running',
                        fps,
                        frameCount: engine.frameCount,
                        totalCycles: engine.totalCycles,
                        cpuPc: engine.cpuPc,
                        audioPeak: windowPeak,
                        held: manager.held,
                        paused: paused || covered,
                        rewinding,
                        rewind: rewind === null || !rewind.available ? null : {
                            depth: rewind.depth,
                            seconds: rewind.seconds,
                            bytes: rewind.bytes,
                        },
                        audio: audio === null ? null : {
                            state: audio.state,
                            fill: audio.fill,
                            targetFill: audio.targetFill,
                            underruns: audio.underruns,
                            dropped: audio.dropped,
                        },
                    }));
                    windowFrames = 0;
                    windowPeak = 0;
                    windowStartedAt = nowMs;
                }
            };

            /**
             * Put a cartridge in the slot and start running.
             *
             * Also reaches the machine's own reset() and clears the audio, so
             * that changing games does not leave the previous one's sound
             * queued up or its keys held down.
             */
            const applyRom = (bytes: Uint8Array, path: string, countPlay = true): boolean => {
                bootLog('renderer', 'applyRom begin', path);
                if (!engine.loadRom(bytes)) {
                    flash(`could not load ${path}: ${engine.lastError}`);
                    return false;
                }
                bootLog('renderer', 'applyRom loaded', `${engine.width}x${engine.height}`);

                // What the native tools do before their loop, so a run here can
                // be compared against theirs cycle for cycle.
                engine.reset();
                bootLog('renderer', 'applyRom reset');

                // Now that there is a cartridge, the core knows its geometry.
                sizeCanvas();
                bootLog('renderer', 'applyRom canvas', `${engine.width}x${engine.height}`);

                // The cheats for this cartridge. The list is read from the ref
                // at load time, so a game loaded from the library gets the
                // list the screen is showing for it -- and a list that arrives
                // a moment later is applied by the effect below.
                engine.setCheats(outward.current.cheats ?? []);

                manager.releaseEverything();
                engine.releaseAllButtons();
                audio?.clear();

                // A new cartridge means the old snapshots describe a machine
                // that no longer exists, and the ring's slot size depended on
                // the old cartridge's RAM.
                rewind?.destroy();
                rewind = new Rewind(engine);
                bootLog('renderer', 'applyRom rewind', rewind.available ? 'on' : 'off');

                loaded = true;
                loadedPath = path;
                // The previous game's tail belongs to the previous game.
                flushPlaytime();
                playPath = path;
                playSeconds = 0;
                paused = false;
                // A rewind key held while the library was on screen must not
                // carry into the game that just started.
                rewinding = false;
                nextFrameTime = performance.now() / 1000;

                // A cartridge that halted the machine took the frame loop with
                // it -- see the `runFrame` failure above, which cancels the
                // animation frame -- so putting a new one in has to start the
                // loop again. Without this the game loads, the title appears,
                // and nothing runs.
                //
                // Not in self test mode: there the loop is deliberately never
                // started, so that N frames is exactly N frames.
                if (animationFrame === 0 && !window.fc.selftestOnly) {
                    animationFrame = requestAnimationFrame(tick);
                }

                setStatus((s) => ({
                    ...s,
                    state: 'running',
                    paused: false,
                    message: null,
                    // And the previous cartridge's halt, if that is what
                    // stopped the loop. Loading a game is not a way to keep
                    // reading "the emulator met an opcode it does not
                    // implement" for the rest of the session.
                    error: null,
                    romPath: path,
                    romSummary: engine.romSummary,
                }));

                if (countPlay) {
                    void window.fc.notePlayed(path);
                }
                // And which cartridge this is, so that saving and loading know
                // where the slots belong. The main process cannot work it out
                // from the command line alone -- a game picked out of the
                // library was never on it.
                void window.fc.setCartridge(path);
                return true;
            };

            /** Read the bytes and put them in the machine that is already built. */
            const loadRomIntoMachine = async (path: string, countPlay = true): Promise<boolean> => {
                const bytes = await window.fc.readRom(path);
                if (bytes === null) {
                    flash('could not read that game');
                    return false;
                }
                return applyRom(bytes, path, countPlay);
            };

            /**
             * Load a game, building the machine for it first if needed.
             *
             * Every entry point goes through here -- a click in the library,
             * the game named on the command line, a test, and a core change --
             * rather than only the lazy path, because the machine may already
             * exist for another console or another core: the application
             * started with a NES game on the default core, the player then
             * picks a Game Boy one, or asks for Mesen. Handing bytes to the
             * wrong core is what "the core did not accept this cartridge"
             * looks like.
             *
             * `countPlay` is false for a reload: swapping the emulator under a
             * running game is powering the console off and on, not playing the
             * game again.
             */
            const loadInto = async (path: string, countPlay: boolean): Promise<boolean> => {
                const wanted = chooseCore(path, outward.current.cores);
                if (!sameCore(coreInUse, wanted)) {
                    await stopMachine();
                    await startOnce(wanted);
                    // start() has just replaced actions.current with the
                    // loaders for the machine it built; let that one do the
                    // work, rather than handing bytes to the dead engine this
                    // closure captured.
                    return actions.current.loadInto(path, countPlay);
                }
                return loadRomIntoMachine(path, countPlay);
            };

            const loadRom = (path: string): Promise<boolean> => loadInto(path, true);

            /**
             * Put the cartridge already in the slot into a freshly built
             * machine, on whatever core is now preferred.
             *
             * The player changing the core while a game is running is the one
             * caller. With no cartridge there is nothing to restart.
             */
            const reload = (): Promise<boolean> => {
                if (!loaded || loadedPath === null) {
                    return Promise.resolve(false);
                }
                const wanted = chooseCore(loadedPath, outward.current.cores);
                if (sameCore(coreInUse, wanted)) {
                    // Nothing changed -- the caller's effect also runs on the
                    // first render, and a power cycle nobody asked for would
                    // be a visible stutter at start up.
                    return Promise.resolve(false);
                }
                return loadInto(loadedPath, false);
            };

            const unload = (): void => {
                // Before the cartridge comes out: what was played is still
                // owed to it, and after this line nobody knows whose it was.
                flushPlaytime();
                playPath = null;
                playSeconds = 0;
                loaded = false;
                loadedPath = null;
                paused = false;
                rewinding = false;
                rewind?.destroy();
                rewind = null;
                manager.releaseEverything();
                engine.releaseAllButtons();
                audio?.clear();
                // No cartridge, no save slots. The next game will name its own.
                void window.fc.setCartridge(null);

                // And no cartridge on screen either. `romPath` is what says a
                // cartridge is in the slot, and half the interface is derived
                // from it -- see unloaded() in engineStatus.ts, where the whole
                // transition lives so that it can be tested without a window.
                setStatus(unloaded);
            };

            actions.current = {
                ...actions.current, loadRom, loadInto, reload, unload,
                command: (c) => commandHandler?.(c),

                /**
                 * Stop for a picture, and start again when it is gone.
                 *
                 * Everything held is let go of on the way in. That is what
                 * keeps a key held across the transition -- open the preview
                 * with the arrow still down -- from arriving in a game that
                 * resumes with Mario already walking, because the key-up will
                 * be swallowed by the preview and the pad would otherwise stay
                 * pressed for good.
                 */
                setPictureHidden: (hidden) => {
                    pictureHidden = hidden;
                    if (hidden === covered) {
                        return;
                    }
                    covered = hidden;
                    if (covered) {
                        flushPlaytime();
                        manager.releaseEverything();
                        engine.releaseAllButtons();
                        audio?.clear();
                    } else {
                        // Now, so the first frame after coming back runs at
                        // once rather than four frames in a row catching up on
                        // the time the preview was up.
                        nextFrameTime = performance.now() / 1000;
                    }
                    setStatus((s) => ({ ...s, paused: paused || covered }));
                },
            };

            // A machine built while the preview was already open starts paused,
            // which is what the status line has to say before anything else
            // happens to it.
            if (covered) {
                setStatus((s) => ({ ...s, paused: true }));
            }

            /** A short note in the status line, gone again in two seconds. */
            const flash = (text: string): void => {
                setStatus((s) => ({ ...s, message: text }));
                window.clearTimeout(flashTimer);
                flashTimer = window.setTimeout(() => {
                    setStatus((s) => ({ ...s, message: null }));
                }, 2000);
            };

            const saveTo = async (slot: number): Promise<void> => {
                const bytes = engine.saveState();
                if (bytes === null) {
                    flash('nothing to save');
                    return;
                }
                const written = await window.fc.saveState(slot, bytes);
                flash(written ? `saved slot ${slot}` : `could not save slot ${slot}`);
            };

            const loadFrom = async (slot: number): Promise<void> => {
                const bytes = await window.fc.loadState(slot);
                if (bytes === null) {
                    flash(`slot ${slot} is empty`);
                    return;
                }
                if (!engine.loadState(bytes)) {
                    flash('that save is not for this game');
                    return;
                }
                // The audio queued before the jump describes a machine that no
                // longer exists, and neither do the snapshots.
                audio?.clear();
                rewind?.reset();
                flash(`loaded slot ${slot}`);
            };

            /**
             * Hand the picture on screen to the caller as a PNG.
             *
             * The canvas is read rather than the framebuffer, because the
             * canvas is the thing the player is looking at, and because
             * Chromium's encoder is right there. It is 256x240 -- the backing
             * store -- not whatever size it is drawn at: the scaling is a CSS
             * decision and a screenshot is a picture of the console's output,
             * not of this window.
             */
            const capture = (asCover: boolean): void => {
                canvas.toBlob((blob) => {
                    if (blob === null) {
                        flash('could not encode the picture');
                        return;
                    }
                    void blob.arrayBuffer().then((buffer) => {
                        outward.current.onScreenshot?.(new Uint8Array(buffer), asCover);
                    });
                }, 'image/png');
            };

            commandHandler = (command: CommandName): void => {
                // Nothing is commanded while the console is not on screen.
                // See `commandsAllowed`: the settings screen takes the pad for
                // its tester and its bindings, and a command that fired there
                // would be a save nobody asked for.
                // Logged before anything acts on it, so that "it fired twice"
                // can be told apart from "the second one came from somewhere
                // else" with the terminal open. The pads log their own line on
                // the way in; this one is the delivery.
                console.log(`command: ${command}`);

                if (!(outward.current.commandsAllowed?.() ?? true)) {
                    console.log(`command: ${command} refused -- the console is not on screen`);
                    return;
                }

                switch (command) {
                case 'pause':
                    paused = !paused;
                    if (paused) {
                        // A pause is where a session tends to end, so the
                        // remainder is written down here rather than waiting
                        // for a flush that may never come.
                        flushPlaytime();
                        audio?.clear();
                    } else {
                        nextFrameTime = performance.now() / 1000;
                        void audio?.resume();
                    }
                    setStatus((s) => ({ ...s, paused }));
                    flash(paused ? 'paused' : 'running');
                    break;
                case 'reset':
                    engine.reset();
                    audio?.clear();
                    rewind?.reset();
                    flash('reset');
                    break;
                case 'screenshot':
                    capture(false);
                    break;
                case 'screenshot-cover':
                    capture(true);
                    break;
                case 'save1': case 'save2': case 'save3':
                    void saveTo(Number(command.slice(4)));
                    break;
                case 'load1': case 'load2': case 'load3':
                    void loadFrom(Number(command.slice(4)));
                    break;
                case 'quicksave':
                    void saveTo(0);
                    break;
                case 'quickload':
                    void loadFrom(0);
                    break;
                }
            };

            // Ready, with no cartridge in the slot. applyRom() above fills in
            // the game's details when one arrives.
            setStatus({ ...INITIAL_STATUS, state: 'running', audioError });
            bootLog(
                'renderer',
                'machine ready',
                `${Date.now() - buildStarted}ms total (no cartridge loaded)`,
            );

            // The hook `pnpm run selftest` drives, through
            // webContents.executeJavaScript. Nothing in the game uses it.
            //
            // It takes a plan rather than a frame count because input has to
            // be part of the test. A scripted press is applied at the same
            // point in the frame the real one would be, and the picture is
            // hashed at each requested frame so the caller can compare more
            // than just the end state.
            window.__fc = {
                ready: true,

                /** Which switches are down, as the emulator has them. Used by
                 *  `electron . --keytest`, which presses real keys through
                 *  Chromium and then asks what arrived. */
                held: () => manager.held,

                /**
                 * Frames completed since power on. Paired with a reading taken
                 * a few seconds earlier, this is how `--audiotest` finds out
                 * whether the emulator kept up while audio was running.
                 */
                frames: () => engine.frameCount,

                /**
                 * The live audio pipeline: fill, underruns, dropped samples.
                 *
                 * `--audiotest` runs the application in real time and then
                 * asks for this, which is the only way to find out whether
                 * samples are reaching the audio thread. The self test cannot
                 * answer that, because it runs a whole run's worth of frames
                 * in one go and never touches the ring.
                 */
                audio: () => (audio === null ? { peak: peakSeen, error: audioError } : {
                    state: audio.state,
                    fill: audio.fill,
                    targetFill: audio.targetFill,
                    underruns: audio.underruns,
                    dropped: audio.dropped,
                    peak: peakSeen,
                    error: audioError,
                }),

                selftest: async (plan) => {
                    let error: string | null = null;
                    let peak = 0;
                    const audioChunks: Float32Array[] = [];

                    // Group the script by frame, so the inner loop does not
                    // search for each frame's events.
                    const byFrame = new Map<number, typeof plan.script>();
                    for (const event of plan.script) {
                        const events = byFrame.get(event.frame) ?? [];
                        events.push(event);
                        byFrame.set(event.frame, events);
                    }

                    const wanted = new Set(plan.snapshots);
                    const hashes: { frame: number; hash: string }[] = [];

                    // The round trip, through the real save path: serialize,
                    // hand the bytes to the main process, let it write them to
                    // disk, read them back and put the machine into them.
                    //
                    // It happens in the middle of every self test run, which
                    // means it is checked against the native build's own
                    // hashes: a round trip that changed anything would show up
                    // as a mismatch rather than needing a test of its own.
                    let roundtripError: string | null = null;

                    for (let frame = 1; frame <= plan.frames; frame += 1) {
                        // Pressed before the frame runs, which is where a real
                        // press lands: the game samples the port once per
                        // frame, during vblank.
                        for (const event of byFrame.get(frame) ?? []) {
                            engine.setButton(Button[event.button], event.pressed);
                        }

                        if (!engine.runFrame()) {
                            error = `the CPU halted at frame ${frame}`;
                            break;
                        }

                        // The ring the self test drives directly, because the
                        // loop below is not the animation frame loop and would
                        // otherwise never fill it.
                        rewind?.record();

                        // Drained by hand rather than through drainAudio():
                        // this loop runs a whole run's worth of frames in one
                        // go, which is far more than the ring holds, and the
                        // point here is to keep a copy to hash.
                        const samples = engine.takeSamples();
                        if (samples.length > 0) {
                            for (let i = 0; i < samples.length; i += 1) {
                                if (samples[i] > peak) {
                                    peak = samples[i];
                                }
                            }
                            audioChunks.push(samples.slice());
                        }

                        if (plan.roundtrip !== null && frame === plan.roundtrip.frame) {
                            const bytes = engine.saveState();
                            if (bytes === null) {
                                roundtripError = 'nothing to save';
                            } else if (!await window.fc.saveState(plan.roundtrip.slot, bytes)) {
                                roundtripError = 'the main process could not write the save';
                            } else {
                                const back = await window.fc.loadState(plan.roundtrip.slot);
                                if (back === null || !engine.loadState(back)) {
                                    roundtripError = 'the save did not load back';
                                }
                            }
                        }

                        if (wanted.has(frame)) {
                            hashes.push({ frame, hash: await hashPixels() });
                        }
                    }

                    // Wind back and replay, at the very end so that the audio
                    // collected above is not disturbed by frames running a
                    // second time.
                    //
                    // This is the property that matters for rewind: going back
                    // five snapshots and running forward the same number of
                    // frames has to land on exactly the frame it left, or the
                    // snapshots are not what they claim to be.
                    let rewindHash: string | null = null;
                    if (plan.rewindSteps > 0 && rewind !== null) {
                        let stepped = 0;
                        for (let i = 0; i < plan.rewindSteps; i += 1) {
                            if (!rewind.stepBack()) {
                                break;
                            }
                            stepped += 1;
                        }

                        for (let i = 0; i < stepped * rewind.everyFrames; i += 1) {
                            if (!engine.runFrame()) {
                                error = 'the CPU halted while replaying after a rewind';
                                break;
                            }
                        }

                        rewindHash = await hashPixels();
                        rewind.reset();
                    }

                    blit();

                    // The samples, as raw little endian float32, in the order
                    // the APU produced them. The same bytes
                    // `fc_headless --samples` writes, so the two can be
                    // compared with cmp -- which is the only way to be sure
                    // the mixing did not change on the way to the speaker.
                    const total = audioChunks.reduce((sum, chunk) => sum + chunk.length, 0);
                    const all = new Float32Array(total);
                    let offset = 0;
                    for (const chunk of audioChunks) {
                        all.set(chunk, offset);
                        offset += chunk.length;
                    }
                    const audioDigest = await crypto.subtle.digest(
                        'SHA-256',
                        new Uint8Array(all.buffer, all.byteOffset, all.byteLength),
                    );
                    const audioHash = Array.from(new Uint8Array(audioDigest))
                        .map((byte) => byte.toString(16).padStart(2, '0'))
                        .join('');

                    return {
                        frameCount: engine.frameCount,
                        totalCycles: engine.totalCycles,
                        cpuPc: engine.cpuPc,
                        sampleRate: engine.sampleRate,
                        audioPeak: peak,
                        audioHash,
                        audioSamples: total,
                        rewindHash,
                        audioState: audio === null ? null : audio.state,
                        audioError,
                        hashes,
                        error: error ?? roundtripError,
                    };
                },
            };

            // A game named on the command line loads straight away (the bytes
            // were fetched at the top, before the core was chosen); without one
            // the application sits on the library screen until the player
            // picks something.
            if (boot !== null && !bootApplied && !disposed) {
                bootApplied = true;
                if (!applyRom(boot.bytes, boot.path)) {
                    loaded = false;
                }
            }

            // In self test mode the machine is left exactly where
            // load_rom + reset put it, so that N frames from here run the same
            // cycles as N frames from the same place in the native build.
            if (!window.fc.selftestOnly) {
                animationFrame = requestAnimationFrame(tick);
            } else if (!loaded) {
                fail('no ROM. Start with --rom <game.nes>, or put one in packages/fc-core/tests/data/');
            }
        };

        /**
         * Build the machine, once.
         *
         * The first caller wins and everybody else waits on the same promise:
         * `start()` is called from the mount below and from loadRom(), and a
         * player who clicks two games in a row must not build two emulators.
         */
        let startPromise: Promise<void> | null = null;
        let pendingCore: CoreChoice | null = null;
        let coreInUse: CoreChoice | null = null;
        // A game named on the command line is loaded once, by the first
        // start(). A later start() -- a machine rebuilt for another console --
        // must not reach back and load it again over the game the player just
        // picked.
        let bootApplied = false;
        const startOnce = (core?: CoreChoice): Promise<void> => {
            if (core !== undefined) {
                pendingCore = core;
            }
            return (startPromise ??= start());
        };

        /**
         * Take the machine apart, so the next start() builds another one.
         *
         * Loading a Game Boy game into a NES machine is not a thing the
         * hardware can do, and a libretro core cannot become another core. So
         * a game from a different console stops this machine -- frame loop,
         * speaker, save state ring and all -- and start() builds the right one
         * in its place.
         */
        const stopMachine = async (): Promise<void> => {
            if (animationFrame !== 0) {
                cancelAnimationFrame(animationFrame);
                animationFrame = 0;
            }
            rewind?.destroy();
            rewind = null;
            await audio?.close();
            audio = null;
            emulator?.destroy();
            emulator = null;
            startPromise = null;
            coreInUse = null;
        };

        // When to build the machine.
        //
        // Not when the window opens. Loading the WebAssembly module, building
        // the Emulator and opening the audio ring is work that has nothing to
        // do with showing the game library, and doing it at start up means the
        // application is unavailable until it finishes -- for a screen the
        // player may sit on for a while. The machine is built on demand
        // instead: the first game that is loaded starts it.
        //
        // Test modes are the exception, and the reason for the `eager` flag on
        // the bridge. A self test, a key test, an audio test or a layout check
        // needs the emulator at a known moment, and some never go through
        // loadRom() at all. See --fc-eager in the main process.
        if (window.fc.eager) {
            void startOnce().catch((error: unknown) => {
                const message = error instanceof Error ? error.message : String(error);
                bootLog('renderer', 'machine build FAILED', message);
                if (!disposed) {
                    fail(message);
                }
            });
        } else {
            // The library screen, with no machine behind it yet. The status
            // says running because the application is running; there is simply
            // no cartridge. command() and unload() do nothing until there is
            // something to command.
            setStatus({ ...INITIAL_STATUS, state: 'running' });
            actions.current = {
                ...actions.current,

                /**
                 * There is no machine to stop yet, so this only remembers and
                 * reports. It is not a wasted branch: the status line is what
                 * says the picture is hidden, and the preview can be opened on
                 * a library with no cartridge in it -- which is exactly what
                 * `--list` does.
                 */
                setPictureHidden: (hidden) => {
                    pictureHidden = hidden;
                    setStatus((s) => ({ ...s, paused: hidden }));
                },
                loadRom: async (path: string) => {
                    // The machine may not exist yet. Build the right one for
                    // this game; the loader it installs does the load itself.
                    await startOnce(chooseCore(path, outward.current.cores));
                    return actions.current.loadRom(path);
                },
                unload: () => undefined,
                command: () => undefined,
            };
        }

        return () => {
            disposed = true;
            if (animationFrame !== 0) {
                cancelAnimationFrame(animationFrame);
            }
            if (padFrame !== 0) {
                cancelAnimationFrame(padFrame);
            }
            detachKeyboard?.();
            gamepad?.destroy();
            gamepad = null;
            rewind?.destroy();
            rewind = null;
            commandHandler = null;
            window.clearTimeout(flashTimer);
            delete window.__fc;
            // Closing the context stops the audio thread before the ring is
            // abandoned underneath it.
            void audio?.close();
            audio = null;
            emulator?.destroy();
            emulator = null;
        };
    }, [canvasRef]);

    // Push a changed cheat list into the machine. Reading a file and changing a
    // cheat are both asynchronous, and the machine exists by the time either
    // lands, so one effect is enough for both.
    useEffect(() => {
        actions.current.applyCheats();
    }, [handlers.cheats]);

    return {
        status,
        loadRom: (path: string) => actions.current.loadRom(path),
        reload: () => actions.current.reload(),
        unload: () => actions.current.unload(),
        command: (command: CommandName) => actions.current.command(command),
        setPictureHidden: (hidden: boolean) => actions.current.setPictureHidden(hidden),
        poke: (address: number, value: number) => actions.current.poke(address, value),
        peek: (address: number) => actions.current.peek(address),
    };
}
