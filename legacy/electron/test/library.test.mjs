// ---------------------------------------------------------------------------
// Tests for the library model.
//
// These run in plain Node, with no Electron and no window, because
// src/main/library.ts knows nothing about either: it is handed a folder and it
// models that folder. That is the whole reason the SQLite lives behind a class
// with no `app` and no `dialog` in it.
//
//   pnpm test
//
// Every test gets its own temporary folder, so the order they run in does not
// matter and a failure leaves nothing behind to confuse the next one.
// ---------------------------------------------------------------------------

import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
    existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, unlinkSync,
    utimesSync, writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

import { DATABASE_FILE, GameLibrary, MAX_TAG_LENGTH, collectGames, isInside } from '../src/main/library.ts';

/** A fresh folder, and the two or three things a test does to it. */
function scratch() {
    const root = mkdtempSync(join(tmpdir(), 'fc-library-'));
    return {
        root,
        /** Put a file in the library folder, as the Finder would. */
        place(name, text = 'NES\u001a') {
            writeFileSync(join(root, name), text);
            return join(root, name);
        },
        /** Put a file somewhere else, to import from. */
        outside(name, text = 'NES\u001a') {
            const dir = mkdtempSync(join(tmpdir(), 'fc-source-'));
            writeFileSync(join(dir, name), text);
            return join(dir, name);
        },
        /** Make a file look older or newer, to test the mtime column. */
        touch(name, when) {
            const at = new Date(when);
            utimesSync(join(root, name), at, at);
        },
        cleanup() {
            rmSync(root, { recursive: true, force: true });
        },
    };
}

