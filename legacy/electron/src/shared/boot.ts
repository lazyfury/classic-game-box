// ---------------------------------------------------------------------------
// A stopwatch for application start up.
//
// Starting this application is a chain of processes: the dev script, its
// TypeScript compile, the Vite dev server, Electron's main process, its
// preload, and finally the renderer. When it is slow, "slow" is the only thing
// anybody can say about it, because the four processes each print their own
// timestamps onto the same terminal and nothing lines them up.
//
// So there is one clock. The first process -- scripts/dev.mjs -- writes its
// own start time into FC_BOOT_T0, every child inherits it, and every line says
// how long it has been since *that* moment rather than since the process it
// happens to come from. The lines then interleave into a single timeline, and
// "startup took 3.4 seconds" becomes "the TypeScript compile was 2.1 of them".
//
// It is deliberately tiny and dependency-free: the same file is imported by
// the main process (CommonJS, tsc), the preload (CommonJS, tsc) and the
// renderer (bundled by Vite), so anything clever would have to work in all
// three worlds at once. `console.log` and `Date.now` do.
// ---------------------------------------------------------------------------

/** When the application's start up began, in epoch milliseconds. */
let origin = Date.now();

/**
 * Adopt the timeline the parent process started.
 *
 * A zero or missing value means "this process is on its own", which is what
 * running the packaged application -- where there is no dev script -- looks
 * like. That is not an error, so the fallback is to have started just now.
 */
export function setBootOrigin(when: number): void {
    if (Number.isFinite(when) && when > 0) {
        origin = when;
    }
}

/** The shared start of the run, in epoch milliseconds. */
export function bootOrigin(): number {
    return origin;
}

/** How long since the shared start, in whole milliseconds. */
export function bootElapsed(): number {
    return Date.now() - origin;
}

/**
 * One line of the start up trace.
 *
 * `scope` names the process ("dev", "main", "preload", "renderer") and `phase`
 * names the step. The `+NNNms` column is the whole point: it is the same clock
 * on every line, so sorting one run's output by that column -- rather than by
 * the order the terminal happened to flush it -- is the trace.
 */
export function bootLog(scope: string, phase: string, detail?: string): void {
    const now = Date.now();
    // HH:MM:SS.mmm, carved out of the ISO string, so a line from the dev
    // script and a line from the renderer can be placed even without the
    // shared clock.
    const at = new Date(now).toISOString().slice(11, 23);
    const elapsed = String(bootElapsed()).padStart(6, ' ');
    const tail = detail === undefined || detail === '' ? '' : `  ${detail}`;
    console.log(`[boot] ${at} +${elapsed}ms  ${scope.padEnd(9)}${phase}${tail}`);
}

/**
 * Time an async step and log it when it finishes.
 *
 * Returns whatever the work returned, so a call site can read
 * `const x = await bootTime('renderer', 'wasm', () => load())` and the timing
 * is part of the statement rather than a second line somebody can forget.
 */
export async function bootTime<T>(
    scope: string,
    phase: string,
    work: () => Promise<T>,
): Promise<T> {
    const started = Date.now();
    try {
        const result = await work();
        bootLog(scope, phase, `${Date.now() - started}ms`);
        return result;
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        bootLog(scope, `${phase} FAILED`, `${Date.now() - started}ms  ${message}`);
        throw error;
    }
}
