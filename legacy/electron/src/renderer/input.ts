// ---------------------------------------------------------------------------
// Input
//
// The console has two controller ports and eight switches in each. Everything
// above this file is about turning whatever the players are holding into those
// sixteen booleans, and this file is where the sources are merged.
//
// There are two sources, the keyboard and the gamepads, and more than one of
// each can be holding the same switch:
//
//   Two sources can hold the same switch at once. If each source called
//   setButton directly, letting go of a key would also let go of the button a
//   pad is still pressing, and it would only misbehave for somebody using
//   both at once -- which is nobody, until it is somebody.
//
// So each source reports its own state into the manager, the manager keeps the
// OR per (port, switch), and the console hears only the combined result, and
// only when it changes.
//
// Which port a source's switches go to is the source's business, not the
// manager's. The keyboard asks its binding map (bindings.ts); a pad asks its
// assignment. The manager only ever hears "port 1, A, down".
//
// Why `event.code` and not `event.key`
// ------------------------------------
// `code` names the physical key; `key` names the character it would produce.
// The front end is deciding where a key *is*, not what it says, so a player on
// an AZERTY keyboard keeps Z and X under the same two fingers as everybody
// else. This is the same reason a native front end binds virtual key codes
// rather than characters: it is the position that is being mapped, not the
// letter.
// ---------------------------------------------------------------------------

import {
    DEFAULT_COMMAND_BINDINGS, commandKeyId, isHeldCommand,
    type CommandName as CommandNameFromContract,
    type GamepadButtonName,
    type HeldCommandName as HeldCommandNameFromContract,
} from '../shared/api.ts';
import { commandForKey, type CommandMaps } from './commands.ts';
import type { ResolvedBinding } from './bindings';

/**
 * The eight switches. The names are the contract with the C enum in
 * packages/fc-core/src/ffi/emulator_api.h; the numbers behind them live there and in
 * wasm/emulator.mjs's `Button`.
 */
export type ButtonName = GamepadButtonName;

/**
 * Where a switch's state came from.
 *
 * A pad is named individually -- `pad:0`, `pad:1` -- rather than all pads
 * sharing one source. The manager releases a source on its own, and two pads
 * are two things that can be unplugged independently: player 2's button must
 * not be dropped because player 1's controller ran out of battery.
 */
export type InputSource = 'keyboard' | `pad:${number}`;

/** The source name for one pad. */
export function padSource(index: number): InputSource {
    return `pad:${index}`;
}

/** One key on the keyboard, and where it goes. See bindings.ts. */
export type KeyMap = ReadonlyMap<string, ResolvedBinding>;

/**
 * Keys that do something other than press a switch.
 *
 * The commands and their defaults live in the contract (see
 * `DEFAULT_COMMAND_BINDINGS` in shared/api.ts), because the saved file holds
 * them and the settings screen edits them. What the *listener* uses is the
 * player's own list, read through `handlers.commands()`; the three tables
 * below are the defaults, derived from that one list so that the two cannot
 * disagree, and kept because the settings screen needs to know which keys are
 * spoken for.
 */
export type CommandName = CommandNameFromContract;
export type HeldCommandName = HeldCommandNameFromContract;
export { isHeldCommand };

/** The defaults, as a key-at-a-time table without Shift. */
export const KEY_COMMANDS: Readonly<Record<string, CommandName>> = Object.freeze(
    Object.fromEntries(
        DEFAULT_COMMAND_BINDINGS
            .filter((binding) => binding.key !== null && !binding.key.shift
                && !isHeldCommand(binding.command))
            .map((binding) => [binding.key!.code, binding.command as CommandName]),
    ),
);

/** The same with Shift, which is how twelve commands fit on six keys. */
export const KEY_COMMANDS_SHIFTED: Readonly<Record<string, CommandName>> = Object.freeze(
    Object.fromEntries(
        DEFAULT_COMMAND_BINDINGS
            .filter((binding) => binding.key !== null && binding.key.shift)
            .map((binding) => [binding.key!.code, binding.command as CommandName]),
    ),
);

/** And the held one, which is not a table the others could be part of. */
export const HELD_KEY_COMMANDS: Readonly<Record<string, HeldCommandName>> = Object.freeze(
    Object.fromEntries(
        DEFAULT_COMMAND_BINDINGS
            .filter((binding) => binding.key !== null && isHeldCommand(binding.command))
            .map((binding) => [binding.key!.code, binding.command as HeldCommandName]),
    ),
);





/**
 * The single place that decides what the sixteen switches are doing.
 *
 * `apply` is called with the combined state of one (port, switch), and only
 * when it changes, so the emulator is never told the same thing twice and never
 * told something that one source contradicts.
 */
