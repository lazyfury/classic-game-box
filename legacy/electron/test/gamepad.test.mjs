// ---------------------------------------------------------------------------
// The gamepad mapping.
//
// Reading a pad cannot be tested here: `navigator.getGamepads()` does not exist
// in Node, and on macOS calling it starts a service that makes the application
// impossible to quit (see FcBridge.gamepadEnabled). The mapping is the part
// with decisions in it, and it is a plain function for exactly that reason.
//
//   pnpm test
// ---------------------------------------------------------------------------

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { join } from 'node:path';

import { mapPad, PAD_INDICES, samePads } from '../src/renderer/gamepad.ts';
import { InputManager } from '../src/renderer/input.ts';
import {
    EMPTY_READING,
    GAMEPAD_BUTTONS,
    gamepadBinaryPath,
    parseGamepadLine,
    sameReading,
} from '../src/main/gamepad.ts';
import { NO_GAMEPAD_READING, PAD_BUTTONS } from '../src/shared/api.ts';

/** A pad with nothing pressed. */
function pad({ buttons = {}, axes = [0, 0] } = {}) {
    const list = Array.from({ length: 16 }, () => ({ pressed: false }));
    for (const [index, on] of Object.entries(buttons)) {
        list[Number(index)] = { pressed: on };
    }
    return { buttons: list, axes };
}

function down(result) {
    return Object.entries(result).filter(([, on]) => on).map(([name]) => name).sort();
}

test('nothing pressed means nothing down', () => {
    assert.deepEqual(down(mapPad(pad())), []);
});

test('the d-pad is buttons 12 to 15', () => {
    assert.deepEqual(down(mapPad(pad({ buttons: { [PAD_INDICES.LEFT]: true } }))), ['LEFT']);
    assert.deepEqual(down(mapPad(pad({ buttons: { [PAD_INDICES.RIGHT]: true } }))), ['RIGHT']);
    assert.deepEqual(down(mapPad(pad({ buttons: { [PAD_INDICES.UP]: true } }))), ['UP']);
    assert.deepEqual(down(mapPad(pad({ buttons: { [PAD_INDICES.DOWN]: true } }))), ['DOWN']);
});

test('the face buttons land on A and B, not on each other', () => {
    // Index 0 is the bottom button on a modern pad, where the letter A is
    // printed; index 1 is the right hand one. Getting these the wrong way
    // round is the classic gamepad bug, and it is invisible until somebody
    // tries to jump and shoots instead.
    assert.deepEqual(down(mapPad(pad({ buttons: { [PAD_INDICES.A]: true } }))), ['A']);
    assert.deepEqual(down(mapPad(pad({ buttons: { [PAD_INDICES.B]: true } }))), ['B']);
});

test('start and select are where the spec says', () => {
    assert.deepEqual(down(mapPad(pad({ buttons: { [PAD_INDICES.START]: true } }))), ['START']);
    assert.deepEqual(down(mapPad(pad({ buttons: { [PAD_INDICES.SELECT]: true } }))), ['SELECT']);
});

test('the stick duplicates the d-pad', () => {
    assert.deepEqual(down(mapPad(pad({ axes: [-1, 0] }))), ['LEFT']);
    assert.deepEqual(down(mapPad(pad({ axes: [1, 0] }))), ['RIGHT']);
    // Axis 1 is positive downwards.
    assert.deepEqual(down(mapPad(pad({ axes: [0, 1] }))), ['DOWN']);
    assert.deepEqual(down(mapPad(pad({ axes: [0, -1] }))), ['UP']);
});

test('a stick at rest does not drift', () => {
    // A worn stick sits at 0.2 or so when nobody is touching it. Without a
    // deadzone the player walks into a wall and cannot work out why.
    assert.deepEqual(down(mapPad(pad({ axes: [0.2, -0.3] }))), []);
    assert.deepEqual(down(mapPad(pad({ axes: [0.49, 0] }))), []);
    assert.deepEqual(down(mapPad(pad({ axes: [0.51, 0] }))), ['RIGHT']);
});

