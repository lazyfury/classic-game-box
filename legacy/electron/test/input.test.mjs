// ---------------------------------------------------------------------------
// Tests for input.ts.
//
// These run in Node, with no Electron and no browser, because input.ts does
// not need either: it is a map and a small state machine. The parts that do
// need a window -- the event listeners -- are covered end to end by
// electron/verify.sh's input scenario.
//
//   pnpm test
// ---------------------------------------------------------------------------

import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
    HELD_KEY_COMMANDS, InputManager, KEY_COMMANDS, KEY_COMMANDS_SHIFTED, padSource,
} from '../src/renderer/input.ts';

/** A manager that records everything it is told, so a test can see both what
 *  the console would hear and how many times it would hear it. */
function recording() {
    const calls = [];
    const manager = new InputManager((port, button, pressed) => calls.push([port, button, pressed]));
    return { manager, calls };
}

// -- the command keys -------------------------------------------------------

test('R resets and is not a button', () => {
    assert.equal(KEY_COMMANDS.KeyR, 'reset');
    assert.equal(HELD_KEY_COMMANDS.Backspace, 'rewind');
    assert.equal(KEY_COMMANDS_SHIFTED.F1, 'load1');
});

test('a command key is not also a movement key', () => {
    // A key that did both would reset the game and walk left at the same time.
    // The settings screen refuses to bind these; this is the other half of
    // that door.
    for (const code of Object.keys(KEY_COMMANDS)) {
        assert.ok(!(code in HELD_KEY_COMMANDS), `${code} is both`);
    }
});

// -- the manager ------------------------------------------------------------

test('a press reaches the console, and only once', () => {
    const { manager, calls } = recording();

    manager.set(0, 'A', true, 'keyboard');
    manager.set(0, 'A', true, 'keyboard');

    // The second press is the browser repeating a held key. The console has
    // eight switches, not a stream of events; telling it twice would be noise.
    assert.deepEqual(calls, [[0, 'A', true]]);
});

test('a release reaches the console once', () => {
    const { manager, calls } = recording();

    manager.set(0, 'A', true, 'keyboard');
    manager.set(0, 'A', false, 'keyboard');
    manager.set(0, 'A', false, 'keyboard');

    assert.deepEqual(calls, [[0, 'A', true], [0, 'A', false]]);
});

test('the same button on two ports is two different switches', () => {
    const { manager, calls } = recording();

    manager.set(0, 'A', true, 'keyboard');
    manager.set(1, 'A', true, 'keyboard');
    manager.set(0, 'A', false, 'keyboard');

    // The console has two controller ports. Releasing player 1's A must not
    // release player 2's, and the manager keys its state by (port, button)
    // for exactly this reason.
    assert.deepEqual(calls, [[0, 'A', true], [1, 'A', true], [0, 'A', false]]);
    assert.deepEqual(manager.held, ['A']);
    assert.deepEqual(manager.heldOn(1), ['A']);
    assert.deepEqual(manager.heldOn(0), []);
});

test('two sources holding one button keep it held', () => {
    const { manager, calls } = recording();

    manager.set(0, 'A', true, 'keyboard');
    manager.set(0, 'A', true, padSource(0));
    manager.set(0, 'A', false, 'keyboard');

    // The pad is still holding A, so the console must not be told it was
    // released. This is the whole reason the manager exists.
    assert.deepEqual(calls, [[0, 'A', true]]);
    assert.deepEqual(manager.held, ['A']);

    manager.set(0, 'A', false, padSource(0));
    assert.deepEqual(calls, [[0, 'A', true], [0, 'A', false]]);
});

test('one pad disconnecting does not drop the other pad', () => {
    const { manager, calls } = recording();

    manager.set(0, 'A', true, padSource(0));
    manager.set(1, 'B', true, padSource(1));

    manager.releaseAll(padSource(0));

    // Player 2 is still holding their pad. This is why a pad is its own
    // source rather than all pads sharing the word "gamepad".
    assert.deepEqual(calls, [[0, 'A', true], [1, 'B', true], [0, 'A', false]]);
    assert.deepEqual(manager.heldOn(1), ['B']);
});

test('losing focus releases the keyboard but not the gamepad', () => {
    const { manager, calls } = recording();

    manager.set(0, 'RIGHT', true, 'keyboard');
    manager.set(0, 'A', true, padSource(0));

    manager.releaseAll('keyboard');

    // Right is let go; A is not. A player can alt-tab away with a pad still
    // in their hands, and the pad is still pressed.
    assert.deepEqual(calls, [[0, 'RIGHT', true], [0, 'A', true], [0, 'RIGHT', false]]);
    assert.deepEqual(manager.held, ['A']);
});

test('releasing a source that holds nothing says nothing', () => {
    const { manager, calls } = recording();

    manager.releaseAll('keyboard');

    assert.deepEqual(calls, []);
});

test('a button with no sources left is forgotten entirely', () => {
    const { manager } = recording();

    manager.set(0, 'B', true, 'keyboard');
    manager.set(0, 'B', false, 'keyboard');

    assert.deepEqual(manager.held, []);
});

test('releaseEverything clears without inventing releases', () => {
    const { manager, calls } = recording();

    manager.set(0, 'START', true, 'keyboard');
    manager.releaseEverything();

    // Used on shutdown, when the emulator is about to be destroyed and there
    // is nobody left to tell.
    assert.deepEqual(manager.held, []);
    assert.deepEqual(calls, [[0, 'START', true]]);
});