export class InputManager {
    /** Keyed by `${port}:${button}`, so the port is part of the identity. */
    readonly #held = new Map<string, Set<InputSource>>();
    readonly #apply: (port: number, button: ButtonName, pressed: boolean) => void;

    constructor(apply: (port: number, button: ButtonName, pressed: boolean) => void) {
        this.#apply = apply;
    }

    /** Press or release one switch on one port, for one source. */
    set(port: number, button: ButtonName, pressed: boolean, from: InputSource): void {
        const key = `${port}:${button}`;
        const sources = this.#held.get(key) ?? new Set<InputSource>();
        const wasDown = sources.size > 0;

        if (pressed) {
            sources.add(from);
        } else {
            sources.delete(from);
        }

        const isDown = sources.size > 0;
        if (isDown) {
            this.#held.set(key, sources);
        } else {
            this.#held.delete(key);
        }

        // Only when the *combined* state changed. A source letting go of a
        // button it was never holding -- a gamepad-only button during a
        // keyboard focus loss, say -- must not be reported to the console as
        // a press, and a key repeating while held must not be reported at
        // all. Comparing against the previous combined state is what makes
        // both of those fall out for free, instead of being special cases
        // somebody has to remember.
        if (isDown !== wasDown) {
            this.#apply(port, button, isDown);
        }
    }