test('buttons and a stick at once do not cancel', () => {
    // Left on the d-pad while the stick is pushed right must still be left.
    // The two are OR-ed, and OR-ing them the other way round would make the
    // stick win, which nobody expects.
    const result = mapPad(pad({ buttons: { [PAD_INDICES.LEFT]: true }, axes: [1, 0] }));
    assert.equal(result.LEFT, true);
    assert.equal(result.RIGHT, true);
});

test('a pad reports into the input manager once per change', () => {
    // The manager is what keeps a pad and a keyboard from clobbering each
    // other. This is the same pad read twice, which is what polling does
    // sixty times a second, and it must not produce sixty events.
    const calls = [];
    const manager = new InputManager((port, button, pressed) => calls.push([port, button, pressed]));

    const state = mapPad(pad({ buttons: { [PAD_INDICES.A]: true } }));
    for (const [button, on] of Object.entries(state)) {
        manager.set(0, button, on, 'pad:0');
    }
    for (const [button, on] of Object.entries(state)) {
        manager.set(0, button, on, 'pad:0');
    }

    assert.deepEqual(calls, [[0, 'A', true]]);
    assert.deepEqual(manager.held, ['A']);
});

// ---------------------------------------------------------------------------
// The native helper's protocol.
//
// This is the half of the native path that can be tested from Node: the
// helper's stdout is a pipe of JSON lines, and what to do with those lines is
// a plain function. Reading a real pad cannot be tested here -- that needs a
// pad and an operating system -- but the parser multiplying that reading can,
// and the cases that matter are the malformed ones.
//
//   pnpm test
// ---------------------------------------------------------------------------

/** One pads line, as the helper would write it. */
function line(pads) {
    return JSON.stringify({ type: 'pads', pads });
}

test('the hello line is not a reading', () => {
    assert.equal(parseGamepadLine('{"pid":42,"type":"hello","version":2}'), null);
});

test('anything that is not JSON is dropped, not guessed at', () => {
    assert.equal(parseGamepadLine(''), null);
    assert.equal(parseGamepadLine('Swift runtime warning'), null);
    assert.equal(parseGamepadLine('{"type":"pads"oops}'), null);
    assert.equal(parseGamepadLine('null'), null);
    assert.equal(parseGamepadLine('[]'), null);
});

test('a pads line carries exactly the eight switches per pad', () => {
    const reading = parseGamepadLine(line([
        { index: 0, id: 'Xbox Wireless Controller', buttons: { A: true, START: true } },
    ]));

    assert.equal(reading.pads.length, 1);
    assert.equal(reading.pads[0].index, 0);
    assert.equal(reading.pads[0].id, 'Xbox Wireless Controller');
    assert.deepEqual(
        Object.keys(reading.pads[0].buttons).sort(),
        [...GAMEPAD_BUTTONS].sort(),
    );
    assert.deepEqual(down(reading.pads[0].buttons), ['A', 'START']);
});

test('two pads are two readings in one message', () => {
    const reading = parseGamepadLine(line([
        { index: 0, id: 'pad A', buttons: { A: true } },
        { index: 1, id: 'pad B', buttons: { B: true } },
    ]));

    assert.deepEqual(reading.pads.map((pad) => pad.index), [0, 1]);
    assert.deepEqual(down(reading.pads[0].buttons), ['A']);
    assert.deepEqual(down(reading.pads[1].buttons), ['B']);
});

test('a button that is missing or not true is not pressed', () => {
    // The helper always writes all eight, but a reader that treats a missing
    // key as "pressed" is one that sticks a button down for the rest of the
    // session. Exact true, or nothing.
    const reading = parseGamepadLine(line([
        { index: 0, buttons: { A: 1, B: 'true', LEFT: null, RIGHT: false } },
    ]));
    assert.deepEqual(down(reading.pads[0].buttons), []);
});

