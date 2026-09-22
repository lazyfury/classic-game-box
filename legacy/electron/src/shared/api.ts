// ---------------------------------------------------------------------------
// The contract between the main process and the renderer.
//
// Both sides import this file, which is the only reason it exists: a channel
// name spelled once cannot be misspelled on one side, and changing the shape
// of a message breaks the build instead of failing at runtime in the middle of
// a game.
//
// Keep it small. Everything the renderer can ask the main process for is a
// hole in the wall between the emulator and the operating system, and every
// hole needs a reason.
// ---------------------------------------------------------------------------

export const IpcChannel = {
    /** Read the ROM named on the command line. Returns BootRom or null. */
    GetBootRom: 'fc:get-boot-rom',

    /** The whole library: every game and every screenshot. */
    Library: 'fc:library',

    /** Read one ROM by path, for when the player picks one from the list. */
    ReadRom: 'fc:read-rom',

    /** Remember that a game was played, so the list can put it first next time. */
    NotePlayed: 'fc:note-played',

    /** Add counted emulated seconds to a game's total play time. */
    NotePlaytime: 'fc:note-playtime',

    /** Write save state bytes into a slot. Returns false if it could not. */
    SaveState: 'fc:save-state',

    /** Read a slot back, or null if it is empty. */
    LoadState: 'fc:load-state',

    /** Which slots have something in them. */
    ListSaves: 'fc:list-saves',

    /** Show the ROM folder in the Finder. */
    OpenFolder: 'fc:open-folder',

    /** Copy ROMs into the library, chosen through a native open panel. */
    AddGames: 'fc:add-games',

    /** Pin a game to the top of the list, or unpin it. */
    TogglePinned: 'fc:toggle-pinned',

    /** Replace a game's tags with exactly these words. */
    SetGameTags: 'fc:set-game-tags',

    /** Delete a game from the library, after a native confirmation. */
    RemoveGame: 'fc:remove-game',

    /** Point the library at another folder, chosen through a native panel. */
    ChooseLibraryDirectory: 'fc:choose-library-directory',

    /** Which cartridge is in the slot, so its save states can be filed. */
    SetCartridge: 'fc:set-cartridge',

    /** Write a PNG taken from the picture into the library. */
    SaveScreenshot: 'fc:save-screenshot',

    /** Make one screenshot the game's cover. */
    SetScreenshotCover: 'fc:set-screenshot-cover',

    /** Delete a screenshot: its file, and its row. */
    RemoveScreenshot: 'fc:remove-screenshot',

    /** Show one screenshot in the file browser, selected. */
    RevealScreenshot: 'fc:reveal-screenshot',

    /** Rewrite the input settings: keyboard mode, bindings, pad assignment. */
    WriteInputSettings: 'fc:write-input-settings',

    /** Remember which emulator core to use for each console. */
    WriteCoreSelection: 'fc:write-core-selection',

    /** Remember how the library list is ordered, and which way round. */
    WriteLibraryView: 'fc:write-library-view',

    /** The cheats saved for one cartridge. */
    ReadCheats: 'fc:read-cheats',

    /** Replace the cheats saved for one cartridge. */
    WriteCheats: 'fc:write-cheats',

    /** The window preferences: scanline overlay, middle column width. */
    ReadPreferences: 'fc:read-preferences',

    /** Change one window preference, and remember it. */
    WritePreference: 'fc:write-preference',

    /** The native gamepad helper's current reading, asked for on start up. */
    GetGamepadState: 'fc:get-gamepad-state',

    /** Pushed by the main process whenever the pad changes. */
    GamepadState: 'fc:gamepad-state',
} as const;

/**
 * The orders the library list can be put in.
 *
 * Here rather than in the renderer because both sides have to know them: the
 * renderer draws them and the main process validates the remembered choice
 * against them, and a key that only one side knows is a setting that silently
 * falls back to the default.
 */
export type SortKey = 'recent' | 'added' | 'name' | 'playtime' | 'size';

/** Which end of an order comes first. */
export type SortDirection = 'asc' | 'desc';

/** The orders, in the order the control lists them. */
export const SORT_KEYS: readonly SortKey[] = Object.freeze([
    'recent', 'added', 'name', 'playtime', 'size',
]);

/**
 * The direction each order starts in, and the direction it means.
 *
 * Every one of these is the end a person means by the name alone: “most
 * recent” first, not oldest first; “biggest” first, not smallest; names A to
 * Z. The arrow on a chip is for the times that is the wrong end -- a ROM
 * folder sorted so the newest addition is at the *bottom* is the awkward
 * case, and it is the reason the arrow exists at all.
 */
export const DEFAULT_DIRECTION: Readonly<Record<SortKey, SortDirection>> = Object.freeze({
    recent: 'desc',
    added: 'desc',
    name: 'asc',
    playtime: 'desc',
    size: 'desc',
});

