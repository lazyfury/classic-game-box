// ---------------------------------------------------------------------------
// Tests for the wasm boundary in wasm/emulator.mjs.
//
// The thing this file exists for: the C interface returns a uint64_t for the
// cycle counter, Emscripten hands that back as a BigInt, and a BigInt looks
// exactly like a number until the first arithmetic expression it appears in --
// at which point JavaScript throws instead of coercing. That produced a
// renderer that worked in a headless test and crashed the moment a status line
// displayed it.
//
//   pnpm test
// ---------------------------------------------------------------------------

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { Emulator, EXPECTED_ABI } from '../../wasm/emulator.mjs';
import createFcCore from '../../wasm/dist/fc_core.mjs';

// A symlink into a real ROM folder. Nothing is committed there, so this whole
// file skips itself on a machine that has no ROMs.
const ROM_PATH = fileURLToPath(new URL('../../packages/fc-core/tests/data/super-mario-bros.nes', import.meta.url));
const hasRom = existsSync(ROM_PATH);

async function bootedEmulator() {
    const emulator = await Emulator.create({ module: await createFcCore() });
    assert.ok(emulator.loadRom(new Uint8Array(readFileSync(ROM_PATH))), emulator.lastError);
    emulator.reset();
    for (let frame = 0; frame < 10; frame += 1) {
        emulator.runFrame();
    }
    return emulator;
}

test('every counter the front end touches is a plain number', { skip: !hasRom }, async () => {
    const emulator = await bootedEmulator();
    try {
        // typeof catches BigInt, which is what the cycle counter used to be.
        assert.equal(typeof emulator.totalCycles, 'number', 'totalCycles is a BigInt');
        assert.equal(typeof emulator.frameCount, 'number', 'frameCount is a BigInt');
        assert.equal(typeof emulator.cpuPc, 'number', 'cpuPc is a BigInt');
        assert.equal(typeof emulator.samplesPending, 'number', 'samplesPending is a BigInt');
        assert.equal(typeof emulator.pixel(0, 0), 'number', 'pixel() is a BigInt');
    } finally {
        emulator.destroy();
    }
});

test('a counter can be compared and divided like a number', { skip: !hasRom }, async () => {
    const emulator = await bootedEmulator();
    try {
        // This is the shape the status bar was using. With a BigInt the first
        // comparison throws "Cannot mix BigInt and other types", so what is
        // being asserted is that it returns at all.
        assert.ok(emulator.totalCycles > 0);
        assert.ok(Number.isFinite(emulator.totalCycles / 1_000_000));
    } finally {
        emulator.destroy();
    }
});

test('the ABI the wrapper expects is the ABI the module speaks', { skip: !hasRom }, async () => {
    // A stale fc_core.wasm must not load quietly. See wasm/glue.cpp.
    const module = await createFcCore();
    assert.equal(module._fc_wasm_abi(), EXPECTED_ABI);
    assert.equal(module._fc_wasm_screen_width(), 256);
    assert.equal(module._fc_wasm_screen_height(), 240);
    assert.equal(module._fc_wasm_sample_rate(), 44100);
});

// -- save states ------------------------------------------------------------

test('a state round trips, and the machine carries on identically', { skip: !hasRom }, async () => {
    const machine = await bootedEmulator();
    try {
        const state = machine.saveState();
        assert.ok(state instanceof Uint8Array, 'saveState did not return bytes');
        assert.equal(String.fromCharCode(...state.slice(0, 4)), 'FCST');

        // Where it would have gone without the round trip.
        for (let frame = 0; frame < 30; frame += 1) {
            machine.runFrame();
        }
        const expected = machine.framebufferBytes().slice();
        const expectedCycles = machine.totalCycles;

        assert.ok(machine.loadState(state));
        for (let frame = 0; frame < 30; frame += 1) {
            machine.runFrame();
        }

        assert.deepEqual(machine.framebufferBytes(), expected);
        assert.equal(machine.totalCycles, expectedCycles);
    } finally {
        machine.destroy();
    }
});

test('a state for another cartridge is refused', { skip: !hasRom }, async () => {
    const machine = await bootedEmulator();
    try {
        const state = machine.saveState();

        // The PRG size lives at offset 12 and is what makes a state from
        // another game detectable without hashing the ROM.
        const wrong = Uint8Array.from(state);
        wrong[12] = (wrong[12] + 1) & 0xFF;
        assert.equal(machine.loadState(wrong), false);

        const future = Uint8Array.from(state);
        future[4] = 99;
        assert.equal(machine.loadState(future), false);

        assert.equal(machine.loadState(state.slice(0, state.length - 64)), false);
        assert.equal(machine.loadState(new Uint8Array(0)), false);
    } finally {
        machine.destroy();
    }
});

test('the mapper says whether it saves its bank registers', { skip: !hasRom }, async () => {
    const machine = await bootedEmulator();
    try {
        // Super Mario Bros is NROM, which is one of the mappers that does.
        assert.equal(typeof machine.mapperSavesState, 'boolean');
    } finally {
        machine.destroy();
    }
});

// -- cheats and peek --------------------------------------------------------
//
// The whole chain, one layer above test_cheats.cpp: console RAM, the C bus it
// goes through, the four-byte packing `setCheats` writes, and the mirroring
// that falls out of the address decoder. `$075A` is where Super Mario Bros
// keeps the number of lives.

test('a poke lands in console RAM, and its mirrors agree', { skip: !hasRom }, async () => {
    const machine = await bootedEmulator();
    try {
        machine.poke(0x075A, 0x63);
        assert.equal(machine.peek(0x075A), 0x63);
        assert.equal(machine.peek(0x0F5A), 0x63);
        assert.equal(machine.peek(0x175A), 0x63);
    } finally {
        machine.destroy();
    }
});

test('peek answers nothing for a register', { skip: !hasRom }, async () => {
    const machine = await bootedEmulator();
    try {
        // Reading $2002 through the bus clears the vblank flag; a peek must
        // be a question that does not change the answer.
        assert.equal(machine.peek(0x2002), 0);
    } finally {
        machine.destroy();
    }
});

test('a frozen cheat is put back at the start of every frame', { skip: !hasRom }, async () => {
    const machine = await bootedEmulator();
    try {
        machine.setCheats([{ address: 0x075A, value: 0x63, freeze: true, enabled: true }]);
        assert.equal(machine.cheatCount, 1);

        for (let frame = 0; frame < 5; frame += 1) {
            machine.runFrame();
        }

        assert.equal(machine.peek(0x075A), 0x63);
    } finally {
        machine.destroy();
    }
});

test('a cheat that is switched off is remembered but not written', { skip: !hasRom }, async () => {
    const machine = await bootedEmulator();
    try {
        machine.poke(0x075A, 0x02);
        machine.setCheats([{ address: 0x075A, value: 0x63, freeze: true, enabled: false }]);
        assert.equal(machine.cheatCount, 1);

        machine.runFrame();

        assert.notEqual(machine.peek(0x075A), 0x63);
    } finally {
        machine.destroy();
    }
});

test('an empty cheat list empties the core', { skip: !hasRom }, async () => {
    const machine = await bootedEmulator();
    try {
        machine.setCheats([{ address: 0x075A, value: 0x63, freeze: true, enabled: true }]);
        assert.equal(machine.cheatCount, 1);
        machine.setCheats([]);
        assert.equal(machine.cheatCount, 0);
    } finally {
        machine.destroy();
    }
});