test('an empty pads list is a reading', () => {
    // It is how the last pad going away gets let go of. Dropping the line
    // would leave the jump button held forever.
    const reading = parseGamepadLine(line([]));
    assert.notEqual(reading, null);
    assert.deepEqual(reading.pads, []);
});

test('a pad with no slot is dropped, not guessed at', () => {
    // The index is the pad's only name: two identical controllers report the
    // same id, so a pad with no index could be listed and could not be
    // assigned or released.
    const reading = parseGamepadLine(line([
        { id: 'no slot', buttons: { A: true } },
        { index: 2, id: 'has a slot', buttons: { A: true } },
    ]));
    assert.deepEqual(reading.pads.map((pad) => pad.index), [2]);
});

test('a nameless pad is still a pad', () => {
    const reading = parseGamepadLine(line([{ index: 0 }]));
    assert.equal(reading.pads[0].id, 'Gamepad');
});

test('the empty reading is one value, written twice', () => {
    // Written out in src/main/gamepad.ts because Node cannot resolve an
    // extensionless import of shared/api at run time. If the two ever drift,
    // the main process and the renderer would disagree about "nothing is
    // connected", so they are compared here rather than trusted.
    assert.deepEqual(EMPTY_READING, NO_GAMEPAD_READING);
});

test('sameReading notices a button, a name, a slot and a count', () => {
    const idle = parseGamepadLine(line([{ index: 0, id: 'pad A' }]));
    assert.equal(sameReading(idle, idle), true);
    assert.equal(sameReading(idle, parseGamepadLine(
        line([{ index: 0, id: 'pad A', buttons: { A: true } }]),
    )), false);
    assert.equal(sameReading(idle, parseGamepadLine(
        line([{ index: 0, id: 'pad B' }]),
    )), false);
    assert.equal(sameReading(idle, parseGamepadLine(
        line([{ index: 1, id: 'pad A' }]),
    )), false);
    assert.equal(sameReading(idle, parseGamepadLine(line([
        { index: 0, id: 'pad A' },
        { index: 1, id: 'pad B' },
    ]))), false);
    assert.equal(sameReading(idle, EMPTY_READING), false);
});

test('the helper binary is looked for where the build script puts it', () => {
    // The name is platform-dependent, and a Windows spawn of a file without
    // `.exe` fails with ENOENT -- the exact "the helper was not built"
    // symptom, from a helper that was. Both names are asserted from any host
    // by passing the platform in.
    assert.equal(
        gamepadBinaryPath('/app', 'darwin'),
        join('/app', 'native', 'bin', 'fc-gamepad'),
    );
    assert.equal(
        gamepadBinaryPath('/app', 'win32'),
        join('/app', 'native', 'bin', 'fc-gamepad.exe'),
    );
    assert.equal(gamepadBinaryPath('/app').endsWith(`fc-gamepad${process.platform === 'win32' ? '.exe' : ''}`), true);
});

// -- the report the status line draws ----------------------------------------

test('a report changes when a button does', () => {
    // The settings screen's pad tester is drawn from this, and the status line
    // is only redrawn when the report differs. A comparison that ignored
    // buttons would say "the same" every time one was pressed, and the tester
    // would sit there dark while somebody pressed every button on the
    // controller wondering why.
    const pad = (buttons) => ({
        pads: [{ index: 0, id: 'Xbox', mapping: 'native', port: 0, buttons }],
    });
    const all = (down) => Object.fromEntries(PAD_BUTTONS.map((name) => [name, down.includes(name)]));

    assert.equal(samePads(pad(all([])), pad(all([]))), true);
    assert.equal(samePads(pad(all([])), pad(all(['L1']))), false);
    assert.equal(samePads(pad(all(['L1'])), pad(all(['L1', 'R2']))), false);

    // And the things the summary is made of still matter.
    const one = pad(all([]));
    assert.equal(samePads(one, { pads: [] }), false);
    assert.equal(samePads(one, {
        pads: [{ index: 0, id: 'Xbox', mapping: 'native', port: 1, buttons: all([]) }],
    }), false);
});
