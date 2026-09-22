// ---------------------------------------------------------------------------
// Tests for the command map.
//
// Plain Node: this file decides which key does what, and that decision is what
// stands between a player's keypress and the game. Everything here is a
// function of the settings, so the machine the key would have reached is not
// needed to know what it would have meant.
//
//   pnpm test
// ---------------------------------------------------------------------------

import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
    COMMAND_ORDER, NO_CAPTURE, NO_CHORDS, arbitrateChords, captureDone, captureUpdate,
    commandBindings, commandForKey, commandMaps, normaliseBinding,
} from '../src/renderer/commands.ts';
import {
    DEFAULT_COMMAND_BINDINGS, DEFAULT_INPUT_SETTINGS, EXTRA_PAD_BUTTONS, GAMEPAD_SWITCHES,
    PAD_BUTTONS,
} from '../src/shared/api.ts';

const settings = (overrides = {}) => ({ ...DEFAULT_INPUT_SETTINGS, ...overrides });

// -- the defaults ------------------------------------------------------------

test('the keys nobody had to learn are still the defaults', () => {
    const maps = commandMaps(settings());

    assert.equal(commandForKey(maps, 'Escape', false), 'pause');
    assert.equal(commandForKey(maps, 'KeyP', false), 'pause');
    assert.equal(commandForKey(maps, 'KeyR', false), 'reset');
    assert.equal(commandForKey(maps, 'F12', false), 'screenshot');
    assert.equal(commandForKey(maps, 'F12', true), 'screenshot-cover');
    assert.equal(commandForKey(maps, 'F5', false), 'quicksave');
    assert.equal(commandForKey(maps, 'F6', false), 'quickload');
    assert.equal(commandForKey(maps, 'Backspace', false), 'rewind');

    // Shift is part of the binding, not a modifier on top of one: the F row
    // saves and loads, and neither is the other's fallback.
    assert.equal(commandForKey(maps, 'F1', false), 'save1');
    assert.equal(commandForKey(maps, 'F1', true), 'load1');
    assert.equal(commandForKey(maps, 'F3', true), 'load3');

    // And a key nobody bound means nothing, rather than the nearest thing.
    assert.equal(commandForKey(maps, 'KeyQ', false), null);
    assert.equal(commandForKey(maps, 'Space', false), null);
});

test('every command is in the list the settings screen walks', () => {
    const bound = new Set(DEFAULT_COMMAND_BINDINGS.map((binding) => binding.command));
    for (const command of bound) {
        assert.ok(COMMAND_ORDER.includes(command), `${command} is missing from COMMAND_ORDER`);
    }
    // And the other way round: a command listed but never bound would be a row
    // in the settings screen that can never have a key.
    for (const command of COMMAND_ORDER) {
        assert.ok(bound.has(command), `${command} is listed but has no default binding`);
    }
});

// -- what the player has saved -----------------------------------------------

test('no commands in the file means the defaults, an empty list means none', () => {
    // A config file written before commands existed has no field at all.
    assert.equal(commandBindings(settings()).length, DEFAULT_COMMAND_BINDINGS.length);

    // And a player who deleted every binding gets what they asked for: the two
    // are not the same thing.
    const maps = commandMaps(settings({ commands: [] }));
    assert.equal(maps.keyed.size, 0);
    assert.equal(commandForKey(maps, 'Escape', false), null);
});

test('a binding the application does not understand is dropped, not kept', () => {
    const maps = commandMaps(settings({
        commands: [
            { command: 'pause', key: { code: 'KeyQ', shift: false }, pads: [] },
            { command: 'somethingElse', key: { code: 'KeyW', shift: false }, pads: [] },
            { command: 'screenshot', key: { code: '', shift: false }, pads: [] },
            null,
        ],
    }));

    assert.equal(commandForKey(maps, 'KeyQ', false), 'pause');
    assert.equal(commandForKey(maps, 'KeyW', false), null);
    assert.equal(maps.bindings.length, 2);
    // The screenshot binding survives with a key that is not a key: it has
    // become "not on the keyboard", which is an honest answer.
    const screenshot = maps.bindings.find((binding) => binding.command === 'screenshot');
    assert.equal(screenshot?.key, null);
});

test('a pad button that does not exist is dropped', () => {
    const binding = normaliseBinding({
        command: 'pause',
        key: null,
        pads: [['L1'], ['THUMBSTICK'], ['GUIDE', 'L1'], 7],
    });
    // Two chords survive, one of them trimmed to the buttons that exist; the
    // one with nothing left in it is gone rather than kept as an empty chord
    // -- an empty chord is complete in every reading, so it would fire the
    // command on every frame.
    assert.deepEqual(binding, {
        command: 'pause', key: null, pads: [['L1'], ['GUIDE', 'L1']],
    });
});

