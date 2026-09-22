// ---------------------------------------------------------------------------
// The keyboard map and the pad assignment.
//
// Both are decisions -- which key belongs to which player, which pad is player
// 1 -- and neither needs a window, so both are testable here. The rule that
// matters is the one the whole feature turns on: in one-player mode the
// arrows and WASD are the same person, and in two-player mode they are not.
//
//   pnpm test
// ---------------------------------------------------------------------------

import { test } from 'node:test';
import assert from 'node:assert/strict';

import { DEFAULT_INPUT_SETTINGS } from '../src/shared/api.ts';
import {
    DEFAULT_BINDINGS, activeBindings, padPort, resolveBindings,
} from '../src/renderer/bindings.ts';

const single = (over = {}) => ({ ...DEFAULT_INPUT_SETTINGS, keyboard: '1p', ...over });
const dual = (over = {}) => ({ ...DEFAULT_INPUT_SETTINGS, keyboard: '2p', ...over });

test('every switch is reachable, on both players', () => {
    const resolved = [...resolveBindings(dual()).values()];
    for (const port of [0, 1]) {
        const buttons = new Set(
            resolved.filter((binding) => binding.port === port).map((binding) => binding.button),
        );
        for (const button of ['A', 'B', 'SELECT', 'START', 'UP', 'DOWN', 'LEFT', 'RIGHT']) {
            assert.ok(buttons.has(button), `player ${port + 1} has no ${button}`);
        }
    }
});

test('one player: the arrows and WASD are the same person', () => {
    const map = resolveBindings(single());

    assert.equal(map.get('KeyW').port, 0);
    assert.equal(map.get('ArrowUp').port, 0);
    assert.equal(map.get('KeyZ').port, 0);
    assert.equal(map.get('KeyJ').port, 0);
    assert.equal(map.get('Enter').port, 0);
    assert.equal(map.get('Space').port, 0);

    // And the four directions still mean the four directions.
    assert.equal(map.get('KeyW').button, 'UP');
    assert.equal(map.get('ArrowUp').button, 'UP');
});

test('one player: the keyboard can be player 2 instead', () => {
    const map = resolveBindings(single({ keyboardPlayer: 1 }));

    assert.equal(map.get('KeyW').port, 1);
    assert.equal(map.get('ArrowUp').port, 1);
    assert.equal(map.get('Enter').port, 1);
});

test('two players: WASD is player 1 and the arrows are player 2', () => {
    const map = resolveBindings(dual());

    assert.equal(map.get('KeyW').port, 0);
    assert.equal(map.get('KeyD').port, 0);
    assert.equal(map.get('KeyJ').port, 0);
    assert.equal(map.get('Enter').port, 0);

    assert.equal(map.get('ArrowUp').port, 1);
    assert.equal(map.get('KeyZ').port, 1);
    assert.equal(map.get('Space').port, 1);

    // The same physical key never drives two ports.
    const used = new Set([...map.keys()]);
    assert.equal(used.size, DEFAULT_BINDINGS.length);
});

test('custom bindings replace the defaults wholesale', () => {
    const map = resolveBindings(single({
        bindings: [{ code: 'KeyQ', port: 0, button: 'A' }],
    }));

    assert.equal(map.size, 1);
    assert.deepEqual(map.get('KeyQ'), { port: 0, button: 'A' });
    assert.equal(map.get('KeyW'), undefined);
});

test('a custom binding keeps its port in two-player mode', () => {
    const map = resolveBindings(dual({
        bindings: [
            { code: 'KeyQ', port: 0, button: 'A' },
            { code: 'KeyO', port: 1, button: 'B' },
        ],
    }));

    assert.equal(map.get('KeyQ').port, 0);
    assert.equal(map.get('KeyO').port, 1);
});

test('the first binding for a key wins, so a duplicate is not dead', () => {
    // The resolver returns a map, so a second binding on the same key would be
    // silently unreachable. The settings screen swaps rather than duplicates;
    // this is the resolver being predictable about what a hand-edited file
    // does.
    const map = resolveBindings(dual({
        bindings: [
            { code: 'KeyQ', port: 0, button: 'A' },
            { code: 'KeyQ', port: 1, button: 'B' },
        ],
    }));

    assert.equal(map.size, 1);
    assert.deepEqual(map.get('KeyQ'), { port: 0, button: 'A' });
});

test('null bindings mean the defaults', () => {
    assert.equal(activeBindings(DEFAULT_INPUT_SETTINGS), DEFAULT_BINDINGS);
});

// -- pad assignment ---------------------------------------------------------

test('with no assignment the first two pads are the two players', () => {
    assert.equal(padPort(DEFAULT_INPUT_SETTINGS, 0), 0);
    assert.equal(padPort(DEFAULT_INPUT_SETTINGS, 1), 1);
    assert.equal(padPort(DEFAULT_INPUT_SETTINGS, 2), -1);
});

test('an explicit assignment wins, including "ignore this one"', () => {
    const settings = { ...DEFAULT_INPUT_SETTINGS, padPorts: [1, 0, -1] };
    assert.equal(padPort(settings, 0), 1);
    assert.equal(padPort(settings, 1), 0);
    assert.equal(padPort(settings, 2), -1);
});

test('a gap in the array still falls back to the default', () => {
    const settings = { ...DEFAULT_INPUT_SETTINGS, padPorts: [] };
    assert.equal(padPort(settings, 1), 1);
});
