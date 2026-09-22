// ---------------------------------------------------------------------------
// Tests for moving through the screenshot gallery.
//
// Plain Node: gallery.ts is one decision, and the decision is what happens at
// the ends. An off-by-one there is invisible until somebody has the last
// picture open and presses the arrow, which is exactly the kind of thing that
// should not be found by hand.
//
//   pnpm test
// ---------------------------------------------------------------------------

import { test } from 'node:test';
import assert from 'node:assert/strict';

import { atEnd, atStart, neighbour } from '../src/renderer/gallery.ts';

test('stepping stays inside the list', () => {
    assert.equal(neighbour(1, 1, 3), 2);
    assert.equal(neighbour(1, -1, 3), 0);
});

test('the ends stop rather than wrap', () => {
    // Past the last picture is the last picture, not the first one: the list is
    // newest first, and the two ends of it are the two things a player is least
    // likely to want next to each other.
    assert.equal(neighbour(2, 1, 3), 2);
    assert.equal(neighbour(0, -1, 3), 0);

    // Which is what makes the buttons able to say so.
    assert.equal(atStart(0), true);
    assert.equal(atStart(1), false);
    assert.equal(atEnd(2, 3), true);
    assert.equal(atEnd(1, 3), false);
});

test('a big step is clamped, not skipped out of the list', () => {
    assert.equal(neighbour(0, 99, 5), 4);
    assert.equal(neighbour(4, -99, 5), 0);
});

test('an empty list has nowhere to step to', () => {
    assert.equal(neighbour(0, 1, 0), 0);
    assert.equal(neighbour(0, -1, 0), 0);
    // Both ends at once, so both buttons are disabled and neither lies.
    assert.equal(atStart(0), true);
    assert.equal(atEnd(0, 0), true);
});
