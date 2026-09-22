// ---------------------------------------------------------------------------
// The game library, as a database.
//
// Until now the library *was* the folder listing: whatever .nes files were in
// one directory, with the times they were last played kept in a JSON file
// beside the save states. That is enough to list games and nothing else --
// there is nowhere to put a favourite, nowhere to put a play count, and no way
// to tell a file that was renamed from a file that was replaced.
//
// So the model moves into SQLite. Three things make that worth the trouble:
//
//   1. The database describes a *game*, not a file name. A pinned game can be
//      remembered, and a rename can be followed rather than thrown away.
//   2. The directory is still the truth about what exists. Every listing
//      rescans it and reconciles, so dropping a .nes in with the Finder works
//      exactly as it always did and deleting one makes it disappear from the
//      list. The database is a model *of* the folder, not a replacement for it.
//   3. It lives in the library folder, so a library is one folder: the ROMs,
//      their metadata, and nothing else to keep in sync. Move the folder and
//      the library moves with it.
//
// SQLite here is Node's own `node:sqlite` -- the same C library everything
// else links, compiled into the runtime. It is synchronous, which suits this:
// the statements run on the main process between IPC calls, the table has tens
// of rows, and a worker thread would add a message protocol to buy nothing
// measurable. The real prize is that there is no native module to rebuild
// against Electron's ABI, which is the sort of thing that works until somebody
// clones the repository on a Tuesday.
//
// This file deliberately knows nothing about Electron. It is handed a folder
// and it models that folder, which is what makes it testable from plain Node
// (see test/library.test.mjs).
// ---------------------------------------------------------------------------

import {
    copyFileSync, existsSync, mkdirSync, readdirSync, statSync, unlinkSync, writeFileSync,
} from 'node:fs';
import { randomBytes } from 'node:crypto';
import { basename, extname, join, resolve, sep } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

import type { GameEntry, Screenshot } from '../shared/api';

/** The database file. Named so, and placed inside the library folder, because
 *  a library is one folder that can be moved, copied or deleted whole. */
export const DATABASE_FILE = 'library.sqlite';

/**
 * Where screenshots are kept, relative to the library folder.
 *
 * A directory of its own, beside the ROMs and the database, because it is a
 * different kind of thing: the ROMs are what the player brought, the database
 * is what the application knows, and these are what it made. One folder still,
 * so a library can be copied between machines with a drag.
 */
export const SCREENSHOT_DIRECTORY = 'screenshots';

/** What this build understands. A database with a higher number was written by
 *  a newer build, and guessing at its columns would corrupt it. */
const SCHEMA_VERSION = 3;

/**
 * Schema 1: the games, and what the database knows about them.
 *
 * Kept separate from the screenshots below so that upgrading a version 1
 * library is a `CREATE TABLE`, not a copy of every row through a new table.
 * The alternative -- one SCHEMA string and a version bump -- would be simpler
 * to read and would throw away every pin and play count the first time it ran.
 */
const GAMES_SCHEMA = `
CREATE TABLE IF NOT EXISTS games (
    id             INTEGER PRIMARY KEY,
    -- The file name relative to the library folder. Relative, not absolute,
    -- so that moving the library does not invalidate every row.
    file           TEXT    NOT NULL UNIQUE,
    title          TEXT    NOT NULL,
    size           INTEGER NOT NULL,
    mtime_ms       INTEGER NOT NULL,
    added_at       INTEGER NOT NULL,
    last_played_at INTEGER NOT NULL DEFAULT 0,
    play_count     INTEGER NOT NULL DEFAULT 0,
    pinned         INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1))
);

CREATE INDEX IF NOT EXISTS games_order
    ON games (pinned DESC, last_played_at DESC, title COLLATE NOCASE);
`;

/**
 * Schema 2: screenshots, and which of them is the cover.
 *
 * The cover is a flag on one of the game's screenshots rather than a
 * `cover_id` column on `games`, and that choice does a lot of work:
 *
 *   * there is one picture, stored once, so setting a cover cannot leave a
 *     stale copy of the old one behind;
 *   * deleting the cover is not a special case -- the row goes, and the game
 *     either has another screenshot to promote or it does not;
 *   * `games` never changes shape, which is why this migration is additive.
 *
 * `file` is relative to the library folder for the same reason the games'
 * rows are: a library that is moved is still a library.
 *
 * ON DELETE CASCADE so that a game that leaves takes its pictures with it, but
 * the deletion in scan() is explicit as well, because a cascade cannot delete
 * the files on disk and the rows must never outlive them.
 */
