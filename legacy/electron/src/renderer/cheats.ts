// ---------------------------------------------------------------------------
// Cheat addresses, as a person types them.
//
// A NES address is written in hex, usually with a `$` in front of it -- `$075A`
// is where Super Mario Bros. keeps its lives -- and a value is usually written
// in decimal, because that is what "3 lives" means. The panel takes both, and
// this is the part that turns text into numbers, kept away from React so a
// Node test can reach it.
// ---------------------------------------------------------------------------

import type { Cheat } from '../shared/api';

/**
 * Read a hex address: `$075A`, `0x075A`, `075A` and `75A` all mean the same
 * thing. Anything else is null, because silently reading `7F` as 7 would put a
 * value somewhere the player did not ask for.
 */
export function parseAddress(text: string): number | null {
    const digits = text.trim().replace(/^\$/, '').replace(/^0x/i, '');
    if (!/^[0-9a-f]{1,4}$/i.test(digits)) {
        return null;
    }
    const value = Number.parseInt(digits, 16);
    return Number.isInteger(value) && value >= 0 && value <= 0xFFFF ? value : null;
}

/**
 * Read a byte: decimal by default, hex when it says so (`$FF`, `0xFF`).
 *
 * Decimal by default because the value in a cheat almost always reads as a
 * count -- three lives, nine coins -- and hex because a value copied out of a
 * hex editor should not have to be converted by hand.
 */
export function parseValue(text: string): number | null {
    const trimmed = text.trim();
    const hex = /^\$/.test(trimmed) || /^0x/i.test(trimmed);
    const digits = trimmed.replace(/^\$/, '').replace(/^0x/i, '');
    if (digits === '' || !/^[0-9a-f]+$/i.test(digits)) {
        return null;
    }
    const value = Number.parseInt(digits, hex ? 16 : 10);
    return Number.isInteger(value) && value >= 0 && value <= 0xFF ? value : null;
}

/** `$075A`, the way the address is shown in the panel. */
export function formatAddress(address: number): string {
    return `$${address.toString(16).toUpperCase().padStart(4, '0')}`;
}

/** A new row: frozen and on, because that is what a player almost always wants. */
export function emptyCheat(): Cheat {
    return { label: '', address: 0, value: 0, freeze: true, enabled: true };
}