/**
 * Whether an order has a second end that means anything.
 *
 * “Recently played” does not. The list is *about* what was touched last, so
 * oldest-first is not the other view of it -- it is the same view read
 * backwards, and the games somebody actually plays are exactly the ones that
 * would be pushed to the bottom. So the control offers no arrow for it, and
 * whatever a remembered value says about it is ignored rather than obeyed.
 */
export function isReversible(sort: SortKey): boolean {
    return sort !== 'recent';
}

/**
 * The direction an order is actually applied in.
 *
 * A chosen direction for an order that has no other end is dropped here, at
 * the one place every caller goes through -- the list, the saved file and the
 * control all agree because there is one rule rather than three.
 */
export function directionOf(sort: SortKey, chosen: SortDirection): SortDirection {
    return isReversible(sort) ? chosen : DEFAULT_DIRECTION[sort];
}

/**
 * How the library list is ordered, as one remembered value.
 *
 * One object rather than a preference per field, and written whole, for the
 * same reason the input settings are: the order and the five directions are
 * one setting, and a file holding half of the old value and half of the new
 * one would be a list that comes back in an order nobody chose.
 *
 * A direction per order, with only the selected one in effect: turning “尺寸”
 * round should not also turn “名称” round. The others are remembered, not
 * applied -- the sort control is single-select.
 */
export interface LibraryViewSettings {
    sort: SortKey;
    directions: Record<SortKey, SortDirection>;
}

/** What a library that has never been sorted looks like: the default order,
 *  and every order at the end its name means. */
export const DEFAULT_LIBRARY_VIEW: LibraryViewSettings = Object.freeze({
    sort: 'recent',
    directions: { ...DEFAULT_DIRECTION },
});

/** One game in the library. */
export interface GameEntry {
    path: string;
    /** The file name without its extension, which is the closest thing to a
     *  title that can be read without running the game. */
    name: string;
    size: number;
    /** Pinned games sort to the top of every list and have a section of their
     *  own. This is the first thing the library database can express that a
     *  directory listing cannot. */
    pinned: boolean;
    /** How many times the cartridge has been run. */
    playCount: number;
    /**
     * Total time played, in emulated seconds, across every run.
     *
     * Emulated rather than wall-clock: the renderer counts the frames it ran
     * (see useEmulator), so a paused game and a minimised window add nothing.
     * Whole seconds, accumulated by the main process -- a window that closes
     * loses at most the few seconds since the last flush, and can never lose
     * what was already stored, because every write is an addition.
     */
    playSeconds: number;
    /**
     * The words the player gave it, in alphabetical order, without
     * duplicates and without regard to case. Empty is the common case.
     *
     * A word rather than a colour: the swatch's colour is derived from the
     * name (see src/renderer/libraryView.ts), so two games labelled "RPG"
     * share both the word and the colour without either being stored twice.
     */
    tags: string[];
    /** When it entered the library, and when it was last run. 0 means never. */
    addedAt: number;
    lastPlayedAt: number;
    /**
     * The screenshot used as this game's cover, relative to the library root,
     * or null if it has none. Null is not an error state: a game with no
     * cover is drawn as a coloured card with its name on it.
     *
     * Relative rather than absolute, and a path rather than a URL, because
     * this is what the database stores -- see src/main/library.ts. Turn it
     * into something an `<img>` can fetch with `libraryAssetUrl`.
     */
    cover: string | null;
    /** How many screenshots have been taken of it. */
    screenshots: number;
}

/** One screenshot, as the screenshots section sees it. */
export interface Screenshot {
    id: number;
    /** The game it was taken from. */
    gamePath: string;
    game: string;
    /** Relative to the library root. */
    file: string;
    createdAt: number;
    /** Whether this is the game's cover. At most one per game. */
    isCover: boolean;
}

/**
 * The library and its screenshots, in one answer.
 *
 * The two move together -- taking a screenshot changes the cover and the
 * screenshot count on a game -- so the verbs that change either one return
 * both. One round trip, and no window in which the screen is showing a game
 * with a cover it no longer has.
 */
export interface LibraryState {
    library: Library;
    screenshots: Screenshot[];
}

export interface Library {
    /** Where these came from, so the screen can say so. */
    directory: string;
    /** The SQLite file inside it, which models these rows. */
    database: string;
    games: GameEntry[];
}

/**
 * The console's eight switches, named the way the C enum names them.
 *
 * Spelled out here rather than in the renderer because the main process has
 * to understand them too: the native helper reports these names on stdout, and
 * a name that only one side knows is a mapping that silently drops a button.
 */
export type GamepadButtonName =
    | 'A' | 'B' | 'SELECT' | 'START'
    | 'UP' | 'DOWN' | 'LEFT' | 'RIGHT';

/** The eight, in the order the C enum lists them. */
export const GAMEPAD_SWITCHES: readonly GamepadButtonName[] = Object.freeze([
    'A', 'B', 'SELECT', 'START', 'UP', 'DOWN', 'LEFT', 'RIGHT',
]);

