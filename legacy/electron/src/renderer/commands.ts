// ---------------------------------------------------------------------------
// The command map: which key and which pad button do which thing.
//
// The console's eight switches go to the game; everything else the player asks
// for -- pause, screenshot, save, load, reset, rewind -- is a *command*, and
// this is where a list of bindings becomes something the input listener can
// look a key up in.
//
// Two things are worth being plain about.
//
// **The saved list is not trusted.** config.json is a text file a person can
// edit and an older build can leave behind, so a binding that names a command
// nothing knows, or a pad button that does not exist, is dropped here rather
// than carried around. What is left is a map that always answers.
//
// **The map is the whole of the resolution.** Three lookups -- a key with
// Shift, a key without, and a pad button -- rather than a function that walks
// the list on every keypress. It is built once per settings change and read on
// every event, which is the way round that matters: a key event is on the
// critical path, and a settings change is not.
//
// No React and no DOM: the resolver is the part with decisions in it, and this
// file is plain TypeScript so that a Node test can reach it
// (test/commands.test.mjs).
// ---------------------------------------------------------------------------

import {
    commandBindings, commandKeyId, isHeldCommand,
    type CommandBinding, type CommandName, type ExtraPadButtonName,
    type HeldCommandName, type InputSettings, type PadCombo,
} from '../shared/api.ts';

// Re-exported so that the settings screen and the tests have one place to
// import the command vocabulary from, whichever side of the renderer they are.
export { COMMAND_ORDER, commandBindings, normaliseBinding } from '../shared/api.ts';

export interface CommandMaps {
    /** `Shift+F1` or `F12` -- see `commandKeyId` -- to the command. */
    readonly keyed: ReadonlyMap<string, CommandName | HeldCommandName>;
    /** The pad chords that fire a command, in the order they were bound. */
    readonly combos: readonly CommandCombo[];
    /** The bindings themselves, for the settings screen. */
    readonly bindings: readonly CommandBinding[];
}

/** One chord, and what it fires. */
export interface CommandCombo {
    readonly command: CommandName;
    /** The buttons that must all be down at once. Never empty. */
    readonly buttons: readonly ExtraPadButtonName[];
}

/** How a chord reads, for a log line or a tooltip. */
export function comboLabel(combo: CommandCombo): string {
    return combo.buttons.join('+');
}

/**
 * The three lookups the input path uses.
 *
 * A key bound twice keeps the first binding: the list is a priority order, the
 * same way the switch bindings are, so what the settings screen shows first is
 * what happens.
 */
export function commandMaps(settings: InputSettings): CommandMaps {
    const bindings = commandBindings(settings);
    const keyed = new Map<string, CommandName | HeldCommandName>();
    const combos: CommandCombo[] = [];

    for (const binding of bindings) {
        if (binding.key !== null) {
            const id = commandKeyId(binding.key);
            const existing = keyed.get(id);
            // A held command wins a contested key. It is the one whose absence
            // is felt: a held command does nothing at all unless the key is
            // held, while a one-shot command can be given another key.
            if (existing === undefined || (isHeldCommand(binding.command) && !isHeldCommand(existing))) {
                keyed.set(id, binding.command);
            }
        }
        // Only the one-shot commands reach the pad list: a held command on a
        // pad chord would need the release, which a reading sampled once a
        // frame cannot give cleanly. Rewind stays on the keyboard.
        if (isHeldCommand(binding.command)) {
            continue;
        }
        for (const chord of binding.pads) {
            if (chord.length > 0) {
                combos.push({ command: binding.command, buttons: chord });
            }
        }
    }

    return { keyed, combos, bindings };
}

/**
 * What a key event means, if anything.
 *
 * `shift` is taken from the event rather than from the key: a binding *is* the
 * combination, so `Shift+F1` and `F1` are two entries and neither is a
 * modifier on the other.
 */
export function commandForKey(
    maps: CommandMaps,
    code: string,
    shift: boolean,
): CommandName | HeldCommandName | null {
    return maps.keyed.get(commandKeyId({ code, shift })) ?? null;
}

/**
 * A chord being captured from a pad.
 *
 * The rule is the same one the bindings themselves follow: a chord is the set
 * of buttons that is down **at once**. So the capture remembers the largest
 * set that has been down together since it began, and commits that when the
 * player lets go of everything. Pressing L1, releasing it and then pressing R1
 * is not L1+R1 -- and pretending otherwise would make a deliberate combination
 * out of two separate presses.
 *
 * No timers and no debounce: the reading arrives once a frame, the screen shows
 * what has been collected as it happens, and the commit is the moment nothing
 * is held.
 */
export interface ComboCapture {
    /** The buttons down right now. */
    readonly held: readonly ExtraPadButtonName[];
    /** The largest set that has been down at once, which is what will be bound. */
    readonly best: PadCombo;
}

export const NO_CAPTURE: ComboCapture = { held: [], best: [] };