const SCREENSHOTS_SCHEMA = `
CREATE TABLE IF NOT EXISTS screenshots (
    id         INTEGER PRIMARY KEY,
    game_id    INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    file       TEXT    NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    is_cover   INTEGER NOT NULL DEFAULT 0 CHECK (is_cover IN (0, 1))
);

CREATE INDEX IF NOT EXISTS screenshots_by_game
    ON screenshots (game_id, is_cover DESC, created_at DESC);

CREATE INDEX IF NOT EXISTS screenshots_by_date
    ON screenshots (created_at DESC);
`;

/**
 * Schema 3: the tags, and the total play time.
 *
 * A tag is a word and a row rather than a column on `games`, because a game
 * has as many as the player cares to give it, and two games labelled "RPG"
 * should share one word rather than two spellings of it. `COLLATE NOCASE` on
 * the name is what makes those one word: "RPG" and "rpg" cannot both exist,
 * so the table cannot grow a second copy of a tag when somebody types it in
 * differently on a different day. The colour is deliberately *not* stored --
 * it is derived from the word (see src/renderer/libraryView.ts), so a tag is a
 * word everywhere and a colour only where it is drawn, and there is no colour
 * picker to add one.
 *
 * `play_seconds` is a column on `games` for the opposite reason: it is one
 * number per game, and nothing ever reads it without the game it belongs to.
 *
 * ON DELETE CASCADE on both columns of `game_tags`, so a deleted game takes
 * its labels with it. A tag left pointing at nothing is deleted by setTags(),
 * which is the only code that can make one unused.
 */
const TAGS_SCHEMA = `
CREATE TABLE IF NOT EXISTS tags (
    id   INTEGER PRIMARY KEY,
    name TEXT    NOT NULL UNIQUE COLLATE NOCASE
);

CREATE TABLE IF NOT EXISTS game_tags (
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags(id)   ON DELETE CASCADE,
    PRIMARY KEY (game_id, tag_id)
);

CREATE INDEX IF NOT EXISTS game_tags_by_tag ON game_tags (tag_id);
`;

/**
 * How a library at each version becomes the next one.
 *
 * `MIGRATIONS[n]` is what a library whose `user_version` is `n` needs to run
 * to reach `n + 1`: a fresh folder (0) runs all of them in order, and an
 * existing library runs only the ones it is missing. Written as steps rather
 * than as one create-everything schema because the old shapes have to keep
 * working -- the ALTER of the last step is all an existing library gets, and
 * re-creating the `games` table to add one column would throw away every pin
 * and play count in it.
 */
const MIGRATIONS: readonly (readonly string[])[] = [
    // 0 -> 1: the games.
    [GAMES_SCHEMA],
    // 1 -> 2: the screenshots.
    [SCREENSHOTS_SCHEMA],
    // 2 -> 3: the tags, and the play time.
    [
        'ALTER TABLE games ADD COLUMN play_seconds INTEGER NOT NULL DEFAULT 0',
        TAGS_SCHEMA,
    ],
];

/** A row, as SQLite hands it over: snake_case, integers for booleans. */
interface Row {
    id: number;
    file: string;
    title: string;
    size: number;
    mtime_ms: number;
    added_at: number;
    last_played_at: number;
    play_count: number;
    play_seconds: number;
    pinned: number;
}

/** What a file on disk looks like, which is all the reconcile needs. */
interface DiskFile {
    size: number;
    mtime: number;
}

/** A game row with the two things that are derived from the screenshots. */
interface GameRow {
    file: string;
    title: string;
    size: number;
    added_at: number;
    last_played_at: number;
    play_count: number;
    play_seconds: number;
    pinned: number;
    cover: string | null;
    screenshots: number;
}

/** One row of `game_tags` joined to the word it points at. */
interface GameTagRow {
    game_file: string;
    name: string;
}

/** A screenshot row, and the game it belongs to. */
interface ScreenshotRow {
    id: number;
    file: string;
    created_at: number;
    is_cover: number;
    game_file: string;
    game_title: string;
}

/** True when `candidate` is the root itself or something inside it. Resolved
 *  first, so that `..` is collapsed before the comparison rather than after --
 *  without that, a path like `/library/../secrets` would pass. */
