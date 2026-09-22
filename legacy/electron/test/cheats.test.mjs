// ---------------------------------------------------------------------------
// Tests for cheats.ts -- the text half of a cheat.
//
// An address is hex and a value is usually decimal, and the only thing this
// decides is how text becomes numbers. The cases that matter are the refusals:
// a half-typed address read as a smaller number would put a byte somewhere the
// player did not ask for, which is the one failure a cheat must not have.
//
//   pnpm test
// ---------------------------------------------------------------------------

import { test } from 'node:test';
import assert from 'node:assert/strict';

import { emptyCheat, formatAddress, parseAddress, parseValue } from '../src/renderer/cheats.ts';

test('an address is hex, however it is written', () => {
    assert.equal(parseAddress('$075A'), 0x075A);
    assert.equal(parseAddress('0x075A'), 0x075A);
    assert.equal(parseAddress('075A'), 0x075A);
    assert.equal(parseAddress('75A'), 0x075A);
    assert.equal(parseAddress('  $075a  '), 0x075A);
});

test('an address that is not one is null, not a guess', () => {
    assert.equal(parseAddress(''), null);
    assert.equal(parseAddress('$'), null);
    assert.equal(parseAddress('$FFFFF'), null);
    assert.equal(parseAddress('hello'), null);
    assert.equal(parseAddress('7G'), null);
});

test('a value is decimal by default and hex when it says so', () => {
    assert.equal(parseValue('3'), 3);
    assert.equal(parseValue('255'), 255);
    assert.equal(parseValue('$FF'), 255);
    assert.equal(parseValue('0x63'), 99);
});

test('a value that is not a byte is null', () => {
    assert.equal(parseValue('256'), null);
    assert.equal(parseValue('$100'), null);
    assert.equal(parseValue('-1'), null);
    assert.equal(parseValue(''), null);
    assert.equal(parseValue('abc'), null);
});

test('an address is shown as four hex digits', () => {
    assert.equal(formatAddress(0x075A), '$075A');
    assert.equal(formatAddress(0), '$0000');
    assert.equal(formatAddress(0xFFFF), '$FFFF');
});

test('a new cheat is frozen and on', () => {
    // Frozen, because a value the game rewrites is the only kind worth having;
    // on, because a row nobody can see do anything looks broken.
    assert.deepEqual(emptyCheat(), {
        label: '', address: 0, value: 0, freeze: true, enabled: true,
    });
});
