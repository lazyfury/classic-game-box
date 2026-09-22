// ---------------------------------------------------------------------------
// The native gamepad helper, from the main process's side.
//
// The renderer cannot read a gamepad on macOS: the browser's Gamepad API
// starts Chromium's own HID service, and that service keeps the application
// from quitting. On Windows the same helper process is used for a different
// reason -- one reader, one protocol, and no platform code in the page -- and
// there it is the C++ helper in native/gamepad-cpp, reading XInput. So the
// reading happens in a separate process on both platforms and this file is the
// pipe to it.
//
//   spawn  native/bin/fc-gamepad[.exe]
//     |
//     |  {"type":"pad","connected":true,"id":"...","buttons":{...}}   one
//     |  JSON object per line, only when something changes
//     v
//   parse and filter  -->  webContents.send(IpcChannel.GamepadState, reading)
//
// Why the filtering is here and not in the renderer
// ------------------------------------------------
// The helper already polls at 60Hz and only writes a line when a button
// changes, so the pipe is quiet. What this adds is one more comparison against
// the *last reading*: a pad that is disconnected and reconnected, or a line
// that arrives twice, must not turn into two identical messages to the page.
// The property that matters is that the renderer hears a change exactly once.
//
// Nothing here imports Electron. The state machine is testable from plain Node
// (test/gamepad.test.mjs), which is the only reason it is a separate file from
// index.ts: a parser that can only be tested by launching a window is a parser
// that does not get tested.
// ---------------------------------------------------------------------------

import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

import type { GamepadReading, PadButtonName, PadReading } from '../shared/api';

/**
 * Every name a reading carries, in the order the settings screen lists them:
 * the console's eight, then the nine a command may be bound to.
 *
 * Written out rather than imported from shared/api because this module is
 * loaded by a plain Node test, and Node cannot resolve the extensionless
 * import of another `.ts` file at run time. A test asserts that the two lists
 * are the same one.
 */
export const GAMEPAD_BUTTONS: readonly PadButtonName[] = [
    'A', 'B', 'SELECT', 'START', 'UP', 'DOWN', 'LEFT', 'RIGHT',
    'L1', 'R1', 'L2', 'R2', 'L3', 'R3',
    'FACE_X', 'FACE_Y', 'GUIDE',
];

/**
 * Nothing plugged in.
 *
 * Written out rather than imported from shared/api because this module is
 * loaded by a plain Node test, and Node cannot resolve the extensionless
 * import of another `.ts` file at run time. The two are checked equal by a
 * test, so they cannot drift.
 */
export const EMPTY_READING: GamepadReading = { pads: [] };

/** Where the built helper lives, given the Electron application folder.
 *
 * The name is platform-dependent: a Windows executable carries `.exe`, and a
 * `spawn` of a file without it fails with ENOENT -- which reads as "the helper
 * was never built" when in fact it was. `platform` is a parameter so that the
 * Windows name can be tested from a machine that is not Windows.
 */
export function gamepadBinaryPath(
    electronRoot: string,
    platform: NodeJS.Platform = process.platform,
): string {
    const name = platform === 'win32' ? 'fc-gamepad.exe' : 'fc-gamepad';
    return join(electronRoot, 'native', 'bin', name);
}

/** Two readings that would tell the renderer the same thing. */
export function sameReading(a: GamepadReading, b: GamepadReading): boolean {
    if (a.pads.length !== b.pads.length) {
        return false;
    }
    return a.pads.every((pad, position) => {
        const other = b.pads[position];
        return other !== undefined
            && pad.index === other.index
            && pad.id === other.id
            && GAMEPAD_BUTTONS.every((button) => pad.buttons[button] === other.buttons[button]);
    });
}

/**
 * One pad in a `pads` message.
 *
 * Returns null for anything without a usable slot, because the slot is the
 * pad's only name: two identical controllers report the same `id`, so a pad
 * with no index could be listed and could not be assigned, or released.
 */
function parsePad(raw: unknown): PadReading | null {
    if (raw === null || typeof raw !== 'object') {
        return null;
    }
    const pad = raw as { index?: unknown; id?: unknown; buttons?: unknown };
    if (typeof pad.index !== 'number' || !Number.isInteger(pad.index) || pad.index < 0) {
        return null;
    }

    const source = (pad.buttons ?? {}) as Record<string, unknown>;
    const buttons = {} as Record<PadButtonName, boolean>;
    for (const button of GAMEPAD_BUTTONS) {
        // Exactly true, so a missing key, a null, or a string is "not pressed"
        // rather than something the renderer has to defend against.
        buttons[button] = source[button] === true;
    }

    return {
        index: pad.index,
        id: typeof pad.id === 'string' && pad.id !== '' ? pad.id : 'Gamepad',
        buttons,
    };
}

/**
 * One line of the helper's output, as a reading.
 *
 * The helper reports the whole list every time it changes, rather than one
 * pad at a time, because the list is what the renderer draws and what a
 * disconnect is measured against: "pad 1 is gone" is only meaningful next to
 * "pads 0 and 2 are still here".
 *
 * Returns null for a line that is not a pads message -- the `hello` line, a
 * warning the framework printed, a line somebody's `console.log` got into --
 * because the right answer to "I do not understand this" is to drop it, not
 * to guess a state from it.
 */
