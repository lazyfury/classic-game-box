// ---------------------------------------------------------------------------
// How wide the middle column is, and the handle that changes it.
//
// A splitter, written out rather than taken from a library, for the same
// reason the stylesheet is written out: the interesting part is not the
// mechanics, it is the five decisions that make a drag feel right.
//
// 1. **The listeners go on the window, not on the handle.** The obvious
//    version listens for `pointermove` on the seven-pixel divider, and it
//    works until the pointer outruns it -- the moment the hand moves faster
//    than the events do, the pointer is over the panel and the drag stops
//    dead. `setPointerCapture` is the usual answer and it is a better one on
//    paper, but it is also the sort of thing that quietly does nothing in a
//    synthetic event or an unusual pointer, and a handle that only works
//    while the cursor is inside it is worse than one that always works.
//    Window listeners move with the pointer wherever it goes, they are the
//    same call in every browser, and they can be tested.
//
// 2. **The width is a number in state, not a measurement.** Reading
//    `getBoundingClientRect()` on every move would work, but it makes the
//    drag depend on layout having settled -- and layout is the thing being
//    changed. `startWidth + (now - start)` is arithmetic on two numbers that
//    were both true when the drag began, so it cannot chase its own tail.
//
// 3. **The start is captured in the closure, not in a ref.** The drag's state
//    is needed by exactly one drag, and the listeners that need it are removed
//    when it ends. A ref would outlive them and need clearing.
//
// 4. **Clamped against the window, not just against itself.** A panel that can
//    be dragged wider than the window can never be dragged back. The ceiling
//    is whatever leaves the picture a sensible amount of room, and it is
//    re-applied on window resize so that shrinking the window and growing it
//    again does not resurrect a width that no longer fits.
//
// 5. **The keyboard has to work.** A divider that can only be moved by
//    dragging is a control some people cannot use at all, so it is a
//    `role="separator"` with a tab stop, arrow keys, and Home/End.
//
// The width is remembered between runs. It is a preference about the window,
// not about the games, so it lives in the main process's config file rather
// than in the library's database. It is not in the renderer's localStorage
// any more: the first synchronous DOM Storage access blocks a renderer for
// seconds while Electron's storage service starts, which was the whole of the
// application's start up time. See ReadPreferences in src/main/index.ts.
// ---------------------------------------------------------------------------

import {
    useCallback, useEffect, useRef, useState,
    type KeyboardEvent as ReactKeyboardEvent,
    type PointerEvent as ReactPointerEvent,
} from 'react';

import type { Preferences } from '../shared/api';

/** Where the divider starts, and where a double click puts it back. */
const DEFAULT_WIDTH = 320;

/** Narrower than this and the cards stop being cards; wider and the picture
 *  has nothing left to sit in. */
const MIN_WIDTH = 280;
const MAX_WIDTH = 720;

/** One arrow key press. Small enough to be a nudge, large enough to see. */
const STEP = 16;

/**
 * The widest the panel may be in this window.
 *
 * Leaves the rail and the play column their own minimums: the rail is 76px,
 * the divider 7, and the picture wants roughly 260 before it is a thumbnail.
 */
function ceiling(): number {
    return Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, window.innerWidth - 350));
}

function clamp(width: number): number {
    return Math.min(Math.max(Math.round(width), MIN_WIDTH), ceiling());
}

export interface PanelWidth {
    /** CSS pixels. Hand it to the stylesheet as `--panel-width`. */
    width: number;
    /** True while a drag is in progress, for the highlight. */
    dragging: boolean;
    /** Spread onto the divider element. */
    handleProps: {
        role: 'separator';
        tabIndex: number;
        'aria-orientation': 'vertical';
        'aria-label': string;
        'aria-valuenow': number;
        'aria-valuemin': number;
        'aria-valuemax': number;
        onPointerDown: (event: ReactPointerEvent<HTMLDivElement>) => void;
        onKeyDown: (event: ReactKeyboardEvent<HTMLDivElement>) => void;
        onDoubleClick: () => void;
    };
}