export function isInside(root: string, candidate: string): boolean {
    const base = resolve(root);
    const target = resolve(candidate);
    return target === base || target.startsWith(base + sep);
}

/** The game's title: the file name without its extension. A .nes contains no
 *  title, no publisher and no date that can be read without running it, and
 *  this is the closest thing to a name that can be had for free. */
function titleOf(file: string): string {
    return basename(file, extname(file));
}

/**
 * The extensions this emulator can open.
 *
 * Kept in step with the renderer's core table (electron/src/renderer/systems.ts)
 * and the shared list `main/index.ts` filters its open panel by: a file the
 * library lists but the renderer has no core for would be a game that appears
 * and then fails. It is written out here rather than imported because this file
 * is executed directly by Node in the tests, where a cross-file import without
 * an extension does not resolve.
 */
const ROM_EXTENSIONS = ['nes', 'gba', 'gb', 'gbc'];

function isRom(name: string): boolean {
    const lower = name.toLowerCase();
    return ROM_EXTENSIONS.some((extension) => lower.endsWith(`.${extension}`));
}

/** Long enough to be a word, short enough to fit on a card without wrapping. */
export const MAX_TAG_LENGTH = 24;

/**
 * The tags a list of typed words amounts to.
 *
 * Trimmed, emptied out, and deduplicated case-insensitively -- "RPG" and
 * "rpg" are one tag, and the first spelling seen is the one that is kept, so
 * a word does not change case under the player. Done here rather than at the
 * input, because it has to hold for every caller: the renderer sends the
 * whole list, the tests send whatever they like, and the database's unique
 * index is the last line of defence rather than the first.
 */
export function normaliseTags(tags: readonly string[]): string[] {
    const seen = new Set<string>();
    const kept: string[] = [];

    for (const raw of tags) {
        if (typeof raw !== 'string') {
            continue;
        }
        const name = raw.trim().slice(0, MAX_TAG_LENGTH);
        if (name === '') {
            continue;
        }
        const key = name.toLocaleLowerCase();
        if (seen.has(key)) {
            continue;
        }
        seen.add(key);
        kept.push(name);
    }

    return kept;
}

/** The eight bytes every PNG starts with. */
const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/**
 * Whether these bytes are a PNG.
 *
 * Not a decode and not a substitute for one: it is the check that stops this
 * folder filling up with things that are not pictures, and the check that
 * makes the `screenshots/*.png` rule the protocol handler enforces true.
 */
function isPng(bytes: Uint8Array): boolean {
    if (bytes.length < PNG_SIGNATURE.length) {
        return false;
    }
    return PNG_SIGNATURE.every((byte, index) => bytes[index] === byte);
}

/**
 * The ROMs a set of dropped or chosen paths amount to.
 *
 * A drag from the Finder can be a file or a folder, and a folder is what
 * somebody means when they drop a directory of games on the window, so it is
 * expanded a level. Recursion is deliberately not offered: a folder of ROMs is
 * a folder of ROMs, but a folder containing a folder of ROMs is usually
 * somebody's Documents directory arriving by accident, and copying that in
 * would be a surprise.
 *
 * Nothing is reported about what was skipped. The renderer asked for games;
 * whether a path was a .zip, a directory with no ROMs in it, or a file that
 * had already been deleted is not interesting enough to interrupt with.
 */
export function collectGames(paths: readonly string[]): string[] {
    const found: string[] = [];

    for (const path of paths) {
        if (typeof path !== 'string' || path === '') {
            continue;
        }
        try {
            const info = statSync(path);
            if (info.isDirectory()) {
                for (const name of readdirSync(path).sort()) {
                    if (!isRom(name)) {
                        continue;
                    }
                    try {
                        const child = join(path, name);
                        if (statSync(child).isFile()) {
                            found.push(child);
                        }
                    } catch {
                        // Vanished between listing and stat. Ignore.
                    }
                }
            } else if (info.isFile() && isRom(path)) {
                found.push(path);
            }
        } catch {
            // A path that is not there is not a game.
        }
    }

    return found;
}

/**
 * The library folder, modelled.
 *
 * One open handle per library, for the lifetime of the process. `open()` is
 * synchronous and cheap; `scan()` is the call that does the work, and it is
 * the only one a caller has to think about, because it is the one that reads
 * the folder.
 */
export class GameLibrary {
    /** The folder the ROMs and the database live in. Absolute. */
    readonly root: string;
    /** The .sqlite file inside it. */
    readonly databasePath: string;

