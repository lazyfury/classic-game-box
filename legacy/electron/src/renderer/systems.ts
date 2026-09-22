// ---------------------------------------------------------------------------
// Which console a game is for, and which core runs it.
//
// The front end runs any libretro core the same way; the only things it has to
// decide are *which console* a file is for and *which core* to run that console
// on. A file extension answers the first, and a player's choice answers the
// second -- 设置 → 模拟器核心 lets them pick between emulators that target the
// same console. This is the one place both mappings live.
//
// A core is more than a file name. The frame rate and the sample rate are
// properties of the console (and, for mGBA, of the machine it is emulating),
// and the audio pipeline is built around the sample rate -- so they travel
// together with the module here rather than being read from the core. The NES
// core could be asked for its geometry at any time, but mGBA's only answers
// once a game is in, and the audio device has to exist before that. So the
// numbers are known up front.
//
// The values:
//
//   FC (NES)   256x240, 60.0988 fps, 44100 Hz   (the PPU divides the NTSC burst)
//   Mesen      256x240, 60.0998 fps, 48000 Hz   (what Mesen reports)
//   GBA        240x160, 59.7275 fps, 65536 Hz
//   GB / GBC   160x144, 59.7275 fps, 131072 Hz  (mGBA resamples the GB clock)
//
// These are what the cores report through retro_get_system_av_info; a change
// to them would show up as audio at the wrong pitch, which is the point of
// keeping the number next to the module and not somewhere the core could
// disagree with it.
// ---------------------------------------------------------------------------

// The explicit `.ts` is what lets a Node test import this file directly: the
// test runner strips the types but does not guess extensions, and the import
// below is a runtime one (the shared core table), not a type-only import.
import { CORES_BY_SYSTEM, type CoreId, type CoreSelection, type SystemId } from '../shared/api.ts';

export type { CoreId, CoreSelection, SystemId };

export interface CoreChoice {
    /** Which core, for comparing one choice against another. */
    readonly id: CoreId;
    /** What the settings screen calls it. */
    readonly name: string;
    /** The console it emulates. */
    readonly system: SystemId;
    /** The emscripten module's file name inside wasm/dist. */
    readonly module: string;
    /** What the core produces, in samples per second. */
    readonly sampleRate: number;
    /** One emulated frame, in seconds. */
    readonly frameSeconds: number;
}

/**
 * The metadata for every (core, console) pair this application can run.
 *
 * `CORES_BY_SYSTEM` decides which pairs exist, their order and the default;
 * this table adds the parts the main process has no business knowing -- the
 * module to import, and the sample rate and frame length the audio and video
 * pipelines are built around.
 */
const CORES: readonly CoreChoice[] = [
    {
        id: 'fc',
        name: 'Classic Game Box Core',
        system: 'nes',
        module: 'fc_libretro.mjs',
        sampleRate: 44100,
        frameSeconds: 1 / 60.0988,
    },
    {
        id: 'mesen',
        name: 'Mesen',
        system: 'nes',
        module: 'mesen_libretro.mjs',
        sampleRate: 48000,
        frameSeconds: 1 / 60.0998,
    },
    {
        id: 'mgba',
        name: 'mGBA',
        system: 'gba',
        module: 'mgba_libretro.mjs',
        sampleRate: 65536,
        frameSeconds: 1 / 59.7275,
    },
    {
        id: 'mgba',
        name: 'mGBA',
        system: 'gb',
        module: 'mgba_libretro.mjs',
        sampleRate: 131072,
        frameSeconds: 1 / 59.7275,
    },
];

/** The extension of a path, lower case and without the dot, or ''. */
export function extensionOf(path: string): string
{
    const dot = path.lastIndexOf('.');
    if (dot < 0) {
        return '';
    }
    return path.slice(dot + 1).toLowerCase();
}

/**
 * The console a game is for.
 *
 * Anything unrecognised is treated as a NES game, which keeps the old
 * behaviour for a file with no extension and gives a clear failure -- a NES
 * core that refuses the cartridge -- for a file that is not a ROM at all.
 */
export function systemForExtension(path: string): SystemId
{
    const extension = extensionOf(path);
    if (extension === 'gba') {
        return 'gba';
    }
    if (extension === 'gb' || extension === 'gbc') {
        return 'gb';
    }
    return 'nes';
}

/**
 * The consoles the application knows, in the order the settings screen lists
 * them. A console with more than one core is a console the player can choose
 * an emulator for.
 */
/**
 * The consoles the application knows, in the order the settings screen lists
 * them. A console with more than one core is a console the player can choose
 * an emulator for.
 *
 * `name` is for the settings screen, where there is room to spell a console
 * out; `short` is for the badges on the library cards, where the whole word
 * would be wider than the card.
 */
export const SYSTEMS: readonly { id: SystemId; name: string; short: string }[] = Object.freeze([
    { id: 'nes', name: 'NES / FC', short: 'NES' },
    { id: 'gba', name: 'Game Boy Advance', short: 'GBA' },
    { id: 'gb', name: 'Game Boy / Color', short: 'GB' },
]);

/**
 * The cores that can run a console, in the order the screen lists them.
 *
 * The order -- and therefore the default, which is the first -- comes from
 * `CORES_BY_SYSTEM`, the table the main process validates a saved selection
 * against. This function only attaches the parts a core needs to be run (its
 * module and rates); a pair the shared table lists but this file has no entry
 * for is dropped rather than offered and then failed.
 */
export function coresFor(system: SystemId): CoreChoice[]
{
    return (CORES_BY_SYSTEM[system] as readonly CoreId[])
        .map((id) => CORES.find((core) => core.system === system && core.id === id))
        .filter((core): core is CoreChoice => core !== undefined);
}

/**
 * The core to run a game on.
 *
 * The player's pick for the game's console wins; without one, or with an ID
 * that does not target that console, the default is used. Never null: every
 * supported console has at least one core, and an unknown extension is a NES
 * game.
 */
export function chooseCore(path: string, selection?: CoreSelection): CoreChoice
{
    const system = systemForExtension(path);
    const available = coresFor(system);
    const wanted = selection?.[system];
    return available.find((core) => core.id === wanted) ?? available[0];
}

/**
 * Whether two choices need different machines.
 *
 * Two cores differ when they are different modules, or when the same module
 * renders a different console (mGBA on a GBA versus a Game Boy -- different
 * sample rate, and the previous machine cannot become the other).
 */
export function sameCore(a: CoreChoice | null, b: CoreChoice): boolean
{
    return a !== null && a.id === b.id && a.system === b.system;
}
