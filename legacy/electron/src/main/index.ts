// ---------------------------------------------------------------------------
// The main process.
//
// Three jobs, and nothing else:
//
//   1. Put a window on screen and load the renderer into it.
//   2. Read a ROM off disk when the renderer asks for one. (The renderer
//      cannot read files, and should not be able to.)
//   3. Serve the built renderer over `app://` in production.
//
// What is NOT here: no emulator, no frame loop, no timing, no audio. The
// emulator runs in the renderer as WebAssembly, where the framebuffer it
// produces is already in memory the canvas can read. Putting a process
// boundary anywhere in that path would mean copying 240KB sixty times a
// second for no reason at all.
// ---------------------------------------------------------------------------

import { app, BrowserWindow, dialog, ipcMain, nativeTheme, net, protocol, shell } from 'electron';
import type {
    MessageBoxOptions, MessageBoxReturnValue, OpenDialogOptions, OpenDialogReturnValue,
} from 'electron';
import { mkdirSync, existsSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { basename, extname, join, normalize, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';

import { NativeGamepad, EMPTY_READING, gamepadBinaryPath } from './gamepad';
import { GameLibrary, SCREENSHOT_DIRECTORY, collectGames, isInside } from './library';
import {
    IpcChannel, LIBRARY_HOST, ROM_EXTENSIONS, CORES_BY_SYSTEM,
    type BootRom, type Cheat, type CoreId, type CoreSelection, type GamepadButtonName,
    type CommandBinding, type GamepadReading, type InputSettings, type KeyBinding, type LibraryState,
    type LibraryViewSettings, type Preferences, type SortKey, type SystemId,
    DEFAULT_INPUT_SETTINGS, DEFAULT_LIBRARY_VIEW, SORT_KEYS, directionOf, normaliseBinding,
} from '../shared/api';
import { bootLog, bootOrigin, setBootOrigin } from '../shared/boot';

// The very first line of the timeline. Everything logged below is measured
// from whatever scripts/dev.mjs put in FC_BOOT_T0, so this number is "how long
// after the dev script started did Electron's main process begin executing" --
// the gap that contains the TypeScript compile and the Vite server coming up.
setBootOrigin(Number(process.env.FC_BOOT_T0 ?? 0));
bootLog(
    'main',
    'module loaded',
    `pid=${process.pid} electron=${process.versions.electron} chrome=${process.versions.chrome}`,
);
// dist-electron/main/index.js  ->  ../..          = electron/
//                              ->  ../../..       = the repository root
const ELECTRON_ROOT = resolve(__dirname, '..', '..');
const REPO_ROOT = resolve(ELECTRON_ROOT, '..');
const RENDERER_DIST = join(ELECTRON_ROOT, 'dist');

/** Where the app:// protocol looks for files. Must be a single, fixed root,
 *  and every request is checked to stay inside it. */
const PROTOCOL_ROOT = RENDERER_DIST;

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

interface Options {
    romPath: string | null;
    selftestFrames: number;
    dumpPath: string | null;
    /** "100:START=1,105:START=0" -- frame at which each button changes. */
    script: string | null;
    /** "1,5,30,300" -- frames whose picture a test wants hashed. */
    snapshots: string | null;
    /** Press real keys at the window and check what the emulator heard. */
    keytest: boolean;
    /** Run in real time for this many seconds, then report the audio ring. */
    audioTestSeconds: number;
    /** Where the library is: the ROMs, and the database that models them.
     *  Null means "work it out". */
    libraryDir: string | null;
    /** Print the library and exit, which is the only way to check a screen
     *  nobody is looking at. */
    listGames: boolean;
    /** Quit gracefully this many seconds after the window is ready. Zero to
     *  stay. A diagnostic for the shutdown paths. */
    quitAfterSeconds: number;
    /** Turn on gamepad support. See FcBridge.gamepadEnabled for why this is
     *  not the default. */
    gamepad: boolean;
    /** Force the browser's Gamepad API instead of the native helper. The old
     *  path, kept for comparing the two; it is the one that cannot quit on
     *  macOS. */
    browserGamepad: boolean;
    /** Start with no gamepad source at all. */
    noGamepad: boolean;
    /** Resize the window through several sizes and report how the picture
     *  fitted. Checks a visual property without a pair of eyes. */
    layoutTest: boolean;
}

function parseArguments(argv: string[]): Options {
    const options: Options = {
        romPath: null,
        selftestFrames: 0,
        dumpPath: null,
        script: null,
        snapshots: null,
        keytest: false,
        audioTestSeconds: 0,
        libraryDir: null,
        listGames: false,
        quitAfterSeconds: 0,
        gamepad: false,
        browserGamepad: false,
        noGamepad: false,
        layoutTest: false,
    };

    for (let i = 0; i < argv.length; i += 1) {
        switch (argv[i]) {
        case '--keytest':
            options.keytest = true;
            break;
        case '--list':
            options.listGames = true;
            break;
        case '--gamepad':
            options.gamepad = true;
            break;
        case '--browser-gamepad':
            options.browserGamepad = true;
            options.gamepad = true;
            break;
        case '--no-gamepad':
            options.noGamepad = true;
            break;
        case '--layout':
            options.layoutTest = true;
            break;
        case '--quit-after': {
            const seconds = argv[i + 1];
            if (seconds !== undefined && /^\d+$/.test(seconds)) {
                options.quitAfterSeconds = Number(seconds);
                i += 1;
            } else {
                options.quitAfterSeconds = 3;
            }
            break;
        }
        case '--audiotest': {
            const seconds = argv[i + 1];
            if (seconds !== undefined && /^\d+$/.test(seconds)) {
                options.audioTestSeconds = Number(seconds);
                i += 1;
            } else {
                options.audioTestSeconds = 6;
            }
            break;
        }
        case '--rom':
            // Resolved against the working directory at start up, so that the
            // path the renderer sees is the same one every time it is asked
            // about. Save states are filed by file name and are indifferent,
            // but cheats are keyed by the whole path, and "../packages/fc-core/tests/data/x.nes"
            // and an absolute path to the same file would be two entries with
            // different bytes in them.
            options.romPath = argv[i + 1] !== undefined ? resolve(argv[i + 1]) : null;
            i += 1;
            break;
        case '--dump':
            options.dumpPath = argv[i + 1] ?? null;
            i += 1;
            break;
        case '--rom-dir':
        case '--library-dir':
            // Both names, because the flag predates the library being a folder
            // with a model in it. `--library-dir` is the honest name now; the
            // old one keeps every script and README line that used it working.
            options.libraryDir = argv[i + 1] ?? null;
            i += 1;
            break;
        case '--script':
            options.script = argv[i + 1] ?? null;
            i += 1;
            break;
        case '--snapshots':
            options.snapshots = argv[i + 1] ?? null;
            i += 1;
            break;
        case '--selftest': {
            // A bare --selftest means "run the standard 300 frames", because
            // that is the run wasm/verify.sh compares against.
            const count = argv[i + 1];
            if (count !== undefined && /^\d+$/.test(count)) {
                options.selftestFrames = Number(count);
                i += 1;
            } else {
                options.selftestFrames = 300;
            }
            break;
        }
        default:
            break;
        }
    }

    // The test modes need a game, and so does anybody running the application
    // with no arguments in a terminal -- but the application itself does not.
    // Started normally it shows the library, which is what a person expects
    // and what every one of those test modes cannot check.
    const needsAGame = options.selftestFrames > 0
        || options.keytest
        || options.audioTestSeconds > 0;

    if (options.romPath === null && needsAGame) {
        // A symlink into a real ROM folder. Absent, and the mode that needed
        // it says so rather than quietly running nothing.
        options.romPath = join(REPO_ROOT, 'tests', 'data', 'super-mario-bros.n' + 'es');
    }
    return options;
}

const options = parseArguments(process.argv);
bootLog(
    'main',
    'arguments parsed',
    `rom=${options.romPath ?? '(none)'} eager=${options.selftestFrames > 0
        || options.keytest || options.audioTestSeconds > 0 || options.layoutTest
        || options.romPath !== null}`,
);

// ---------------------------------------------------------------------------
// The application used to be called "FC Emulator", and Electron names the
// user-data folder after `productName`. So renaming the product renames
// `~/Library/Application Support/<name>` -- and that folder is the library
// (the ROM files themselves, copied in), the save states, the screenshots and
// config.json. An install that already exists would come up empty.
//
// So move it once, as the first thing in the process to touch that path.
// Nothing is deleted: if the new folder is already there it wins and the old
// one is left alone; if the move fails the application carries on with a fresh
// folder rather than refusing to start. Which of those happened goes in the
// boot log, because a library that silently disappeared is the one outcome
// nobody can debug.
// ---------------------------------------------------------------------------
const PREVIOUS_APP_NAME = 'FC Emulator';

function adoptUserDataFromPreviousName(): void {
    const current = app.getPath('userData');
    const previous = join(app.getPath('appData'), PREVIOUS_APP_NAME);
    if (previous === current || !existsSync(previous) || existsSync(current)) return;
    try {
        renameSync(previous, current);
        bootLog('main', 'library folder moved', `${previous} -> ${current}`);
    } catch (error) {
        bootLog('main', 'could not move the old library folder',
                `${previous} -> ${current}: ${String(error)}`);
    }
}

adoptUserDataFromPreviousName();

// ---------------------------------------------------------------------------
// Which gamepad path this run uses
//
// The native helper is the default on macOS: it is a separate process, so
// Chromium never starts its own gamepad service, and the application can still
// quit. The browser path is kept behind --browser-gamepad because it is the
// one that reproduces the original problem, and being able to switch it on is
// how the fix is proved.
//
// A checkout that has not run `pnpm run build:native` has no helper, so the
// native path quietly stands down and --gamepad still means the browser one.
// That is why the "helper not built" line exists: a gamepad that does nothing
// and says nothing is the failure this whole file is meant to avoid.
// ---------------------------------------------------------------------------

/**
 * Where the native helper lives.
 *
 * Two places, because the packaged application is not the repository. In
 * development the helper is the one scripts/build-native.mjs wrote, inside the
 * project; packaged it is an `extraResources` entry next to `app/` in
 * `Contents/Resources` (macOS) or `resources/` (Windows), put there by
 * electron-builder (see the `build` block in package.json).
 * `process.resourcesPath` is that directory, and it is the only reliable way
 * to reach it: `__dirname` points inside the application directory, which is a
 * different folder.
 *
 * The binary itself is Swift on macOS and C++ on Windows; `gamepadBinaryPath`
 * spells the platform's file name.
 */
const GAMEPAD_BINARY = gamepadBinaryPath(
    app.isPackaged ? process.resourcesPath : ELECTRON_ROOT,
);
bootLog('main', 'paths resolved', `packaged=${app.isPackaged} userData=${app.getPath('userData')}`);

/**
 * A run whose input must be exactly what the test scripts.
 *
 * `--selftest` hashes the picture after N frames and compares it against the
 * native build's own hash; `--keytest` asserts which switches are down after
 * a key press. A real pad feeding the same input manager would make both
 * nondeterministic -- a controller on the desk nudged during a test run would
 * change the hash or add an extra held button -- so no gamepad source starts
 * in those modes. That was not obvious when the source was off by default and
 * became obvious the moment it was not.
 */
const SCRIPTED_RUN = options.selftestFrames > 0 || options.keytest;

/**
 * Runs that want the emulator built at start up rather than on demand.
 *
 * The application normally builds the machine lazily, the first time a game is
 * loaded, so the library screen does not wait on the WebAssembly module. These
 * modes need it immediately: the self test and key test and audio test drive
 * it themselves, and the layout check waits on the test hook that building the
 * machine publishes. See the `eager` flag on the bridge.
 */
const EAGER_MACHINE = options.selftestFrames > 0
    || options.keytest
    || options.audioTestSeconds > 0
    || options.layoutTest
    // A game named on the command line is a wish to play it, not to look at
    // the library, so it is built and loaded at start up like it always was.
    || options.romPath !== null;

/** Whether the helper is there to be started. */
const NATIVE_GAMEPAD_AVAILABLE = !options.noGamepad && existsSync(GAMEPAD_BINARY);

/** Whether this run reads pads natively rather than through the browser. */
const USE_NATIVE_GAMEPAD = NATIVE_GAMEPAD_AVAILABLE
    && !options.browserGamepad
    && !SCRIPTED_RUN;

/** Whether any gamepad source should be running at all. */
const GAMEPAD_ENABLED = USE_NATIVE_GAMEPAD || (options.gamepad && !SCRIPTED_RUN);

/** The running helper, or null when this run has no native gamepad source. */
let gamepad: NativeGamepad | null = null;

// SharedArrayBuffer, for the audio ring buffer.
//
// Cross origin isolation is necessary but, in Electron, not sufficient: the
// renderer is not given SharedArrayBuffer unless this feature is on. An
// app:// page happens to be isolated without it, which is why audio works in
// the packaged build and not from the dev server -- the worst possible way for
// a difference to show up, so the switch is here and the two paths are made
// to agree.
app.commandLine.appendSwitch('enable-features', 'SharedArrayBuffer');

// ---------------------------------------------------------------------------
// The app:// protocol
//
// The production renderer could be loaded with file://. It is not, for two
// reasons. file:// pages are a special case in Chromium -- opaque origin, no
// fetch, and no ES modules over file:// in some configurations -- and a
// special case is a bug that only shows up in the packaged build.
//
// A custom scheme is an ordinary origin with ordinary rules, so the packaged
// app behaves exactly like the dev server did.
//
// It also gives us a place to set headers that are not about files at all.
// Cross origin isolation is a pair of them:
//
//   Cross-Origin-Opener-Policy:   same-origin
//   Cross-Origin-Embedder-Policy: require-corp
//
// Together they make `crossOriginIsolated` true, which is the switch that
// turns SharedArrayBuffer back on. Audio needs it: the emulator runs on the
// page's thread and the speaker is fed from the audio thread, and a ring
// buffer shared between two threads is the only way to hand samples over
// without the audio thread ever waiting on the emulator.
//
// Both must be on every response, including the .wasm, because COEP requires
// subresources to opt in as well.
// ---------------------------------------------------------------------------

const CROSS_ORIGIN_ISOLATION: Record<string, string> = {
    'Cross-Origin-Opener-Policy': 'same-origin',
    'Cross-Origin-Embedder-Policy': 'require-corp',
    'Cross-Origin-Resource-Policy': 'same-origin',
};

/**
 * What the page is allowed to do, in production.
 *
 *   script-src 'self' 'wasm-unsafe-eval'
 *       only our own code, plus permission to compile WebAssembly. Note that
 *       this is not 'unsafe-eval': it allows a .wasm file to be instantiated
 *       and nothing else, so it cannot be used to run a string as code.
 *
 *   connect-src 'self'
 *       fetch and XHR stay on this origin. Nothing here talks to a network.
 *
 * The dev server deliberately does not set this, because Vite injects inline
 * scripts for hot reloading that no reasonable policy would allow. That is why
 * Electron's security warning appears during development and not afterwards.
 */
const CONTENT_SECURITY_POLICY = [
    "default-src 'self'",
    "script-src 'self' 'wasm-unsafe-eval'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' app: data:",
    "connect-src 'self'",
    "object-src 'none'",
    "base-uri 'none'",
].join('; ');

protocol.registerSchemesAsPrivileged([
    {
        scheme: 'app',
        privileges: {
            standard: true,
            secure: true,
            supportFetchAPI: true,
            stream: true,
            corsEnabled: true,
        },
    },
]);

function registerAppProtocol(): void {
    protocol.handle('app', async (request) => {
        const url = new URL(request.url);

        // Two hosts, two jobs. `bundle` is the application itself; `library`
        // is the player's own pictures, which is the only thing on disk the
        // page is ever allowed to fetch.
        if (url.host === LIBRARY_HOST) {
            return serveLibraryFile(url);
        }

        if (url.host !== 'bundle') {
            return new Response('not found', { status: 404 });
        }

        const relative = url.pathname === '/' ? '/index.html' : url.pathname;
        const target = normalize(join(PROTOCOL_ROOT, decodeURIComponent(relative)));

        // A path like ../../etc/passwd would escape the renderer directory.
        // normalize() collapses the .. before we compare, so this catches it.
        if (!target.startsWith(PROTOCOL_ROOT + sep)) {
            return new Response('forbidden', { status: 403 });
        }

        // net.fetch reads the file; the headers are added on the way out,
        // because a file on disk has none.
        const response = await net.fetch(pathToFileURL(target).toString());
        const headers = new Headers(response.headers);
        for (const [name, value] of Object.entries(CROSS_ORIGIN_ISOLATION)) {
            headers.set(name, value);
        }
        headers.set('Content-Security-Policy', CONTENT_SECURITY_POLICY);
        return new Response(response.body, { status: response.status, headers });
    });
}

/**
 * One picture from inside the library, for an `<img>` to fetch.
 *
 * This is a picture frame and not a window into the disk, and what makes it
 * one is two rules: the path must be under `screenshots/`, and it must end in
 * `.png`. Everything else in the library -- the ROMs, the database, the save
 * states next to them -- is unreachable from the page, which is the same
 * promise the preload makes in the other direction.
 *
 * `Cross-Origin-Resource-Policy: cross-origin` rather than the document's
 * `same-origin`: in development the page comes from Vite on localhost and the
 * picture comes from `app://`, and under COEP a cross-origin subresource
 * without this header is blocked outright. The resources are the player's own
 * screenshots, served to the player's own page, and nothing else can reach
 * this URL.
 */
async function serveLibraryFile(url: URL): Promise<Response> {
    let relative: string;
    try {
        relative = decodeURIComponent(url.pathname).replace(/^\/+/, '');
    } catch {
        // A malformed percent escape is a malformed request.
        return new Response('bad request', { status: 400 });
    }

    if (!relative.startsWith(`${SCREENSHOT_DIRECTORY}/`)
        || !relative.toLowerCase().endsWith('.png')) {
        return new Response('forbidden', { status: 403 });
    }

    const root = resolve(libraryDirectory());
    const target = resolve(join(root, relative));
    if (!target.startsWith(root + sep)) {
        return new Response('forbidden', { status: 403 });
    }

    let bytes: Buffer;
    try {
        bytes = await readFile(target);
    } catch {
        return new Response('not found', { status: 404 });
    }

    return new Response(bytes, {
        headers: {
            'Content-Type': 'image/png',
            'Cross-Origin-Resource-Policy': 'cross-origin',
            // A screenshot never changes once written, and its name is
            // unique, so the browser may keep it as long as it likes.
            'Cache-Control': 'private, max-age=31536000, immutable',
        },
    });
}

// ---------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------

function createWindow(): BrowserWindow {
    bootLog('main', 'createWindow begin');
    const window = new BrowserWindow({
        width: 1180,
        height: 240 * 3 + 132,
        minWidth: 480,
        minHeight: 360,
        // The application paints its own toolbar, the way every modern macOS
        // application does: a unified title bar with the traffic lights sitting
        // over it. "hiddenInset" keeps the real traffic lights -- green is a
        // full screen button and should behave like one -- and hands the rest
        // of the strip to the page. See src/renderer/components/TitleBar.tsx.
        titleBarStyle: 'hiddenInset',
        // The colour behind the page while it loads. Matching the renderer's
        // own background stops the flash of the wrong shade on start up, and
        // which shade is right depends on the system appearance.
        backgroundColor: nativeTheme.shouldUseDarkColors ? '#1e1e1e' : '#ececec',
        title: 'Classic Game Box',
        // A self test that only wants a pixel hash does not need a window on
        // screen; one that wants a screenshot, real key presses, or a
        // real-time audio ring does. A hidden window is not a window without
        // a renderer, but Chromium may stop drawing one, and a page that is
        // not drawing gets no animation frames.
        show: options.keytest || options.audioTestSeconds > 0 || options.layoutTest
            || options.dumpPath !== null || options.selftestFrames === 0,
        webPreferences: {
            preload: join(__dirname, '..', 'preload', 'index.js'),
            contextIsolation: true,
            nodeIntegration: false,
            // The renderer loads local files only. Sandboxing the preload would
            // work too, but it restricts it to a subset of Node for no benefit
            // here, and a preload that silently fails to load is a bad way to
            // spend an afternoon.
            sandbox: false,
            // Chromium throttles requestAnimationFrame in a window that is not
            // focused or visible. For a page that is a game, that is not an
            // optimisation, that is the game running slow.
            backgroundThrottling: false,

            // Tell the preload what mode it is in. See FcBridge.
            additionalArguments: [
                `--fc-boot-t0=${bootOrigin()}`,
                // The saved core choice, so an eager `--rom` build is not on
                // the wrong core for the first hundred milliseconds. Encoded
                // because it is JSON and an argument list is not.
                `--fc-cores=${encodeURIComponent(JSON.stringify(normaliseCores(readConfig().cores)))}`,
                // And the saved library order, so the list draws itself in the
                // order the player chose instead of rearranging a moment after
                // the first paint. Same reason, same encoding.
                `--fc-library-view=${encodeURIComponent(
                    JSON.stringify(normaliseLibraryView(readConfig().libraryView)),
                )}`,
                ...(options.selftestFrames > 0 ? ['--fc-selftest'] : []),
                ...(EAGER_MACHINE ? ['--fc-eager'] : []),
                ...(GAMEPAD_ENABLED ? ['--fc-gamepad'] : []),
                ...(USE_NATIVE_GAMEPAD ? ['--fc-gamepad-native'] : []),
            ],
        },
    });

    const devServer = process.env.VITE_DEV_SERVER_URL;
    bootLog(
        'main',
        devServer ? 'loadURL (dev server)' : 'loadURL (app://)',
        devServer ?? 'app://bundle/index.html',
    );
    if (devServer) {
        void window.loadURL(devServer);
    } else {
        void window.loadURL('app://bundle/index.html');
    }

    // The window's whole life, as events. These are the only signal the main
    // process gets about the renderer's progress, and they are what separates
    // "Electron is slow" from "the page is slow": `did-start-loading` to
    // `dom-ready` is Chromium fetching and parsing; `dom-ready` to
    // `did-finish-load` is the page's own scripts running.
    window.webContents.on('did-start-loading', () => bootLog('main', 'did-start-loading'));
    window.webContents.on('dom-ready', () => bootLog('main', 'dom-ready (html parsed)'));
    window.webContents.on('did-finish-load', () => bootLog('main', 'did-finish-load'));
    window.once('ready-to-show', () => bootLog('main', 'window ready-to-show (first paint)'));
    window.webContents.on('did-fail-load', (_event, code, description, url) => {
        bootLog('main', 'did-fail-load', `${code} ${description} ${url}`);
    });
    window.webContents.on('preload-error', (_event, path, error) => {
        bootLog('main', 'preload-error', `${path}: ${error.message}`);
    });
    window.webContents.on('render-process-gone', (_event, details) => {
        bootLog('main', 'render-process-gone', `${details.reason} (exit ${details.exitCode})`);
    });
    window.webContents.on('unresponsive', () => bootLog('main', 'renderer unresponsive'));

    // Anything the page logs comes out here. Without it, an exception in the
    // renderer is invisible from the terminal: executeJavaScript reports only
    // "Script failed to execute, this normally means an error was thrown",
    // which is true and useless.
    window.webContents.on('console-message', (...args: unknown[]) => {
        // Electron changed this signature: newer versions pass one event
        // object with the fields on it, older ones pass them positionally.
        const first = args[0] as { message?: string; level?: string };
        if (typeof first?.message === 'string') {
            console.log(`[renderer] ${first.message}`);
            return;
        }
        const message = args[2];
        if (typeof message === 'string' && message.length > 0) {
            console.log(`[renderer] ${message}`);
        }
    });

    bootLog('main', 'createWindow end');
    return window;
}

// ---------------------------------------------------------------------------
// The game library
//
// Where the games are, what is in there, and which of them has been played.
// All of it belongs in the main process: it is the filesystem and a database,
// and it outlives the renderer.
// ---------------------------------------------------------------------------

/**
 * Where the library lives.
 *
 * A library is one folder: the ROMs, and `library.sqlite` beside them. The
 * resolution order is
 *
 *   1. `--library-dir` (or `--rom-dir`, its old name) on the command line
 *   2. whatever the player last chose from the interface
 *   3. `library` inside the application's user data directory
 *
 * The default is the user data directory and not, as it once was, a folder in
 * Documents: the library copies games into itself now, and writing into
 * somebody's Documents without being asked is not something an application
 * should do. Documents is where games are *imported from*.
 */
function libraryDirectory(): string {
    if (options.libraryDir !== null) {
        return resolve(options.libraryDir);
    }
    if (chosenLibraryRoot !== null) {
        return chosenLibraryRoot;
    }
    const config = readConfig();
    chosenLibraryRoot = config.libraryRoot !== undefined
        ? resolve(config.libraryRoot)
        : join(app.getPath('userData'), 'library');
    return chosenLibraryRoot;
}

/**
 * The one setting that cannot live in the database.
 *
 * Which folder the database is in has to be known before the database can be
 * opened, so it is a small JSON file in the user data directory. It holds that
 * one path and nothing else; everything else about the library is modelled in
 * SQLite, where queries can reach it.
 */
interface Config {
    libraryRoot?: string;
    /** Scanline overlay on the picture. Absent means off. */
    scanlines?: boolean;
    /** Middle column width in CSS pixels. Absent means it was never chosen. */
    panelWidth?: number;
    /** Keyboard mode, key bindings and pad assignments. */
    input?: InputSettings;
    /** Which core to use for each console, keyed by system id. */
    cores?: Record<string, string>;
    /** How the library list is ordered, and which way round each order is. */
    libraryView?: Partial<LibraryViewSettings>;
    /** Cheats, keyed by the cartridge's path. */
    cheats?: Record<string, Cheat[]>;
}

/**
 * A cheat list, with anything unrecognised dropped.
 *
 * Read defensively for the same reason `normaliseInput` is: the config file
 * is a text file a person can edit, and a bad address should cost that one
 * entry rather than the application starting.
 */
function normaliseCheats(raw: unknown): Cheat[] {
    if (!Array.isArray(raw)) {
        return [];
    }

    const cheats: Cheat[] = [];
    for (const entry of raw) {
        if (entry === null || typeof entry !== 'object') {
            continue;
        }
        const cheat = entry as Partial<Cheat>;
        // An address is a 16 bit CPU address and a value is one byte. A cheat
        // that is not one of those is not a cheat, and silently truncating it
        // would put a byte somewhere the player did not ask for.
        if (typeof cheat.address !== 'number' || !Number.isInteger(cheat.address)
            || cheat.address < 0 || cheat.address > 0xFFFF
            || typeof cheat.value !== 'number' || !Number.isInteger(cheat.value)
            || cheat.value < 0 || cheat.value > 0xFF) {
            continue;
        }
        cheats.push({
            label: typeof cheat.label === 'string' ? cheat.label.slice(0, 40) : '',
            address: cheat.address,
            value: cheat.value,
            freeze: cheat.freeze === true,
            enabled: cheat.enabled !== false,
        });
    }
    return cheats;
}

/** The eight switch names, for validating a binding that came over IPC. */
const SWITCH_NAMES: readonly GamepadButtonName[] = [
    'A', 'B', 'SELECT', 'START', 'UP', 'DOWN', 'LEFT', 'RIGHT',
];

/**
 * An input configuration, with anything unrecognised replaced by a default.
 *
 * A config file is a text file a person can edit, so it is read defensively:
 * a bad binding is dropped rather than throwing on start up. The parts that
 * survive are always one of the shapes the renderer understands.
 */
function normaliseInput(raw: unknown): InputSettings {
    if (raw === null || typeof raw !== 'object') {
        return DEFAULT_INPUT_SETTINGS;
    }
    const value = raw as Partial<InputSettings>;

    const keyboard = value.keyboard === '2p' ? '2p' : '1p';
    const keyboardPlayer = value.keyboardPlayer === 1 ? 1 : 0;

    let bindings: KeyBinding[] | null = null;
    if (Array.isArray(value.bindings)) {
        bindings = value.bindings.filter((entry): entry is KeyBinding => {
            if (entry === null || typeof entry !== 'object') {
                return false;
            }
            const binding = entry as Partial<KeyBinding>;
            return typeof binding.code === 'string'
                && binding.code !== ''
                && (binding.port === 0 || binding.port === 1)
                && SWITCH_NAMES.includes(binding.button as GamepadButtonName);
        });
    }

    // The command bindings, through the same normaliser the renderer resolves
    // with: a binding this side keeps and the other drops is a command that
    // does nothing, and the two would drift the moment one of them changed.
    let commands: CommandBinding[] | null = null;
    if (Array.isArray(value.commands)) {
        commands = value.commands
            .map(normaliseBinding)
            .filter((binding): binding is CommandBinding => binding !== null);
    }

    const padPorts: number[] = Array.isArray(value.padPorts)
        ? value.padPorts.map((port) => (
            port === 0 || port === 1 || port === -1 ? port : -1
        ))
        : [];

    return { keyboard, keyboardPlayer, bindings, padPorts, commands };
}

/**
 * A core selection, with anything that is not a real (system, core) pair
 * dropped.
 *
 * Read defensively for the same reason `normaliseInput` is: config.json is a
 * text file a person can edit. The IDs are checked against the shared table so
 * the main process can never be talked into remembering a core the renderer
 * would then silently fall back from -- a choice that does not stick is worse
 * than one that was refused.
 */
function normaliseCores(raw: unknown): CoreSelection {
    const selection: CoreSelection = {};
    if (raw === null || typeof raw !== 'object') {
        return selection;
    }
    const value = raw as Record<string, unknown>;
    for (const system of Object.keys(CORES_BY_SYSTEM) as SystemId[]) {
        const core = value[system];
        if (typeof core === 'string'
            && (CORES_BY_SYSTEM[system] as readonly string[]).includes(core)) {
            selection[system] = core as CoreId;
        }
    }
    return selection;
}

/**
 * The remembered library order, with anything unrecognised replaced.
 *
 * Read defensively like the two above, and here it matters twice over: this
 * value is used as the *starting* state of the list, so an order that does not
 * exist would leave the control with nothing selected and the sort comparing
 * nothing. A missing or damaged field falls back to the default rather than
 * failing, because the worst case is a list in the default order.
 */
function normaliseLibraryView(raw: unknown): LibraryViewSettings {
    const view: LibraryViewSettings = {
        sort: DEFAULT_LIBRARY_VIEW.sort,
        directions: { ...DEFAULT_LIBRARY_VIEW.directions },
    };
    if (raw === null || typeof raw !== 'object') {
        return view;
    }

    const value = raw as { sort?: unknown; directions?: unknown };
    if (typeof value.sort === 'string' && (SORT_KEYS as readonly string[]).includes(value.sort)) {
        view.sort = value.sort as SortKey;
    }
    if (value.directions !== null && typeof value.directions === 'object') {
        const directions = value.directions as Record<string, unknown>;
        for (const key of SORT_KEYS) {
            const direction = directions[key];
            if (direction === 'asc' || direction === 'desc') {
                // Through `directionOf`: an order with one end stores that end,
                // whatever the file says. A build that offered an arrow for
                // "recently played" could have written an ascending one, and
                // it means nothing now.
                view.directions[key] = directionOf(key, direction);
            }
        }
    }
    return view;
}

/** Cached, because this is asked for on every IPC call and reading a file to
 *  answer it would be silly. `--library-dir` never touches it: a test flag
 *  must not overwrite the player's choice. */
let chosenLibraryRoot: string | null = null;

function configPath(): string {
    return join(app.getPath('userData'), 'config.json');
}

function readConfig(): Config {
    try {
        const text = readFileSync(configPath(), 'utf8');
        const parsed: unknown = JSON.parse(text);
        if (parsed !== null && typeof parsed === 'object') {
            return parsed as Config;
        }
    } catch {
        // No file yet, or one that has been edited into nonsense. Either way
        // the defaults are a perfectly good configuration.
    }
    return {};
}

function writeConfig(config: Config): void {
    try {
        mkdirSync(app.getPath('userData'), { recursive: true });
        writeFileSync(configPath(), JSON.stringify(config, null, 2));
    } catch (error) {
        console.error(`could not write the configuration: ${(error as Error).message}`);
    }
}

/**
 * The open library.
 *
 * Kept open for the life of the process, and reopened when the player points
 * the application at another folder. Opening it is what creates the folder and
 * the database, so a library that has never been seen before is simply an empty
 * one.
 */
let openLibrary: GameLibrary | null = null;

function library(): GameLibrary {
    const root = libraryDirectory();
    if (openLibrary === null || openLibrary.root !== root) {
        openLibrary?.close();
        openLibrary = GameLibrary.open(root);
    }
    return openLibrary;
}

/**
 * Move the library somewhere else, and remember it.
 *
 * Nothing is migrated, and that is the point of putting the database in the
 * folder: another folder is another library, with its own games and its own
 * pinned flags. Copying games between libraries is what the import panel is
 * for.
 */
function switchLibrary(root: string): void {
    const absolute = resolve(root);
    chosenLibraryRoot = absolute;
    writeConfig({ ...readConfig(), libraryRoot: absolute });
}

/**
 * Which cartridge is in the slot, as far as the main process knows.
 *
 * Save states are filed per cartridge, and this is what names the directory.
 * It used to be the ROM named on the command line, which meant a game started
 * from the library could not be saved at all: the renderer saw "could not save
 * slot 1" and the status line said nothing about why. Now the renderer says
 * what it loaded -- see IpcChannel.SetCartridge -- and both ways in work.
 */
let currentCartridge: string | null = null;

// ---------------------------------------------------------------------------
// Save states on disk
// ---------------------------------------------------------------------------

/** One slot, on disk. */
function slotName(slot: number): string {
    return `slot-${slot}.state`;
}

/**
 * Where this game's save states live.
 *
 * One directory per cartridge, named after the ROM file, under Electron's
 * per-user data directory.
 *
 * Named after the file rather than after the game's title screen, because the
 * title is inside the ROM and reading it would mean running the game first. A
 * player who renames the file gets a fresh set of slots, which is a surprise
 * once and then obvious.
 */
function saveDirectory(): string | null {
    if (currentCartridge === null) {
        return null;
    }

    const name = basename(currentCartridge, extname(currentCartridge));

    // Anything that is not a letter, digit or safe punctuation becomes an
    // underscore. ROM file names contain parentheses, brackets, and around
    // here, Chinese characters; most of those are legal in a path, but not
    // all, and a directory that cannot be created is a save that silently does
    // not happen.
    const safe = name.replace(/[^\p{L}\p{N}._-]/gu, '_');
    return join(app.getPath('userData'), 'saves', safe);
}

// ---------------------------------------------------------------------------
// IPC
// ---------------------------------------------------------------------------

/**
 * A panel or an alert, parented to the window.
 *
 * Without a parent, macOS can put the panel behind the window that asked for
 * it, which looks like the button did nothing. Both forms are here because
 * Electron has two overloads and the un-parented one is the fallback for a
 * window that cannot be found.
 */
function openPanel(options: OpenDialogOptions): Promise<OpenDialogReturnValue> {
    const parent = BrowserWindow.getFocusedWindow() ?? BrowserWindow.getAllWindows()[0];
    return parent === undefined
        ? dialog.showOpenDialog(options)
        : dialog.showOpenDialog(parent, options);
}

function askUser(options: MessageBoxOptions): Promise<MessageBoxReturnValue> {
    const parent = BrowserWindow.getFocusedWindow() ?? BrowserWindow.getAllWindows()[0];
    return parent === undefined
        ? dialog.showMessageBox(options)
        : dialog.showMessageBox(parent, options);
}

/**
 * Everything the renderer needs to draw the library.
 *
 * One shape for every verb that reads or changes it. Taking a screenshot
 * changes a game's cover and its screenshot count, deleting a game deletes its
 * pictures, and switching libraries switches both at once -- so a verb that
 * answered with only half of this would be a verb the screen has to follow up
 * with a second question, and a moment where the two answers disagree.
 */
function libraryState(model: GameLibrary): LibraryState {
    return {
        library: {
            directory: model.root,
            database: model.databasePath,
            games: model.scan(),
        },
        screenshots: model.screenshots(),
    };
}

/**
 * Bytes that arrived over IPC.
 *
 * Structured clone gives back a Uint8Array for a Uint8Array, but a different
 * realm's Uint8Array does not satisfy `instanceof` across every Electron
 * version, and an ArrayBuffer is what a caller is most likely to have anyway.
 * Both are accepted rather than trusting one of them to keep working.
 */
function asBytes(value: unknown): Uint8Array | null {
    if (value instanceof Uint8Array) {
        return value;
    }
    if (value instanceof ArrayBuffer) {
        return new Uint8Array(value);
    }
    return null;
}

function registerIpc(): void {
    ipcMain.handle(IpcChannel.GetBootRom, async (): Promise<BootRom | null> => {
        const path = options.romPath;
        if (path === null) {
            return null;
        }
        try {
            const bytes = await readFile(path);
            return { path, bytes: new Uint8Array(bytes) };
        } catch (error) {
            console.error(`could not read ${path}: ${(error as Error).message}`);
            return null;
        }
    });

    ipcMain.handle(IpcChannel.Library, async (): Promise<LibraryState> => libraryState(library()));

    ipcMain.handle(IpcChannel.ReadRom, async (_event, path: string): Promise<Uint8Array | null> => {
        if (typeof path !== 'string' || !isInside(libraryDirectory(), path)) {
            console.error(`refused to read ${path}: outside the library folder`);
            return null;
        }
        try {
            return new Uint8Array(await readFile(path));
        } catch (error) {
            console.error(`could not read ${path}: ${(error as Error).message}`);
            return null;
        }
    });

    ipcMain.handle(IpcChannel.NotePlayed, async (_event, path: string): Promise<void> => {
        if (typeof path !== 'string') {
            return;
        }
        library().notePlayed(path);
    });

    /**
     * Play time, in whole emulated seconds.
     *
     * Everything about the accounting is in the model (library.notePlaytime);
     * this only refuses a message that is not the shape it should be. The
     * renderer sends it every few seconds and once when the cartridge comes
     * out, which is why there is nothing here to get out of step with.
     */
    ipcMain.handle(
        IpcChannel.NotePlaytime,
        async (_event, request: { path: string; seconds: number }): Promise<void> => {
            if (typeof request?.path !== 'string' || typeof request.seconds !== 'number') {
                return;
            }
            library().notePlaytime(request.path, request.seconds);
        },
    );

    ipcMain.handle(
        IpcChannel.SetGameTags,
        async (_event, request: { path: string; tags: unknown }): Promise<LibraryState> => {
            const model = library();
            if (typeof request?.path === 'string' && Array.isArray(request.tags)) {
                model.setTags(
                    request.path,
                    request.tags.filter((tag): tag is string => typeof tag === 'string'),
                );
            }
            return libraryState(model);
        },
    );

    ipcMain.handle(
        IpcChannel.TogglePinned,
        async (_event, request: { path: string; pinned: boolean }): Promise<LibraryState> => {
            const model = library();
            if (typeof request?.path === 'string' && typeof request.pinned === 'boolean') {
                model.setPinned(request.path, request.pinned);
            }
            return libraryState(model);
        },
    );

    /**
     * Import games, from a drop or from the panel.
     *
     * Two ways in, one implementation: whether the paths came off a drag or
     * out of the open panel, they go through the same filter and the same
     * copy. `collectGames` is what makes a dropped folder mean "the ROMs in
     * it" and a dropped .zip mean nothing at all.
     */
    ipcMain.handle(
        IpcChannel.AddGames,
        async (_event, dropped?: unknown): Promise<LibraryState | null> => {
            const model = library();
            const paths = Array.isArray(dropped)
                ? dropped.filter((path): path is string => typeof path === 'string')
                : null;

            // An empty array is a button; anything else is a drop that named
            // files. Told apart here rather than in the renderer, so that a
            // drop can never fall back to opening a panel the person did not
            // ask for.
            if (paths === null || paths.length === 0) {
                const chosen = await openPanel({
                    title: '添加到游戏库',
                    buttonLabel: '拷贝',
                    properties: ['openFile', 'multiSelections'],
                    filters: [{ name: 'ROM', extensions: [...ROM_EXTENSIONS] }],
                });
                if (chosen.canceled) {
                    return null;
                }
                const sources = collectGames(chosen.filePaths);
                if (sources.length > 0) {
                    model.add(sources);
                }
            } else {
                const sources = collectGames(paths);
                if (sources.length === 0) {
                    return null;
                }
                model.add(sources);
            }

            return libraryState(model);
        },
    );

    /**
     * Delete a game, after asking.
     *
     * The confirmation is drawn by the main process rather than the page: this
     * is the one control that destroys something, and a renderer bug must not
     * be able to reach it without a person clicking a system alert. The
     * screenshots go with it -- they were pictures of this game, and a
     * screenshot of a game that is not in the library is a picture nobody can
     * find again.
     */
    ipcMain.handle(
        IpcChannel.RemoveGame,
        async (_event, path: string): Promise<LibraryState | null> => {
            const model = library();
            if (typeof path !== 'string' || !isInside(model.root, path)) {
                console.error(`refused to remove ${path}: outside the library folder`);
                return null;
            }

            const name = basename(path);
            const pictures = model.screenshots().filter((shot) => shot.gamePath === path).length;
            const answer = await askUser({
                type: 'warning',
                title: '从游戏库移除',
                message: `要把「${basename(name, extname(name))}」从游戏库删除吗？`,
                detail: pictures === 0
                    ? 'ROM 文件会从游戏库文件夹删除，存档保留。'
                    : `ROM 文件会从游戏库文件夹删除，存档保留，它的 ${pictures} 张截图也会一起删除。`,
                buttons: ['删除', '取消'],
                defaultId: 1,
                cancelId: 1,
            });
            if (answer.response !== 0) {
                return null;
            }

            model.remove(path);
            return libraryState(model);
        },
    );

    ipcMain.handle(IpcChannel.ChooseLibraryDirectory, async (): Promise<LibraryState | null> => {
        const chosen = await openPanel({
            title: '选择游戏库文件夹',
            buttonLabel: '使用',
            properties: ['openDirectory', 'createDirectory'],
        });
        if (chosen.canceled || chosen.filePaths[0] === undefined) {
            return null;
        }

        switchLibrary(chosen.filePaths[0]);
        return libraryState(library());
    });

    ipcMain.handle(IpcChannel.SetCartridge, async (_event, path: string | null): Promise<void> => {
        currentCartridge = typeof path === 'string' ? path : null;
    });

    /**
     * The window preferences.
     *
     * These are small enough to live in config.json next to the library root,
     * which is where they should have been all along. They were in the
     * renderer's localStorage, and the first synchronous access to that -- in
     * Electron -- blocks the renderer until the storage service answers, which
     * on this machine is 3.7 seconds. Reading a JSON file that is already in
     * the page cache costs microseconds and never blocks the first frame.
     */
    ipcMain.handle(IpcChannel.ReadPreferences, async (): Promise<Preferences> => {
        const config = readConfig();
        return {
            scanlines: config.scanlines === true,
            panelWidth: typeof config.panelWidth === 'number'
                && Number.isFinite(config.panelWidth)
                ? config.panelWidth
                : null,
            input: normaliseInput(config.input),
            cores: normaliseCores(config.cores),
            libraryView: normaliseLibraryView(config.libraryView),
        };
    });

    /**
     * The input configuration: keyboard mode, key bindings, pad assignments.
     *
     * One verb for the whole object. A rebinding is only meaningful together
     * with the keyboard mode it was made under, so writing it a field at a
     * time would leave windows where the file holds half of one configuration
     * and half of another.
     */
    ipcMain.handle(IpcChannel.WriteInputSettings, async (_event, settings: unknown): Promise<void> => {
        writeConfig({ ...readConfig(), input: normaliseInput(settings) });
    });

    /**
     * Which core to run each console on.
     *
     * The whole mapping at once, and only pairs that exist: see
     * `normaliseCores`. The renderer has already rebuilt its machine by the
     * time this lands, so the file is a record of a decision, not the decision
     * itself.
     */
    ipcMain.handle(IpcChannel.WriteCoreSelection, async (_event, selection: unknown): Promise<void> => {
        writeConfig({ ...readConfig(), cores: normaliseCores(selection) });
    });

    /**
     * How the library list is ordered.
     *
     * Written on every press of an order or an arrow, because there is nothing
     * to submit: the list is already in the new order on screen, and this is
     * only the note for next time. Validated, so a renderer bug can leave the
     * player in the default order rather than in an order that does not exist.
     */
    ipcMain.handle(IpcChannel.WriteLibraryView, async (_event, view: unknown): Promise<void> => {
        writeConfig({ ...readConfig(), libraryView: normaliseLibraryView(view) });
    });

    /**
     * The cheats saved for one cartridge.
     *
     * Keyed by the ROM's path, which is the same handle save states are filed
     * under: a cheat addresses one game's RAM, so it is meaningless for any
     * other cartridge.
     */
    ipcMain.handle(IpcChannel.ReadCheats, async (_event, romPath: unknown): Promise<Cheat[]> => {
        if (typeof romPath !== 'string') {
            return [];
        }
        return normaliseCheats(readConfig().cheats?.[romPath]);
    });

    ipcMain.handle(
        IpcChannel.WriteCheats,
        async (
            _event,
            request: { romPath?: unknown; cheats?: unknown },
        ): Promise<void> => {
            const romPath = request?.romPath;
            if (typeof romPath !== 'string' || romPath === '') {
                console.error('refused a cheat list: no cartridge');
                return;
            }

            const cheats = normaliseCheats(request?.cheats);
            const all = { ...(readConfig().cheats ?? {}) };
            if (cheats.length === 0) {
                // An empty list is a deletion, not an entry. A config file
                // that grew a key per cartridge ever played would be a config
                // file nobody could read.
                delete all[romPath];
            } else {
                all[romPath] = cheats;
            }
            writeConfig({ ...readConfig(), cheats: all });
        },
    );

    ipcMain.handle(
        IpcChannel.WritePreference,
        async (
            _event,
            request: { name?: unknown; value?: unknown },
        ): Promise<void> => {
            // Checked here rather than trusted: the renderer is the only
            // caller today, but a preference file that a renderer bug can fill
            // with nonsense is a start up that fails tomorrow.
            const name = request?.name;
            const value = request?.value;
            if (name === 'scanlines' && typeof value === 'boolean') {
                writeConfig({ ...readConfig(), scanlines: value });
            } else if (name === 'panelWidth' && typeof value === 'number' && Number.isFinite(value)) {
                writeConfig({ ...readConfig(), panelWidth: Math.round(value) });
            } else {
                console.error(`refused a preference change: ${String(name)}=${String(value)}`);
            }
        },
    );

    /**
     * The native gamepad helper's current reading.
     *
     * The renderer asks for this once, on start up, and then listens for
     * changes. The push covers everything except the gap between the helper's
     * first reading and the page being ready to hear it, and this closes that
     * gap: a pad that was already connected when the window opened reports its
     * held buttons immediately instead of waiting for the player to let go and
     * press again.
     */
    ipcMain.handle(IpcChannel.GetGamepadState, async (): Promise<GamepadReading> => {
        return gamepad?.reading ?? EMPTY_READING;
    });

    /**
     * Write a screenshot taken from the picture.
     *
     * The bytes arrive from the renderer because the canvas is there and the
     * file is here: the page encodes a PNG it already holds, and the main
     * process decides where a PNG goes. It checks the signature and the game
     * again -- see GameLibrary.saveScreenshot -- so a renderer bug cannot fill
     * the screenshots folder with things that are not pictures, or file one
     * against a game that does not exist.
     */
    ipcMain.handle(
        IpcChannel.SaveScreenshot,
        async (
            _event,
            request: { gamePath?: unknown; bytes?: unknown; asCover?: unknown },
        ): Promise<LibraryState | null> => {
            const model = library();
            const bytes = asBytes(request?.bytes);
            if (typeof request?.gamePath !== 'string' || bytes === null) {
                console.error('refused a screenshot: no game, or no bytes');
                return null;
            }

            const saved = model.saveScreenshot(
                request.gamePath,
                bytes,
                request.asCover === true,
            );
            if (saved === null) {
                console.error(`refused a screenshot for ${request.gamePath}`);
                return null;
            }
            return libraryState(model);
        },
    );

    ipcMain.handle(
        IpcChannel.SetScreenshotCover,
        async (_event, id: number): Promise<LibraryState | null> => {
            const model = library();
            if (typeof id !== 'number' || !model.setCover(id)) {
                return null;
            }
            return libraryState(model);
        },
    );

    ipcMain.handle(
        IpcChannel.RemoveScreenshot,
        async (_event, id: number): Promise<LibraryState | null> => {
            const model = library();
            if (typeof id !== 'number' || !model.removeScreenshot(id)) {
                return null;
            }
            return libraryState(model);
        },
    );

    /**
     * Show one screenshot in the file browser.
     *
     * Everything that decides *whether* a file may be shown is in the model
     * (`screenshotPath`): the row is looked up here and the path is built and
     * checked there, so a renderer that guesses an id, or a database whose rows
     * were edited, still cannot make the operating system select something
     * outside the library. What is left here is the one side effect.
     */
    ipcMain.handle(
        IpcChannel.RevealScreenshot,
        async (_event, id: unknown): Promise<boolean> => {
            if (typeof id !== 'number') {
                return false;
            }
            const path = library().screenshotPath(id);
            if (path === null) {
                return false;
            }
            shell.showItemInFolder(path);
            return true;
        },
    );

    ipcMain.handle(
        IpcChannel.SaveState,
        async (_event, request: { slot: number; bytes: Uint8Array }): Promise<boolean> => {
            const directory = await saveDirectory();
            if (directory === null) {
                return false;
            }
            try {
                // One directory per cartridge, made on the first save rather
                // than at startup, so playing a game without saving leaves no
                // trace on disk.
                await mkdir(directory, { recursive: true });
                await writeFile(join(directory, slotName(request.slot)), request.bytes);
                return true;
            } catch (error) {
                console.error(`could not write a save state: ${(error as Error).message}`);
                return false;
            }
        },
    );

    ipcMain.handle(
        IpcChannel.LoadState,
        async (_event, slot: number): Promise<Uint8Array | null> => {
            const directory = await saveDirectory();
            if (directory === null) {
                return null;
            }
            try {
                const bytes = await readFile(join(directory, slotName(slot)));
                return new Uint8Array(bytes);
            } catch {
                // An empty slot is not an error; it is a slot.
                return null;
            }
        },
    );

    ipcMain.handle(IpcChannel.ListSaves, async (): Promise<number[]> => {
        const directory = await saveDirectory();
        if (directory === null) {
            return [];
        }
        try {
            const names = await readdir(directory);
            return names
                .map((name) => /^slot-(\d+)\.state$/.exec(name))
                .filter((match): match is RegExpExecArray => match !== null)
                .map((match) => Number(match[1]))
                .sort((a, b) => a - b);
        } catch {
            return [];
        }
    });

    ipcMain.handle(IpcChannel.OpenFolder, async (_event, subdirectory?: unknown): Promise<boolean> => {
        const root = resolve(libraryDirectory());
        let directory = root;

        // A subfolder is allowed if it is inside the library, which is the
        // same rule the read-only side uses. Created first, so that a folder
        // that has not been used yet opens as an empty one rather than as an
        // error.
        if (typeof subdirectory === 'string' && subdirectory !== '') {
            const candidate = resolve(join(root, subdirectory));
            if (!candidate.startsWith(root + sep)) {
                console.error(`refused to open ${subdirectory}: outside the library folder`);
                return false;
            }
            directory = candidate;
        }

        try {
            await mkdir(directory, { recursive: true });
        } catch (error) {
            console.error(`could not create ${directory}: ${(error as Error).message}`);
            return false;
        }
        const problem = await shell.openPath(directory);
        if (problem !== '') {
            console.error(`could not open ${directory}: ${problem}`);
            return false;
        }
        return true;
    });
}

// ---------------------------------------------------------------------------
// The self test
//
// Electron can run the renderer, ask it for a picture and hand it back, which
// means the whole shipping path can be checked without a human watching:
//
//   npm run selftest
//
// It asserts the same thing wasm/verify.sh does, one layer further out: after
// 300 frames the CPU must have executed exactly as many cycles as the native
// build did. Then it writes a PNG so the picture can be looked at, because a
// cycle count cannot tell you that the screen is black.
// ---------------------------------------------------------------------------

async function waitFor(check: () => Promise<boolean>, timeoutMs: number): Promise<boolean> {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
        if (await check()) {
            return true;
        }
        await new Promise((r) => setTimeout(r, 100));
    }
    return false;
}

interface ScriptEvent {
    frame: number;
    button: string;
    pressed: boolean;
}

/**
 * Read "100:START=1,105:START=0".
 *
 * A button change is applied *before* the named frame runs, which is where a
 * real press lands: the game samples the controller port once per frame,
 * during vblank, so what matters is the state when the frame starts.
 *
 * The syntax is deliberately tiny and shared with wasm/headless.mjs, so the
 * same scenario string drives every build being compared.
 */
function parseScript(spec: string | null): ScriptEvent[] {
    if (spec === null || spec.trim() === '') {
        return [];
    }

    return spec.split(',').map((entry) => {
        const text = entry.trim();
        const match = /^(\d+):([A-Z]+)=([01])$/.exec(text);
        if (match === null) {
            throw new Error(
                `--script: cannot read "${text}". Expected frame:BUTTON=0|1, as in 100:START=1`,
            );
        }
        return { frame: Number(match[1]), button: match[2], pressed: match[3] === '1' };
    });
}

/** The frames a test wants hashed. Defaults match the PPM set the native and
 *  wasm tools write, so all three can be compared frame for frame. */
function parseSnapshots(spec: string | null, frames: number): number[] {
    if (spec === null || spec.trim() === '') {
        return [1, 5, 30, frames];
    }
    return spec
        .split(',')
        .map((entry) => Number(entry.trim()))
        .filter((frame) => Number.isInteger(frame) && frame >= 1 && frame <= frames);
}

async function runSelfTest(window: BrowserWindow, frames: number, dumpPath: string | null): Promise<void> {
    const becameReady = await waitFor(
        () => window.webContents.executeJavaScript('Boolean(window.__fc && window.__fc.ready)'),
        30_000,
    );

    if (!becameReady) {
        console.error('selftest: the renderer never reported ready');
        exitNow(1);
        return;
    }

    let plan;
    try {
        plan = {
            frames,
            snapshots: parseSnapshots(options.snapshots, frames),
            script: parseScript(options.script),
            // Halfway, which is generally not one of the snapshot frames. The
            // round trip writes a real file through the real IPC path, so an
            // Electron run that passes has also proved save states work in the
            // application and not only in the core.
            roundtrip: frames >= 2 ? { frame: (frames >> 1) + 1, slot: 1 } : null,
            // Ten snapshots is twenty frames; long enough that a ring that
            // loses its place will have gone somewhere obviously wrong.
            rewindSteps: frames >= 60 ? 10 : 0,
        };
    } catch (error) {
        console.error((error as Error).message);
        exitNow(2);
        return;
    }

    const stats = await window.webContents.executeJavaScript(
        `window.__fc.selftest(${JSON.stringify(plan)})`,
    );

    console.log(`frames run     : ${frames}`);
    console.log(`frame counter  : ${stats.frameCount}`);
    console.log(`cpu cycles     : ${stats.totalCycles}`);
    console.log(`cpu pc         : $${stats.cpuPc.toString(16).toUpperCase().padStart(4, '0')}`);
    console.log(`sample rate    : ${stats.sampleRate}`);
    console.log(`audio peak     : ${stats.audioPeak.toFixed(3)}`);
    console.log(`audio samples  : ${stats.audioSamples}`);
    console.log(`error          : ${stats.error ?? 'none'}`);

    // One line per snapshot, in a shape a shell script can read:
    //
    //   hash <frame> <sha256 of the R,G,B bytes>
    //
    // Byte for byte what a PPM holds after its 15 byte header, so the same
    // number comes out of `tail -c +16 frame.ppm | shasum -a 256`.
    for (const entry of stats.hashes as { frame: number; hash: string }[]) {
        console.log(`hash ${entry.frame} ${entry.hash}`);
    }

    // And the audio, as the sha256 of the raw float32 samples. The same bytes
    // `fc_headless --samples` writes, so this is comparable with a plain
    // `shasum` of that file.
    console.log(`audio ${stats.audioHash}`);

    // The rewind check compares itself: winding back and replaying has to land
    // on the frame the run finished on, so both hashes are already here and
    // one has to equal the other.
    let rewindOk = true;
    const lastHash = (stats.hashes as { frame: number; hash: string }[]).at(-1);
    if (stats.rewindHash !== null && lastHash !== undefined) {
        rewindOk = stats.rewindHash === lastHash.hash;
        console.log(`rewind ${stats.rewindHash}`);
        console.log(`rewind check   : ${rewindOk ? 'lands on the same frame' : 'LOST ITS PLACE'}`);
    }

    // Only when somebody asked for one.
    //
    // capturePage() on a hidden window does not resolve: Chromium has nothing
    // to composite, so the request sits there forever. This was latent for
    // four milestones because every run that cared about the picture also
    // asked for a screenshot, which shows the window -- and every run that did
    // not ask simply never reached this line and never exited.
    if (dumpPath !== null) {
        const image = await window.webContents.capturePage();
        await writeFile(dumpPath, image.toPNG());
        console.log(`wrote          : ${dumpPath}`);
    }

    exitNow(stats.error === null && rewindOk ? 0 : 1);
}

/**
 * Press real keys at the window and check what the emulator heard.
 *
 *   pnpm exec electron . --keytest
 *
 * Everything else in this repository tests the emulator. This is the only
 * check that the *application* passes a keystroke through to it: Chromium
 * delivers a genuine KeyboardEvent to the page, src/renderer/input.ts turns it
 * into a button, and the answer is read back out of the emulator's own input
 * manager.
 *
 * It exists because a unit test of the key map cannot tell you that anybody
 * ever calls it, and because a screenshot cannot show that a key was ignored.
 */
async function runKeyTest(window: BrowserWindow): Promise<void> {
    const becameReady = await waitFor(
        () => window.webContents.executeJavaScript('Boolean(window.__fc && window.__fc.ready)'),
        30_000,
    );

    if (!becameReady) {
        console.error('keytest: the renderer never reported ready');
        exitNow(1);
        return;
    }

    window.focus();

    // Record what Chromium actually delivers, so a failure says *why* rather
    // than only that something is wrong. `event.code` is the physical key, and
    // it is what input.ts binds to.
    await window.webContents.executeJavaScript(`
        window.__codes = [];
        window.addEventListener('keydown', (event) => window.__codes.push('down:' + event.code));
        window.addEventListener('keyup', (event) => window.__codes.push('up:' + event.code));
        true;
    `);

    const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

    // Electron names keys the way Chromium's keycodes do, which is neither
    // `event.code` nor `event.key`.
    const presses: { keyCode: string; expect: string[] }[] = [
        { keyCode: 'z', expect: ['B'] },
        { keyCode: 'x', expect: ['A'] },
        { keyCode: 'Left', expect: ['LEFT'] },
        { keyCode: 'Up', expect: ['UP'] },
        { keyCode: 'Return', expect: ['START'] },
        { keyCode: 'Tab', expect: ['SELECT'] },
    ];

    console.log('key            held after keyDown           expected   result');
    let failures = 0;

    for (const press of presses) {
        window.webContents.sendInputEvent({ type: 'keyDown', keyCode: press.keyCode });
        await sleep(40);

        const held = await window.webContents.executeJavaScript('window.__fc.held()') as string[];
        const ok = held.length === press.expect.length
            && press.expect.every((button) => held.includes(button));
        if (!ok) {
            failures += 1;
        }

        console.log(
            `${press.keyCode.padEnd(14)} ${(held.join(' ') || '-').padEnd(29)} `
            + `${press.expect.join(' ').padEnd(10)} ${ok ? 'ok' : 'MISMATCH'}`,
        );

        window.webContents.sendInputEvent({ type: 'keyUp', keyCode: press.keyCode });
        await sleep(40);
    }

    // A key that was pressed and released must leave nothing behind, or the
    // game keeps walking after the player stopped.
    const stillHeld = await window.webContents.executeJavaScript('window.__fc.held()') as string[];
    if (stillHeld.length > 0) {
        console.error(`keytest: still held after keyUp: ${stillHeld.join(' ')}`);
        failures += 1;
    }

    const codes = await window.webContents.executeJavaScript('window.__codes') as string[];
    console.log(`codes seen     : ${codes.join(', ') || '(none)'}`);

    exitNow(failures === 0 ? 0 : 1);
}

/**
 * Run the application in real time, then look at the audio ring.
 *
 *   pnpm exec electron . --audiotest 6
 *
 * This is the only test in the project that is allowed to take real time, and
 * it has to: the thing under test is whether the emulator keeps up with a
 * sound card that is not in a hurry. Every other check runs frames as fast as
 * it can and never touches the ring.
 *
 * What it can prove:
 *
 *   the page is cross origin isolated, so SharedArrayBuffer exists
 *   the AudioWorklet loaded and the context is running
 *   samples are actually crossing to the audio thread
 *   the ring stayed near its target, so the rate control is holding
 *   nothing underran and nothing was dropped
 *   the emulator still managed 60.0988 frames a second while doing it
 *
 * What it cannot prove: that any of it sounds right. That still needs ears.
 */
async function runAudioTest(window: BrowserWindow, seconds: number): Promise<void> {
    const becameReady = await waitFor(
        () => window.webContents.executeJavaScript('Boolean(window.__fc && window.__fc.ready)'),
        30_000,
    );

    if (!becameReady) {
        console.error('audiotest: the renderer never reported ready');
        exitNow(1);
        return;
    }

    const framesBefore = await window.webContents.executeJavaScript('window.__fc.frames()') as number;
    const startedAt = Date.now();

    // Before anything else, say where the page came from and what the browser
    // made of it. Cross origin isolation is the kind of thing that is either
    // true or silently false, and knowing which -- along with the origin the
    // page was actually served from -- turns a mystery into a header to go
    // and look at.
    const probe = await window.webContents.executeJavaScript(`({
        href: location.href,
        isolated: crossOriginIsolated,
        sharedArrayBuffer: typeof SharedArrayBuffer,
    })`) as { href: string; isolated: boolean; sharedArrayBuffer: string };
    console.log(`page           : ${probe.href}`);
    console.log(`isolated       : ${probe.isolated}  (SharedArrayBuffer is ${probe.sharedArrayBuffer})`);

    window.focus();
    const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

    // Press Start once a second, the way a player would. The title screen is
    // silent, so without this the ring still fills but with zeros, which
    // proves the plumbing and nothing about the samples. Pressing repeatedly
    // also removes the question of whether the very first press arrived
    // before the game was ready to notice it.
    interface Snapshot {
        state?: string;
        fill?: number;
        targetFill?: number;
        underruns?: number;
        dropped?: number;
        peak: number;
        error: string | null;
    }

    let audio!: Snapshot;
    for (let second = 1; second <= seconds; second += 1) {
        window.webContents.sendInputEvent({ type: 'keyDown', keyCode: 'Return' });
        await sleep(120);
        window.webContents.sendInputEvent({ type: 'keyUp', keyCode: 'Return' });
        await sleep(880);

        audio = await window.webContents.executeJavaScript('window.__fc.audio()') as Snapshot;
        console.log(`  t+${second}s  peak ${audio.peak.toFixed(3)}  fill ${audio.fill ?? 0}`
            + `  underruns ${audio.underruns ?? 0}  dropped ${audio.dropped ?? 0}`);
    }

    const elapsed = (Date.now() - startedAt) / 1000;
    const framesAfter = await window.webContents.executeJavaScript('window.__fc.frames()') as number;

    const ran = framesAfter - framesBefore;
    const fps = ran / elapsed;

    // 60.0988 is the console's rate. The window is wide because this machine is
    // doing other things, but an emulator that fell to half speed would show
    // up here immediately.
    const keptUp = fps > 55;
    const ok = audio.error === null
        && audio.state === 'running'
        && audio.underruns === 0
        && audio.dropped === 0
        && audio.peak > 0.01
        && keptUp;

    console.log(`seconds        : ${elapsed.toFixed(1)}`);
    console.log(`frames         : ${ran}  (${fps.toFixed(2)} fps, want 60.10)`);
    console.log(`audio state    : ${audio.state ?? 'unavailable'}`);
    console.log(`audio error    : ${audio.error ?? 'none'}`);
    console.log(`audio peak     : ${audio.peak.toFixed(3)}  (0.000 means the game was silent)`);
    console.log(`ring fill      : ${audio.fill ?? 0} (target ${audio.targetFill ?? 0})`);
    console.log(`underruns      : ${audio.underruns ?? 0}`);
    console.log(`dropped        : ${audio.dropped ?? 0}`);
    console.log(`result         : ${ok ? 'ok' : 'FAILED'}`);

    exitNow(ok ? 0 : 1);
}

/**
 * Resize the window and check how the picture fitted.
 *
 *   pnpm exec electron . --layout
 *
 * "Adaptive and centred" is a property of a picture, and there is no hash for
 * one. What there is: the canvas's rectangle, its container's rectangle, and
 * devicePixelRatio -- all of which the renderer will hand over. From those,
 * everything that matters is arithmetic:
 *
 *   the picture is inside its box
 *   the gaps on the four sides are equal, within a pixel of rounding
 *   the aspect ratio is exactly 256:240, so nothing is stretched
 *   the scale is a whole number of DEVICE pixels per game pixel
 *
 * The last one is the one worth having a test for. A scale that is a whole
 * number of CSS pixels is still blurry on a display with a fractional ratio,
 * and that is not something anybody notices until they wonder why the picture
 * looks soft on one monitor and not another.
 */
async function runLayoutTest(window: BrowserWindow): Promise<void> {
    const becameReady = await waitFor(
        () => window.webContents.executeJavaScript('Boolean(window.__fc && window.__fc.ready)'),
        30_000,
    );

    if (!becameReady) {
        console.error('--layout: the renderer never reported ready');
        exitNow(1);
        return;
    }

    const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

    const sizes: [number, number][] = [
        [1280, 800], [1024, 768], [800, 700], [700, 900],
        [520, 460], [1600, 1000], [420, 380], [300, 260],
    ];

    console.log('window        stage        canvas       scale  ratio  centred  fits');
    let failures = 0;

    for (const [width, height] of sizes) {
        window.setSize(width, height);
        await sleep(200);

        const info = await window.webContents.executeJavaScript(`(() => {
            const canvas = document.querySelector('canvas.screen');
            const stage = document.querySelector('.stage');
            if (canvas === null || stage === null) return null;
            const c = canvas.getBoundingClientRect();
            const s = stage.getBoundingClientRect();
            const dpr = window.devicePixelRatio || 1;
            return {
                dpr,
                stage: [s.width, s.height],
                canvas: [c.width, c.height],
                gaps: [c.left - s.left, s.right - c.right, c.top - s.top, s.bottom - c.bottom],
            };
        })()`) as {
            dpr: number;
            stage: [number, number];
            canvas: [number, number];
            gaps: [number, number, number, number];
        } | null;

        if (info === null) {
            console.error(`  ${width}x${height}: the page has no canvas`);
            failures += 1;
            continue;
        }

        const [canvasWidth, canvasHeight] = info.canvas;
        const deviceScale = (canvasWidth * info.dpr) / 256;

        // Fractional is allowed only when the window is too small to hold the
        // picture at 1:1; then there is nothing to round to.
        const scaleIsWhole = Math.abs(deviceScale - Math.round(deviceScale)) < 0.001
            || deviceScale < 1;
        const ratio = canvasWidth / canvasHeight;
        const ratioOk = Math.abs(ratio - 256 / 240) < 0.01;
        // Centred means the two horizontal gaps match each other and the two
        // vertical ones match each other. Not that all four match: that would
        // only be true of a square picture.
        const [left, right, top, bottom] = info.gaps;
        const horizontal = Math.abs(left - right) <= 1;
        const vertical = Math.abs(top - bottom) <= 1;
        const centred = horizontal && vertical;
        const fits = canvasWidth <= info.stage[0] + 1 && canvasHeight <= info.stage[1] + 1;

        const ok = scaleIsWhole && ratioOk && centred && fits;
        if (!ok) {
            failures += 1;
        }

        console.log(
            `${String(width).padStart(4)}x${String(height).padEnd(4)}  `
            + `${Math.round(info.stage[0])}x${Math.round(info.stage[1])}`.padEnd(12)
            + ` ${Math.round(canvasWidth)}x${Math.round(canvasHeight)}`.padEnd(13)
            + `${deviceScale.toFixed(2)}x`.padEnd(6)
            + ` ${ratio.toFixed(3)}`
            + `  ${centred ? 'yes    ' : 'NO     '}`
            + ` ${fits ? 'yes' : 'NO '}`
            + (ok ? '' : '   <- FAILED'),
        );
    }

    console.log();
    console.log(`devicePixelRatio on this display: ${await window.webContents.executeJavaScript('window.devicePixelRatio')}`);
    console.log(failures === 0 ? 'PASSED' : `FAILED: ${failures} of ${sizes.length} sizes`);

    exitNow(failures === 0 ? 0 : 1);
}

/**
 * Print the library, through the real path the renderer uses.
 *
 *   pnpm exec electron . --list
 *
 * Every other check in this project ends in a hash, because every other part of
 * it produces something measurable. A game list does not: it is a screen, and
 * the only honest way to test one without looking at it is to ask the renderer
 * what it was given and read the answer.
 *
 * What it does *not* do is look at the page. It used to -- counting cards,
 * measuring the toolbar, clicking through the screenshot preview -- and that
 * was removed on purpose: those checks tested a layout the author had guessed
 * at, cost more to write than the feature, and could not fail in the one case
 * that mattered (a check that died reported success). See AGENTS.md §2.5.
 */
async function runListGames(window: BrowserWindow): Promise<void> {
    const becameReady = await waitFor(
        () => window.webContents.executeJavaScript('Boolean(window.fc)'),
        30_000,
    );

    if (!becameReady) {
        console.error('--list: the preload never ran');
        exitNow(1);
        return;
    }

    const state = await window.webContents.executeJavaScript(
        'window.fc.library()',
    ) as {
        library: {
            directory: string;
            database: string;
            games: {
                name: string; size: number; pinned: boolean;
                playCount: number; playSeconds: number; tags: string[];
                lastPlayedAt: number; cover: string | null;
                screenshots: number;
            }[];
        };
        screenshots: { game: string; file: string; isCover: boolean }[];
    };
    const library = state.library;

    console.log(`directory      : ${library.directory}`);
    console.log(`database       : ${library.database}`);
    console.log(`games          : ${library.games.length}`);

    for (const game of library.games) {
        const when = game.lastPlayedAt === 0 ? 'never' : new Date(game.lastPlayedAt).toLocaleString();
        const cover = game.cover === null ? '   -  ' : '  cover';
        const played = game.playSeconds === 0 ? '-' : `${Math.round(game.playSeconds / 60)}m`;
        const tags = game.tags.length === 0 ? '-' : game.tags.join(',');
        console.log(
            `  ${game.pinned ? '*' : ' '} ${game.name.padEnd(36)} `
            + `${String(Math.round(game.size / 1024)).padStart(5)}K  `
            + `${String(game.playCount).padStart(3)}x  `
            + `${played.padStart(5)}  `
            + `${tags.padEnd(18)} `
            + `${String(game.screenshots).padStart(2)} shot${cover}  ${when}`,
        );
    }

    console.log(`screenshots    : ${state.screenshots.length}`);
    for (const shot of state.screenshots) {
        console.log(`  ${shot.isCover ? '*' : ' '} ${shot.file.padEnd(38)} ${shot.game}`);
    }

    // And that the renderer cannot read what it was not given.
    const refused = await window.webContents.executeJavaScript(
        "window.fc.readRom('/etc/passwd')",
    );
    console.log(`outside the folder: ${refused === null ? 'refused' : 'READ -- that is a bug'}`);

    exitNow(refused === null ? 0 : 1);
}

/**
 * Leave, now.
 *
 * `exitNow()` is not enough once the renderer has called
 * `navigator.getGamepads()`. Chromium starts a HID poller the first time that
 * is called, and on macOS that is enough to make Electron's own exit path
 * hang: the process stays alive, waiting, with nothing left to wait for.
 *
 * This was found by a test that stopped exiting rather than by a person
 * quitting, which is the good way round -- but it would have hit anybody who
 * closed the window while a game was running, so the window close goes the
 * same way.
 *
 * `process.exit` skips Electron's shut down handlers. There are none here: save
 * states are written when the player asks for them, not on the way out, so
 * there is nothing to flush and nothing to lose.
 */
/** Whether an exit is already under way.
 *
 * `exitNow` destroys the windows on its way out, which fires
 * `window-all-closed` -- and that handler treats a window going away during a
 * check as a failure. Without this flag every *passing* check would exit 1,
 * which is the other way a test lies. */
let exiting = false;

function exitNow(code: number): void {
    if (exiting) {
        return;
    }
    exiting = true;

    // The native gamepad helper is our own child process, so it has to go
    // first: it is holding a HID connection, and a process that is not
    // reaped is a process that can hold the whole application open. (This is
    // the mirror image of the problem the helper was written to solve -- see
    // the note on exitNow's history below.)
    gamepad?.stop();
    gamepad = null;

    // Close the pages first.
    //
    // Chromium's gamepad service is started by the page -- by calling
    // navigator.getGamepads(), or even by adding a gamepadconnected listener --
    // and on macOS it holds a HID connection open in the browser process. Going
    // straight to process.exit() while that is live leaves the process in an
    // uninterruptible wait that never returns: the exit is never delivered.
    //
    // Destroying the windows first tears the page down, which is what tells
    // the service there is nobody left to report to. Then the exit works.
    for (const window of BrowserWindow.getAllWindows()) {
        window.destroy();
    }
    setTimeout(() => process.exit(code), 250);
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

void app.whenReady().then(() => {
    bootLog('main', 'app ready');
    registerAppProtocol();
    registerIpc();
    bootLog('main', 'protocol + ipc registered');

    // The first line of the gamepad diagnosis, and the one that says whether
    // there is anything to diagnose: without this the renderer's own
    // `gamepad:` lines are the only sign, and no sign at all is ambiguous
    // between "the source never started" and "the page never started".
    if (USE_NATIVE_GAMEPAD) {
        // The whole reason the native helper exists: this line and the ones it
        // logs are the difference between a pad that is switched on and one
        // that is not, and neither Chromium nor the browser is involved.
        gamepad = new NativeGamepad({
            binary: GAMEPAD_BINARY,
            onReading: (reading) => {
                // Every window, because there is normally one and "the pad is
                // on the desk" is true of all of them. A reading that arrives
                // before a window exists is not lost: the renderer asks for
                // the current one when it starts.
                for (const window of BrowserWindow.getAllWindows()) {
                    window.webContents.send(IpcChannel.GamepadState, reading);
                }
            },
            onLog: (message) => console.log(message),
        });
        gamepad.start();
    } else if (SCRIPTED_RUN) {
        console.log('gamepad: disabled for a scripted run -- --selftest and --keytest need exactly the input they scripted');
    } else if (options.gamepad) {
        console.log('gamepad: browser path enabled by --gamepad; on macOS this may stop the app quitting');
    } else if (options.noGamepad) {
        console.log('gamepad: switched off by --no-gamepad');
    } else if (!NATIVE_GAMEPAD_AVAILABLE) {
        console.log('gamepad: no native helper; run pnpm run build:native to enable it');
    }

    bootLog(
        'main',
        'gamepad decided',
        USE_NATIVE_GAMEPAD ? `native ${GAMEPAD_BINARY}`
            : SCRIPTED_RUN ? 'off (scripted run)'
                : options.noGamepad ? 'off (--no-gamepad)'
                    : options.gamepad
                        ? (options.browserGamepad ? 'browser (--browser-gamepad)' : 'browser (--gamepad)')
                        : 'off (no native helper built)',
    );

    const window = createWindow();

    if (options.selftestFrames > 0) {
        window.webContents.once('did-finish-load', () => {
            void runSelfTest(window, options.selftestFrames, options.dumpPath);
        });
    }

    if (options.keytest) {
        window.webContents.once('did-finish-load', () => {
            void runKeyTest(window);
        });
    }

    if (options.listGames) {
        window.webContents.once('did-finish-load', () => {
            void runListGames(window).catch((error: unknown) => {
                console.error(`${error instanceof Error ? error.message : String(error)}`);
                exitNow(1);
            });
        });
    }

    if (options.layoutTest) {
        window.webContents.once('did-finish-load', () => {
            void runLayoutTest(window).catch((error: unknown) => {
                console.error(`${error instanceof Error ? error.message : String(error)}`);
                exitNow(1);
            });
        });
    }

    if (options.quitAfterSeconds > 0) {
        setTimeout(() => app.quit(), options.quitAfterSeconds * 1000);
    }

    if (options.audioTestSeconds > 0) {
        window.webContents.once('did-finish-load', () => {
            void runAudioTest(window, options.audioTestSeconds).catch((error: unknown) => {
                console.error(`audiotest: ${error instanceof Error ? error.message : String(error)}`);
                exitNow(1);
            });
        });
    }

    app.on('activate', () => {
        if (BrowserWindow.getAllWindows().length === 0) {
            createWindow();
        }
    });

    bootLog('main', 'app ready handler finished -- window created, waiting on the renderer');
});

app.on('window-all-closed', () => {
    // On macOS a window closing normally leaves the app running, but this app
    // is one window and nothing else, so quitting is the less surprising
    // behaviour even there. See exitNow() for why this is not app.quit().
    bootLog('main', 'window-all-closed');

    // Except during a check. Every check ends in `exitNow` with a verdict, so
    // a window that goes away while one is running is a check that died --
    // a renderer crash, an exception in the script it was driving -- and
    // reporting that as success is the worst thing a test can do. This is not
    // hypothetical: a syntax error in the testing script itself took the
    // window down with it and `--list` exited 0 with nothing checked.
    const checking = options.selftestFrames > 0 || options.keytest
        || options.audioTestSeconds > 0 || options.layoutTest || options.listGames;
    if (checking && !exiting) {
        console.error('the window closed before the check finished: FAILED');
        exitNow(1);
        return;
    }
    exitNow(0);
});