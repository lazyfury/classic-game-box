// ---------------------------------------------------------------------------
// Dropping files on the window.
//
// Three things about drag and drop are easy to get wrong, and all three are
// dealt with here:
//
// 1. **`dragover` must be cancelled.** The default action for a file dragged
//    over a page is to navigate to it, so a drop that is not prevented turns
//    the application into a viewer for whatever the player dropped. Every
//    handler that matters calls `preventDefault`, including the ones that
//    only want to draw a highlight.
//
// 2. **`dragenter`/`dragleave` fire per element.** Dragging across a window
//    with a hundred rows in it produces a stream of both, and toggling a flag
//    on each one makes the highlight flicker. Counting the enters and leaving
//    at zero is what makes it hold still.
//
// 3. **A dropped `File` has no path any more.** Electron 32 removed the
//    `path` property -- a page that can turn a file into a filesystem path is
//    a page that can probe the disk -- and the replacement is
//    `webUtils.getPathForFile`, which the preload wraps. It answers only for
//    a file the operating system actually dragged in, which is exactly the
//    case here and the only case.
//
// What the hook does *not* do is decide what to do with the paths. It reports
// them and returns whether a drag is in progress, so the caller can draw
// whatever it likes and the emulator is not in this file.
// ---------------------------------------------------------------------------

import { useEffect, useRef, useState } from 'react';

/** Whether this event is the operating system dragging files in. A drag of
 *  selected text, or of an element inside the page, is not. */
function carryingFiles(event: DragEvent): boolean {
    return event.dataTransfer?.types.includes('Files') === true;
}

/**
 * Watch the window for file drops.
 *
 * `onDrop` is called with the paths of the files that were dropped, empty
 * paths removed. Returns true while a drag is over the window, for drawing a
 * target.
 */
export function useFileDrop(onDrop: (paths: string[]) => void): boolean {
    const [dragging, setDragging] = useState(false);

    // Depth rather than a boolean: see the note above about flicker.
    const depth = useRef(0);

    // The callback is kept in a ref so that the listeners can be attached
    // once. Re-attaching them on every render would drop a drag that was in
    // progress at the wrong moment. The ref is updated after the render that
    // produced it, which is before any drop can happen.
    const latest = useRef(onDrop);
    useEffect(() => {
        latest.current = onDrop;
    });

    useEffect(() => {
        const onDragEnter = (event: DragEvent): void => {
            if (!carryingFiles(event)) {
                return;
            }
            event.preventDefault();
            depth.current += 1;
            setDragging(true);
        };

        const onDragOver = (event: DragEvent): void => {
            if (!carryingFiles(event)) {
                return;
            }
            event.preventDefault();
            if (event.dataTransfer !== null) {
                event.dataTransfer.dropEffect = 'copy';
            }
        };

        const onDragLeave = (event: DragEvent): void => {
            if (!carryingFiles(event)) {
                return;
            }
            depth.current = Math.max(0, depth.current - 1);
            if (depth.current === 0) {
                setDragging(false);
            }
        };

        const onDrop = (event: DragEvent): void => {
            if (!carryingFiles(event)) {
                return;
            }
            // Cancelled even when nothing is done with it, or the window
            // navigates to the dropped file and the emulator is gone.
            event.preventDefault();
            depth.current = 0;
            setDragging(false);

            const paths: string[] = [];
            for (const file of Array.from(event.dataTransfer?.files ?? [])) {
                try {
                    const path = window.fc.filePath(file);
                    if (path !== '') {
                        paths.push(path);
                    }
                } catch {
                    // Not a file with a path behind it -- a drag from inside
                    // the page, say. There is nothing to import.
                }
            }

            if (paths.length > 0) {
                latest.current(paths);
            }
        };

        window.addEventListener('dragenter', onDragEnter);
        window.addEventListener('dragover', onDragOver);
        window.addEventListener('dragleave', onDragLeave);
        window.addEventListener('drop', onDrop);

        return () => {
            window.removeEventListener('dragenter', onDragEnter);
            window.removeEventListener('dragover', onDragOver);
            window.removeEventListener('dragleave', onDragLeave);
            window.removeEventListener('drop', onDrop);
        };
    }, []);

    return dragging;
}