    readonly #db: DatabaseSync;

    private constructor(root: string, databasePath: string, db: DatabaseSync) {
        this.root = root;
        this.databasePath = databasePath;
        this.#db = db;
    }

    /**
     * Open -- or create -- the library in `root`.
     *
     * Creating it is the common case: a player points the application at an
     * empty folder, or at a folder full of ROMs it has never seen, and both
     * of those should simply work rather than reporting a missing file.
     */
    static open(root: string): GameLibrary {
        const absolute = resolve(root);
        mkdirSync(absolute, { recursive: true });

        const databasePath = join(absolute, DATABASE_FILE);
        const db = new DatabaseSync(databasePath);

        // The cascade above is only a backstop -- scan() deletes explicitly,
        // because a cascade cannot delete the PNGs. This makes it a backstop
        // that actually fires rather than one SQLite ignores by default.
        db.exec('PRAGMA foreign_keys = ON');

        const version = (db.prepare('PRAGMA user_version').get() as { user_version: number })
            .user_version;

        if (version > SCHEMA_VERSION) {
            db.close();
            throw new Error(
                `${databasePath} was written by a newer version of Classic Game Box `
                + `(schema ${version}, this build understands ${SCHEMA_VERSION})`,
            );
        }

        for (let from = version; from < SCHEMA_VERSION; from += 1) {
            for (const statement of MIGRATIONS[from]) {
                db.exec(statement);
            }
        }

        if (version < SCHEMA_VERSION) {
            db.exec(`PRAGMA user_version = ${SCHEMA_VERSION}`);
        }

        return new GameLibrary(absolute, databasePath, db);
    }

    close(): void {
        this.#db.close();
    }

