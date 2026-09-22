// ---------------------------------------------------------------------------
// Tests for engineStatus.ts -- the shape of the engine's report, and what
// happens to it when the cartridge comes out.
//
// This file exists because of a bug that no other test could see. Ejecting a
// cartridge stopped the machine but left `romPath` pointing at the game, and
// every part of the play column is derived from that one field: the title, the
// transport buttons, the placeholder, which card in the library was marked as
// playing, and what the save panel thought it was saving. All of them went on
// describing a cartridge that was no longer in the slot, in a window that
// looked otherwise fine.
//
// The transition lives in a module with no WebAssembly in it precisely so it
// can be tested here, from plain Node, without launching a window.
//
//   pnpm test
// ---------------------------------------------------------------------------

import { test } from 'node:test';
import assert from 'node:assert/strict';

import { INITIAL_STATUS, unloaded } from '../src/renderer/engineStatus.ts';
import { NO_PADS } from '../src/renderer/gamepad.ts';

/** A machine that is running a game and has been for a while. */
function playing() {
    return {
        ...INITIAL_STATUS,
        state: 'running',
        romPath: '/library/Mario.nes',
        romSummary: 'NROM · 2 PRG · 1 CHR · mapper 0',
        fps: 60.1,
        frameCount: 12345,
        totalCycles: 20_000_000,
        cpuPc: 0x8e19,
        audioPeak: 0.412,
        held: ['A', 'RIGHT'],
        paused: true,
        rewinding: true,
        rewind: { depth: 600, seconds: 10, bytes: 6_000_000 },
        message: 'saved slot 1',
        audio: { state: 'running', fill: 2945, targetFill: 3072, underruns: 0, dropped: 0 },
    };
}

test('ejecting takes the cartridge out of the status', () => {
    const after = unloaded(playing());

    // The field the whole play column hangs off. This is the assertion that
    // would have caught the bug.
    assert.equal(after.romPath, null);
    assert.equal(after.romSummary, '');
});

test('ejecting takes the readings with it', () => {
    const after = unloaded(playing());

    // A status bar still counting the frames of a game that was taken out is
    // the same bug wearing a different hat.
    assert.equal(after.fps, 0);
    assert.equal(after.frameCount, 0);
    assert.equal(after.totalCycles, 0);
    assert.equal(after.cpuPc, 0);
    assert.equal(after.audioPeak, 0);
});

test('ejecting lets go of everything the player was doing', () => {
    const after = unloaded(playing());

    assert.equal(after.paused, false);
    assert.equal(after.rewinding, false);
    assert.equal(after.rewind, null);
    assert.deepEqual(after.held, []);
    assert.equal(after.message, null);
});

test('ejecting leaves the machine and the audio pipeline alone', () => {
    const before = playing();
    const after = unloaded(before);

    // The engine is up and will run the next cartridge. It simply has nothing
    // in it: `loading` would be wrong, and so would `halted`.
    assert.equal(after.state, 'running');
    assert.deepEqual(after.audio, before.audio);
    assert.equal(after.audioError, before.audioError);
    assert.equal(after.error, before.error);
    // The pad is still on the desk. Which cartridge is in the slot has nothing
    // to do with what the browser says about it.
    assert.deepEqual(after.gamepad, before.gamepad);
});

test('the empty gamepad report is one value, written twice', () => {
    // engineStatus.ts cannot import it -- see the note at the top of that file
    // -- so it is written out there and this is what keeps the two copies
    // honest.
    assert.deepEqual(INITIAL_STATUS.gamepad, NO_PADS);
});

test('a halted cartridge is over when it is taken out', () => {
    // `halted` means the processor met an opcode this build does not
    // implement. The engine is fine and will run the next cartridge, so the
    // state left behind by the old one must not survive it -- a status line
    // reading "the emulator met an opcode it does not implement" for the rest
    // of the session is not an improvement.
    const halted = { ...playing(), state: 'halted', error: 'the CPU halted at frame 300' };
    assert.equal(unloaded(halted).state, 'running');
});

test('an application error is not something an eject can clear', () => {
    const failed = { ...INITIAL_STATUS, state: 'error', error: 'the renderer could not start' };
    const after = unloaded(failed);
    assert.equal(after.state, 'error');
    assert.equal(after.error, 'the renderer could not start');
});

test('ejecting twice is the same as ejecting once', () => {
    const once = unloaded(playing());
    assert.deepEqual(unloaded(once), once);
});

test('ejecting a machine that never had a cartridge changes nothing', () => {
    assert.deepEqual(unloaded(INITIAL_STATUS), INITIAL_STATUS);
});
