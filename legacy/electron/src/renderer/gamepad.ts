// ---------------------------------------------------------------------------
// Gamepads.
//
// Two sources, one shape: the browser's Gamepad API and the native helper in
// native/gamepad. Both end up in the same list of summaries and the same
// InputManager, and which one is running is a decision the main process makes
// (see FcBridge.gamepadEnabled).
//
// Multiple pads
// -------------
// The console has two ports and Chromium hands over every connected pad, so
// "the first pad found" is no longer the whole story. Each pad has a slot
// (`index`) and an assignment: which port it drives, or none. The assignment
// is a setting, because which controller is player 2 is not something the
// application can guess -- two identical pads report the same name.
//
// The one structural difference between the two sources is direction. The
// browser source is polled, once per animation frame, because the Gamepad API
// has no change events: there is only a snapshot of where everything is now.
// The native helper already polls at 60Hz in its own process and only speaks
// when something changes, so that source sits and waits to be told.
// ---------------------------------------------------------------------------

import type { GamepadReading, PadReading } from '../shared/api.ts';
import {
    EXTRA_PAD_BUTTONS, GAMEPAD_SWITCHES, PAD_BUTTONS,
    type CommandName, type PadButtonName,
} from '../shared/api.ts';
import {
    arbitrateChords, CHORD_WAIT_MS, NO_CHORDS,
    type ChordState, type CommandMaps,
} from './commands.ts';
import type { ButtonName, InputManager, InputSource } from './input';

/**
 * The source name for one pad.
 *
 * A copy of input.ts's `padSource`, kept here rather than imported so that
 * this module has no runtime imports and a plain Node test can load it. The
 * two are one line each; what matters is that both spell `pad:<index>` the
 * same way, and that is asserted in the tests.
 */
function padSource(index: number): InputSource {
    return `pad:${index}`;
}

/** How far the stick has to move before it counts as a direction. The same
 *  0.5 the native helper uses, for the same reason: a worn stick drifts, and
 *  a deadzone is cheaper than a new controller. */
const DEADZONE = 0.5;

/** The standard-mapping button indices this cares about. */
export const PAD_INDICES = {
    A: 0,
    B: 1,
    // 2 and 3 are the other two face buttons, which the console has no switch
    // for -- see ExtraPadButtonName: they are command buttons, not game ones.
    FACE_X: 2,
    FACE_Y: 3,
    L1: 4,
    R1: 5,
    L2: 6,
    R2: 7,
    SELECT: 8,
    START: 9,
    L3: 10,
    R3: 11,
    UP: 12,
    DOWN: 13,
    LEFT: 14,
    RIGHT: 15,
    GUIDE: 16,
} as const;

/** The parts of a Gamepad this looks at. A real one has far more. */
export interface PadState {
    buttons: readonly { pressed: boolean }[];
    axes: readonly number[];
}

/** The eight that reach the console. The rest of a reading is for commands --
 *  see PadHoldings.apply. */
const SWITCHES: readonly ButtonName[] = [...GAMEPAD_SWITCHES];

/**
 * One pad, as the status line and the settings screen see it.
 *
 * This is a summary rather than a reading: `id` for a person to recognise the
 * controller, `mapping` to explain why it might do nothing, and `port` to show
 * or change which player it drives.
 */
export interface PadSummary {
    /** The slot it was found in. */
    index: number;
    /** The device's name. */
    id: string;
    /** 'standard', 'native', or '' when the browser did not recognise it. */
    mapping: string;
    /** The port it drives, or -1 when it is ignored. */
    port: number;
    /**
     * What is down on it right now, all seventeen names.
     *
     * Carried up to the status line so that the settings screen can be a
     * *tester*: a player who does not know which physical button the helper
     * calls L2 finds out by pressing it and watching a chip light up. Without
     * this the only way to discover it is to bind a command and see whether
     * something happens.
     */
    buttons: Record<PadButtonName, boolean>;
}

/** Every pad that is connected right now. */
export interface PadsReport {
    pads: PadSummary[];
}

