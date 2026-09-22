// ---------------------------------------------------------------------------
// Fullscreen
//
// "Fullscreen" here means the whole application window goes full screen and
// everything that is about *choosing* a game gets out of the way -- the title
// bar, the rail, the list, the search box and the divider. What stays is the
// console: the picture, the strip above it with the game's name and the
// machine's lights, the strip below it with the controls, and the readout at
// the very bottom. A picture you cannot pause from across the room is a worse
// picture, which is why the controls are not hidden with the rest.
//
// The DOM's own Fullscreen API rather than the main process's
// `BrowserWindow.setFullScreen`, for two reasons. It takes the element it is
// given, so "fullscreen" is a fact about the page that CSS can read with
// `:fullscreen` and nothing has to be tracked in parallel. And Escape leaves,
// because the browser owns that key -- a native fullscreen window would need
// its own handler, and an application you cannot leave is a worse bug than one
// you cannot enter.
// ---------------------------------------------------------------------------

import { useCallback, useEffect, useState, type RefObject } from 'react';

export interface FullscreenHandle {
    /** Whether the element is fullscreen right now. */
    fullscreen: boolean;
    /** Enter or leave. */
    toggle: () => void;
    /** False when the platform will not allow it, so the button can hide. */
    available: boolean;
}

/**
 * Watch `target` and the document's fullscreen state.
 *
 * The state follows the `fullscreenchange` event rather than the click, because
 * Escape and the window server can both change it without going through
 * `toggle`.
 */
export function useFullscreen(target: RefObject<HTMLElement | null>): FullscreenHandle {
    const [fullscreen, setFullscreen] = useState(false);
    const [available, setAvailable] = useState(false);

    useEffect(() => {
        setAvailable(typeof document !== 'undefined' && document.fullscreenEnabled === true);
    }, []);

    useEffect(() => {
        const onChange = (): void => {
            setFullscreen(
                document.fullscreenElement !== null
                && document.fullscreenElement === target.current,
            );
        };
        document.addEventListener('fullscreenchange', onChange);
        return () => document.removeEventListener('fullscreenchange', onChange);
    }, [target]);

    const toggle = useCallback((): void => {
        const element = target.current;
        if (element === null) {
            return;
        }

        if (document.fullscreenElement !== null) {
            void document.exitFullscreen();
            return;
        }

        // The promise rejects when the request is not allowed -- a window
        // manager setting, a refusal because the call did not come from a
        // gesture. That is not worth a dialog: the button simply does not
        // change, which is what "nothing happened" should look like.
        void element.requestFullscreen().catch(() => undefined);
    }, [target]);

    // F11, because it is what every application has used for this since before
    // there was a DOM. Escape is deliberately left alone: the browser uses it
    // to leave, and a second handler on the same key would race it.
    useEffect(() => {
        const onKeyDown = (event: KeyboardEvent): void => {
            if (event.code !== 'F11' || event.metaKey || event.ctrlKey || event.altKey) {
                return;
            }
            event.preventDefault();
            toggle();
        };
        window.addEventListener('keydown', onKeyDown);
        return () => window.removeEventListener('keydown', onKeyDown);
    }, [toggle]);

    return { fullscreen, toggle, available };
}