export function usePanelWidth(): PanelWidth {
    const [width, setWidth] = useState<number>(DEFAULT_WIDTH);
    const [dragging, setDragging] = useState(false);
    // Whether the stored value has been read yet. Until it has, the write
    // effect below must stay quiet or it would store the default on the way
    // past.
    const [restored, setRestored] = useState(false);
    // The width as the config file last had it. A width that differs from
    // this is what is worth saving, so opening the application does not
    // rewrite the file, and a read that failed does not overwrite whatever
    // was in it with the default.
    const savedWidth = useRef<number>(DEFAULT_WIDTH);

    // The saved width, from the main process.
    //
    // This was a synchronous `localStorage` read once, and the first one in a
    // renderer blocks until Electron's storage service answers -- measured at
    // 3.7 seconds, which was the whole of the application's start up. The main
    // process keeps it in config.json now (see ReadPreferences in
    // src/main/index.ts) and answers in about two milliseconds, so the panel
    // is drawn at its default for a frame and then nudges to where the player
    // left it.
    useEffect(() => {
        let cancelled = false;
        void window.fc.preferences()
            .then((preferences: Preferences) => {
                if (cancelled) {
                    return;
                }
                const next = preferences.panelWidth === null
                    ? DEFAULT_WIDTH
                    : clamp(preferences.panelWidth);
                savedWidth.current = next;
                setWidth(next);
                setRestored(true);
            })
            .catch(() => {
                // A preference that cannot be read is not worth failing start
                // up over, and it is not a reason to write over the file
                // either: the default is shown, and `savedWidth` still says
                // the default, so nothing is saved until the player moves the
                // divider.
                if (!cancelled) {
                    setRestored(true);
                }
            });
        return () => {
            cancelled = true;
        };
    }, []);

    useEffect(() => {
        if (!restored || width === savedWidth.current) {
            return;
        }
        savedWidth.current = width;
        void window.fc.setPreference('panelWidth', width);
    }, [width, restored]);

    // Shrinking the window must not leave the panel wider than the ceiling:
    // flexbox would shrink it on screen while the number stayed behind, so the
    // handle and the column would disagree about where the edge is.
    useEffect(() => {
        const settle = (): void => setWidth((current) => clamp(current));
        window.addEventListener('resize', settle);
        return () => window.removeEventListener('resize', settle);
    }, []);

    const onPointerDown = useCallback((event: ReactPointerEvent<HTMLDivElement>): void => {
        if (event.button !== 0) {
            return;
        }
        // Stop the pointer from selecting text, and from starting a drag of
        // the window itself, while the divider is being moved.
        event.preventDefault();

        const startX = event.clientX;
        const startWidth = width;

        const move = (moved: PointerEvent): void => {
            setWidth(clamp(startWidth + (moved.clientX - startX)));
        };

        const stop = (): void => {
            window.removeEventListener('pointermove', move);
            window.removeEventListener('pointerup', stop);
            window.removeEventListener('pointercancel', stop);
            // A drag that is interrupted by the window losing focus never gets
            // its pointerup, and a divider stuck to the cursor is a bug people
            // remember.
            window.removeEventListener('blur', stop);
            setDragging(false);
        };

        window.addEventListener('pointermove', move);
        window.addEventListener('pointerup', stop);
        window.addEventListener('pointercancel', stop);
        window.addEventListener('blur', stop);
        setDragging(true);
    }, [width]);

    const onKeyDown = useCallback((event: ReactKeyboardEvent<HTMLDivElement>): void => {
        const step = event.key === 'ArrowLeft' ? -STEP : event.key === 'ArrowRight' ? STEP : 0;
        if (step !== 0) {
            event.preventDefault();
            setWidth((current) => clamp(current + step));
            return;
        }
        if (event.key === 'Home') {
            event.preventDefault();
            setWidth(MIN_WIDTH);
        } else if (event.key === 'End') {
            event.preventDefault();
            setWidth(ceiling());
        }
    }, []);

    const onDoubleClick = useCallback((): void => setWidth(DEFAULT_WIDTH), []);

    return {
        width,
        dragging,
        handleProps: {
            role: 'separator',
            tabIndex: 0,
            'aria-orientation': 'vertical',
            'aria-label': '调整中间栏宽度',
            'aria-valuenow': width,
            'aria-valuemin': MIN_WIDTH,
            // A static ceiling for the screen reader; the real one depends on
            // the window and is applied as the drag happens.
            'aria-valuemax': MAX_WIDTH,
            onPointerDown,
            onKeyDown,
            onDoubleClick,
        },
    };
}