/** Nothing plugged in, which is where every session starts. */
export const NO_PADS: PadsReport = { pads: [] };

/**
 * Whether two reports say the same thing.
 *
 * Used to skip a state update -- and so a React render -- on the sixty polls a
 * second that change nothing. Compared field by field rather than by identity,
 * because a poll builds a fresh object every frame.
 */
/**
 * Whether two reports say the same thing.
 *
 * Used to decide whether the status -- and with it the settings screen's pad
 * tester -- has to be redrawn, so it compares everything one of them draws:
 * which pads, where they are assigned, and *which buttons are down*. The last
 * one is what makes the lights light: a report that ignored buttons would be
 * "the same" every time a button was pressed, and the tester would sit there
 * dark while somebody pressed every button on the controller wondering why.
 */
export function samePads(a: PadsReport, b: PadsReport): boolean {
    if (a.pads.length !== b.pads.length) {
        return false;
    }
    return a.pads.every((pad, position) => {
        const other = b.pads[position];
        if (other === undefined
            || pad.index !== other.index
            || pad.id !== other.id
            || pad.mapping !== other.mapping
            || pad.port !== other.port) {
            return false;
        }
        return PAD_BUTTONS.every((button) => pad.buttons[button] === other.buttons[button]);
    });
}
/**
 * Which of the console's eight switches a pad has down.
 *
 * Pulled out of the polling and the manager so that it can be tested. Reading
 * a pad cannot be: `navigator.getGamepads()` exists in a browser and nowhere
 * else, and on macOS it has a side effect serious enough to have its own
 * paragraph in FcBridge.gamepadEnabled. The mapping is the part with decisions
 * in it, so the mapping is the part that is a plain function.
 */
export function mapPad(pad: PadState): Record<PadButtonName, boolean> {
    const pressed = (index: number): boolean => pad.buttons[index]?.pressed ?? false;
    const axis = (index: number): number => pad.axes[index] ?? 0;

    // The stick duplicates the d-pad, because most people reach for it.
    // Axis 1 is positive downwards, which is why down is `>`.
    const stickX = axis(0);
    const stickY = axis(1);

    return {
        LEFT: pressed(PAD_INDICES.LEFT) || stickX < -DEADZONE,
        RIGHT: pressed(PAD_INDICES.RIGHT) || stickX > DEADZONE,
        UP: pressed(PAD_INDICES.UP) || stickY < -DEADZONE,
        DOWN: pressed(PAD_INDICES.DOWN) || stickY > DEADZONE,

        // A is the right hand face button on the console, and index 0 is the
        // bottom one on a modern pad -- which is where the letter A is printed
        // on the pads most people have.
        A: pressed(PAD_INDICES.A),
        B: pressed(PAD_INDICES.B),

        SELECT: pressed(PAD_INDICES.SELECT),
        START: pressed(PAD_INDICES.START),

        // The ones the console has no switch for. They are reported so that a
        // command can be bound to them; nothing in the game can see them.
        FACE_X: pressed(PAD_INDICES.FACE_X),
        FACE_Y: pressed(PAD_INDICES.FACE_Y),
        L1: pressed(PAD_INDICES.L1),
        R1: pressed(PAD_INDICES.R1),
        L2: pressed(PAD_INDICES.L2),
        R2: pressed(PAD_INDICES.R2),
        L3: pressed(PAD_INDICES.L3),
        R3: pressed(PAD_INDICES.R3),
        // The home button. macOS may keep it to itself, in which case it reads
        // as up forever and the player binds a command to something else.
        GUIDE: pressed(PAD_INDICES.GUIDE),
    };
}

/** Everything this module logs goes through here, so a reader can grep for
 *  one word and see the whole story. */
function say(...parts: unknown[]): void {
    console.log('gamepad:', ...parts);
}

/**
 * A gamepad source, whichever kind. Both classes satisfy this by shape.
 *
 * `poll()` exists on the native source too, and does nothing: the frame loop
 * calls it unconditionally, and a push-driven source has nothing to ask for.
 */
export interface GamepadInput {
    readonly report: PadsReport;
    poll(): void;
    destroy(): void;
}