/** One frame of a capture: the buttons that are down, out of the reading. */
export function captureUpdate(
    capture: ComboCapture,
    pressed: readonly ExtraPadButtonName[],
): ComboCapture {
    const held = [...new Set(pressed)].sort();
    return {
        held,
        best: held.length > capture.best.length ? held : capture.best,
    };
}

/** The chord to bind, once the player has let go of everything. */
export function captureDone(capture: ComboCapture): PadCombo | null {
    if (capture.held.length > 0 || capture.best.length === 0) {
        return null;
    }
    return capture.best;
}

/** A chord's identity: the command and the buttons, so a hold fires once. */
function chordKey(combo: CommandCombo): string {
    return `${combo.command}|${combo.buttons.join('+')}`;
}

function sameSet(a: readonly string[], b: readonly string[]): boolean {
    return a.length === b.length && a.every((value, index) => value === b[index]);
}

/**
 * What a hold of pad buttons is doing.
 *
 * The problem this solves is not "did a chord complete" -- that is arithmetic
 * -- but "which chord did the player mean". Press L1 on its own and it is
 * `暂停`; press L1 and R1 together and it is `存档`; and the second press starts
 * with the first button already down, so the naive answer fires 暂停 on the way
 * to 存档. Both at once, and the player asked for one of them.
 *
 * Two rules, and they are the whole of it:
 *
 *   the longest chord wins   if a three button chord and a two button chord are
 *                            both satisfied, only the three fires. A chord
 *                            inside another is *part* of it, not a second
 *                            command.
 *   waiting                  while a longer chord could still complete -- every
 *                            button that is down is part of one -- nothing
 *                            fires yet. If the longer one arrives, it wins; if
 *                            the player lets go first, what they held is what
 *                            they meant, and the longest chord satisfied by it
 *                            fires the moment they let go.
 *
 * The wait is only paid when there is something to wait *for*: with a single
 * chord bound, or with the longest chord already complete, it fires on the
 * frame it happens. A delay on every pad hotkey would be a delay on the thing
 * the player is waiting for.
 */
export interface ChordState {
    /** What is down right now. */
    readonly held: PadCombo;
    /** The largest set that has been down since the player last let go. */
    readonly heldBest: PadCombo;
    /** Chords already fired for this hold, so a hold fires once. */
    readonly fired: readonly string[];
    /** When `held` last changed, on the clock the caller passes in. */
    readonly since: number;
}

export const NO_CHORDS: ChordState = { held: [], heldBest: [], fired: [], since: 0 };

/** What a frame of pad input decided. */
export interface ChordStep {
    /** The state to carry into the next frame. */
    readonly state: ChordState;
    /** The commands to fire, in the order they were bound. */
    readonly fire: readonly CommandName[];
}

/** The chords satisfied by a set of buttons: all of their buttons are down. */
function satisfiedBy(combos: readonly CommandCombo[], buttons: PadCombo): CommandCombo[] {
    return combos.filter((combo) => combo.buttons.every((button) => buttons.includes(button)));
}

/**
 * One frame: the buttons that are down, and what that means.
 *
 * `now` and `waitMs` are the caller's clock and the caller's patience, so that
 * this is a plain function of its arguments -- a test can step it a frame at a
 * time without waiting for anything.
 */
export function arbitrateChords(
    state: ChordState,
    pressed: readonly ExtraPadButtonName[],
    now: number,
    combos: readonly CommandCombo[],
    waitMs: number,
): ChordStep {
    const held = [...new Set(pressed)].sort();
    const changed = !sameSet(held, state.held);
    const since = changed ? now : state.since;
    const heldBest = held.length > state.heldBest.length ? held : state.heldBest;

    // The longest chord the player has satisfied, out of the largest set they
    // held. A shorter chord that fits inside it is part of it, and does not
    // fire.
    const satisfied = satisfiedBy(combos, heldBest);
    const longest = satisfied.reduce((size, combo) => Math.max(size, combo.buttons.length), 0);
    const winners = satisfied.filter((combo) => combo.buttons.length === longest);
    const pending = winners.filter((combo) => !state.fired.includes(chordKey(combo)));
    const fired = [...state.fired, ...pending.map(chordKey)];

    if (held.length > 0) {
        // Nothing else was pressed: the player meant what they are holding.
        if (!changed && pending.length === 0) {
            return { state, fire: [] };
        }
        // Could a longer chord still complete? Every button that is down is
        // part of one, so it is worth waiting.
        const couldGrow = combos.some((combo) => (
            combo.buttons.length > held.length
            && held.every((button) => combo.buttons.includes(button))
        ));
        if (couldGrow && now - since < waitMs) {
            return { state: { held, heldBest, fired: state.fired, since }, fire: [] };
        }
        return { state: { held, heldBest, fired, since }, fire: pending.map((c) => c.command) };
    }

    // Everything is up: what they held is what they meant. The chords were
    // decided from `heldBest`, so a tap that was too short to reach the wait
    // still counts.
    return {
        state: { held: [], heldBest: [], fired: [], since: now },
        fire: pending.map((combo) => combo.command),
    };
}

/** How long a chord waits for the longer one it might be part of. */
export const CHORD_WAIT_MS = 180;
