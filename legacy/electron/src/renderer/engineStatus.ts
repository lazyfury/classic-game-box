// ---------------------------------------------------------------------------
// What the status is, and what it becomes when the slot is emptied.
//
// These are not hook logic, they are the shape of the engine's report and one
// transition on it, and they live apart from the hook for a reason that has
// nothing to do with tidiness: `useEmulator.ts` imports WebAssembly and React,
// so nothing in it can be reached from a plain Node test. A transition that
// only exists inside a `useState` callback can only be tested by driving a
// whole Electron window, which means in practice that it is not tested -- and
// the bug this file was written for was exactly that: an eject that cleared
// the machine but left `romPath` pointing at the game, so the title, the
// transport buttons, the placeholder, the marked card and the save panel all
// went on describing a cartridge that was no longer in the slot.
//
//   pnpm test           (see test/status.test.mjs)
// ---------------------------------------------------------------------------

// `PadsReport` is imported as a *type* and nothing else, which matters here:
// this file is loaded by plain Node from test/status.test.mjs, and Node's type
// stripping cannot resolve an extensionless import of another `.ts` file at
// run time. A value import would make this module untestable; a type import is
// erased before Node ever sees it. The one value that would have crossed --
// the empty report -- is written out below instead, and test/status.test.mjs
// checks that the two stay equal.
import type { PadsReport } from './gamepad';
import type { ButtonName } from './input';

/** Where the engine is in its life cycle. */
export type EngineState = 'loading' | 'running' | 'halted' | 'error';

/** What the audio ring is doing, for the status line. */
export interface AudioStatus {
    /** 'running', 'suspended', 'closed', or 'unavailable'. */
    state: string;
    /** Samples waiting in the ring. */
    fill: number;
    /** Where the fill is being held, by the rate control. */
    targetFill: number;
    /** Times the audio thread ran out. */
    underruns: number;
    /** Samples the ring had no room for. Should stay at zero. */
    dropped: number;
}

export interface EngineStatus {
    state: EngineState;
    /** Emulated frames per second, averaged over the last status window. */
    fps: number;
    frameCount: number;
    totalCycles: number;
    cpuPc: number;
    romSummary: string;
    /**
     * The cartridge in the slot, or null when there is none.
     *
     * This is the single field the whole play column is derived from: the
     * title, whether the transport buttons are enabled, whether the placeholder
     * is showing, which card in the library is marked as playing, and what the
     * save panel thinks it is saving.
     */
    romPath: string | null;
    /** Highest APU sample in the last window. 0.000 means silence. */
    audioPeak: number;
    /** Which switches are down right now, for the status line. */
    held: ButtonName[];
    /** True while the frame loop is held still. */
    paused: boolean;
    /** True while the player is holding the rewind key. */
    rewinding: boolean;
    /** How far back the machine can be wound, or null if it cannot be. */
    rewind: { depth: number; seconds: number; bytes: number } | null;
    /** A short lived note about the last thing the player asked for, such as
     *  "saved slot 1". Null most of the time. */
    message: string | null;
    /** Null until the audio pipeline exists, and if it could not be built. */
    audio: AudioStatus | null;
    audioError: string | null;
    error: string | null;
    /**
     * What the gamepad sources say about the pads, if any are running.
     *
     * This is a copy of readings rather than a fact about the cartridge, so
     * it survives an eject: the pads are still on the desk.
     */
    gamepad: PadsReport;
}

/** Before anything has happened: no machine, no cartridge, no readings. */
export const INITIAL_STATUS: EngineStatus = {
    state: 'loading',
    fps: 0,
    frameCount: 0,
    totalCycles: 0,
    cpuPc: 0,
    romSummary: '',
    romPath: null,
    audioPeak: 0,
    held: [],
    paused: false,
    rewinding: false,
    rewind: null,
    message: null,
    audio: null,
    audioError: null,
    error: null,
    // `NO_PADS` from gamepad.ts, written out. See the note at the top of this
    // file for why it is not imported.
    gamepad: { pads: [] },
};

/**
 * The status with the cartridge taken out.
 *
 * Everything that describes the game being played goes, and everything that
 * describes the application stays: the audio pipeline is still connected, the
 * machine is still running, and the error that stopped the last one is not
 * carried over to the next.
 *
 * The readings go too. They described a machine that is no longer in the slot,
 * and a status bar still counting the frames of a game that was taken out is
 * the same bug wearing a different hat.
 */
export function unloaded(status: EngineStatus): EngineStatus {
    return {
        ...status,
        // A cartridge that halted the processor is over; the engine itself is
        // fine and will run the next one. An *application* error is not
        // something an eject can clear, so it is left alone, and `loading`
        // means the status has not been through its first transition yet.
        state: status.state === 'halted' ? 'running' : status.state,
        paused: false,
        message: null,
        held: [],
        romPath: null,
        romSummary: '',
        fps: 0,
        frameCount: 0,
        totalCycles: 0,
        cpuPc: 0,
        audioPeak: 0,
        rewinding: false,
        rewind: null,
    };
}