/**
 * How long to keep quiet before saying that nothing has turned up.
 *
 * Two seconds of polling at 60Hz. Chromium only reports a pad once it has been
 * used -- a pad that is plugged in and untouched sends nothing, which looks
 * exactly like a pad that does not work -- so the message that says so is worth
 * more than the silence it replaces.
 */
const QUIET_POLLS = 120;

/**
 * What one pad is holding, and on which port, so that a change of assignment
 * or a disconnection can let go of exactly that pad's switches.
 *
 * It is per pad rather than one set for all of them because pads are named
 * sources in the InputManager: player 2's button must not be dropped because
 * player 1's controller ran out of battery.
 */
class PadHoldings {
    readonly #held = new Map<number, Set<ButtonName>>();
    readonly #ports = new Map<number, number>();
    /**
     * What each pad's buttons have meant so far.
     *
     * A chord needs more state than "what is down": it needs how long the set
     * has been the same (to wait for a longer chord) and what has already fired
     * (so a hold fires once). See `arbitrateChords`.
     */
    readonly #chords = new Map<number, ChordState>();
    readonly #manager: InputManager;
    readonly #commands: () => CommandMaps;
    readonly #onCommand: (command: CommandName) => void;

    constructor(
        manager: InputManager,
        commands: () => CommandMaps,
        onCommand: (command: CommandName) => void,
    ) {
        this.#manager = manager;
        this.#commands = commands;
        this.#onCommand = onCommand;
    }

    /** Which indices are currently holding something. */
    get indices(): number[] {
        return [...this.#held.keys()];
    }

    /**
     * Put a pad's current switches into the manager on `port`, and fire the
     * commands its other buttons are bound to.
     *
     * One reading, two destinations, and the split is the whole point: the
     * console's eight go to the game, and the shoulders, triggers, stick
     * clicks and extra face buttons -- which the console has no switch for --
     * go to the application. Nothing a player binds a command to can be
     * pressed by accident while playing.
     */
    apply(
        index: number,
        port: number,
        wanted: Record<PadButtonName, boolean>,
        log: (text: string) => void,
    ): void {
        this.#fireCommands(index, wanted, log);

        if (port < 0) {
            this.release(index);
            return;
        }

        // A pad that changed port has to let go of the old one first, or the
        // old player would keep walking.
        const previous = this.#ports.get(index);
        if (previous !== undefined && previous !== port) {
            this.release(index);
        }

        const source = padSource(index);
        let held = this.#held.get(index);
        if (held === undefined) {
            held = new Set<ButtonName>();
            this.#held.set(index, held);
        }
        this.#ports.set(index, port);

        for (const button of SWITCHES) {
            const on = wanted[button];
            const was = held.has(button);
            if (on !== was) {
                if (on) {
                    held.add(button);
                } else {
                    held.delete(button);
                }
                log(`${on ? 'down' : 'up  '}  pad ${index}  p${port + 1}  ${button}`);
            }
            this.#manager.set(port, button, on, source);
        }
    }

    /**
     * Decide what this pad's buttons mean, and fire the commands it decided on.
     *
     * The decision is not made here: it is `arbitrateChords`, which knows that
     * a three button chord beats a two button chord and that a one button chord
     * has to wait a moment for the longer one it might be part of. All this
     * does is feed it the reading once a frame and carry its state from one
     * frame to the next.
     */
    #fireCommands(
        index: number,
        wanted: Record<PadButtonName, boolean>,
        log: (text: string) => void,
    ): void {
        const held = EXTRA_PAD_BUTTONS.filter((button) => wanted[button]);
        const maps = this.#commands();
        const step = arbitrateChords(
            this.#chords.get(index) ?? NO_CHORDS,
            held,
            performance.now(),
            maps.combos,
            CHORD_WAIT_MS,
        );
        this.#chords.set(index, step.state);

        for (const command of step.fire) {
            log(`command  pad ${index}  ${held.join('+')}  ->  ${command}`);
            this.#onCommand(command);
        }
    }

