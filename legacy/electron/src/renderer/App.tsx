// ---------------------------------------------------------------------------
// The application shell -- three columns and a title bar.
//
//   ┌──────────────────────────────────────────────────────────────┐
//   │ ●●●   Classic Game Box                                   ⧉ ⧉ ⧉   │  unified title bar
//   ├────────┬─────────────────────┬───────────────────────────────┤
//   │ 功能区  │ 中间栏              │ 游戏画面                       │
//   │ 游戏库  │ 卡片 / 截图 / 存档  │ canvas + 控制条                │
//   │ 置顶    │                     │                               │
//   │ 截图    │                     │                               │
//   │ 存档    │                     │                               │
//   │ 设置    │                     │                               │
//   │ 关于    │                     │                               │
//   ├────────┴─────────────────────┴───────────────────────────────┤
//   │ status bar: fps · cyc · PC · audio · 按键                     │
//   └──────────────────────────────────────────────────────────────┘
//
// The left rail picks what the middle column is; the right column is the
// console and is always the same console. The canvas inside it is mounted
// once and never unmounted: the emulator binds to it when the WebAssembly
// module is built, and tearing it down to show a list would power the machine
// off and on again. See PlayPanel.
//
// State that the whole shell needs lives here -- which section, the search
// text, the sort order, the library itself -- because two columns have to
// agree about it. State that only one panel needs stays in that panel.
//
// The library and its screenshots are one value, `LibraryState`, because they
// are drawn together and revised together: taking a screenshot changes a
// game's cover. Every call below either reads that whole value back or changes
// something and gets the whole new value back, so there is one copy of the
// truth and no way for the screen to be showing half of one revision and half
// of another.
// ---------------------------------------------------------------------------

import * as Tooltip from '@radix-ui/react-tooltip';
import { Plus } from 'lucide-react';
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';

import AboutPanel from './components/AboutPanel';
import CheatsPanel from './components/CheatsPanel';
import LibraryPanel from './components/LibraryPanel';
import PlayPanel from './components/PlayPanel';
import TabsView from './components/TabsView';
import SavesPanel from './components/SavesPanel';
import ScreenshotsPanel from './components/ScreenshotsPanel';
import ScreenshotPreview from './components/ScreenshotPreview';
import SettingsIndex from './components/SettingsIndex';
import SettingsPanes from './components/SettingsPanes';
import Sidebar from './components/Sidebar';
import StatusBar from './components/StatusBar';
import TitleBar from './components/TitleBar';
import { gameTitle } from './format';
import { neighbour } from './gallery';
import {
    NO_FILTER, newGames, type LibraryFilter, type LibraryViewSettings, type SortKey,
} from './libraryView';
import type { CommandName } from './input';
import { SECTION_BY_ID, type SectionId } from './sections';
import { DEFAULT_SETTINGS_GROUP, SETTINGS_GROUPS, type SettingsGroupId } from './settings';
import { useEmulator } from './useEmulator';
import { useFileDrop } from './useFileDrop';
import { useFullscreen } from './useFullscreen';
import { usePanelWidth } from './usePanelWidth';
import { usePixelScale } from './usePixelScale';
import { bootLog } from '../shared/boot';

import type { CSSProperties } from 'react';
import {
    DEFAULT_INPUT_SETTINGS, type Cheat, type CoreSelection, type InputSettings,
    type LibraryState, type Preferences,
} from '../shared/api';

/** The four save slots, in the order the keyboard numbers them. */
const SAVE_COMMANDS: readonly CommandName[] = ['quicksave', 'save1', 'save2', 'save3'];
const LOAD_COMMANDS: readonly CommandName[] = ['quickload', 'load1', 'load2', 'load3'];

/** Before the first read comes back: no games, no pictures, no folder. */
const NOTHING_YET: LibraryState = {
    library: { directory: '', database: '', games: [] },
    screenshots: [],
};