export function parseGamepadLine(line: string): GamepadReading | null {
    let parsed: unknown;
    try {
        parsed = JSON.parse(line);
    } catch {
        return null;
    }

    if (parsed === null || typeof parsed !== 'object') {
        return null;
    }
    const message = parsed as { type?: unknown; pads?: unknown };
    if (message.type !== 'pads' || !Array.isArray(message.pads)) {
        return null;
    }

    const pads: PadReading[] = [];
    for (const raw of message.pads) {
        const pad = parsePad(raw);
        if (pad !== null) {
            pads.push(pad);
        }
    }
    return { pads };
}

export interface NativeGamepadOptions {
    /** The executable to run; see gamepadBinaryPath. */
    binary: string;
    /** Called for every reading that differs from the last one, and once with
     *  an empty reading when the helper exits. */
    onReading: (reading: GamepadReading) => void;
    /** Everything worth saying, already prefixed. Goes to the terminal. */
    onLog?: (message: string) => void;
}

/**
 * The child process, and the last thing it said.
 *
 * One instance per application. `start()` is idempotent so that a restart
 * after an unexpected exit is a single call, and `stop()` is deliberately
 * quiet: exiting is normal, and a "the helper exited" line during shutdown is
 * noise that makes a real crash harder to see.
 */
export class NativeGamepad {
    readonly #binary: string;
    readonly #onReading: (reading: GamepadReading) => void;
    readonly #onLog: (message: string) => void;

    #child: ChildProcess | null = null;
    #reading: GamepadReading = EMPTY_READING;
    #stopping = false;

    constructor(options: NativeGamepadOptions) {
        this.#binary = options.binary;
        this.#onReading = options.onReading;
        this.#onLog = options.onLog ?? (() => undefined);
    }

    /** What the pad was doing the last time it said anything. */
    get reading(): GamepadReading {
        return this.#reading;
    }

    get running(): boolean {
        return this.#child !== null;
    }

    /**
     * Start the helper. Returns false when it could not be started at all --
     * which on a fresh checkout means it has not been built yet, and is worth
     * a line that says so rather than a gamepad that quietly does nothing.
     */
    start(): boolean {
        if (this.#child !== null) {
            return true;
        }
        if (!existsSync(this.#binary)) {
            this.#onLog(`gamepad: helper not built (${this.#binary}) -- run pnpm run build:native`);
            return false;
        }

        this.#stopping = false;
        let child: ChildProcess;
        try {
            child = spawn(this.#binary, [], {
                stdio: ['pipe', 'pipe', 'pipe'],
                // Windows only, and ignored elsewhere: a console helper
                // spawned from a GUI application opens a console window of
                // its own unless it is told not to. A black box flashing up
                // when the game starts is not a crash, but it looks like one.
                windowsHide: true,
            });
        } catch (error) {
            this.#onLog(`gamepad: could not start the helper: ${(error as Error).message}`);
            return false;
        }
        this.#child = child;
        this.#onLog(`gamepad: helper started (${this.#binary})`);

        // Line by line, because that is the protocol: one JSON object per
        // line, flushed by the helper. readline handles the case where a line
        // arrives split across two chunks, which is not hypothetical on a pipe.
        const lines = createInterface({ input: child.stdout! });
        lines.on('line', (line) => this.#handle(line));

        // The framework and the Swift runtime write here. Forwarding it means
        // a crash mid-run is visible in the terminal instead of arriving as a
        // gamepad that silently stopped working.
        child.stderr?.on('data', (chunk: Buffer) => {
            const text = chunk.toString().trim();
            if (text !== '') {
                this.#onLog(`gamepad: ${text}`);
            }
        });

        child.on('error', (error) => {
            this.#onLog(`gamepad: helper error: ${error.message}`);
        });

        child.on('exit', (code, signal) => {
            this.#child = null;
            if (!this.#stopping) {
                this.#onLog(`gamepad: helper exited (${signal ?? code ?? 'unknown'})`);
            }
            // Whatever the pad was holding, let go of it. There is nobody left
            // to release the buttons.
            this.#publish(EMPTY_READING);
        });

        return true;
    }

    /**
     * Stop it, and mean it.
     *
     * Closing stdin is the polite request: the helper is watching it and exits
     * when it closes, which is the clean path. SIGTERM is the backup for a
     * helper that is wedged before it reaches the read loop, and SIGKILL is
     * the last resort for one that ignores a signal. On Windows both names are
     * emulated by Node as TerminateProcess, which is what the watchdog is
     * for; the stdin close is still the path that matters, because it is the
     * only one that lets the helper flush first. A gamepad helper must never
     * be the reason the application does not quit.
     */
    stop(): void {
        const child = this.#child;
        if (child === null) {
            return;
        }
        this.#stopping = true;
        try {
            child.stdin?.end();
        } catch {
            // The pipe is already gone; the kill below is what matters.
        }
        child.kill('SIGTERM');

        // If it has not exited by the time the watchdog runs, the polite
        // signal was not enough. `child.killed` cannot answer that question
        // -- it goes true the moment kill() is called, whether or not the
        // process obeyed -- so ask whether an exit code or signal has
        // actually been recorded.
        const watchdog = setTimeout(() => {
            if (child.exitCode === null && child.signalCode === null) {
                child.kill('SIGKILL');
            }
        }, 500);
        // Do not hold the event loop open for the watchdog.
        watchdog.unref?.();
        this.#child = null;
    }

    #handle(line: string): void {
        const reading = parseGamepadLine(line);
        if (reading === null) {
            return;
        }
        this.#publish(reading);
    }

    #publish(reading: GamepadReading): void {
        if (sameReading(reading, this.#reading)) {
            return;
        }
        this.#reading = reading;
        this.#onReading(reading);
    }
}