/**
 * The buttons a pad reports that the console has no switch for: the shoulders,
 * the triggers, the stick clicks, the two extra face buttons, and the system
 * keys.
 *
 * They exist for one reason. A *command* -- pause, screenshot, save, load --
 * needs a button that does not also press something in the game, and every one
 * of the console's eight is the game's. Binding screenshot to A would take a
 * picture every time the player jumped.
 *
 * Named after where they are rather than after what is printed on them: the
 * same physical shoulder is L on a DualSense, LB on an Xbox pad and L1 on a
 * Super NES pad, and the helper's job is to report the button, not the legend.
 */
export type ExtraPadButtonName =
    | 'FACE_X' | 'FACE_Y'
    | 'L1' | 'R1' | 'L2' | 'R2' | 'L3' | 'R3'
    | 'GUIDE';

/** The nine, in the order the settings screen offers them. */
export const EXTRA_PAD_BUTTONS: readonly ExtraPadButtonName[] = Object.freeze([
    'L1', 'R1', 'L2', 'R2', 'L3', 'R3',
    'FACE_X', 'FACE_Y', 'GUIDE',
]);

/** Everything a pad reports: the game's eight, and the nine that are not. */
export type PadButtonName = GamepadButtonName | ExtraPadButtonName;

export const PAD_BUTTONS: readonly PadButtonName[] = Object.freeze([
    ...GAMEPAD_SWITCHES,
    ...EXTRA_PAD_BUTTONS,
]);

/**
 * One physical pad, as the helper or the browser sees it.
 *
 * `index` is the slot it was found in, which is what the settings screen
 * lists and what a player assignment names. It is stable for as long as the
 * pad stays connected, and it is the only handle there is: two identical
 * controllers report the same `id`, so the name cannot identify one.
 */
export interface PadReading {
    index: number;
    /** The framework's or browser's name for the pad. */
    id: string;
    /** Which of them are down. Every name is present; a pad that has no
     *  trigger reports that trigger as up rather than leaving it out, so the
     *  reading is the same shape whatever is plugged in. */
    buttons: Record<PadButtonName, boolean>;
}

/**
 * Every pad that is connected right now.
 *
 * A list rather than one pad, because the console has two ports and two
 * players may each want a controller. One of these crosses the IPC boundary
 * every time the list changes, and not once per frame: a reading that is the
 * same as the last one is filtered out by the main process before it is sent.
 */
export interface GamepadReading {
    pads: PadReading[];
}

/** Nothing plugged in, which is where every session starts. */
export const NO_GAMEPAD_READING: GamepadReading = { pads: [] };

/**
 * One key on the keyboard, and where it goes.
 *
 * `code` is a `KeyboardEvent.code` -- the physical key, not the character it
 * produces -- so the same binding works on an AZERTY keyboard. `port` is the
 * console's controller port: 0 is player 1, 1 is player 2.
 */
export interface KeyBinding {
    code: string;
    port: number;
    button: GamepadButtonName;
}

/**
 * How the keyboard and the pads are wired to the console's two ports.
 *
 * The keyboard can be one player or two. As one, every key drives the same
 * port -- so the arrows and WASD are the same person, which is what a single
 * player expects. As two, the keys keep their own ports and the usual split
 * puts WASD on player 1 and the arrows on player 2.
 */
export interface InputSettings {
    /** Whether the keyboard is one player or two. */
    keyboard: '1p' | '2p';
    /** In '1p' mode, the port the keyboard drives. Ignored in '2p'. */
    keyboardPlayer: number;
    /** The bindings, or null for the built-in defaults. */
    bindings: KeyBinding[] | null;
    /**
     * Which port each connected pad drives, by pad index. `-1` means the pad
     * is ignored. A missing entry falls back to the index: pad 0 to player 1,
     * pad 1 to player 2, everything after that unused.
     */
    padPorts: number[];
    /** What fires each command, or null for the built-in defaults. */
    commands: CommandBinding[] | null;
}

/** Everything at its default, which is also what a fresh config file means. */
export const DEFAULT_INPUT_SETTINGS: InputSettings = {
    keyboard: '1p',
    keyboardPlayer: 0,
    bindings: null,
    padPorts: [],
    commands: null,
};

/**
 * Things the player asks the application to do, rather than pressing a switch
 * on the console.
 *
 * Here rather than in the renderer because the saved file holds them: a
 * binding is a line in config.json, and a command name only the renderer knows
 * is a line that silently does nothing after an upgrade.
 */
export type CommandName =
    | 'pause'
    | 'reset'
    | 'screenshot'
    | 'screenshot-cover'
    | 'save1' | 'save2' | 'save3'
    | 'load1' | 'load2' | 'load3'
    | 'quicksave' | 'quickload';