export default function App() {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const stageRef = useRef<HTMLDivElement>(null);
    const appRef = useRef<HTMLDivElement>(null);

    // The whole shell is what goes fullscreen; CSS hides the parts that are
    // about choosing a game. The play column, its two strips and the readout
    // stay. See useFullscreen.
    const { fullscreen, toggle: toggleFullscreen, available: fullscreenAvailable } =
        useFullscreen(appRef);

    const [section, setSection] = useState<SectionId>('library');
    const [state, setState] = useState<LibraryState>(NOTHING_YET);
    const [loading, setLoading] = useState(true);
    const [query, setQuery] = useState('');
    /**
     * Which end each order comes first, and which order is in effect.
     *
     * A direction per order rather than one for the list, because turning
     * “尺寸” round to smallest-first should not also turn “名称” round: they
     * are five different questions, and the answer to one is not the answer
     * to another. Only the selected order's direction is ever applied -- the
     * rest are remembered, not in effect, which is what the single-select
     * control means.
     *
     * One value, coming from the main process before the first render (see
     * `libraryView` on the bridge), so a list opens in the order it was closed
     * in and never rearranges itself a moment after the first paint. The
     * value the screen starts with is therefore the saved one, or the defaults
     * if nothing has ever been saved.
     */
    const [view, setView] = useState<LibraryViewSettings>(window.fc.libraryView);
    const sort = view.sort;
    const directions = view.directions;

    /**
     * The same value, for the handlers.
     *
     * `view` is what a render depends on; this is what a handler reads. A
     * handler that captured the state from the render it was built in would
     * drop the second change made before React committed the first -- two
     * presses of an arrow in one frame would keep only the second flip.
     */
    const viewRef = useRef(view);
    // The emulator and tag filters. Held here with the sort rather than
    // inside the panel, so that switching from 游戏库 to 置顶游戏 does not
    // quietly throw away what the player had narrowed the list to.
    const [filter, setFilter] = useState<LibraryFilter>(NO_FILTER);
    const [saves, setSaves] = useState<number[]>([]);
    // Off until the saved preference arrives, a fraction of a second later
    // and invisibly. The value lives in the main process; see the preferences
    // effect below for why it is not in localStorage any more.
    const [scanlines, setScanlines] = useState(false);
    const [input, setInput] = useState<InputSettings>(DEFAULT_INPUT_SETTINGS);
    // The main process read config.json before the window existed and handed
    // the selection down the argument chain, so this is right on the first
    // render -- which matters for a `--rom` run, whose machine is built
    // eagerly and must not start on the default core and then swap. See
    // `coreSelection` on the bridge. The asynchronous preferences call below
    // carries the same value for a change made while the app is running.
    const [cores, setCores] = useState<CoreSelection>(window.fc.coreSelection);
    const [cheats, setCheats] = useState<Cheat[]>([]);
    const [notice, setNotice] = useState<string | null>(null);

    /**
     * Which screenshot is being looked at, by id, or null.
     *
     * Here rather than inside the screenshots panel, because two columns need
     * it now: the middle one lists the pictures and marks this one, and the
     * right one -- the play column -- is where the picture is shown. That is
     * the rule this file has always had for shared state, and moving the
     * preview into the play column is what made this state shared.
     *
     * An id and not an index, so that deleting or reordering the list beneath
     * it cannot quietly turn it into a different picture; the index is looked
     * up again for every render instead.
     */
    const [previewId, setPreviewId] = useState<number | null>(null);

    /**
     * Which group of settings the play column is showing.
     *
     * Here with the preview rather than inside the index, because two columns
     * need it: the index marks the chosen group and the play column renders it.
     */
    const [settingsTab, setSettingsTab] = useState<SettingsGroupId>(DEFAULT_SETTINGS_GROUP);
    const previewIndex = state.screenshots.findIndex((shot) => shot.id === previewId);
    const previewing = previewIndex >= 0;

    /**
     * Which pane of the play column is showing.
     *
     * The preview first: a screenshot that is open is what the column is for,
     * wherever the rail happens to be. Then the settings group the index asked
     * for, and otherwise the console -- which is the case the column exists for
     * and the one every other section puts back.
     */
    const activePane = previewing
        ? 'preview'
        : (section === 'settings' ? settingsTab : 'console');

    /**
     * Whether the console is the thing on screen.
     *
     * It decides two things, and they are the same decision: the machine stops
     * while it is false (`setPictureHidden`), and commands are refused while it
     * is false. Neither a game nor a screenshot nor a setting is being *played*
     * when the picture is not there, and a save that fired from the settings
     * screen -- where somebody is holding a pad to bind it -- would be a save
     * nobody asked for.
     */
    const consoleOnScreen = activePane === 'console';

    // How wide the middle column is. The divider between it and the picture is
    // the drag; see usePanelWidth for why it is written out by hand.
    const panel = usePanelWidth();

    // A short note about the last thing the player asked for, gone again in a
    // moment. It belongs to the shell rather than to a panel, because the
    // panels come and go and the feedback should not.
    const noticeTimer = useRef(0);
    const flash = useCallback((text: string): void => {
        setNotice(text);
        window.clearTimeout(noticeTimer.current);
        noticeTimer.current = window.setTimeout(() => setNotice(null), 2600);
    }, []);

    useEffect(() => () => window.clearTimeout(noticeTimer.current), []);

    // Which cartridge is in the slot, for the screenshot callback. A ref
    // because that callback is handed to the emulator once and must not
    // rebuild the machine every time a game is loaded.
    const cartridge = useRef<string | null>(null);

    /**
     * Write a screenshot the emulator just handed up.
     *
     * The bytes come from the canvas, so this is where they stop being a
     * picture and become a file: the main process names it, writes it into the
     * library's screenshots folder and files it under the game, then answers
     * with the library as it now is.
     *
     * `asCover` is the 更新封面 button: the picture replaces whatever the
     * game's card was showing, and — because the card is drawn from the state
     * that comes back — the change is on screen by the time the toast has
     * faded in.
     */
    const saveScreenshot = useCallback(async (png: Uint8Array, asCover: boolean): Promise<void> => {
        const path = cartridge.current;
        if (path === null) {
            return;
        }
        const next = await window.fc.saveScreenshot(path, png, asCover);
        if (next === null) {
            flash(asCover ? '封面没有更新' : '截图没有保存');
            return;
        }
        setState(next);
        flash(asCover ? '已更新封面' : '已保存截图');
    }, [flash]);

    const { status, loadRom, reload, unload, command, poke, peek, setPictureHidden } =
        useEmulator(canvasRef, {
            input,
            cheats,
            cores,
            commandsAllowed: () => consoleOnScreen,
        onScreenshot: (png, asCover) => void saveScreenshot(png, asCover),
    });

    cartridge.current = status.romPath;

    // The canvas keeps its 256x240 backing store -- that is the emulator's
    // output and nothing changes it -- and this decides how large those 256
    // pixels are drawn. See usePixelScale for why it is a whole number.
    const picture = usePixelScale(stageRef, 256, 240);

    const refreshLibrary = useCallback(async (): Promise<void> => {
        setLoading(true);
        const started = Date.now();
        bootLog('renderer', 'library: reading');
        try {
            const next = await window.fc.library();
            bootLog(
                'renderer',
                'library: loaded',
                `${next.library.games.length} games, ${next.screenshots.length} shots, `
                + `${Date.now() - started}ms`,
            );
            setState(next);
        } finally {
            setLoading(false);
        }
    }, []);

    const refreshSaves = useCallback(async (): Promise<void> => {
        try {
            setSaves(await window.fc.listSaves());
        } catch {
            setSaves([]);
        }
    }, []);

    useEffect(() => {
        bootLog('renderer', 'App mounted');
        void refreshLibrary();
    }, [refreshLibrary]);

    // The saved window preferences, read once over the bridge.
    //
    // These used to be `localStorage`, read in the renderer. The first
    // synchronous DOM Storage access in Electron blocks the renderer while its
    // storage service starts -- 3.7 seconds on the machine this was measured
    // on, which was the whole of the application's start up. The main process
    // keeps them in config.json now and answers in about two milliseconds, off
    // the critical path. See ReadPreferences in src/main/index.ts.
    useEffect(() => {
        let cancelled = false;
        void window.fc.preferences()
            .then((preferences: Preferences) => {
                if (cancelled) {
                    return;
                }
                bootLog(
                    'renderer',
                    'preferences loaded',
                    `scanlines=${preferences.scanlines} `
                    + `panelWidth=${preferences.panelWidth ?? 'default'}`,
                );
                if (preferences.scanlines) {
                    setScanlines(true);
                }
                setInput(preferences.input);
                setCores(preferences.cores);
                // The saved order again, because a second window -- or a hand
                // edit of config.json -- may have changed it since start up.
                // Nothing is written back in response: this is a read.
                viewRef.current = preferences.libraryView;
                setView(preferences.libraryView);
            })
            .catch((error: unknown) => {
                bootLog('renderer', 'preferences FAILED', String(error));
            });
        return () => {
            cancelled = true;
        };
    }, []);

    /** Turn the overlay on or off, and remember it. */
    const changeScanlines = useCallback((on: boolean): void => {
        setScanlines(on);
        void window.fc.setPreference('scanlines', on);
    }, []);

    /**
     * Change the input settings, and remember them.
     *
     * The settings reach the emulator through `useEmulator`'s handlers, which
     * are read afresh at every key event and every frame -- so a rebinding
     * takes effect immediately without rebuilding the machine or the
     * listeners.
     */
    const changeInput = useCallback((next: InputSettings): void => {
        setInput(next);
        void window.fc.saveInputSettings(next);
    }, []);

    /**
     * Change which core runs a console, and remember it.
     *
     * The machine is not rebuilt here. It is an effect on `cores` that does
     * it, and it has to be: `reload` reads the selection out of the handlers
     * ref, which is only updated once this render has been committed, so a
     * call from inside this callback would read the old choice and find
     * nothing to do. The effect also catches the other way the selection
     * changes -- the saved preference arriving after start up, which is a race
     * with the machine a `--rom` run builds eagerly.
     */
    const changeCores = useCallback((next: CoreSelection): void => {
        setCores(next);
        void window.fc.saveCoreSelection(next);
    }, []);

    // Rebuild the running machine when the core choice changes.
    //
    // Runs on mount too, and `reload` answers that with "the core is
    // unchanged" and does nothing. When a selection does differ from the core
    // in use, the cartridge in the slot is loaded into a freshly built
    // machine, without counting as a play. `reload` goes through a ref because
    // the handle hands out a fresh function every render, and depending on it
    // would run this effect every render rather than every choice.
    const reloadRef = useRef(reload);
    reloadRef.current = reload;
    useEffect(() => {
        void reloadRef.current();
    }, [cores]);

    // Cheats belong to one cartridge, so they are read when the cartridge
    // changes and written back under its path. Loading one game while another
    // game's cheats are on screen would put bytes into the wrong RAM, so the
    // list is emptied the moment the slot is.
    useEffect(() => {
        const path = status.romPath;
        if (path === null) {
            setCheats([]);
            return undefined;
        }
        let cancelled = false;
        void window.fc.readCheats(path)
            .then((list: Cheat[]) => {
                if (!cancelled) {
                    setCheats(list);
                }
            })
            .catch(() => {
                if (!cancelled) {
                    setCheats([]);
                }
            });
        return () => {
            cancelled = true;
        };
    }, [status.romPath]);

    const changeCheats = useCallback((next: Cheat[]): void => {
        setCheats(next);
        if (status.romPath !== null) {
            void window.fc.writeCheats(status.romPath, next);
        }
    }, [status.romPath]);

    // The commit, as opposed to the render above: layout effects run after
    // React has written the DOM and before the browser paints it, so this
    // bracket around them is the time the first screen actually took to
    // build.
    useLayoutEffect(() => {
        bootLog('renderer', 'App first commit (DOM written)');
    }, []);

    // The slot list is per cartridge, so it is re-read whenever the cartridge
    // changes and whenever the panel that shows it is opened.
    useEffect(() => {
        if (section === 'saves') {
            void refreshSaves();
        }
    }, [section, status.romPath, refreshSaves]);

    const play = useCallback(
        async (path: string): Promise<void> => {
            if (!await loadRom(path)) {
                return;
            }
            // The main process has just been told this game was played, which
            // is a row it updates. A moment later the library is read again,
            // and a game sorted by "recent" moves to the top where the player
            // just looked for it.
            window.setTimeout(() => void refreshLibrary(), 300);
        },
        [loadRom, refreshLibrary],
    );

    /**
     * Every library change answers with the whole state, so the screen is
     * never left guessing. Null means the player closed a panel or declined an
     * alert, and the screen keeps what it had.
     *
     * Written out at each call site rather than factored into one function,
     * because a helper defined in the component body and captured by a
     * useCallback with empty dependencies is a stale closure waiting to
     * happen.
     */
    const addGames = useCallback(async (): Promise<void> => {
        const next = await window.fc.addGames();
        if (next !== null) {
            setState(next);
        }
    }, []);

    const chooseDirectory = useCallback(async (): Promise<void> => {
        const next = await window.fc.chooseLibraryDirectory();
        if (next !== null) {
            setState(next);
            setQuery('');
            // A filter names consoles and tags of the library that was just
            // replaced; keeping it could leave the new one looking empty for
            // no visible reason.
            setFilter(NO_FILTER);
        }
    }, []);

    const setTags = useCallback(async (path: string, tags: string[]): Promise<void> => {
        setState(await window.fc.setGameTags(path, tags));
    }, []);

    const togglePinned = useCallback(async (path: string, pinned: boolean): Promise<void> => {
        setState(await window.fc.togglePinned(path, pinned));
    }, []);

    const removeGame = useCallback(async (path: string): Promise<void> => {
        const next = await window.fc.removeGame(path);
        if (next !== null) {
            setState(next);
        }
    }, []);

    const setCover = useCallback(async (id: number): Promise<void> => {
        const next = await window.fc.setScreenshotCover(id);
        if (next !== null) {
            setState(next);
            flash('已设为封面');
        }
    }, [flash]);

    const removeShot = useCallback(async (id: number): Promise<void> => {
        const next = await window.fc.removeScreenshot(id);
        if (next !== null) {
            setState(next);
            flash('已删除截图');
        }
    }, [flash]);

    /**
     * The machine stops while the preview is over the picture.
     *
     * An effect on the open/closed fact rather than something the two buttons
     * remember to do, because there are four ways out of the preview -- the X,
     * Escape, deleting what is on screen, and the list changing underneath it --
     * and a game that stayed paused after one of them is a bug nobody would
     * look for in a close button.
     */
    useEffect(() => {
        setPictureHidden(!consoleOnScreen);
    }, [consoleOnScreen, setPictureHidden]);

    /**
     * Walking to another section closes the preview.
     *
     * Every other section is about something other than the picture being
     * looked at, and the play column is the one column that never changes --
     * so going to 游戏库 or 设置 and finding a screenshot where the game should
     * be is a column that stopped following the rail. Closing it is also what
     * puts the console back on screen, since the two are the same pane.
     */
    useEffect(() => {
        setPreviewId(null);
    }, [section]);

    /**
     * The other pane of the play column: a screenshot, at size.
     *
     * A sibling of PlayPanel in the workspace rather than a child of it, so it
     * takes the whole column -- the strip with the game's name, the picture and
     * the transport buttons all give way to it, and it is not a picture in a
     * frame inside a console. Which of the two is shown is CSS's business: the
     * TabsView below shows one and hides the other (see the stylesheet), and
     * neither is ever unmounted.
     */
    const preview = previewIndex < 0 ? null : (
        <ScreenshotPreview
            shots={state.screenshots}
            index={previewIndex}
            onIndex={(next) => setPreviewId(state.screenshots[next]?.id ?? null)}
            onReveal={(id) => void revealShot(id)}
            onSetCover={(id) => void setCover(id)}
            onRemove={(id) => {
                // The picture being looked at is about to be gone; the one that
                // takes its place in the list is the next one, which is what
                // the grid shows too.
                const next = state.screenshots[
                    neighbour(previewIndex, 1, state.screenshots.length)
                ];
                void removeShot(id);
                setPreviewId(next === undefined || next.id === id ? null : next.id);
            }}
            onClose={() => setPreviewId(null)}
        />
    );

    /**
     * Show one picture in the file browser.
     *
     * Nothing to update on the way back: the file browser is the operating
     * system's, and the application has nothing to draw about it. The answer is
     * only used to say that it did not work -- which happens when the row is
     * gone, or when it led outside the library (see `screenshotPath`).
     */
    const revealShot = useCallback(async (id: number): Promise<void> => {
        if (!await window.fc.revealScreenshot(id)) {
            flash('找不到这个文件');
        }
    }, [flash]);

    /**
     * Games dropped on the window.
     *
     * `null` back means the main process found nothing it would copy: a .zip,
     * a folder with no ROMs in it, or a file that had gone. Saying so is the
     * difference between "that did not work" and "that did nothing", which
     * look identical on screen.
     */
    const dropped = useFileDrop((paths) => {
        void (async (): Promise<void> => {
            const next = await window.fc.addGames(paths);
            if (next === null) {
                flash('拖进来的不是 .nes 文件');
                return;
            }
            setState(next);
            flash(`已添加 ${paths.length} 个文件`);
        })();
    });

    /** The library folder, or a folder inside it -- `screenshots`, say. */
    const openFolder = useCallback((subdirectory?: string): void => {
        void window.fc.openFolder(subdirectory);
    }, []);

    const eject = useCallback((): void => {
        unload();
        void refreshLibrary();
        // Said out loud, because an eject is mostly the *absence* of things:
        // the picture goes back to the placeholder and the buttons grey out,
        // and neither of those is a confirmation that the button worked.
        flash('卡带已弹出');
    }, [unload, refreshLibrary, flash]);

    /** A slot button. The command goes through the keyboard's own handler, and
     *  the disk is re-read a moment later -- the write is asynchronous and
     *  there is no completion event to wait for. */
    const saveTo = useCallback((slot: number): void => {
        command(SAVE_COMMANDS[slot] ?? 'quicksave');
        window.setTimeout(() => void refreshSaves(), 500);
    }, [command, refreshSaves]);

    const loadFrom = useCallback((slot: number): void => {
        command(LOAD_COMMANDS[slot] ?? 'quickload');
    }, [command]);

    const library = state.library;
    const games = library.games;
    const loaded = status.romPath !== null;
    const title = loaded ? gameTitle(status.romPath as string) : null;

    /**
     * The one writer for the library order.
     *
     * The order and the five directions are one remembered value, so there is
     * one place that changes the screen and one place that writes the file --
     * two setters writing two halves of one value would race each other, and
     * a list that came back half in the new order would be worse than one that
     * came back in the old one.
     */
    const changeView = useCallback((next: LibraryViewSettings): void => {
        viewRef.current = next;
        setView(next);
        // Not awaited: the list is already in the new order on screen, and the
        // file is a note for next time. A failure to write is the main
        // process's to log, not this screen's to report.
        void window.fc.saveLibraryView(next);
    }, []);

    /** Choose an order. The one that is already on is not a change. */
    const changeSort = useCallback((next: SortKey): void => {
        if (next !== viewRef.current.sort) {
            changeView({ ...viewRef.current, sort: next });
        }
    }, [changeView]);

    /**
     * Turn one order round.
     *
     * Pressing the chip that is already on, not a separate button: the arrow
     * is part of the chip, so the direction of an order is set where that
     * order is chosen. Radix reports an empty value when the selected item is
     * pressed again, which is the press this answers to.
     */
    const flipDirection = useCallback((key: SortKey): void => {
        const current = viewRef.current;
        changeView({
            ...current,
            directions: {
                ...current.directions,
                [key]: current.directions[key] === 'asc' ? 'desc' : 'asc',
            },
        });
    }, [changeView]);

    /**
     * The three card sections, which differ only in what they filter to.
     *
     * `fresh` is passed only by the whole-library section: a game nobody has
     * played is a fact about the library, and offering to start it from the
     * pinned or recently-played list would be offering something those lists
     * are not about.
     */
    const cards = (
        panelTitle: string,
        shown: typeof games,
        emptyTitle: string,
        fresh: typeof games = [],
    ) => (
        <LibraryPanel
            title={panelTitle}
            directory={library.directory}
            games={shown}
            loading={loading}
            query={query}
            onQuery={setQuery}
            sort={sort}
            onSort={changeSort}
            directions={directions}
            onFlip={flipDirection}
            filter={filter}
            onFilter={setFilter}
            fresh={fresh}
            activePath={status.romPath}
            onPick={(path) => void play(path)}
            onAdd={() => void addGames()}
            onChooseDirectory={() => void chooseDirectory()}
            onOpenFolder={() => openFolder()}
            onSetTags={(path, tags) => void setTags(path, tags)}
            onTogglePinned={(path, pinned) => void togglePinned(path, pinned)}
            onRemove={(path) => void removeGame(path)}
            emptyTitle={emptyTitle}
        />
    );

    const middle = (() => {
        switch (section) {
        case 'library':
            return cards('游戏库', games, '这个游戏库里还没有游戏。', newGames(games));
        case 'pinned':
            return cards(
                '置顶游戏',
                games.filter((game) => game.pinned),
                '还没有置顶的游戏。在游戏库里点图钉，把它放到最上面。',
            );
        case 'screenshots':
            return (
                <ScreenshotsPanel
                    screenshots={state.screenshots}
                    previewId={previewId}
                    onPreview={setPreviewId}
                    onOpenFolder={() => openFolder('screenshots')}
                    onReveal={(id) => void revealShot(id)}
                    onSetCover={(id) => void setCover(id)}
                    onRemove={(id) => void removeShot(id)}
                />
            );
        case 'saves':
            return (
                <SavesPanel
                    game={title}
                    saves={saves}
                    onSave={saveTo}
                    onLoad={loadFrom}
                    onEject={eject}
                />
            );
        case 'cheats':
            return (
                <CheatsPanel
                    game={title}
                    cheats={cheats}
                    onCheats={changeCheats}
                    peek={peek}
                    poke={poke}
                />
            );
        case 'settings':
            return <SettingsIndex active={settingsTab} onSelect={setSettingsTab} />;
        case 'about':
            return <AboutPanel />;
        }
    })();

    const subtitle = title === null
        ? SECTION_BY_ID[section].title
        : `${title} · ${SECTION_BY_ID[section].title}`;

    return (
        // One tooltip provider for the window, so that the hover intent
        // delay is shared: moving along the toolbar should not restart it.
        // It renders no DOM -- see TitleBar for where the tooltips are.
        <Tooltip.Provider delayDuration={400} skipDelayDuration={300}>
            <div className="app" ref={appRef}>
                <TitleBar
                    section={SECTION_BY_ID[section].title}
                    subtitle={subtitle}
                    busy={loading}
                    onRefresh={() => void refreshLibrary()}
                    onOpenFolder={() => openFolder()}
                    onAbout={() => setSection('about')}
                />

                <div
                    className="workspace"
                    style={{ '--panel-width': `${panel.width}px` } as CSSProperties}
                >
                    <Sidebar
                        active={section}
                        onSelect={setSection}
                    />

                    {middle}

                    {/* Between the panel and the picture. A separator, not a
                        button: it moves the edge and it is a tab stop. */}
                    <div
                        className={panel.dragging ? 'splitter splitter-active' : 'splitter'}
                        {...panel.handleProps}
                    />

                    {/* The play column is two tabs: the console, and a
                        screenshot over it. Both are always mounted (see
                        TabsView); which one is showing is which screenshot is
                        open. */}
                    <TabsView
                        active={activePane}
                        panes={[
                            {
                                id: 'console',
                                content: (
                                    <PlayPanel
                                        canvasRef={canvasRef}
                                        stageRef={stageRef}
                                        picture={picture}
                                        status={status}
                                        scanlines={scanlines}
                                        onCommand={command}
                                        onEject={eject}
                                        onGoToLibrary={() => setSection('library')}
                                        fullscreen={fullscreen}
                                        fullscreenAvailable={fullscreenAvailable}
                                        onToggleFullscreen={toggleFullscreen}
                                    />
                                ),
                            },
                            { id: 'preview', content: preview },
                            ...SETTINGS_GROUPS.map((group) => ({
                                id: group.id,
                                content: (
                                    <SettingsPanes
                                        group={group.id}
                                        status={status}
                                        picture={picture}
                                        scanlines={scanlines}
                                        onScanlines={changeScanlines}
                                        input={input}
                                        onInput={changeInput}
                                        cores={cores}
                                        onCores={changeCores}
                                        gamepadEnabled={window.fc.gamepadEnabled}
                                        gamepadNative={window.fc.gamepadNative}
                                        library={library}
                                        screenshots={state.screenshots.length}
                                        onChooseDirectory={() => void chooseDirectory()}
                                    />
                                ),
                            })),
                        ]}
                    />
                </div>

                <StatusBar
                    status={status}
                    gamepadEnabled={window.fc.gamepadEnabled}
                    gamepadNative={window.fc.gamepadNative}
                />

                {notice !== null && (
                    <div className="toast" role="status">{notice}</div>
                )}

                {/* Over everything, and not in the way of anything: it has no
                    pointer events, so the drop lands on the window as it
                    would have without it. */}
                {dropped && (
                    <div className="dropzone" aria-hidden="true">
                        <div className="dropzone-card">
                            <Plus size={22} />
                            <p className="dropzone-title">松手添加到游戏库</p>
                            <p className="dropzone-hint">
                                .nes 文件，或一个装满游戏的文件夹
                            </p>
                        </div>
                    </div>
                )}
            </div>
        </Tooltip.Provider>
    );
}
