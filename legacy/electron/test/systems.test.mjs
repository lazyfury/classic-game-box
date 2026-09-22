// ---------------------------------------------------------------------------
// Tests for the console-and-core table in src/renderer/systems.ts.
//
// The two decisions this file owns are easy to get subtly wrong and expensive
// to notice: which console a file belongs to, and which core a pick resolves
// to. A wrong console hands a cartridge to the wrong emulator ("the core did
// not accept this cartridge"), and a wrong core choice -- one that silently
// falls back -- looks like the setting not sticking.
//
//   pnpm test
// ---------------------------------------------------------------------------

import { test } from 'node:test';
import assert from 'node:assert/strict';

import { CORES_BY_SYSTEM } from '../src/shared/api.ts';
import {
    chooseCore, coresFor, extensionOf, sameCore, systemForExtension,
} from '../src/renderer/systems.ts';

test('the extension names the console', () => {
    assert.equal(systemForExtension('Super Mario Bros.nes'), 'nes');
    assert.equal(systemForExtension('game.gba'), 'gba');
    assert.equal(systemForExtension('game.gb'), 'gb');
    assert.equal(systemForExtension('game.gbc'), 'gb');
    // Anything unrecognised is a NES game, which is the old behaviour and the
    // one that fails clearly -- the NES core refuses the cartridge.
    assert.equal(systemForExtension('no-extension'), 'nes');
    assert.equal(systemForExtension('archive.zip'), 'nes');
});

test('the extension itself, lower cased', () => {
    assert.equal(extensionOf('game.NES'), 'nes');
    assert.equal(extensionOf('a.b.c.gbc'), 'gbc');
    assert.equal(extensionOf('no-dot'), '');
});

test('without a choice, each console gets its default core', () => {
    // Mesen is the NES default; the built-in FC core is the second choice.
    assert.equal(chooseCore('game.nes').id, 'mesen');
    assert.equal(chooseCore('game.gba').id, 'mgba');
    assert.equal(chooseCore('game.gb').id, 'mgba');
});

test('a NES pick chooses between the two NES cores', () => {
    const fc = chooseCore('game.nes', { nes: 'fc' });
    const mesen = chooseCore('game.nes', { nes: 'mesen' });
    assert.equal(fc.id, 'fc');
    assert.equal(mesen.id, 'mesen');
    assert.notEqual(fc.module, mesen.module);
    // The default and an explicit Mesen pick are the same machine.
    assert.ok(sameCore(mesen, chooseCore('game.nes')));
});

test('a pick for the wrong console is ignored, not honoured', () => {
    // Mesen is a NES core; asking for it on a Game Boy must not select it.
    const core = chooseCore('game.gb', { gb: 'mesen' });
    assert.equal(core.system, 'gb');
    assert.equal(core.id, 'mgba');
});

test('coresFor lists each console in the shared order, default first', () => {
    assert.deepEqual(coresFor('nes').map((core) => core.id), ['mesen', 'fc']);
    assert.deepEqual(coresFor('gba').map((core) => core.id), ['mgba']);
    assert.deepEqual(coresFor('gb').map((core) => core.id), ['mgba']);
});

test('every pair the shared table promises can actually be run', () => {
    // The main process validates a saved selection against CORES_BY_SYSTEM and
    // the renderer resolves it through this table. A pair in one and not the
    // other would be a choice that is remembered and then quietly ignored.
    for (const [system, ids] of Object.entries(CORES_BY_SYSTEM)) {
        const available = coresFor(system).map((core) => core.id);
        assert.deepEqual(available, [...ids], `${system} cores`);
    }
});

test('sameCore is true only for the same core on the same console', () => {
    const fc = chooseCore('game.nes', { nes: 'fc' });
    const mesen = chooseCore('game.nes', { nes: 'mesen' });
    const gba = chooseCore('game.gba');
    const gb = chooseCore('game.gb');

    // The default is Mesen, so a pick of Mesen is the same machine.
    assert.ok(sameCore(mesen, chooseCore('game.nes')));
    assert.ok(sameCore(fc, chooseCore('game.nes', { nes: 'fc' })));
    assert.ok(!sameCore(fc, mesen));
    assert.ok(!sameCore(null, fc));
    // The same module on two consoles is still two machines: mGBA on a GBA and
    // on a Game Boy run at different rates.
    assert.equal(gba.module, gb.module);
    assert.ok(!sameCore(gba, gb));
});