/**
 * Commands whose meaning is "while the button is down" rather than "when it
 * goes down".
 *
 * Rewinding is the only one. It cannot be a toggle: holding has to walk
 * backwards through the snapshots at a steady rate, and letting go has to stop
 * exactly where it is -- which needs to know about the release, and none of the
 * one-shot commands above ever do.
 */
export type HeldCommandName = 'rewind';

/**
 * Which of the two a command is.
 *
 * A property of the command, not of the binding: "screenshot" is a thing that
 * happens and "rewind" is a thing you hold, whoever's finger is on it.
 */
export function isHeldCommand(command: CommandName | HeldCommandName): command is HeldCommandName {
    return command === 'rewind';
}

/** A key, and whether Shift is part of it. */
export interface CommandKey {
    /** A DOM `KeyboardEvent.code`, e.g. `KeyP` or `F12`. */
    code: string;
    /** Shift is part of the binding, not a modifier on the way to it: F1 saves
     *  and Shift+F1 loads, which is how twelve commands fit on six keys. */
    shift: boolean;
}

/**
 * Buttons that have to be held *together* to mean something.
 *
 * One button is a chord of one, which is why there is no separate case for it:
 * a list of chords covers "R1", "L1+R1" and "hold L2 and press R2" with the
 * same shape. The order does not matter -- a chord is a set that is down at
 * once, not a sequence -- and it is stored sorted so that two equal chords
 * compare equal.
 */
export type PadCombo = ExtraPadButtonName[];

/**
 * One command, and what fires it.
 *
 * Two devices because a player may have either: the keyboard is always there,
 * a pad may not be. A binding with no key is a command that is only on the pad
 * (or on neither, which is how a player un-binds something).
 */
export interface CommandBinding {
    command: CommandName | HeldCommandName;
    /** The key, or null for "not on the keyboard". */
    key: CommandKey | null;
    /**
     * The chords that fire it, on any connected pad.
     *
     * Only the *extra* buttons: the console's eight belong to the game, and a
     * command bound to one of them would fire every time the player pressed
     * it. The pad indices are deliberately not part of this -- a command is
     * something the application does, not something player 1 does.
     *
     * A list, though the settings screen edits only the first: a hand-edited
     * file may want two ways to reach the same command, and dropping the
     * second would be a silent surprise.
     */
    pads: PadCombo[];
}

/** Whether this is a pad button a command may be bound to. */
function isExtraButton(name: unknown): name is ExtraPadButtonName {
    return typeof name === 'string' && (EXTRA_PAD_BUTTONS as readonly string[]).includes(name);
}

/** A chord, sorted and deduplicated, so that two equal chords are equal. */
export function normaliseCombo(raw: unknown): PadCombo {
    if (!Array.isArray(raw)) {
        return [];
    }
    return [...new Set(raw.filter(isExtraButton))].sort();
}

/** `{ code, shift }` in one value, so a map can be keyed by it. */
export function commandKeyId(key: CommandKey): string {
    return `${key.shift ? 'Shift+' : ''}${key.code}`;
}

/** The commands the settings screen lists, in the order it lists them. */
export const COMMAND_ORDER: readonly (CommandName | HeldCommandName)[] = Object.freeze([
    'pause',
    'screenshot',
    'screenshot-cover',
    'quicksave',
    'quickload',
    'reset',
    'save1', 'save2', 'save3',
    'load1', 'load2', 'load3',
    'rewind',
]);

/** Whether this is a command the application knows. */
function isKnownCommand(name: unknown): name is CommandName | HeldCommandName {
    return typeof name === 'string' && (COMMAND_ORDER as readonly string[]).includes(name);
}

/** A key that is a key: a code and a flag, or null. */
function normaliseKey(raw: unknown): CommandKey | null {
    if (raw === null || typeof raw !== 'object') {
        return null;
    }
    const key = raw as Partial<CommandKey>;
    if (typeof key.code !== 'string' || key.code === '' || key.code.length > 32) {
        return null;
    }
    return { code: key.code, shift: key.shift === true };
}

/**
 * One binding out of the saved file, or null if there is nothing usable in it.
 *
 * The pad list is deduplicated and filtered; a binding that ends up with
 * neither a key nor a button is kept, because "this command is bound to
 * nothing" is a thing a player can mean -- it is how a command is turned off.
 */
export function normaliseBinding(raw: unknown): CommandBinding | null {
    if (raw === null || typeof raw !== 'object') {
        return null;
    }
    const binding = raw as Partial<CommandBinding>;
    if (!isKnownCommand(binding.command)) {
        return null;
    }
    // Two shapes are accepted, on purpose. The screen writes chords -- a list
    // of lists -- and a person editing config.json by hand is much more likely
    // to write a button name than a list of one name. Reading the second as a
    // chord of one is the same meaning, spelled shorter.
    const pads = Array.isArray(binding.pads)
        ? binding.pads
            .map((entry) => (typeof entry === 'string' ? normaliseCombo([entry]) : normaliseCombo(entry)))
            .filter((combo) => combo.length > 0)
        : [];
    return { command: binding.command, key: normaliseKey(binding.key), pads };
}