    /**
     * Let go of everything one source was holding, and leave the other
     * sources alone. Losing keyboard focus must not drop a gamepad button:
     * the player is still holding the pad.
     */
    releaseAll(from: InputSource): void {
        // Copy the keys first. `set` mutates the map, and a Map cannot be
        // iterated while it is being changed.
        for (const key of [...this.#held.keys()]) {
            const [port, button] = split(key);
            this.set(port, button, false, from);
        }
    }

    /** Let go of everything on every source and both ports. */
    releaseEverything(): void {
        this.#held.clear();
    }

    /**
     * What is down right now, across both ports.
     *
     * The union, without saying who is holding it: the status line is a
     * diagnostic and "A LEFT" is more readable than "1P A, 2P LEFT". The
     * settings screen is where the per-port detail belongs, and it reads the
     * assignments rather than the live state.
     */
    get held(): ButtonName[] {
        const seen = new Set<ButtonName>();
        for (const key of this.#held.keys()) {
            seen.add(split(key)[1]);
        }
        return [...seen];
    }

    /** What is down on one port, for a per-player display. */
    heldOn(port: number): ButtonName[] {
        const seen = new Set<ButtonName>();
        for (const key of this.#held.keys()) {
            const [keyPort, button] = split(key);
            if (keyPort === port) {
                seen.add(button);
            }
        }
        return [...seen];
    }
}

/** Split a `${port}:${button}` map key back into its two halves. */
function split(key: string): [number, ButtonName] {
    const colon = key.indexOf(':');
    return [Number(key.slice(0, colon)), key.slice(colon + 1) as ButtonName];
}

export interface KeyboardHandlers {
    /**
     * The key map in force, read afresh for every event.
     *
     * A function rather than a map so that rebinding takes effect immediately:
     * the listeners are installed once, at mount, and the player may change
     * the bindings an hour later.
     */
    bindings: () => KeyMap;
    /**
     * The commands in force, read afresh for every event.
     *
     * The same shape as `bindings` above and for the same reason: the player
     * may rebind a command an hour into a session, and the listeners were
     * installed once.
     */
    commands?: () => CommandMaps;
    /** A key that does something once, when it goes down. */
    onCommand?: (command: CommandName) => void;
    /** A key that does something for as long as it is held. */
    onCommandState?: (command: HeldCommandName, held: boolean) => void;
}

/**
 * Listen for keys, for as long as the returned function is not called.
 *
 * Nothing in here has a repeat rate, a debounce, or a turbo mode. The hardware
 * has none of those, and a front end that invents them is a front end that
 * makes one game feel wrong to fix another.
 */
export function attachKeyboard(manager: InputManager, handlers: KeyboardHandlers): () => void {
    // Which held commands are down, by the key that is holding them: the
    // browser repeats a held key thirty times a second and the command has to
    // be reported once, and the keyup has to release the command the keydown
    // started even if the player rebound the key in between.
    const heldKeys = new Map<string, HeldCommandName>();

    // Which binding each held key went down with. Kept so that a keyup
    // releases exactly the switch its keydown pressed: the player may change
    // the keyboard mode, or rebind the key, while it is still down -- and
    // resolving the keyup against the *new* map would leave the old port's
    // switch held forever.
    const pressed = new Map<string, ResolvedBinding>();

    /**
     * Whether the key event belongs to a text field rather than the console.
     *
     * The library has a search box, and typing "wasd" into it must search for
     * those letters, not walk Mario left. Without this the listener would
     * swallow the keystroke -- `preventDefault` and all -- and the player
     * could not type the name of a game. Every other element in the page is a
     * button or a label, where the game bindings are still what is wanted.
     */
    const isTyping = (target: EventTarget | null): boolean => {
        if (!(target instanceof HTMLElement)) {
            return false;
        }
        return target.isContentEditable
            || target.tagName === 'INPUT'
            || target.tagName === 'TEXTAREA'
            || target.tagName === 'SELECT';
    };

    /**
     * Whether something else has taken the keyboard for now.
     *
     * The screenshot preview is what this exists for: the picture on the right
     * is covered by a preview, the game is paused, and the arrows are walking
     * through pictures instead of through a level. It is not enough for the
     * preview to stop the events itself -- that handler is one line away from
     * being deleted by accident, and the failure is a button stuck down -- so
     * the page can *say* who owns the keyboard and the console believes it.
     *
     * Deliberately not `aria-modal`: the preview is not modal (the list beside
     * it is still usable), and roles are for people, not for the emulator. This
     * attribute is the emulator's, and it means exactly one thing.
     *
     * Asked of the document rather than of `event.target`, because the target
     * is whatever happens to be focused -- click the background and it is
     * `body` -- and the answer has to be the same either way.
     */
    const keyboardTaken = (): boolean =>
        document.querySelector('[data-keyboard-captured="true"]') !== null;

    const bindingFor = (event: KeyboardEvent): ResolvedBinding | undefined =>
        handlers.bindings().get(event.code);

    /** The command a key event means, or null. See commands.ts. */
    const commandFor = (event: KeyboardEvent): CommandName | HeldCommandName | null => {
        const maps = handlers.commands?.();
        return maps === undefined ? null : commandForKey(maps, event.code, event.shiftKey);
    };

    const onKeyDown = (event: KeyboardEvent): void => {
        if (isTyping(event.target) || keyboardTaken()) {
            return;
        }

        // Commands first, so a rebinding cannot take Escape or a function key
        // away from the application. The two lists are kept disjoint by the
        // settings screen anyway; this is the second lock on the same door.
        const command = commandFor(event);
        if (command !== null) {
            event.preventDefault();
            if (isHeldCommand(command)) {
                // Once per hold, not once per repeat. And the command is
                // remembered by key, so the release resolves to it even if the
                // binding changed while the key was down.
                const id = commandKeyId({ code: event.code, shift: event.shiftKey });
                if (!heldKeys.has(id)) {
                    heldKeys.set(id, command);
                    handlers.onCommandState?.(command, true);
                }
            } else {
                handlers.onCommand?.(command);
            }
            return;
        }

        const binding = bindingFor(event);
        if (binding !== undefined) {
            // Space and the arrows do things to a web page -- scrolling it --
            // and a game screen must not scroll.
            event.preventDefault();
            pressed.set(event.code, binding);
            manager.set(binding.port, binding.button, true, 'keyboard');
        }
    };

    const onKeyUp = (event: KeyboardEvent): void => {
        if (isTyping(event.target) || keyboardTaken()) {
            return;
        }

        // The command the key went *down* with, for the same reason the switch
        // bindings below are resolved that way: a rebinding while the key is
        // held must not leave the machine walking backwards forever.
        const heldId = commandKeyId({ code: event.code, shift: event.shiftKey });
        const held = heldKeys.get(heldId);
        if (held !== undefined) {
            event.preventDefault();
            heldKeys.delete(heldId);
            handlers.onCommandState?.(held, false);
            return;
        }

        // The binding it went down with, not the one in force now. See the
        // note on `pressed` above.
        const binding = pressed.get(event.code);
        if (binding !== undefined) {
            event.preventDefault();
            pressed.delete(event.code);
            manager.set(binding.port, binding.button, false, 'keyboard');
        }
    };

    // The important one. Without this, a key held while the player switches to
    // another application stays held forever: the keyup goes to whatever has
    // focus now, and the game keeps running with the d-pad stuck down.
    //
    // Only the keyboard is released. A gamepad is a different source and the
    // player may still be holding it.
    const onBlur = (): void => {
        pressed.clear();
        manager.releaseAll('keyboard');

        // And anything that was being held down. Losing focus mid rewind would
        // otherwise leave the machine walking backwards forever.
        for (const command of heldKeys.values()) {
            handlers.onCommandState?.(command, false);
        }
        heldKeys.clear();
    };

    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', onBlur);

    return () => {
        window.removeEventListener('keydown', onKeyDown);
        window.removeEventListener('keyup', onKeyUp);
        window.removeEventListener('blur', onBlur);
        manager.releaseAll('keyboard');
    };
}