test('a command may be bound to nothing at all, on purpose', () => {
    const maps = commandMaps(settings({
        commands: [{ command: 'screenshot', key: null, pads: [] }],
    }));
    // Present in the list -- so the settings screen shows the row -- and
    // reachable from nowhere, which is what "off" looks like.
    assert.equal(maps.bindings.length, 1);
    assert.equal(commandForKey(maps, 'F12', false), null);
});

// -- the pad -----------------------------------------------------------------

test('a pad binding is a chord, and a chord fires when it is complete', () => {
    const maps = commandMaps(settings({
        commands: [
            { command: 'screenshot', key: null, pads: [['R1']] },
            { command: 'pause', key: null, pads: [['L1']] },
            { command: 'quicksave', key: null, pads: [['L1', 'R1']] },
            { command: 'reset', key: null, pads: [['L2'], ['L3', 'R3']] },
            { command: 'rewind', key: null, pads: [['L1', 'L2']] },
        ],
    }));

    assert.deepEqual(
        maps.combos.map((combo) => `${combo.buttons.join('+')}->${combo.command}`),
        ['R1->screenshot', 'L1->pause', 'L1+R1->quicksave', 'L2->reset', 'L3+R3->reset'],
    );
    // Rewind is held, and a reading sampled once a frame cannot give a clean
    // release, so it stays on the keyboard.
    assert.equal(maps.combos.some((combo) => combo.command === 'rewind'), false);
});

/** A few frames of pad input, through the real decision function. */
function play(combos, frames, waitMs = 180) {
    let state = NO_CHORDS;
    let clock = 0;
    const fired = [];
    for (const buttons of frames) {
        const step = arbitrateChords(state, buttons, clock, combos, waitMs);
        state = step.state;
        fired.push(...step.fire);
        clock += 16;
    }
    return fired;
}

const chords = (...specs) => specs.map(([command, buttons]) => ({ command, buttons }));

test('a one button chord waits for the longer one it might belong to', () => {
    // The bug this exists for: 暂停 on L1, 存档 on L1+R1. Press the pair
    // quickly and the naive answer fires 暂停 on the way down.
    const combos = chords(['pause', ['L1']], ['quicksave', ['L1', 'R1']]);

    assert.deepEqual(play(combos, [['L1'], ['L1', 'R1'], ['L1', 'R1']]), ['quicksave']);
});

test('and a quick tap still fires when the player lets go', () => {
    const combos = chords(['pause', ['L1']], ['quicksave', ['L1', 'R1']]);

    // Pressed and released between two readings: no frame ever saw the pair,
    // and what they held is what they meant.
    assert.deepEqual(play(combos, [['L1'], []]), ['pause']);
});

test('a hold that is never completed fires once the wait is up', () => {
    const combos = chords(['pause', ['L1']], ['quicksave', ['L1', 'R1']]);

    // Frames are 16ms apart, so fourteen of them is past the 180ms wait: the
    // chord fires once, not once a frame.
    const frames = Array.from({ length: 14 }, () => ['L1']);
    assert.deepEqual(play(combos, frames), ['pause']);
});

test('a three button chord beats a two button chord', () => {
    const combos = chords(
        ['pause', ['L1']],
        ['quicksave', ['L1', 'R1']],
        ['screenshot', ['L1', 'R1', 'L2']],
    );

    // The two button chord is complete for a frame before the third arrives,
    // and it must not fire: it is part of the three.
    assert.deepEqual(
        play(combos, [['L1'], ['L1', 'R1'], ['L1', 'R1', 'L2'], ['L1', 'R1', 'L2']]),
        ['screenshot'],
    );
});

test('with nothing to wait for, a chord fires on the frame it completes', () => {
    // One chord bound on the pad: no longer chord could arrive, so there is
    // nothing to wait for and a delay would be a delay on the point of it.
    const combos = chords(['quicksave', ['L1', 'R1']]);
    assert.deepEqual(play(combos, [['L1'], ['L1', 'R1']]), ['quicksave']);
});

test('letting go of one button does not fire the chord again', () => {
    const combos = chords(['quicksave', ['L1', 'R1']]);

    assert.deepEqual(
        play(combos, [['L1'], ['L1', 'R1'], ['R1'], ['R1'], []]),
        ['quicksave'],
    );
});

test('two commands on the same chord both fire', () => {
    const combos = chords(['quicksave', ['L1', 'R1']], ['screenshot', ['L1', 'R1']]);
    assert.deepEqual(play(combos, [['L1', 'R1']]), ['quicksave', 'screenshot']);
});