/**
 * The bindings in force, defaults included.
 *
 * `null` means "the player has never touched this", which is what a config file
 * from before commands existed means as well. An empty *array* means the player
 * deleted every binding, and is respected: the two are not the same thing.
 */
export function commandBindings(settings: InputSettings): readonly CommandBinding[] {
    if (settings.commands === null) {
        return DEFAULT_COMMAND_BINDINGS;
    }
    return settings.commands
        .map(normaliseBinding)
        .filter((binding): binding is CommandBinding => binding !== null);
}

/**
 * What fires what, out of the box.
 *
 * Unchanged from the keys the application hard-coded before they could be
 * changed, because those are the ones hands already know: Escape or P to
 * pause, the F row for the slots, F12 for a screenshot. What is new is that
 * they are a *list* rather than a switch statement, so the settings screen can
 * show them and the file can hold them.
 *
 * No pad buttons by default. Every pad is different -- a DualSense has a
 * touchpad where an Xbox pad has nothing, and a small pad may have no
 * shoulders at all -- so the pad side starts empty and the player fills it in
 * with the buttons their pad actually has.
 */
export const DEFAULT_COMMAND_BINDINGS: readonly CommandBinding[] = Object.freeze([
    { command: 'pause', key: { code: 'Escape', shift: false }, pads: [] },
    { command: 'pause', key: { code: 'KeyP', shift: false }, pads: [] },
    { command: 'reset', key: { code: 'KeyR', shift: false }, pads: [] },
    { command: 'screenshot', key: { code: 'F12', shift: false }, pads: [] },
    { command: 'screenshot-cover', key: { code: 'F12', shift: true }, pads: [] },
    { command: 'quicksave', key: { code: 'F5', shift: false }, pads: [] },
    { command: 'quickload', key: { code: 'F6', shift: false }, pads: [] },
    { command: 'save1', key: { code: 'F1', shift: false }, pads: [] },
    { command: 'save2', key: { code: 'F2', shift: false }, pads: [] },
    { command: 'save3', key: { code: 'F3', shift: false }, pads: [] },
    { command: 'load1', key: { code: 'F1', shift: true }, pads: [] },
    { command: 'load2', key: { code: 'F2', shift: true }, pads: [] },
    { command: 'load3', key: { code: 'F3', shift: true }, pads: [] },
    { command: 'rewind', key: { code: 'Backspace', shift: false }, pads: [] },
]);

/**
 * Which console a cartridge is for.
 *
 * The extension decides this, and it is the unit a player thinks in: "NES" or
 * "Game Boy". Several emulator cores can target the same console, so the core
 * is chosen *per system* rather than per file.
 */
export type SystemId = 'nes' | 'gba' | 'gb';

/** The emulator cores this application can run. */
export type CoreId = 'fc' | 'mesen' | 'mgba';

/**
 * The core the player picked for each console, if they picked one.
 *
 * Absent means "the default", which is the first core listed for that system
 * in CORES_BY_SYSTEM. A partial record rather than a full one so that adding a
 * console later does not need every existing config file rewritten.
 */
export type CoreSelection = Partial<Record<SystemId, CoreId>>;

/**
 * The cores that target each console, the default first.
 *
 * Shared between the renderer -- which adds the WebAssembly module, sample
 * rate and frame length to each entry -- and the main process, which checks a
 * selection before writing it. It is deliberately only the IDs here: the main
 * process has no business knowing a core's module name, and the renderer has
 * no business inventing an ID the main process will then refuse.
 *
 * Mesen is first, so it is the NES default; the home-grown FC core is one
 * click away in 设置 → 模拟器核心. The order is the order the segmented control
 * draws, so it is also what the screen shows as chosen when nothing is saved.
 */
export const CORES_BY_SYSTEM: Readonly<Record<SystemId, readonly CoreId[]>> = Object.freeze({
    nes: ['mesen', 'fc'],
    gba: ['mgba'],
    gb: ['mgba'],
});

/**
 * One cheat: a byte, at an address, put back when the game overwrites it.
 *
 * The address is in the CPU's own 16 bit space -- console RAM lives at
 * $0000-$07FF, and `address` is one of those -- which is the same space a Game
 * Genie code names and the same one the debugger shows. `label` is for the
 * list on screen and nothing else; the machine never sees it.
 */
export interface Cheat {
    label: string;
    address: number;
    value: number;
    /** Rewrite it at the start of every frame, rather than only once. */
    freeze: boolean;
    /** Remembered but switched off. */
    enabled: boolean;
}

/** The host the `app://` protocol serves library files under. */
export const LIBRARY_HOST = 'library';

