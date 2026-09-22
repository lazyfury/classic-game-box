// ---------------------------------------------------------------------------
// The keyboard map: which key is which switch, and which player it belongs to.
//
// Two players share one keyboard, and the only thing that makes that possible
// is a convention: one hand on WASD and the other on the arrows. So the
// defaults below are in two groups -- player 1's left hand and player 2's
// right hand -- and the console's two ports are what the groups are wired to.
//
// One player and two players are the same list read two ways. In '2p' the
// ports are the ones written below, so WASD and the arrows are two people. In
// '1p' every key is redirected to one chosen port, so the same list becomes a
// single player who happens to have both hands covered -- which is what the
// original keyboard map was before there was a second port to think about.
//
// This file is deliberately free of React, Electron and the DOM. The resolver
// is the part with decisions in it, and it is a plain function so a Node test
// can reach it (test/bindings.test.mjs).
// ---------------------------------------------------------------------------

import type { GamepadButtonName, InputSettings, KeyBinding } from '../shared/api';

/** One key and where it goes, after the mode has been applied. */
export interface ResolvedBinding {
    port: number;
    button: GamepadButtonName;
}

/**
 * The default map.
 *
 * Player 1 is the left hand cluster: WASD for the d-pad, J and K for the two
 * face buttons, Enter and Tab for Start and Select. Player 2 is the right
 * hand one: the arrows, Z and X (which is what a single player's right hand
 * already used), and Space and right Shift.
 *
 * The order matters in exactly one case: when two entries share a key, the
 * first wins, so the list is a priority order rather than a set.
 */
export const DEFAULT_BINDINGS: readonly KeyBinding[] = Object.freeze([
    // Player 1
    { code: 'KeyW', port: 0, button: 'UP' },
    { code: 'KeyS', port: 0, button: 'DOWN' },
    { code: 'KeyA', port: 0, button: 'LEFT' },
    { code: 'KeyD', port: 0, button: 'RIGHT' },
    { code: 'KeyJ', port: 0, button: 'B' },
    { code: 'KeyK', port: 0, button: 'A' },
    { code: 'Enter', port: 0, button: 'START' },
    { code: 'Tab', port: 0, button: 'SELECT' },

    // Player 2
    { code: 'ArrowUp', port: 1, button: 'UP' },
    { code: 'ArrowDown', port: 1, button: 'DOWN' },
    { code: 'ArrowLeft', port: 1, button: 'LEFT' },
    { code: 'ArrowRight', port: 1, button: 'RIGHT' },
    { code: 'KeyZ', port: 1, button: 'B' },
    { code: 'KeyX', port: 1, button: 'A' },
    { code: 'Space', port: 1, button: 'START' },
    { code: 'ShiftRight', port: 1, button: 'SELECT' },
]);

/** The bindings in force: the player's own if they have made any, else the defaults. */
export function activeBindings(settings: InputSettings): readonly KeyBinding[] {
    return settings.bindings ?? DEFAULT_BINDINGS;
}

/**
 * The key map, with the keyboard mode applied.
 *
 * In '1p' every binding is redirected to `keyboardPlayer`; in '2p' it keeps
 * the port it was given. That one line is the whole difference between "the
 * arrows and WASD are the same person" and "they are two", and it is why a
 * custom binding does not have to know anything about the mode it was made in:
 * the port on the binding is the two-player answer, and single-player mode
 * overrides it.
 */
export function resolveBindings(
    settings: InputSettings,
): Map<string, ResolvedBinding> {
    const resolved = new Map<string, ResolvedBinding>();
    for (const binding of activeBindings(settings)) {
        // First wins. A later duplicate is dropped rather than replacing an
        // earlier one, so reordering the list is how a conflict is resolved.
        if (resolved.has(binding.code)) {
            continue;
        }
        resolved.set(binding.code, {
            port: settings.keyboard === '1p' ? settings.keyboardPlayer : binding.port,
            button: binding.button,
        });
    }
    return resolved;
}

/**
 * Which port a connected pad drives.
 *
 * The assignment is by pad index, which is a slot rather than a device: two
 * identical controllers report the same name, so the index is the only handle
 * there is. An explicit entry wins, including `-1` meaning "ignore this one";
 * with no entry the first pad is player 1 and the second is player 2, which is
 * what somebody who plugs in two pads and changes nothing expects.
 */
export function padPort(settings: InputSettings, index: number): number {
    const assigned = settings.padPorts[index];
    if (assigned !== undefined) {
        return assigned;
    }
    return index <= 1 ? index : -1;
}

/**
 * Whether a pad port setting is one this build understands.
 *
 * Used by the settings screen before it writes, and by the main process when
 * it reads a file that a person may have edited.
 */
export function isPort(value: unknown): value is number {
    return value === 0 || value === 1 || value === -1;
}
