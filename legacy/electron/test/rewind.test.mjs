// ---------------------------------------------------------------------------
// Rewind.
//
// The property: winding back k snapshots and replaying the same number of
// frames has to land on exactly the frame it left. Anything less and the
// snapshots are not what they claim to be -- and the failure would show up as
// a game that behaves differently after a rewind, which is the hardest kind of
// bug to attribute to anything.
//
//   pnpm test
// ---------------------------------------------------------------------------

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { Emulator } from '../../wasm/emulator.mjs';
import createFcCore from '../../wasm/dist/fc_core.mjs';
import { Rewind } from '../src/renderer/rewind.ts';

const ROM_PATH = fileURLToPath(new URL('../../packages/fc-core/tests/data/super-mario-bros.nes', import.meta.url));
const hasRom = existsSync(ROM_PATH);

async function booted() {
    const emulator = await Emulator.create({ module: await createFcCore() });
    assert.ok(emulator.loadRom(new Uint8Array(readFileSync(ROM_PATH))), emulator.lastError);
    emulator.reset();
    return emulator;
}

/** A fingerprint of the picture, so two moments can be compared. */
function fingerprint(emulator) {
    const bytes = emulator.framebufferBytes();
    let hash = 2166136261;
    for (let i = 0; i < bytes.length; i += 1) {
        hash = Math.imul(hash ^ bytes[i], 16777619) >>> 0;
    }
    return hash;
}

test('winding back and replaying lands on the same frame', { skip: !hasRom }, async () => {
    const emulator = await booted();
    const rewind = new Rewind(emulator, { slots: 64, everyFrames: 2 });
    try {
        assert.ok(rewind.available);

        for (let frame = 0; frame < 120; frame += 1) {
            emulator.runFrame();
            rewind.record();
        }

        const expected = fingerprint(emulator);
        const depth = rewind.depth;
        assert.ok(depth > 10, `expected a ring of snapshots, got ${depth}`);

        let stepped = 0;
        for (let i = 0; i < 5; i += 1) {
            if (rewind.stepBack()) {
                stepped += 1;
            }
        }
        assert.equal(stepped, 5);

        for (let i = 0; i < stepped * rewind.everyFrames; i += 1) {
            emulator.runFrame();
        }

        assert.equal(fingerprint(emulator), expected, 'the replay did not land where it started');
    } finally {
        rewind.destroy();
        emulator.destroy();
    }
});

test('the ring forgets the oldest snapshot, not the newest', { skip: !hasRom }, async () => {
    const emulator = await booted();
    // Two slots, so the ring wraps almost immediately.
    const rewind = new Rewind(emulator, { slots: 2, everyFrames: 1 });
    try {
        for (let frame = 0; frame < 20; frame += 1) {
            emulator.runFrame();
            rewind.record();
        }

        // Full, and it stays full: recording into a full ring overwrites the
        // oldest rather than refusing to write.
        assert.equal(rewind.depth, 2);

        assert.ok(rewind.stepBack());
        assert.ok(rewind.stepBack());
        assert.equal(rewind.depth, 0);
        assert.equal(rewind.stepBack(), false, 'stepping back past the start must fail');
    } finally {
        rewind.destroy();
        emulator.destroy();
    }
});

test('an empty ring refuses to wind back', { skip: !hasRom }, async () => {
    const emulator = await booted();
    const rewind = new Rewind(emulator, { slots: 8 });
    try {
        assert.equal(rewind.depth, 0);
        assert.equal(rewind.stepBack(), false);
        assert.equal(rewind.seconds, 0);
    } finally {
        rewind.destroy();
        emulator.destroy();
    }
});

test('reset throws the history away', { skip: !hasRom }, async () => {
    const emulator = await booted();
    const rewind = new Rewind(emulator, { slots: 16, everyFrames: 1 });
    try {
        for (let frame = 0; frame < 16; frame += 1) {
            emulator.runFrame();
            rewind.record();
        }
        assert.ok(rewind.depth > 0);

        rewind.reset();

        // The snapshots describe a machine that no longer exists, so keeping
        // them would let the player rewind into it.
        assert.equal(rewind.depth, 0);
        assert.equal(rewind.stepBack(), false);
    } finally {
        rewind.destroy();
        emulator.destroy();
    }
});

test('recording is on a frame count, not an animation frame count', { skip: !hasRom }, async () => {
    const emulator = await booted();
    const rewind = new Rewind(emulator, { slots: 64, everyFrames: 4 });
    try {
        // Three frames is fewer than the interval, so nothing is due yet.
        for (let frame = 0; frame < 3; frame += 1) {
            emulator.runFrame();
            rewind.record();
        }
        assert.equal(rewind.depth, 0);

        emulator.runFrame();
        rewind.record();
        assert.equal(rewind.depth, 1);
    } finally {
        rewind.destroy();
        emulator.destroy();
    }
});