/**
 * A URL for a file inside the library, for an `<img>` to fetch.
 *
 * The renderer cannot read the filesystem, so the picture has to arrive the
 * way every other subresource in a page does: a URL, fetched by Chromium,
 * cached by Chromium, and decoded off the main thread. The alternative -- the
 * bytes over IPC, made into a blob URL per card -- would mean a round trip and
 * a live object per thumbnail, for no benefit at all.
 *
 * The handler behind this URL serves one directory and one file type; see
 * registerAppProtocol in src/main/index.ts. It is not a window into the disk.
 */
export function libraryAssetUrl(file: string): string {
    return `app://${LIBRARY_HOST}/${file.split('/').map(encodeURIComponent).join('/')}`;
}

/**
 * The preferences that belong to this window rather than to the library.
 *
 * They used to live in the renderer's `localStorage`, which turned out to be a
 * start up hazard: the first synchronous DOM Storage access blocks the
 * renderer for seconds while Electron's storage service starts -- measured at
 * 3.7 seconds, which was the whole of the application's start up time. They
 * are ordinary settings, so they live in the same config file the library root
 * does (see src/main/index.ts) and reach the page over ordinary asynchronous
 * IPC, which answers in about two milliseconds.
 */
export interface Preferences {
    /** Whether the scanline overlay is drawn over the picture. */
    scanlines: boolean;
    /** Middle column width in CSS pixels, or null when it has never been set. */
    panelWidth: number | null;
    /** How the keyboard and the pads are wired to the two ports. */
    input: InputSettings;
    /** Which emulator core to run each console on. Empty means the defaults. */
    cores: CoreSelection;
    /** How the library list is ordered, and how far each order was turned. */
    libraryView: LibraryViewSettings;
}

/** Which preference a write is about. */
export type PreferenceName = 'scanlines' | 'panelWidth';

/** The value of a preference. The name says which type it has to be. */
export type PreferenceValue = boolean | number;

/**
 * A ROM the main process read off disk, on its way to the renderer.
 */
export interface BootRom {
    /** Where it came from, for the window title. */
    path: string;
    /** The whole .nes file. Structured clone handles a Uint8Array directly. */
    bytes: Uint8Array;
}

/** What the renderer reports back about a run. */
export interface FrameStats {
    frameCount: number;
    totalCycles: number;
    cpuPc: number;
    sampleRate: number;
    /** Highest APU sample seen, so a silent emulator is visible as 0.000. */
    audioPeak: number;
    /** Frames per second actually achieved, measured over the last second. */
    fps: number;
}

/**
 * The only thing this app puts on `window`. Deliberately not "electron" and
 * deliberately not forty methods: the renderer is a game screen, and the
 * smaller this surface stays, the less there is to get wrong.
 */
export interface FcBridge {
    getBootRom(): Promise<BootRom | null>;

    /**
     * The whole library: every game, and every screenshot.
     *
     * One call rather than two, because the two are drawn together -- a card
     * shows a game's cover and its screenshot count, and the screenshots
     * section lists the pictures themselves -- and two calls would be two
     * chances for the screen to be showing half of one state and half of
     * another.
     */
    library(): Promise<LibraryState>;

    /**
     * Read one ROM by path.
     *
     * The path is checked against the library folder before anything is
     * opened: the renderer can name a game, not a file. It is given no ability
     * to read from the filesystem, and this is the one place that could become
     * one.
     */
    readRom(path: string): Promise<Uint8Array | null>;

    /**
     * Say that a game was played.
     *
     * The library's memory belongs in the main process because it outlives
     * the renderer, and because it is a row in a database that only the main
     * process has open.
     */
    notePlayed(path: string): Promise<void>;

    /**
     * Add to a game's total play time.
     *
     * The renderer counts the seconds because it is the only side that knows
     * how much emulation happened; the main process does the adding because it
     * owns the database. Called every few seconds while a game runs and once
     * more when the cartridge comes out.
     */
    notePlaytime(path: string, seconds: number): Promise<void>;

    /**
     * Replace a game's tags with exactly these words.
     *
     * One verb for the whole list, like the input settings: the editor has
     * every tag on screen, and adding one at a time would let the stored list
     * disagree with the drawn one for as long as the second call took.
     */
    setGameTags(path: string, tags: readonly string[]): Promise<LibraryState>;

    /**
     * Save states on disk.
     *
     * A slot is a small integer. Slot 0 is the quick save key; 1, 2 and 3 are
     * F1 to F3. The main process owns the files, because the renderer has no
     * filesystem access and should not be given any to write a save file.
     */
    saveState(slot: number, bytes: Uint8Array): Promise<boolean>;
    loadState(slot: number): Promise<Uint8Array | null>;
    listSaves(): Promise<number[]>;

    /**
     * Open the library folder -- or a folder inside it -- in the Finder.
     *
     * The empty state can say where the games go, but a path is only useful
     * if the player can get to it, and the screenshots live in a subfolder
     * that is otherwise invisible. This is the one convenience the renderer is
     * given over the filesystem, and it names a folder rather than a file: the
     * main process decides what "inside the library" means, and creates the
     * folder if it is not there yet.
     */
    openFolder(subdirectory?: string): Promise<boolean>;

