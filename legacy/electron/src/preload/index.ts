// ---------------------------------------------------------------------------
// The preload script.
//
// This is the only thing that runs with access to both Node (through Electron)
// and the page. It runs in an isolated world, so what it defines on `window`
// is a deliberate, frozen copy -- not a live object the page can walk back
// into the main process through.
//
// The whole Node attack surface of this application is the object below:
// named verbs, each of which the main process checks again before it touches
// anything. The page cannot open a path, read a directory or spawn a process;
// it can only ask for a game, a save slot, or the library folder -- and the
// two verbs that change the library go through a native panel or a native
// confirmation that the page cannot draw or dismiss itself.
// ---------------------------------------------------------------------------

import { contextBridge, ipcRenderer, webUtils, type IpcRendererEvent } from 'electron';

import {
    DEFAULT_LIBRARY_VIEW, IpcChannel,
    type BootRom, type Cheat, type CoreSelection, type FcBridge, type GamepadReading,
    type InputSettings, type LibraryState, type LibraryViewSettings, type Preferences,
} from '../shared/api';
import { bootLog, setBootOrigin } from '../shared/boot';

/**
 * The shared start up clock, as the main process passed it down.
 *
 * `additionalArguments` is the established way this application tells the
 * preload what kind of run it is in, so the origin rides along with --fc-eager
 * and the rest. The environment is a fallback for a main process that was
 * started before the argument existed, and a missing value is normal: the
 * packaged application has no dev script and therefore no shared origin.
 */
function bootTimestamp(): number {
    const prefixed = process.argv.find((argument) => argument.startsWith('--fc-boot-t0='));
    if (prefixed !== undefined) {
        return Number(prefixed.slice('--fc-boot-t0='.length));
    }
    return Number(process.env.FC_BOOT_T0 ?? 0);
}

setBootOrigin(bootTimestamp());

/**
 * The core selection the main process read from config.json before the window
 * existed, as it passed it down. Parsed defensively: a malformed value costs
 * the selection, not the window.
 */
function bootCoreSelection(): CoreSelection
{
    const prefixed = process.argv.find((argument) => argument.startsWith('--fc-cores='));
    if (prefixed === undefined) {
        return {};
    }
    try {
        const parsed: unknown = JSON.parse(
            decodeURIComponent(prefixed.slice('--fc-cores='.length)),
        );
        return parsed !== null && typeof parsed === 'object' ? parsed as CoreSelection : {};
    } catch {
        return {};
    }
}

/**
 * The saved library order, as the main process read it from config.json.
 *
 * Parsed defensively for the same reason the selection above is: a malformed
 * value costs the remembered order, not the window. The default is complete,
 * so the renderer never has to ask whether a field arrived -- it is what the
 * list uses when nobody has ever sorted it.
 */
function bootLibraryView(): LibraryViewSettings
{
    const prefixed = process.argv.find((argument) => argument.startsWith('--fc-library-view='));
    if (prefixed === undefined) {
        return DEFAULT_LIBRARY_VIEW;
    }
    try {
        const parsed: unknown = JSON.parse(
            decodeURIComponent(prefixed.slice('--fc-library-view='.length)),
        );
        return parsed !== null && typeof parsed === 'object'
            ? { ...DEFAULT_LIBRARY_VIEW, ...parsed as LibraryViewSettings }
            : DEFAULT_LIBRARY_VIEW;
    } catch {
        return DEFAULT_LIBRARY_VIEW;
    }
}

bootLog('preload', 'script started');

/**
 * The renderer's one gamepad listener, held here rather than in the page.
 *
 * `onGamepadState` replaces whatever was listening before, so a component that
 * mounts twice cannot end up with two listeners and two button presses per
 * press. The page never sees an ipcRenderer, only the callback it handed over.
 */
let gamepadListener: ((event: IpcRendererEvent, reading: GamepadReading) => void) | null = null;