    /**
     * Let go of the switches one pad was holding.
     *
     * Called every frame for a pad that is assigned to nothing, and that is why
     * it does *not* touch the chord state: a pad whose port is 关闭 still has
     * buttons, and somebody binding a command to it is holding exactly the pad
     * that is assigned to nothing. Forgetting what they pressed every frame
     * would leave a one button chord waiting for a longer one forever -- the
     * state resets before the wait is up -- which is silence, not a bug you can
     * see.
     */
    release(index: number): void {
        const held = this.#held.get(index);
        const port = this.#ports.get(index);
        if (held !== undefined && port !== undefined) {
            const source = padSource(index);
            for (const button of held) {
                this.#manager.set(port, button, false, source);
            }
        }
        this.#held.delete(index);
        this.#ports.delete(index);
    }

    /** A pad that has gone: its switches and what its buttons meant. */
    forget(index: number): void {
        this.release(index);
        this.#chords.delete(index);
    }

    releaseAll(): void {
        for (const index of [...new Set([...this.#held.keys(), ...this.#chords.keys()])]) {
            this.forget(index);
        }
    }
}

/** Sum up a set of pads in index order, attaching each one's port. */
function summarise(
    portFor: (index: number) => number,
    pads: readonly PadReading[],
    mapping: string,
): PadsReport {
    return {
        pads: [...pads]
            .sort((a, b) => a.index - b.index)
            .map((pad) => ({
                index: pad.index,
                id: pad.id,
                mapping,
                port: portFor(pad.index),
                buttons: pad.buttons,
            })),
    };
}

// ---------------------------------------------------------------------------
// The browser source
// ---------------------------------------------------------------------------

export class GamepadSource implements GamepadInput {
    readonly #portFor: (index: number) => number;
    readonly #holdings: PadHoldings;
    #report: PadsReport = NO_PADS;
    #polls = 0;
    #saidNothing = false;

    constructor(
        manager: InputManager,
        portFor: (index: number) => number,
        commands: () => CommandMaps,
        onCommand: (command: CommandName) => void,
    ) {
        this.#portFor = portFor;
        this.#holdings = new PadHoldings(manager, commands, onCommand);

        // A pad that was already plugged in sends no connect event when the
        // page loads, but polling finds it on the first frame regardless.
        // These two listeners are for the ones that come and go afterwards.
        window.addEventListener('gamepadconnected', this.#onConnected);
        window.addEventListener('gamepaddisconnected', this.#onDisconnected);
    }

    /** What the browser last said, for the status line and the settings screen. */
    get report(): PadsReport {
        return this.#report;
    }

    /** Call once per animation frame. */
    poll(): void {
        this.#polls += 1;

        // Chromium's list is indexed by slot, with holes. Keep the holes: the
        // slot is what the assignment names, so pressing pads into a dense
        // array would silently reassign somebody else's controller.
        const present = new Map<number, Gamepad>();
        for (const candidate of navigator.getGamepads()) {
            if (candidate !== null && candidate.connected) {
                present.set(candidate.index, candidate);
            }
        }

        // A pad that has gone. Its switches are let go here, once, so a pad
        // that runs out of battery mid jump does not leave the jump held.
        for (const index of this.#holdings.indices) {
            if (!present.has(index)) {
                this.#holdings.forget(index);
                say(`disconnected  pad ${index}`);
            }
        }

        const settings = this.#portFor;
        const mapped = new Map<number, Record<PadButtonName, boolean>>();
        const pads: PadReading[] = [];
        for (const [index, pad] of present) {
            const buttons = mapPad(pad);
            mapped.set(index, buttons);
            pads.push({ index, id: pad.id, buttons });
        }
        this.#report = summarise(settings, pads, '');

        // Describe any pad we have not seen before, once.
        for (const [index, pad] of present) {
            if (this.#described.has(index)) {
                continue;
            }
            say(
                `connected     pad ${index} "${pad.id}"`,
                `mapping=${pad.mapping === '' ? '(none)' : pad.mapping}`,
            );
            if (pad.mapping !== 'standard') {
                say('              not the standard mapping: the button indices are a guess');
            }
        }
        this.#described = new Set(present.keys());

        for (const [index, buttons] of mapped) {
            const port = this.#portFor(index);
            this.#holdings.apply(index, port, buttons, (text) => say(text));
        }

        if (present.size === 0) {
            this.#sayNothingYet();
        }
    }

    /** The slots whose connect line has already been printed. */
    #described = new Set<number>();

    destroy(): void {
        window.removeEventListener('gamepadconnected', this.#onConnected);
        window.removeEventListener('gamepaddisconnected', this.#onDisconnected);
        this.#holdings.releaseAll();
        this.#report = NO_PADS;
    }

    readonly #onConnected = (): void => {
        // Nothing to do: poll() picks it up on the next frame.
    };

    readonly #onDisconnected = (): void => {
        // poll() notices the missing pad and releases it there, which keeps
        // the release in one place rather than in two that can drift.
    };

    /** The one-off note that two seconds have gone by with nothing there. */
    #sayNothingYet(): void {
        if (this.#saidNothing || this.#polls < QUIET_POLLS) {
            return;
        }
        this.#saidNothing = true;
        say('no pad after two seconds. In the order worth checking:');
        say('  1. the window has to be focused. The Gamepad API only exposes pads to');
        say('     the focused document, so a pad nobody is looking at is invisible --');
        say('     click the game window, then press a button.');
        say('  2. Chromium only lists a gamepad once it has been used. Press a button');
        say('     or move a stick on the pad itself.');
        say('  3. macOS may be withholding the device. System Settings → Privacy &');
        say('     Security → Input Monitoring, tick Electron (or this application),');
        say('     then quit and start it again -- macOS reads that list at launch.');
    }
}

// ---------------------------------------------------------------------------
// The native helper, from the renderer's side
// ---------------------------------------------------------------------------

export class NativeGamepadSource implements GamepadInput {
    readonly #portFor: (index: number) => number;
    readonly #holdings: PadHoldings;
    #report: PadsReport = NO_PADS;
    /** The pads the last reading described, so a connect can be logged once. */
    #known = new Set<number>();

    constructor(
        manager: InputManager,
        portFor: (index: number) => number,
        commands: () => CommandMaps,
        onCommand: (command: CommandName) => void,
    ) {
        this.#portFor = portFor;
        this.#holdings = new PadHoldings(manager, commands, onCommand);

        // Ask first, then listen. The helper may already have a reading from
        // before this page existed -- a pad that was connected while the
        // window was loading -- and a change-only push would never repeat it.
        // The two can race, which is harmless: applying a reading is
        // idempotent.
        window.fc.onGamepadState((reading: GamepadReading) => this.#apply(reading));
        void window.fc.getGamepadState()
            .then((reading: GamepadReading) => this.#apply(reading))
            .catch(() => undefined);
    }

    get report(): PadsReport {
        return this.#report;
    }

    /** Nothing to ask for. The helper speaks first. */
    poll(): void {
        // Deliberately empty.
    }

    destroy(): void {
        window.fc.offGamepadState();
        this.#holdings.releaseAll();
        this.#report = NO_PADS;
    }

    #apply(reading: GamepadReading): void {
        const settings = this.#portFor;
        const present = new Set(reading.pads.map((pad) => pad.index));

        // Pads that have gone.
        for (const index of this.#holdings.indices) {
            if (!present.has(index)) {
                this.#holdings.forget(index);
            }
        }

        for (const pad of reading.pads) {
            if (!this.#known.has(pad.index)) {
                say(`connected     pad ${pad.index} "${pad.id}" (native, GameController)`);
            }
            const port = this.#portFor(pad.index);
            this.#holdings.apply(pad.index, port, pad.buttons, (text) => say(text));
        }

        this.#known = present;
        this.#report = summarise(settings, reading.pads, 'native');
    }
}

/** Every name a reading carries, exported so the status code can walk one. */
export const PAD_BUTTON_NAMES: readonly PadButtonName[] = [...PAD_BUTTONS];