    /**
     * Copy ROMs into the library.
     *
     * With no argument, the files are chosen through a native open panel,
     * which the renderer cannot open and cannot influence: it says "let the
     * player pick some games", and the main process decides what that means.
     *
     * With paths -- from a drag and drop -- those are what gets copied. The
     * renderer is allowed to name them because a drop is a person pointing at
     * a file; the main process still filters them down to ROMs before
     * anything is opened, so the worst a renderer bug can do is copy a .nes
     * it should not have.
     *
     * Returns null if the player closed the panel, or if what arrived was not
     * a ROM.
     */
    addGames(paths?: readonly string[]): Promise<LibraryState | null>;

    /**
     * The path a dropped `File` came from.
     *
     * This is the one question about a file the renderer can ask, and it is
     * the only way to answer it: Electron removed the `path` property from
     * `File` objects in version 32, because a page that can turn a file into
     * a filesystem path is a page that can probe the disk. The replacement is
     * `webUtils.getPathForFile`, which the preload wraps -- and which only
     * ever answers for a file the operating system actually dragged in.
     */
    filePath(file: File): string;

    /** Pin a game to the top of the list. Returns the library, re-read. */
    togglePinned(path: string, pinned: boolean): Promise<LibraryState>;

    /**
     * Delete a game, file and row -- and its screenshots with it.
     *
     * Confirmed with a native alert first, in the main process, because this
     * is the one button in the interface that destroys something. Returns
     * null if it was declined.
     */
    removeGame(path: string): Promise<LibraryState | null>;

    /**
     * Point the library at another folder.
     *
     * The choice is remembered across runs. The database lives in whichever
     * folder is chosen, which is why switching libraries switches everything:
     * a library is a folder.
     */
    chooseLibraryDirectory(): Promise<LibraryState | null>;

    /**
     * Write a PNG taken from the picture, and file it under a game.
     *
     * The bytes come from the renderer because that is where the canvas is;
     * the file and the row are the main process's, because it owns the disk
     * and the database. Rejects bytes that are not a PNG, and a game that is
     * not in the library.
     *
     * `asCover` is the 更新封面 button: the new picture replaces whatever the
     * game's card was showing. Without it, only the first screenshot of a game
     * becomes the cover.
     */
    saveScreenshot(
        gamePath: string,
        bytes: Uint8Array,
        asCover?: boolean,
    ): Promise<LibraryState | null>;

    /**
     * Make a screenshot the cover of the game it belongs to.
     *
     * A cover is not a separate picture stored twice: it is a flag on one of
     * the game's screenshots, so setting a cover and taking a screenshot are
     * the same kind of thing and deleting the cover is not a special case.
     */
    setScreenshotCover(id: number): Promise<LibraryState | null>;

    /** Delete a screenshot: the PNG from the library folder, and its row. */
    removeScreenshot(id: number): Promise<LibraryState | null>;

    /**
     * Show one screenshot in the file browser.
     *
     * The renderer names the screenshot's *row* and never a path, and that is
     * the point: the main process looks the file up, checks that it is inside
     * the library, and only then hands it to the operating system. A page that
     * can name a path here is a page that can make the file browser select
     * `/etc/passwd`.
     *
     * Returns false when the row is not there or led outside the folder.
     */
    revealScreenshot(id: number): Promise<boolean>;

    /**
     * The window preferences: scanlines and the middle column width.
     *
     * Read once on start up. Unlike the library these are small and local, so
     * there is nothing to stream and nothing to keep in sync -- the page asks
     * for them, draws with them, and writes them back when they change.
     */
    preferences(): Promise<Preferences>;

    /**
     * Remember one preference.
     *
     * Fire and forget from the page's point of view: the screen has already
     * changed by the time this is called, and the disk is the main process's
     * business. A failure to write is logged there, not thrown here.
     */
    setPreference(name: PreferenceName, value: PreferenceValue): Promise<void>;

    /**
     * Replace the whole input configuration.
     *
     * One verb for the whole object rather than a field at a time, because
     * the parts are read together: a rebinding is only meaningful with the
     * keyboard mode it was made under, and two writes would leave a window
     * where the file holds half of one configuration and half of another.
     */
    saveInputSettings(settings: InputSettings): Promise<void>;

    /**
     * Replace the whole core selection.
     *
     * One verb for the whole mapping, like the input settings: the choice is
     * small, always written as a unit, and a field-at-a-time API would let the
     * file disagree with the screen for as long as the second write took.
     */
    saveCoreSelection(selection: CoreSelection): Promise<void>;

