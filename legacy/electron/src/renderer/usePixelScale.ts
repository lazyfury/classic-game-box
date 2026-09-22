// ---------------------------------------------------------------------------
// How big should one NES pixel be on this screen?
//
// The picture is 256x240 and it was drawn with hard edges, one pixel at a
// time, by hardware that had no idea what a "subpixel" was. The only way to
// show it on a modern display without inventing detail that was never there is
// to make every NES pixel the same whole number of screen pixels. Anything
// else -- filling the window, `width: 100%`, a fractional scale -- divides
// neighbouring pixels into different physical widths, and a row of identical
// ground tiles comes out looking like it is breathing.
//
// Two things make this less obvious than it sounds.
//
// Device pixels, not CSS pixels
// -----------------------------
// The count has to be an integer in *device* pixels. On a 2x display a CSS
// scale of 3 is 6 device pixels and is fine; on a 1.5x display it is 4.5 and
// is not. Working in device pixels throughout means a fractional device ratio
// does not quietly reintroduce the blur it was supposed to avoid.
//
// Moving the window between screens
// ---------------------------------
// The container does not change size when a window is dragged from a laptop
// screen to an external one, so a resize observer sees nothing. What changes
// is `devicePixelRatio`, and the only event for it is a media query. Without
// that listener the picture goes soft on the second monitor and nothing on
// screen explains why.
// ---------------------------------------------------------------------------

import { useEffect, useState, type RefObject } from 'react';

export interface PixelScale {
    /** CSS pixels per game pixel. May be fractional only when the window is
     *  smaller than the picture, where the alternative is not fitting. */
    scale: number;
    width: number;
    height: number;
}

/**
 * Watch `containerRef` and report the largest whole-pixel size that fits.
 *
 * `width` and `height` are the game's own resolution, so this is not specific
 * to the NES: hand it any picture and any box.
 */
export function usePixelScale(
    containerRef: RefObject<HTMLElement | null>,
    width: number,
    height: number,
): PixelScale {
    const [size, setSize] = useState<PixelScale>({ scale: 1, width, height });

    useEffect(() => {
        const container = containerRef.current;
        if (container === null) {
            return;
        }

        const measure = (): void => {
            const available = container.getBoundingClientRect();
            const dpr = window.devicePixelRatio || 1;

            if (available.width < 1 || available.height < 1) {
                return;
            }

            // Everything in device pixels, then converted back at the end.
            const fit = Math.min(
                (available.width * dpr) / width,
                (available.height * dpr) / height,
            );

            // Round down, so the picture never overflows its box. Below one
            // there is nothing to round down to, and a window smaller than the
            // picture has to either scale fractionally or clip -- fractional is
            // the better of two bad options.
            const scale = fit >= 1 ? Math.floor(fit) : fit;

            setSize({
                scale: scale / dpr,
                width: (width * scale) / dpr,
                height: (height * scale) / dpr,
            });
        };

        measure();

        const observer = new ResizeObserver(measure);
        observer.observe(container);

        // For the display change that a resize observer cannot see.
        const unwatchDpr = watchDevicePixelRatio(measure);
        window.addEventListener('resize', measure);

        return () => {
            observer.disconnect();
            unwatchDpr();
            window.removeEventListener('resize', measure);
        };
    }, [containerRef, width, height]);

    return size;
}

/**
 * Call `onChange` whenever `devicePixelRatio` changes.
 *
 * There is no `devicepixelratiochange` event. The trick is to ask for a media
 * query at the *current* ratio: the moment the ratio changes, that query stops
 * matching, the listener fires, and it re-registers itself for the new one.
 */
function watchDevicePixelRatio(onChange: () => void): () => void {
    let query: MediaQueryList | null = null;
    let disposed = false;

    const listen = (): void => {
        if (disposed) {
            return;
        }
        query?.removeEventListener('change', handle);

        const ratio = window.devicePixelRatio || 1;
        query = window.matchMedia(`(resolution: ${ratio}dppx)`);
        query.addEventListener('change', handle);
    };

    const handle = (): void => {
        onChange();
        listen();
    };

    listen();

    return () => {
        disposed = true;
        query?.removeEventListener('change', handle);
    };
}