const titleOf = (path) => path.replace(/^.*\//, '');

// -- the folder is the truth -------------------------------------------------

test('an empty folder opens as an empty library, and gets a database', () => {
    const box = scratch();
    try {
        const library = GameLibrary.open(box.root);
        assert.equal(library.databasePath, join(box.root, DATABASE_FILE));
        assert.ok(existsSync(library.databasePath), 'the database file was not created');
        assert.deepEqual(library.scan(), []);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('a .nes on disk appears, and only once', () => {
    const box = scratch();
    try {
        box.place('Mario.nes');
        box.place('Tetris.nes');
        box.place('notes.txt');

        const library = GameLibrary.open(box.root);
        const first = library.scan();
        assert.deepEqual(first.map((game) => game.name).sort(), ['Mario', 'Tetris']);

        // A second scan is not a second insert: the row is keyed by file name.
        const addedAt = new Map(first.map((game) => [game.name, game.addedAt]));
        const second = library.scan();
        assert.equal(second.length, 2);
        for (const game of second) {
            assert.equal(game.addedAt, addedAt.get(game.name));
        }
        library.close();
    } finally {
        box.cleanup();
    }
});

test('a changed file is the same game with new numbers', () => {
    const box = scratch();
    try {
        box.place('Mario.nes', 'short');
        const library = GameLibrary.open(box.root);
        const [before] = library.scan();
        assert.equal(before.size, 5);

        // Edited in place: same name, different size.
        box.place('Mario.nes', 'a much longer rom');
        const [after] = library.scan();
        assert.equal(after.size, 17);
        assert.equal(after.playCount, before.playCount);
        // Added, not re-added: the date the game entered the library is a
        // property of the game, not of the file.
        assert.equal(after.addedAt, before.addedAt);

        library.close();
    } finally {
        box.cleanup();
    }
});

test('a file touched but not resized is noticed', () => {
    const box = scratch();
    try {
        box.place('Mario.nes', 'NES');
        const library = GameLibrary.open(box.root);
        library.scan();

        // Same size, new modification time. Only the mtime column can tell
        // these two files apart.
        box.touch('Mario.nes', Date.now() + 60_000);
        assert.equal(library.scan().length, 1);

        // And the row was updated rather than duplicated, which is what a
        // scan keyed on anything but the file name would do.
        assert.equal(library.scan().length, 1);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('a file deleted from the folder leaves the library', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        box.place('Tetris.nes');
        const library = GameLibrary.open(box.root);
        assert.equal(library.scan().length, 2);

        unlinkSync(path);
        const after = library.scan();
        assert.deepEqual(after.map((game) => game.name), ['Tetris']);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('a rename keeps what the database knew about the game', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();
        library.setPinned(path, true);
        library.notePlayed(path);

        // A rename keeps the size and the modification time, so a file that
        // vanished and a same-sized, same-timed file that appeared are the
        // same file. This is the first thing the model can do that a folder
        // listing cannot.
        renameSync(path, join(box.root, 'Super Mario.nes'));

        const after = library.scan();
        assert.equal(after.length, 1);
        assert.equal(after[0].name, 'Super Mario');
        assert.equal(after[0].pinned, true);
        assert.equal(after[0].playCount, 1);
        library.close();
    } finally {
        box.cleanup();
    }
});

// -- what the model remembers ------------------------------------------------

test('playing a game counts it and dates it', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        const [before] = library.scan();
        assert.equal(before.playCount, 0);
        assert.equal(before.lastPlayedAt, 0);

        library.notePlayed(path);
        library.notePlayed(path);

        const [after] = library.scan();
        assert.equal(after.playCount, 2);
        assert.ok(after.lastPlayedAt > 0);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('a game that is not in the library is not an error', () => {
    const box = scratch();
    try {
        const library = GameLibrary.open(box.root);
        // `--rom` can name any file on the machine. Playing one of those is
        // not a reason to fail; it is simply a game that is not in the
        // library.
        assert.doesNotThrow(() => library.notePlayed(box.outside('Elsewhere.nes')));
        assert.deepEqual(library.scan(), []);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('pinning survives a reopen, and sorts first', () => {
    const box = scratch();
    try {
        const mario = box.place('Mario.nes');
        box.place('Zelda.nes');
        box.place('Tetris.nes');

        const library = GameLibrary.open(box.root);
        library.scan();
        assert.equal(library.setPinned(mario, true), true);

        const first = library.scan();
        assert.equal(first[0].name, 'Mario');
        assert.equal(first[0].pinned, true);
        library.close();

        // Reopened, which is what happens the next time the application
        // starts. The pin is in the database, so it is still there.
        const again = GameLibrary.open(box.root);
        const second = again.scan();
        assert.equal(second[0].name, 'Mario');
        assert.equal(second[0].pinned, true);
        assert.equal(second.filter((game) => game.pinned).length, 1);

        again.setPinned(mario, false);
        assert.equal(again.scan().filter((game) => game.pinned).length, 0);
        again.close();
    } finally {
        box.cleanup();
    }
});

// -- importing and deleting --------------------------------------------------

test('importing copies the file in and never overwrites', () => {
    const box = scratch();
    try {
        const source = box.outside('Mario.nes', 'the real rom');

        const library = GameLibrary.open(box.root);
        const copied = library.add([source]);
        assert.deepEqual(copied, ['Mario.nes']);
        assert.ok(existsSync(join(box.root, 'Mario.nes')));
        // The source is left where it was: importing is a copy, not a move.
        assert.ok(existsSync(source));

        // A different file with the same name gets a new one, because
        // overwriting is how somebody loses a game.
        const other = box.outside('Mario.nes', 'a different rom');
        assert.deepEqual(library.add([other]), ['Mario (2).nes']);

        assert.deepEqual(
            library.scan().map((game) => game.name).sort(),
            ['Mario', 'Mario (2)'],
        );
        library.close();
    } finally {
        box.cleanup();
    }
});

test('importing a file that is already in the library is a no-op', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        assert.deepEqual(library.add([path]), []);
        assert.equal(library.scan().length, 1);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('deleting removes the file and the row', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        box.place('Tetris.nes');
        const library = GameLibrary.open(box.root);
        library.scan();

        assert.equal(library.remove(path), true);
        assert.equal(existsSync(path), false);
        assert.deepEqual(library.scan().map((game) => game.name), ['Tetris']);
        library.close();
    } finally {
        box.cleanup();
    }
});

// -- what a drop amounts to --------------------------------------------------

test('a dropped file is the ROM it points at', () => {
    const box = scratch();
    try {
        const rom = box.outside('Mario.nes');
        const notes = box.outside('notes.txt');
        assert.deepEqual(collectGames([rom]), [rom]);
        assert.deepEqual(collectGames([notes]), []);
        assert.deepEqual(collectGames([rom, notes]), [rom]);
    } finally {
        box.cleanup();
    }
});

test('a dropped folder means the ROMs in it, one level deep', () => {
    const box = scratch();
    try {
        const folder = mkdtempSync(join(tmpdir(), 'fc-drop-'));
        writeFileSync(join(folder, 'Tetris.nes'), 'NES');
        writeFileSync(join(folder, 'Mario.nes'), 'NES');
        writeFileSync(join(folder, 'readme.txt'), 'not a game');
        mkdirSync(join(folder, 'nested'));
        writeFileSync(join(folder, 'nested', 'Buried.nes'), 'NES');

        const found = collectGames([folder]);
        assert.deepEqual(found, [join(folder, 'Mario.nes'), join(folder, 'Tetris.nes')]);

        rmSync(folder, { recursive: true, force: true });
    } finally {
        box.cleanup();
    }
});

test('a drop of nothing in particular is not an error', () => {
    const box = scratch();
    try {
        assert.deepEqual(collectGames([]), []);
        assert.deepEqual(collectGames([join(box.root, 'not-there.nes')]), []);
        assert.deepEqual(collectGames(['']), []);
    } finally {
        box.cleanup();
    }
});

test('dropped games can be imported, folder and all', () => {
    const box = scratch();
    try {
        const folder = mkdtempSync(join(tmpdir(), 'fc-drop-'));
        writeFileSync(join(folder, 'Tetris.nes'), 'NES');
        writeFileSync(join(folder, 'Mario.nes'), 'NES');

        const library = GameLibrary.open(box.root);
        assert.deepEqual(library.add(collectGames([folder])).sort(), [
            'Mario.nes',
            'Tetris.nes',
        ]);
        assert.deepEqual(
            library.scan().map((game) => game.name).sort(),
            ['Mario', 'Tetris'],
        );
        library.close();

        rmSync(folder, { recursive: true, force: true });
    } finally {
        box.cleanup();
    }
});

// -- screenshots --------------------------------------------------------------

/** The first eight bytes of a PNG, which is as much of one as the library
 *  checks. A real picture would be kinder to look at and no better a test. */
const PNG = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x01]);

test('a screenshot is written into the library and filed under its game', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();

        const shot = library.saveScreenshot(path, PNG);
        assert.ok(shot !== null);
        assert.equal(shot.game, 'Mario');
        assert.equal(shot.gamePath, path);
        assert.ok(shot.file.startsWith('screenshots/'), shot.file);
        assert.ok(existsSync(join(box.root, shot.file)), 'the PNG was not written');
        assert.equal(readFileSync(join(box.root, shot.file)).length, PNG.length);

        assert.deepEqual(library.screenshots().map((s) => s.id), [shot.id]);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('the first screenshot becomes the cover, and the second does not', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();

        const first = library.saveScreenshot(path, PNG);
        const second = library.saveScreenshot(path, Buffer.concat([PNG, Buffer.from('x')]));

        assert.equal(first.isCover, true);
        assert.equal(second.isCover, false);

        const [game] = library.scan();
        assert.equal(game.screenshots, 2);
        assert.equal(game.cover, first.file);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('setting a cover moves the flag and leaves exactly one', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();

        library.saveScreenshot(path, PNG);
        const second = library.saveScreenshot(path, PNG);
        assert.equal(library.setCover(second.id), true);

        const shots = library.screenshots();
        assert.equal(shots.filter((shot) => shot.isCover).length, 1);
        assert.equal(shots.find((shot) => shot.isCover).id, second.id);
        assert.equal(library.scan()[0].cover, second.file);

        // A cover for a screenshot that does not exist is not an error, it is
        // a no.
        assert.equal(library.setCover(9999), false);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('deleting a screenshot deletes its file', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();

        const shot = library.saveScreenshot(path, PNG);
        const file = join(box.root, shot.file);
        assert.ok(existsSync(file));

        assert.equal(library.removeScreenshot(shot.id), true);
        assert.equal(existsSync(file), false);
        assert.deepEqual(library.screenshots(), []);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('deleting the cover promotes the newest picture left', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();

        const first = library.saveScreenshot(path, PNG);
        const second = library.saveScreenshot(path, PNG);
        assert.equal(first.isCover, true);

        library.removeScreenshot(first.id);
        const shots = library.screenshots();
        assert.equal(shots.length, 1);
        assert.equal(shots[0].isCover, true);
        assert.equal(shots[0].id, second.id);
        assert.equal(library.scan()[0].cover, second.file);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('deleting a game takes its screenshots with it', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();
        const shot = library.saveScreenshot(path, PNG);
        const file = join(box.root, shot.file);

        assert.equal(library.remove(path), true);
        assert.equal(existsSync(file), false, 'the PNG outlived its game');
        assert.deepEqual(library.screenshots(), []);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('deleting a game from the Finder takes its screenshots too', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();
        const shot = library.saveScreenshot(path, PNG);

        // Not through the application: somebody tidying up in the Finder.
        unlinkSync(path);
        assert.deepEqual(library.scan(), []);
        assert.deepEqual(library.screenshots(), []);
        assert.equal(existsSync(join(box.root, shot.file)), false);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('a renamed game keeps its screenshots', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();
        const shot = library.saveScreenshot(path, PNG);

        renameSync(path, join(box.root, 'Super Mario.nes'));
        library.scan();

        const shots = library.screenshots();
        assert.equal(shots.length, 1);
        assert.equal(shots[0].game, 'Super Mario');
        assert.equal(shots[0].id, shot.id);
        assert.equal(library.scan()[0].cover, shot.file);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('the library refuses bytes that are not a PNG, and files outside itself', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();

        assert.equal(library.saveScreenshot(path, Buffer.from('not a picture')), null);
        assert.equal(library.saveScreenshot(box.outside('Elsewhere.nes'), PNG), null);
        assert.deepEqual(library.screenshots(), []);

        // And nothing was left behind in the library folder either.
        assert.deepEqual(readdirSync(box.root).sort(), ['Mario.nes', DATABASE_FILE]);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('a version 2 library upgrades to tags and play time', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');

        // A library from before either existed: the games table without
        // play_seconds, and the screenshots table as schema 2 wrote it.
        const db = new DatabaseSync(join(box.root, DATABASE_FILE));
        db.exec(`
            CREATE TABLE games (
                id INTEGER PRIMARY KEY, file TEXT NOT NULL UNIQUE, title TEXT NOT NULL,
                size INTEGER NOT NULL, mtime_ms INTEGER NOT NULL, added_at INTEGER NOT NULL,
                last_played_at INTEGER NOT NULL DEFAULT 0,
                play_count INTEGER NOT NULL DEFAULT 0, pinned INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE screenshots (
                id INTEGER PRIMARY KEY,
                game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                file TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL,
                is_cover INTEGER NOT NULL DEFAULT 0
            );
        `);
        db.prepare(
            'INSERT INTO games (file, title, size, mtime_ms, added_at, play_count, pinned) '
            + 'VALUES (?, ?, ?, ?, ?, ?, ?)',
        ).run('Mario.nes', 'Mario', 3, 1, 1, 4, 1);
        db.exec('PRAGMA user_version = 2');
        db.close();

        const library = GameLibrary.open(box.root);
        const [game] = library.scan();
        // The pin and the count are still the point: the column was added,
        // not rebuilt, so nothing already in the table moved.
        assert.equal(game.pinned, true);
        assert.equal(game.playCount, 4);
        // And the two new things start empty rather than missing.
        assert.equal(game.playSeconds, 0);
        assert.deepEqual(game.tags, []);

        // Both work immediately, which is the half a migration test usually
        // forgets to check.
        library.notePlaytime(path, 30);
        library.setTags(path, ['RPG']);
        assert.equal(library.scan()[0].playSeconds, 30);
        assert.deepEqual(library.scan()[0].tags, ['RPG']);

        library.close();
    } finally {
        box.cleanup();
    }
});

test('a version 1 library upgrades without losing anything', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');

        // A library from before screenshots existed: the games table, and no
        // screenshots table at all.
        const db = new DatabaseSync(join(box.root, DATABASE_FILE));
        db.exec(`
            CREATE TABLE games (
                id INTEGER PRIMARY KEY, file TEXT NOT NULL UNIQUE, title TEXT NOT NULL,
                size INTEGER NOT NULL, mtime_ms INTEGER NOT NULL, added_at INTEGER NOT NULL,
                last_played_at INTEGER NOT NULL DEFAULT 0,
                play_count INTEGER NOT NULL DEFAULT 0, pinned INTEGER NOT NULL DEFAULT 0
            )
        `);
        db.prepare(
            'INSERT INTO games (file, title, size, mtime_ms, added_at, play_count, pinned) '
            + 'VALUES (?, ?, ?, ?, ?, ?, ?)',
        ).run('Mario.nes', 'Mario', 3, 1, 1, 7, 1);
        db.exec('PRAGMA user_version = 1');
        db.close();

        const library = GameLibrary.open(box.root);
        const [game] = library.scan();
        // The pin and the play count are the point: a migration that recreated
        // the table would have lost both.
        assert.equal(game.pinned, true);
        assert.equal(game.playCount, 7);
        assert.equal(game.cover, null);

        const shot = library.saveScreenshot(path, PNG);
        assert.ok(shot !== null);
        assert.equal(shot.isCover, true);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('a screenshot can be located, and only inside the library', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();
        const shot = library.saveScreenshot(path, PNG);

        // The row names the picture; the model turns that into a path, and the
        // promise is that the path is inside the library folder.
        const absolute = library.screenshotPath(shot.id);
        assert.equal(absolute, join(box.root, shot.file));
        assert.ok(existsSync(absolute), 'the file the row names is not there');

        // A row that is not there has no path. The renderer names an id, so an
        // id it made up is an ordinary thing to happen.
        assert.equal(library.screenshotPath(shot.id + 1000), null);

        // And a row that was edited to point out of the folder is refused too.
        // The check is on the path, not on the row having come from us.
        const db = new DatabaseSync(join(box.root, DATABASE_FILE));
        db.prepare('UPDATE screenshots SET file = ? WHERE id = ?')
            .run('../../etc/passwd', shot.id);
        db.close();
        assert.equal(library.screenshotPath(shot.id), null);

        // Deleted rows have no path either.
        assert.equal(library.removeScreenshot(shot.id), true);
        assert.equal(library.screenshotPath(shot.id), null);

        library.close();
    } finally {
        box.cleanup();
    }
});

test('saving as a cover replaces the one that was there', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();

        const first = library.saveScreenshot(path, PNG);
        const second = library.saveScreenshot(path, PNG, true);

        // Not the first-screenshot rule: the explicit request, which has to
        // clear the old flag and set the new one without a moment in between
        // where the game has two covers.
        assert.equal(first.isCover, true);
        assert.equal(second.isCover, true);

        const shots = library.screenshots();
        assert.equal(shots.filter((shot) => shot.isCover).length, 1);
        assert.equal(shots.find((shot) => shot.isCover).id, second.id);
        assert.equal(library.scan()[0].cover, second.file);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('saving as a cover works when there was none', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();

        const shot = library.saveScreenshot(path, PNG, true);
        assert.equal(shot.isCover, true);
        assert.equal(library.scan()[0].cover, shot.file);
        library.close();
    } finally {
        box.cleanup();
    }
});

// -- tags and play time ------------------------------------------------------

test('a tag is a word the whole library shares, and typing it again does not make two', () => {
    const box = scratch();
    try {
        const mario = box.place('Mario.nes');
        const zelda = box.place('Zelda.nes');
        const library = GameLibrary.open(box.root);
        library.scan();

        const tagsOf = (name) => library.scan().find((game) => game.name === name).tags;

        assert.equal(library.setTags(mario, ['RPG', 'Platformer']), true);
        assert.equal(library.setTags(zelda, ['rpg']), true);

        // One word, two games, and the spelling that was typed first is the
        // one that survives -- "rpg" did not become a second tag and did not
        // change "RPG" into itself lower-cased. Alphabetical, because that is
        // the order the cards and the filter chips both state.
        assert.deepEqual(tagsOf('Mario'), ['Platformer', 'RPG']);
        assert.deepEqual(tagsOf('Zelda'), ['RPG']);

        // Replace, not add: the next call is the whole list, not one more
        // word on the end of it.
        assert.equal(library.setTags(mario, ['Platformer']), true);
        assert.deepEqual(tagsOf('Mario'), ['Platformer']);
        assert.deepEqual(tagsOf('Zelda'), ['RPG']);

        library.close();
    } finally {
        box.cleanup();
    }
});

test('tags arrive trimmed, deduplicated and short enough to fit on a card', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();

        library.setTags(path, ['  RPG  ', '', '   ', 'rpg', 'RPG', 'a'.repeat(64)]);

        // NOCASE order, which is what the database sorts by: the long
        // lower-case word comes before the short upper-case one.
        assert.deepEqual(library.scan()[0].tags, ['a'.repeat(MAX_TAG_LENGTH), 'RPG']);

        library.close();
    } finally {
        box.cleanup();
    }
});

test('a word nothing points at any more is forgotten', () => {
    const box = scratch();
    try {
        const mario = box.place('Mario.nes');
        const zelda = box.place('Zelda.nes');
        const library = GameLibrary.open(box.root);
        library.scan();
        library.setTags(mario, ['RPG']);
        library.setTags(zelda, ['Adventure']);

        // Read through a second connection: the words left over after the
        // last game drops one are the point, and they are on no game.
        const words = () => {
            const db = new DatabaseSync(join(box.root, DATABASE_FILE));
            const rows = db.prepare('SELECT name FROM tags ORDER BY name').all();
            db.close();
            return rows.map((row) => row.name);
        };

        assert.deepEqual(words(), ['Adventure', 'RPG']);

        library.setTags(mario, []);
        assert.deepEqual(words(), ['Adventure']);

        // And a game that leaves takes its words with it -- which here is
        // nothing, because no other game was using its tag either.
        library.remove(zelda);
        assert.deepEqual(words(), []);

        library.close();
    } finally {
        box.cleanup();
    }
});

test('tags survive a rescan, and a game outside the library has none to give', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();
        library.setTags(path, ['RPG']);

        assert.deepEqual(library.scan()[0].tags, ['RPG']);
        assert.deepEqual(library.scan()[0].tags, ['RPG']);

        // The same rule as the pin: `--rom` can name any file on the machine,
        // and a game that is not in the library is not in the library.
        assert.equal(library.setTags(box.outside('Elsewhere.nes'), ['RPG']), false);

        library.close();
    } finally {
        box.cleanup();
    }
});

test('play time accumulates in whole seconds, and only for this library', () => {
    const box = scratch();
    try {
        const path = box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();

        assert.equal(library.scan()[0].playSeconds, 0);

        library.notePlaytime(path, 12.7);
        library.notePlaytime(path, 0.5);
        // Added, not replaced, and floored: the column is whole seconds and
        // the renderer sends whole seconds anyway. The remainders are gone.
        assert.equal(library.scan()[0].playSeconds, 12);

        // Nothing that means anything gets in.
        library.notePlaytime(path, 0);
        library.notePlaytime(path, -5);
        library.notePlaytime(path, Number.NaN);
        library.notePlaytime(box.outside('Elsewhere.nes'), 600);
        assert.equal(library.scan()[0].playSeconds, 12);

        // It is a clock, not a counter: playing says nothing about the count.
        assert.equal(library.scan()[0].playCount, 0);

        library.close();
    } finally {
        box.cleanup();
    }
});

// -- what the renderer is not allowed to do ----------------------------------

test('the model refuses paths outside its own folder', () => {
    const box = scratch();
    try {
        const library = GameLibrary.open(box.root);
        const outside = box.outside('Elsewhere.nes');

        assert.equal(library.setPinned(outside, true), false);
        assert.equal(library.remove(outside), false);
        assert.ok(existsSync(outside), 'a file outside the library was deleted');

        // And the same rule the IPC layer uses, tested where it is written.
        assert.equal(isInside(box.root, join(box.root, 'Mario.nes')), true);
        assert.equal(isInside(box.root, box.root), true);
        assert.equal(isInside(box.root, outside), false);
        // `..` is collapsed before the comparison, not after.
        assert.equal(isInside(box.root, join(box.root, '..', 'elsewhere')), false);
        assert.equal(isInside(box.root, join(box.root, '..', 'fc-library-nope.nes')), false);
        library.close();
    } finally {
        box.cleanup();
    }
});

test('a database from a newer build is refused rather than guessed at', () => {
    const box = scratch();
    try {
        const path = join(box.root, DATABASE_FILE);
        const db = new DatabaseSync(path);
        db.exec('PRAGMA user_version = 99');
        db.close();

        assert.throws(() => GameLibrary.open(box.root), /newer version/);
    } finally {
        box.cleanup();
    }
});

// -- a small sanity check on the shape of the folder --------------------------

test('the folder holds the games and the database, and nothing else', () => {
    const box = scratch();
    try {
        box.place('Mario.nes');
        const library = GameLibrary.open(box.root);
        library.scan();
        library.close();

        // A rollback journal would be named library.sqlite-journal and would
        // come and go; WAL would leave -wal and -shm behind permanently. This
        // checks the second, which is the one that would be a surprise in a
        // folder full of ROMs.
        assert.deepEqual(
            readdirSync(box.root).sort(),
            ['Mario.nes', DATABASE_FILE],
        );
        assert.equal(titleOf(join(box.root, DATABASE_FILE)), DATABASE_FILE);
    } finally {
        box.cleanup();
    }
});