    /**
     * Remember how the library list is ordered.
     *
     * Written whenever the order or one of its arrows is pressed, so the
     * list comes back the way it was left. Same one-verb-for-the-whole-value
     * shape as the two above: the order and the directions are one setting.
     */
    saveLibraryView(view: LibraryViewSettings): Promise<void>;

    /** The cheats saved for one cartridge, or an empty list. */
    readCheats(romPath: string): Promise<Cheat[]>;

    /** Replace the cheats saved for one cartridge. */
    writeCheats(romPath: string, cheats: Cheat[]): Promise<void>;

    /**
     * Say which cartridge is in the slot.
     *
     * Save states are filed per cartridge, and the main process owns those
     * files. It used to work this out from the ROM named on the command line,
     * which meant saving silently failed for any game started from the
     * library. This is how it is told instead.
     */
    setCartridge(path: string | null): Promise<void>;

    /**
     * When this run's start up began, in epoch milliseconds.
     *
     * The renderer is a separate process from the dev script and the main
     * process, so its `Date.now()` origin is not theirs. This is the shared
     * origin, handed down the chain, and it is what lets a boot trace print
     * the renderer's first paint on the same clock as the TypeScript compile
     * that happened several seconds before it. Zero when nobody set one -- the
     * packaged application, or the renderer opened on its own -- and the
     * renderer then falls back to its own origin, which is still correct, just
     * not comparable. See src/shared/boot.ts.
     */
    readonly bootT0: number;

    /**
     * True when the app was started with --selftest.
     *
     * In that mode the renderer must not start its frame loop. The test is a
     * parity check: after N frames the CPU must have run exactly the number of
     * cycles the native build ran, from exactly the same starting state. A
     * loop that had already run a few frames would leave the machine past that
     * state, and resetting is not enough to undo it -- Machine::reset() clears
     * the CPU, PPU and APU, but a mapper's bank registers are the cartridge's
     * and survive it.
     */
    readonly selftestOnly: boolean;

    /**
     * True when the machine must be built at start up rather than on demand.
     *
     * Normally the WebAssembly module and the emulator are built lazily, the
     * first time a game is loaded, so that the library screen appears without
     * waiting for them. That is wrong for a check that wants the machine at a
     * known moment -- `--selftest`, `--keytest` and `--audiotest` -- and for
     * `--layout`, which waits on the test hook that building the machine is
     * what publishes. Those runs pass `--fc-eager`.
     */
    readonly eager: boolean;

    /**
     * The saved core selection, known before the first render.
     *
     * The asynchronous `preferences()` call cannot be: a `--rom` run builds
     * the machine eagerly, and it would build it on the default core and then
     * swap cores a moment later. The main process already read config.json
     * before it created the window, so it hands the same selection down the
     * argument chain the other boot flags use. `preferences()` carries it too,
     * and that is what a change made while the app is running goes through.
     */
    readonly coreSelection: CoreSelection;

    /**
     * The saved library order, known before the first render.
     *
     * Handed down the same way the core selection is, and for a related
     * reason: the asynchronous `preferences()` call arrives a moment after the
     * first paint, and a list that draws itself in the default order and then
     * rearranges is a list that was never in the order the player chose. The
     * alternative -- writing whatever the default was before the saved value
     * came back -- would also overwrite the setting on a start up that was
     * killed in between.
     */
    readonly libraryView: LibraryViewSettings;

    /**
     * Ask whether a gamepad source should be started, and which kind.
     *
     * `gamepadEnabled` says a source is running. `gamepadNative` says it is
     * the native helper in `native/gamepad` rather than the browser's Gamepad
     * API. The renderer has to know which, because the two are fed in opposite
     * directions: the browser source is polled once per animation frame, and
     * the native one pushes readings over IPC as they change.
     */
    readonly gamepadEnabled: boolean;

    /**
     * True when the gamepad source is the native helper.
     *
     * The native helper exists because of what is documented above on
     * `gamepadEnabled`: it is a separate process using Apple's GameController
     * framework, so Chromium never touches HID and the application can still
     * quit. It is macOS-only, and on macOS it is now the default.
     */
    readonly gamepadNative: boolean;

    /** The native helper's latest reading. */
    getGamepadState(): Promise<GamepadReading>;

    /**
     * Listen for readings from the native helper.
     *
     * Push, not pull. The helper already polls at 60Hz and already filters out
     * readings that have not changed, so the renderer only ever hears about a
     * button going down or coming up. `offGamepadState` stops the listening;
     * a renderer that forgets to call it would keep a listener per mount.
     */
    onGamepadState(callback: (reading: GamepadReading) => void): void;
    offGamepadState(): void;
}

/**
 * The file extensions the emulator can run, and the one test for them.
 *
 * Shared rather than written twice: the main process filters the library by
 * these, and the renderer picks a libretro core by them. A file the library
 * lists but the renderer has no core for is a game that appears and then
 * fails, which is the drift keeping one list prevents.
 */
export const ROM_EXTENSIONS = ['nes', 'gba', 'gb', 'gbc'] as const;