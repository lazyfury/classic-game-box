// ---------------------------------------------------------------------------
// Where the renderer's start up time went.
//
// The boot trace can say *that* the renderer took 1.2 seconds to report ready,
// but not what it was waiting on. The browser already knows: it keeps a
// performance entry for the document (`navigation`), for each paint (`paint`)
// and for every subresource it fetched (`resource`). This reads those back out
// and prints the handful that are on the start up path.
//
// The filter matters more than the code. A dev page has a resource entry for
// every module Vite serves -- several hundred -- and printing all of them
// would bury the three that are actually slow. So this prints the WASM module,
// the audio worklet, and nothing else, plus the paint and navigation marks
// that frame all of it.
// ---------------------------------------------------------------------------

import { bootLog } from '../shared/boot';

/**
 * Report every long task the browser notices.
 *
 * A long task is a single stretch of the main thread longer than 50ms, which
 * is the browser's own definition of "this is why the page felt stuck". The
 * boot trace can say that two seconds passed between the first paint and the
 * first effect, and only this can say whether they were spent running one
 * piece of code or waiting for something.
 */
export function watchLongTasks(): void {
    if (typeof PerformanceObserver === 'undefined') {
        return;
    }
    try {
        const observer = new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) {
                if (entry.duration < 50) {
                    continue;
                }
                bootLog(
                    'renderer',
                    'LONG TASK',
                    `${Math.round(entry.duration)}ms starting at ${Math.round(entry.startTime)}ms`,
                );
            }
        });
        observer.observe({ entryTypes: ['longtask'] });
    } catch {
        // Not every engine reports longtask. Missing it is not a failure, so
        // this stays quiet rather than breaking start up over a diagnostic.
    }
}

/** Just the file part of a URL, which is the part that identifies an asset. */
function shortName(name: string): string {
    try {
        return new URL(name).pathname.split('/').pop() ?? name;
    } catch {
        return name;
    }
}

/**
 * Print the browser's own timing for this page.
 *
 * Call it once the machine is ready, so that the numbers are final: a
 * resource entry for the .wasm is only written when the fetch finishes.
 */
export function logPageTiming(): void {
    if (typeof performance === 'undefined') {
        return;
    }

    const navigation = performance.getEntriesByType('navigation')[0] as
        PerformanceNavigationTiming | undefined;
    if (navigation !== undefined) {
        bootLog('renderer', 'navigation', [
            `responseStart=${Math.round(navigation.responseStart)}ms`,
            `domInteractive=${Math.round(navigation.domInteractive)}ms`,
            `domContentLoaded=${Math.round(navigation.domContentLoadedEventEnd)}ms`,
            `load=${Math.round(navigation.loadEventEnd)}ms`,
        ].join(' '));
    }

    for (const entry of performance.getEntriesByType('paint')) {
        bootLog('renderer', `paint ${entry.name}`, `${Math.round(entry.startTime)}ms`);
    }

    for (const entry of performance.getEntriesByType('resource')) {
        if (!/(fc_core|pcm-worklet|\.wasm)/.test(entry.name)) {
            continue;
        }
        const resource = entry as PerformanceResourceTiming;
        const size = resource.transferSize > 0
            ? `  ${Math.round(resource.transferSize / 1024)}KB`
            : '';
        bootLog('renderer', 'resource', `${shortName(resource.name)} ${Math.round(resource.duration)}ms${size}`);
    }
}