    /**
     * Make the model agree with the folder, and return the model.
     *
     * This is the whole of the synchronisation, and it is deliberately one
     * function: read the directory, add what is new, update what changed,
     * forget what is gone. There is no file watcher and no cached list, so
     * there is no way for the model and the folder to drift apart -- a
     * listing is always a scan.
     *
     * A rename is detected rather than treated as a delete and an insert. A
     * rename leaves the size and the modification time untouched, so a file
     * that vanished and a same-sized, same-timed file that appeared are the
     * same file under a new name, and its pinned flag and play count come
     * with it. That is the first thing the database can do that a directory
     * listing cannot.
     */
    scan(): GameEntry[] {
        const onDisk = this.#readDirectory();

        const known = new Map<string, Row>();
        for (const row of this.#db.prepare('SELECT * FROM games').all() as unknown as Row[]) {
            known.set(row.file, row);
        }

        const insert = this.#db.prepare(
            'INSERT INTO games (file, title, size, mtime_ms, added_at) VALUES (?, ?, ?, ?, ?)',
        );
        const refresh = this.#db.prepare('UPDATE games SET size = ?, mtime_ms = ? WHERE file = ?');
        const reidentify = this.#db.prepare(
            'UPDATE games SET file = ?, title = ?, size = ?, mtime_ms = ? WHERE file = ?',
        );
        const forget = this.#db.prepare('DELETE FROM games WHERE file = ?');

        const appeared = [...onDisk.keys()].filter((file) => !known.has(file));
        const vanished = [...known.values()].filter((row) => !onDisk.has(row.file));

        // The pictures of a game that is gone go with it. Not in the
        // transaction -- an unlink can fail, and a file that survives a
        // rolled-back database is better than a row pointing at nothing.
        const orphaned = vanished.map((row) => this.#screenshotFiles(row.file));

        this.#transaction(() => {
            const now = Date.now();

            for (const file of appeared) {
                const stat = onDisk.get(file) as DiskFile;
                const twin = vanished.findIndex(
                    (row) => row.size === stat.size && row.mtime_ms === stat.mtime,
                );
                if (twin >= 0) {
                    // A rename. The row keeps its id, so every screenshot taken
                    // of this game stays attached to it.
                    const [row] = vanished.splice(twin, 1);
                    orphaned.splice(twin, 1);
                    reidentify.run(file, titleOf(file), stat.size, stat.mtime, row.file);
                } else {
                    insert.run(file, titleOf(file), stat.size, stat.mtime, now);
                }
            }

            for (const file of onDisk.keys()) {
                const stat = onDisk.get(file) as DiskFile;
                const row = known.get(file);
                if (row !== undefined && (row.size !== stat.size || row.mtime_ms !== stat.mtime)) {
                    refresh.run(stat.size, stat.mtime, file);
                }
            }

            for (const row of vanished) {
                this.#db.prepare('DELETE FROM screenshots WHERE game_id = ?').run(row.id);
                forget.run(row.file);
            }
        });

        for (const files of orphaned) {
            this.#unlink(files);
        }

        return this.#select();
    }

    /**
     * Record that a game was run.
     *
     * Silently does nothing for a path outside the library, which is not an
     * error: `--rom` can name any file on the machine, and a game that is not
     * in the library is simply not in the library.
     */
    notePlayed(path: string): void {
        const file = this.#fileOf(path);
        if (file === null) {
            return;
        }
        this.#db.prepare(
            'UPDATE games SET last_played_at = ?, play_count = play_count + 1 WHERE file = ?',
        ).run(Date.now(), file);
    }

    /**
     * Add time to a game's total.
     *
     * The renderer counts emulated frames, because a frame is the only
     * duration this application can honestly measure: a wall clock would
     * count a paused game, a minimised window and a lunch break as play. So
     * the number arrives already counted, and all this does is add it.
     *
     * Whole seconds, floored here, and a path outside the library does
     * nothing -- the same rule as notePlayed, for the same reason: `--rom`
     * can name any file on the machine, and a game that is not in the library
     * is simply not in the library.
     */
    notePlaytime(path: string, seconds: number): void {
        if (!Number.isFinite(seconds) || seconds <= 0) {
            return;
        }
        const file = this.#fileOf(path);
        if (file === null) {
            return;
        }
        this.#db.prepare('UPDATE games SET play_seconds = play_seconds + ? WHERE file = ?')
            .run(Math.floor(seconds), file);
    }

    /**
     * Replace a game's tags with exactly these words.
     *
     * Replace rather than add one and remove one, because the caller has the
     * whole list on screen: two verbs would be two round trips and two
     * chances for the stored list to disagree with the drawn one.
     *
     * A word is created if it is new and shared if it is not, so "RPG" on ten
     * games is one row. The words nobody points at any more are deleted at
     * the end -- they would otherwise be rows the filter chips, which are
     * built from the games, could never show.
     */
    setTags(path: string, tags: readonly string[]): boolean {
        const file = this.#fileOf(path);
        if (file === null) {
            return false;
        }
        const id = this.#gameIdOf(file);
        if (id === null) {
            return false;
        }

        const wanted = normaliseTags(tags);
        const find = this.#db.prepare('SELECT id FROM tags WHERE name = ?');
        const link = this.#db.prepare(
            'INSERT OR IGNORE INTO game_tags (game_id, tag_id) VALUES (?, ?)',
        );

        this.#transaction(() => {
            this.#db.prepare('DELETE FROM game_tags WHERE game_id = ?').run(id);
            for (const name of wanted) {
                // INSERT OR IGNORE against the COLLATE NOCASE unique index: an
                // existing "rpg" is found rather than duplicated, and the
                // SELECT below then uses that row's own spelling.
                this.#db.prepare('INSERT OR IGNORE INTO tags (name) VALUES (?)').run(name);
                const row = find.get(name) as { id: number } | undefined;
                if (row !== undefined) {
                    link.run(id, row.id);
                }
            }
            this.#pruneTags();
        });

        return true;
    }

    /** Pin a game to the top of the library, or unpin it. */
    setPinned(path: string, pinned: boolean): boolean {
        const file = this.#fileOf(path);
        if (file === null) {
            return false;
        }
        const result = this.#db.prepare('UPDATE games SET pinned = ? WHERE file = ?')
            .run(pinned ? 1 : 0, file);
        return Number(result.changes) > 0;
    }

    /**
     * Copy ROMs into the library.
     *
     * Copying, not referencing: a library that pointed at files elsewhere
     * would break the moment one of them moved, and "the library is this
     * folder" is the property the whole design is built on. A source that is
     * already inside the folder is skipped -- copying a file onto itself is
     * not what "add" means -- and a name that is taken is made unique rather
     * than overwritten, because overwriting is how a player loses a game.
     *
     * Returns the file names that were written, for the caller to report.
     */
    add(sources: readonly string[]): string[] {
        const copied: string[] = [];
        for (const source of sources) {
            if (isInside(this.root, source)) {
                continue;
            }
            const target = this.#freeName(basename(source));
            copyFileSync(source, join(this.root, target));
            copied.push(target);
        }
        return copied;
    }

    /**
     * Delete a game: the file from the folder, and the row from the model.
     *
     * There is no "remove from the library but keep the file", because the
     * folder is the library. Anything else would need a second place to hide
     * a game, and the next scan would find it again anyway.
     */
    remove(path: string): boolean {
        const file = this.#fileOf(path);
        if (file === null) {
            return false;
        }

        // The pictures first: a row that outlives its PNG is a broken
        // thumbnail forever, whereas a PNG that outlives its row is a few
        // kilobytes nobody will ever look at.
        const pictures = this.#screenshotFiles(file);

        try {
            unlinkSync(join(this.root, file));
        } catch {
            return false;
        }

        this.#transaction(() => {
            const id = this.#gameIdOf(file);
            if (id !== null) {
                this.#db.prepare('DELETE FROM screenshots WHERE game_id = ?').run(id);
            }
            // The game's tag links go with the row, by cascade; the words
            // they used may now belong to nobody.
            this.#db.prepare('DELETE FROM games WHERE file = ?').run(file);
            this.#pruneTags();
        });

        this.#unlink(pictures);
        return true;
    }

    // -- screenshots --------------------------------------------------------

    /**
     * Every screenshot in the library, newest first.
     *
     * One list, not one per game: the screenshots section shows them all, and
     * a card in the games list only needs the cover and a count, which the
     * game query already carries. Ordered in SQL so that the screen and the
     * database cannot disagree about what "newest" means.
     */
    screenshots(): Screenshot[] {
        return this.#selectScreenshots();
    }

    /**
     * Write a PNG and file it under a game.
     *
     * `asCover` is the difference between the two buttons in the transport:
     *
     *   false -- the first screenshot of a game becomes its cover, because a
     *            game with a picture and no cover is a card showing a coloured
     *            rectangle for no reason. Every one after that is just a
     *            screenshot until somebody says otherwise.
     *   true  -- this picture is the cover, whatever it was before. That is
     *            the 更新封面 button: take where you are now and put it on the
     *            card.
     *
     * Returns the new row, or null when the bytes are not a PNG or the game is
     * not in this library.
     */
    saveScreenshot(gamePath: string, bytes: Uint8Array, asCover = false): Screenshot | null {
        const file = this.#fileOf(gamePath);
        if (file === null) {
            return null;
        }

        // Checked here rather than trusted from the caller: this is the one
        // place a file gets written, and the eight byte signature is what
        // makes a .png a .png. Everything downstream assumes it.
        if (!isPng(bytes)) {
            return null;
        }

        const id = this.#gameIdOf(file);
        if (id === null) {
            return null;
        }

        // The name is opaque on purpose. The directory is storage, and the
        // model is the database: a name that encoded the game and the date
        // would be a second, weaker copy of rows that already hold both --
        // and it would have to be rewritten every time a game was renamed.
        const name = `${Date.now()}-${randomBytes(4).toString('hex')}.png`;
        const relative = `${SCREENSHOT_DIRECTORY}/${name}`;

        mkdirSync(this.#screenshotDirectory(), { recursive: true });
        writeFileSync(join(this.root, relative), bytes);

        const first = (this.#db.prepare(
            'SELECT COUNT(*) AS n FROM screenshots WHERE game_id = ?',
        ).get(id) as { n: number }).n === 0;

        const cover = asCover || first;

        // One transaction, because "exactly one cover per game" is the
        // invariant and clearing the old flag and setting the new one are two
        // writes. A failure between them would leave a game with none, or
        // with two.
        this.#transaction(() => {
            if (cover) {
                this.#db.prepare('UPDATE screenshots SET is_cover = 0 WHERE game_id = ?').run(id);
            }
            this.#db.prepare(
                'INSERT INTO screenshots (game_id, file, created_at, is_cover) VALUES (?, ?, ?, ?)',
            ).run(id, relative, Date.now(), cover ? 1 : 0);
        });

        return this.#selectScreenshots().find((shot) => shot.file === relative) ?? null;
    }

    /**
     * Where a screenshot's PNG is, absolute, or null if it has no business
     * being opened.
     *
     * The renderer names a *row*, never a path -- and this is why. The path is
     * built here and checked here, so a database whose rows were edited (or a
     * renderer that asked for id 12 because it guessed) cannot reach a file
     * outside the library. The caller is the one that shows it in the file
     * browser; everything that decides *whether* it may is in this method.
     */
    screenshotPath(id: number): string | null {
        const row = this.#db.prepare('SELECT file FROM screenshots WHERE id = ?')
            .get(id) as { file: string } | undefined;
        if (row === undefined) {
            return null;
        }

        const absolute = resolve(this.root, row.file);
        if (!isInside(this.root, absolute)) {
            console.error(`refused to reveal ${row.file}: outside the library folder`);
            return null;
        }
        return absolute;
    }

    /**
     * Make one screenshot the cover of its game.
     *
     * Both halves of the change are in one transaction, because "exactly one
     * cover per game" is the invariant and there is a moment in the middle
     * where there would be two.
     */
    setCover(id: number): boolean {
        const row = this.#db.prepare('SELECT game_id FROM screenshots WHERE id = ?')
            .get(id) as { game_id: number } | undefined;
        if (row === undefined) {
            return false;
        }

        this.#transaction(() => {
            this.#db.prepare('UPDATE screenshots SET is_cover = 0 WHERE game_id = ?')
                .run(row.game_id);
            this.#db.prepare('UPDATE screenshots SET is_cover = 1 WHERE id = ?').run(id);
        });
        return true;
    }

    /**
     * Delete one screenshot.
     *
     * If it was the cover, the newest of the game's remaining pictures takes
     * over. Leaving the game without one is the other option, and it is the
     * worse one: the player deleted a picture, not the game's appearance.
     */
    removeScreenshot(id: number): boolean {
        const row = this.#db.prepare('SELECT file, game_id, is_cover FROM screenshots WHERE id = ?')
            .get(id) as { file: string; game_id: number; is_cover: number } | undefined;
        if (row === undefined) {
            return false;
        }

        this.#transaction(() => {
            this.#db.prepare('DELETE FROM screenshots WHERE id = ?').run(id);
            if (row.is_cover === 1) {
                this.#db.prepare(`
                    UPDATE screenshots SET is_cover = 1 WHERE id = (
                        SELECT id FROM screenshots WHERE game_id = ?
                        ORDER BY created_at DESC LIMIT 1
                    )
                `).run(row.game_id);
            }
        });

        this.#unlink([row.file]);
        return true;
    }

    // -- internals ----------------------------------------------------------

    /** The absolute path of the screenshots directory inside this library. */
    #screenshotDirectory(): string {
        return join(this.root, SCREENSHOT_DIRECTORY);
    }

    /** The id of the game whose file name is `file`, or null. */
    #gameIdOf(file: string): number | null {
        const row = this.#db.prepare('SELECT id FROM games WHERE file = ?').get(file) as
            { id: number } | undefined;
        return row?.id ?? null;
    }

    /** The relative paths of every screenshot belonging to a game. */
    #screenshotFiles(gameFile: string): string[] {
        const rows = this.#db.prepare(`
            SELECT s.file FROM screenshots s
            JOIN games g ON g.id = s.game_id
            WHERE g.file = ?
        `).all(gameFile) as unknown as { file: string }[];
        return rows.map((row) => row.file);
    }

    /** Delete files, ignoring the ones that are already gone. */
    #unlink(files: readonly string[]): void {
        for (const file of files) {
            try {
                unlinkSync(join(this.root, file));
            } catch {
                // Already deleted, or never written. Neither is a problem.
            }
        }
    }

    /** Every .nes file in the folder, and nothing else. */
    #readDirectory(): Map<string, DiskFile> {
        const found = new Map<string, DiskFile>();

        let entries;
        try {
            entries = readdirSync(this.root, { withFileTypes: true });
        } catch {
            // A folder that cannot be read has no games in it, which is a
            // better answer than an exception on the way to the screen.
            return found;
        }

        for (const entry of entries) {
            if (!isRom(entry.name)) {
                continue;
            }
            if (!entry.isFile() && !entry.isSymbolicLink()) {
                continue;
            }
            try {
                // stat, not the directory entry: it follows a symlink and it
                // gives the size and time the reconcile needs. A file that
                // vanished between listing and stat is simply not a game any
                // more.
                const info = statSync(join(this.root, entry.name));
                if (!info.isFile()) {
                    continue;
                }
                found.set(entry.name, {
                    size: info.size,
                    // Milliseconds, floored. Sub-millisecond precision is not
                    // in the database's integer columns and is not needed to
                    // tell two edits apart.
                    mtime: Math.floor(info.mtimeMs),
                });
            } catch {
                // Gone. Ignore.
            }
        }

        return found;
    }

    /** `name.nes` if it is free, otherwise `name (2).nes`, `name (3).nes`… */
    #freeName(name: string): string {
        if (!existsSync(join(this.root, name))) {
            return name;
        }
        const stem = basename(name, extname(name));
        const extension = extname(name);
        for (let n = 2; ; n += 1) {
            const candidate = `${stem} (${n})${extension}`;
            if (!existsSync(join(this.root, candidate))) {
                return candidate;
            }
        }
    }

    /** The file name for a path, or null if the path is not in this library. */
    #fileOf(path: string): string | null {
        if (!isInside(this.root, path)) {
            return null;
        }
        const target = resolve(path);
        if (target === this.root) {
            return null;
        }
        return target.slice(this.root.length + 1);
    }

    #select(): GameEntry[] {
        // The cover and the count are subqueries rather than a join and a
        // GROUP BY, because a game has one cover and one count and this says
        // exactly that. The cover is the newest flagged screenshot: `is_cover`
        // should be unique per game, and picking one is how a bug that made it
        // not unique stays a cosmetic problem instead of a duplicated row.
        const rows = this.#db.prepare(`
            SELECT g.file, g.title, g.size, g.added_at, g.last_played_at,
                   g.play_count, g.play_seconds, g.pinned,
                   (SELECT s.file FROM screenshots s
                     WHERE s.game_id = g.id AND s.is_cover = 1
                     ORDER BY s.created_at DESC LIMIT 1) AS cover,
                   (SELECT COUNT(*) FROM screenshots s WHERE s.game_id = g.id) AS screenshots
            FROM games g
            ORDER BY g.pinned DESC, g.last_played_at DESC, g.title COLLATE NOCASE ASC
        `).all() as unknown as GameRow[];

        const tags = this.#tagsByFile();

        return rows.map((row) => ({
            path: join(this.root, row.file),
            name: row.title,
            size: row.size,
            pinned: row.pinned === 1,
            playCount: row.play_count,
            playSeconds: row.play_seconds,
            addedAt: row.added_at,
            lastPlayedAt: row.last_played_at,
            cover: row.cover,
            screenshots: row.screenshots,
            tags: tags.get(row.file) ?? [],
        }));
    }

    /**
     * Every game's tags, keyed by file name.
     *
     * One query for the whole library rather than one per game: the list is
     * tens of rows and a query per card would be tens of queries for one
     * screenful. Ordered here so that the cards and the filter chips agree on
     * what order a game's tags are in.
     */
    #tagsByFile(): Map<string, string[]> {
        const rows = this.#db.prepare(`
            SELECT g.file AS game_file, t.name AS name
            FROM game_tags gt
            JOIN tags  t ON t.id = gt.tag_id
            JOIN games g ON g.id = gt.game_id
            ORDER BY t.name COLLATE NOCASE ASC
        `).all() as unknown as GameTagRow[];

        const tags = new Map<string, string[]>();
        for (const row of rows) {
            const list = tags.get(row.game_file);
            if (list === undefined) {
                tags.set(row.game_file, [row.name]);
            } else {
                list.push(row.name);
            }
        }
        return tags;
    }

    /** Delete the tags that nothing points at any more. Called inside the
     *  transaction that removed the last link, so a word is never briefly
     *  invisible-but-there. */
    #pruneTags(): void {
        this.#db.exec('DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM game_tags)');
    }

    #selectScreenshots(): Screenshot[] {
        const rows = this.#db.prepare(`
            SELECT s.id, s.file, s.created_at, s.is_cover,
                   g.file AS game_file, g.title AS game_title
            FROM screenshots s
            JOIN games g ON g.id = s.game_id
            ORDER BY s.created_at DESC, s.id DESC
        `).all() as unknown as ScreenshotRow[];

        return rows.map((row) => ({
            id: row.id,
            gamePath: join(this.root, row.game_file),
            game: row.game_title,
            file: row.file,
            createdAt: row.created_at,
            isCover: row.is_cover === 1,
        }));
    }


    /** All of it or none of it. A reconcile that failed halfway would leave
     *  the model describing a folder that never existed. */
    #transaction(work: () => void): void {
        this.#db.exec('BEGIN');
        try {
            work();
            this.#db.exec('COMMIT');
        } catch (error) {
            this.#db.exec('ROLLBACK');
            throw error;
        }
    }
}