test('the old spelling of a pad binding still reads as a chord of one', () => {
    // config.json is a text file a person edits; writing "L1" is far more
    // likely than writing ["L1"].
    const binding = normaliseBinding({ command: 'pause', key: null, pads: ['L1', 'R1'] });
    assert.deepEqual(binding?.pads, [['L1'], ['R1']]);
});

test('a chord is sorted and deduplicated, so equal chords compare equal', () => {
    assert.deepEqual(normaliseBinding({
        command: 'pause', key: null, pads: [['R1', 'L1', 'L1']],
    })?.pads, [['L1', 'R1']]);
    assert.deepEqual(normaliseBinding({ command: 'pause', key: null, pads: [[]] })?.pads, []);
});

test('capturing a chord takes the most that was down at once', () => {
    // L1 and R1 together: one chord of two.
    let capture = captureUpdate(NO_CAPTURE, ['L1']);
    capture = captureUpdate(capture, ['L1', 'R1']);
    assert.deepEqual(capture.held, ['L1', 'R1']);
    assert.equal(captureDone(capture), null, 'still held: nothing to commit yet');

    capture = captureUpdate(capture, []);
    assert.deepEqual(captureDone(capture), ['L1', 'R1']);
});

test('two presses in a row are not a chord', () => {
    // This is the whole rule: a chord is what is down *together*. Somebody who
    // presses L1, lets go, then presses R1 meant L1.
    let capture = captureUpdate(NO_CAPTURE, ['L1']);
    capture = captureUpdate(capture, []);
    assert.deepEqual(captureDone(capture), ['L1']);
});

test('a capture with nothing pressed has nothing to commit', () => {
    assert.equal(captureDone(NO_CAPTURE), null);
    assert.equal(captureDone(captureUpdate(NO_CAPTURE, [])), null);
});

test('the console\'s eight switches are not command buttons', () => {
    // The whole point of the extra ten: binding a command to A would fire it
    // every time the player jumped.
    for (const button of GAMEPAD_SWITCHES) {
        assert.equal(EXTRA_PAD_BUTTONS.includes(button), false, button);
        assert.equal(normaliseBinding({ command: 'pause', key: null, pads: [button] })?.pads.length, 0);
    }
    assert.equal(PAD_BUTTONS.length, GAMEPAD_SWITCHES.length + EXTRA_PAD_BUTTONS.length);
    assert.deepEqual(
        [...PAD_BUTTONS].sort(),
        [...new Set(PAD_BUTTONS)].sort(),
        'a button name appears twice',
    );
});

test('a held command wins a key it shares with a one-shot', () => {
    const maps = commandMaps(settings({
        commands: [
            { command: 'screenshot', key: { code: 'Backspace', shift: false }, pads: [] },
            { command: 'rewind', key: { code: 'Backspace', shift: false }, pads: [] },
        ],
    }));
    assert.equal(commandForKey(maps, 'Backspace', false), 'rewind');
});

test('a key bound twice keeps the first binding', () => {
    const maps = commandMaps(settings({
        commands: [
            { command: 'pause', key: { code: 'F4', shift: false }, pads: [] },
            { command: 'reset', key: { code: 'F4', shift: false }, pads: [] },
        ],
    }));
    assert.equal(commandForKey(maps, 'F4', false), 'pause');
});

test('a pad assigned to nothing still means what its buttons say', () => {
    // The bug this exists for: `release()` runs every frame for a pad whose
    // port is 关闭 (it has to let go of the switches), and it used to forget the
    // chord state with them. A one button chord then waited for a longer one
    // forever -- the state reset before the wait was up -- and the command was
    // silently unreachable on exactly the pad somebody was binding it on.
    //
    // The state is the arbiter's, so what this really pins down is that the
    // *caller* keeps it across frames in which nothing was released. That is
    // `PadHoldings`, which Node cannot reach; what it can reach is that the
    // arbiter itself is stable, which is what the other tests already do. This
    // one is the reminder: one frame of a hold must not erase the previous
    // ones.
    const combos = chords(['pause', ['L1']], ['quicksave', ['L1', 'R1']]);

    // A press held past the wait, with the frames the pad path would feed it.
    const held = Array.from({ length: 14 }, () => ['L1']);
    assert.deepEqual(play(combos, held), ['pause']);

    // And the same press, but where the caller threw the state away every
    // frame -- which is what it used to do -- fires nothing at all.
    let state = NO_CHORDS;
    let clock = 0;
    const withReset = [];
    for (const buttons of held) {
        const step = arbitrateChords(NO_CHORDS, buttons, clock, combos, 180);
        withReset.push(...step.fire);
        state = step.state;
        clock += 16;
    }
    assert.deepEqual(withReset, []);
    assert.notEqual(state, NO_CHORDS);
});
