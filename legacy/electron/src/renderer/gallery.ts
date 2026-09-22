// ---------------------------------------------------------------------------
// Moving through a gallery, and where the ends are.
//
// One decision, in a file of its own so that a Node test can reach it: what
// happens when you press "next" on the last picture, or "previous" on the
// first.
//
// There is no wrap-around. A gallery that jumps from the newest screenshot to
// the oldest because you pressed the arrow once more is a gallery where the
// button does not tell you what it will do -- and this list is "newest first",
// so the two ends are the two things a player is least likely to want next to
// each other. The ends stop, the button beside them goes disabled, and what
// the control does is the same thing it says.
// ---------------------------------------------------------------------------

/**
 * The index `delta` steps away from `index`, held inside the list.
 *
 * Clamped rather than wrapped, and clamped on both ends: at the first picture
 * "previous" answers the first picture again, which is what makes a caller
 * able to disable the button (`neighbour(...) === index`).
 */
export function neighbour(index: number, delta: number, count: number): number {
    if (count <= 0) {
        return 0;
    }
    return Math.min(Math.max(index + delta, 0), count - 1);
}

/** Whether a picture is already at one of the ends, for the disabled state. */
export function atStart(index: number): boolean {
    return index <= 0;
}

export function atEnd(index: number, count: number): boolean {
    return index >= count - 1;
}