const bridge: FcBridge = {
    getBootRom: () => ipcRenderer.invoke(IpcChannel.GetBootRom) as Promise<BootRom | null>,

    saveState: (slot, bytes) =>
        ipcRenderer.invoke(IpcChannel.SaveState, { slot, bytes }) as Promise<boolean>,

    library: () => ipcRenderer.invoke(IpcChannel.Library) as Promise<LibraryState>,

    readRom: (path) =>
        ipcRenderer.invoke(IpcChannel.ReadRom, path) as Promise<Uint8Array | null>,

    notePlayed: (path) =>
        ipcRenderer.invoke(IpcChannel.NotePlayed, path) as Promise<void>,

    notePlaytime: (path, seconds) =>
        ipcRenderer.invoke(IpcChannel.NotePlaytime, { path, seconds }) as Promise<void>,

    setGameTags: (path, tags) =>
        ipcRenderer.invoke(IpcChannel.SetGameTags, { path, tags }) as Promise<LibraryState>,

    loadState: (slot) =>
        ipcRenderer.invoke(IpcChannel.LoadState, slot) as Promise<Uint8Array | null>,

    listSaves: () => ipcRenderer.invoke(IpcChannel.ListSaves) as Promise<number[]>,

    openFolder: (subdirectory) =>
        ipcRenderer.invoke(IpcChannel.OpenFolder, subdirectory) as Promise<boolean>,

    addGames: (paths) =>
        ipcRenderer.invoke(IpcChannel.AddGames, paths) as Promise<LibraryState | null>,

    filePath: (file) => webUtils.getPathForFile(file),

    togglePinned: (path, pinned) =>
        ipcRenderer.invoke(IpcChannel.TogglePinned, { path, pinned }) as Promise<LibraryState>,

    removeGame: (path) =>
        ipcRenderer.invoke(IpcChannel.RemoveGame, path) as Promise<LibraryState | null>,

    chooseLibraryDirectory: () =>
        ipcRenderer.invoke(IpcChannel.ChooseLibraryDirectory) as Promise<LibraryState | null>,

    setCartridge: (path) =>
        ipcRenderer.invoke(IpcChannel.SetCartridge, path) as Promise<void>,

    preferences: () =>
        ipcRenderer.invoke(IpcChannel.ReadPreferences) as Promise<Preferences>,

    setPreference: (name, value) =>
        ipcRenderer.invoke(IpcChannel.WritePreference, { name, value }) as Promise<void>,

    saveInputSettings: (settings: InputSettings) =>
        ipcRenderer.invoke(IpcChannel.WriteInputSettings, settings) as Promise<void>,

    saveCoreSelection: (selection: CoreSelection) =>
        ipcRenderer.invoke(IpcChannel.WriteCoreSelection, selection) as Promise<void>,

    saveLibraryView: (view: LibraryViewSettings) =>
        ipcRenderer.invoke(IpcChannel.WriteLibraryView, view) as Promise<void>,

    readCheats: (romPath) =>
        ipcRenderer.invoke(IpcChannel.ReadCheats, romPath) as Promise<Cheat[]>,

    writeCheats: (romPath, cheats) =>
        ipcRenderer.invoke(IpcChannel.WriteCheats, { romPath, cheats }) as Promise<void>,

    saveScreenshot: (gamePath, bytes, asCover) =>
        ipcRenderer.invoke(
            IpcChannel.SaveScreenshot, { gamePath, bytes, asCover },
        ) as Promise<LibraryState | null>,

    setScreenshotCover: (id) =>
        ipcRenderer.invoke(IpcChannel.SetScreenshotCover, id) as Promise<LibraryState | null>,

    removeScreenshot: (id) =>
        ipcRenderer.invoke(IpcChannel.RemoveScreenshot, id) as Promise<LibraryState | null>,

    revealScreenshot: (id) =>
        ipcRenderer.invoke(IpcChannel.RevealScreenshot, id) as Promise<boolean>,

    // The main process appends this through webPreferences.additionalArguments
    // when it was started with --selftest. Reading it here rather than over
    // IPC keeps the renderer from having to wait for an answer before it can
    // decide whether to start running frames.
    bootT0: bootTimestamp(),
    coreSelection: bootCoreSelection(),
    libraryView: bootLibraryView(),
    selftestOnly: process.argv.includes('--fc-selftest'),
    eager: process.argv.includes('--fc-eager'),
    gamepadEnabled: process.argv.includes('--fc-gamepad'),
    gamepadNative: process.argv.includes('--fc-gamepad-native'),

    getGamepadState: () =>
        ipcRenderer.invoke(IpcChannel.GetGamepadState) as Promise<GamepadReading>,

    onGamepadState: (callback) => {
        if (gamepadListener !== null) {
            ipcRenderer.removeListener(IpcChannel.GamepadState, gamepadListener);
        }
        gamepadListener = (_event, reading) => callback(reading);
        ipcRenderer.on(IpcChannel.GamepadState, gamepadListener);
    },

    offGamepadState: () => {
        if (gamepadListener !== null) {
            ipcRenderer.removeListener(IpcChannel.GamepadState, gamepadListener);
            gamepadListener = null;
        }
    },
};

contextBridge.exposeInMainWorld('fc', bridge);
bootLog('preload', 'bridge exposed', `bootT0=${bridge.bootT0}`);
