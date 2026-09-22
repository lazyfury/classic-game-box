// ---------------------------------------------------------------------------
// Tests for what the library shows and in what order.
//
// Plain Node, like the library model's tests: libraryView.ts is decisions, and
// a decision does not need a window to be tested. The panel that draws the
// result is markup around this file.
//
//   pnpm test
// ---------------------------------------------------------------------------

import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
    DEFAULT_DIRECTION, NO_FILTER, directionOf, emulatorLabel, isFiltered, isReversible, newGames,
    systemLabel, systemsIn, tagHue, tagsIn, toggled, visibleGames,
} from '../src/renderer/libraryView.ts';
import { DEFAULT_LIBRARY_VIEW, SORT_KEYS } from '../src/shared/api.ts';

/** A game with everything the view could look at, and nothing else. */
function game(name, overrides = {}) {
    return {
        path: `/library/${name}.nes`,
        name,
        size: 1024,
        pinned: false,
        playCount: 0,
        playSeconds: 0,
        addedAt: 0,
        lastPlayedAt: 0,
        cover: null,
        screenshots: 0,
        tags: [],
        ...overrides,
    };
}

const names = (games) => games.map((entry) => entry.name);

// -- ordering ---------------------------------------------------------------

test('a pinned game is at the top of every order', () => {
    const games = [
        game('Mario'),
        game('Zelda', { pinned: true }),
    ];

    for (const sort of ['recent', 'name', 'playtime', 'size']) {
        assert.deepEqual(names(visibleGames(games, '', NO_FILTER, sort)), ['Zelda', 'Mario']);
    }
});

test('each order puts the game that wins it first', () => {
    const games = [
        game('A', { lastPlayedAt: 10, playSeconds: 5, size: 300 }),
        game('B', { lastPlayedAt: 30, playSeconds: 1, size: 100 }),
        game('C', { lastPlayedAt: 20, playSeconds: 90, size: 200 }),
    ];

    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'recent')), ['B', 'C', 'A']);
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'playtime')), ['C', 'A', 'B']);
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'size')), ['A', 'C', 'B']);
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'name')), ['A', 'B', 'C']);
});

test('a tie is broken by the name, so the order does not wander', () => {
    const games = [
        game('Zelda', { playSeconds: 60, size: 100 }),
        game('Mario', { playSeconds: 60, size: 100 }),
    ];

    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'playtime')), ['Mario', 'Zelda']);
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'size')), ['Mario', 'Zelda']);
});

test('sorting a list does not sort the caller\'s list', () => {
    const games = [game('B'), game('A')];
    visibleGames(games, '', NO_FILTER, 'name');
    assert.deepEqual(names(games), ['B', 'A']);
});

// -- what is visible --------------------------------------------------------

test('the search box matches the name, ignores case and is trimmed', () => {
    const games = [game('Super Mario Bros'), game('Tetris')];

    assert.deepEqual(names(visibleGames(games, 'mario', NO_FILTER, 'name')), ['Super Mario Bros']);
    assert.deepEqual(names(visibleGames(games, '  TETRIS ', NO_FILTER, 'name')), ['Tetris']);
    assert.deepEqual(names(visibleGames(games, 'nothing', NO_FILTER, 'name')), []);
    assert.deepEqual(names(visibleGames(games, '   ', NO_FILTER, 'name')), ['Super Mario Bros', 'Tetris']);
});

test('the emulator filter keeps only the console that was asked for', () => {
    const games = [game('Mario'), game('Pokemon', { path: '/library/Pokemon.gba' })];

    assert.deepEqual(systemsIn(games), ['nes', 'gba']);
    assert.deepEqual(
        names(visibleGames(games, '', { systems: ['gba'], tags: [] }, 'name')),
        ['Pokemon'],
    );
    assert.deepEqual(
        names(visibleGames(games, '', { systems: ['gb'], tags: [] }, 'name')),
        [],
    );
});

test('the tag filter is any-of, and ignores case', () => {
    const games = [
        game('A', { tags: ['RPG'] }),
        game('B', { tags: ['RPG', 'Hard'] }),
        game('C', { tags: ['Puzzle'] }),
        game('D'),
    ];

    assert.deepEqual(names(visibleGames(games, '', { systems: [], tags: ['rpg'] }, 'name')), ['A', 'B']);
    assert.deepEqual(
        names(visibleGames(games, '', { systems: [], tags: ['RPG', 'Puzzle'] }, 'name')),
        ['A', 'B', 'C'],
    );
    assert.deepEqual(names(visibleGames(games, '', { systems: [], tags: [] }, 'name')), ['A', 'B', 'C', 'D']);
});

test('the filters narrow together, not one at a time', () => {
    const games = [
        game('Mario', { tags: ['RPG'] }),
        game('Pokemon', { path: '/library/Pokemon.gba', tags: ['RPG'] }),
        game('Tetris', { tags: ['Puzzle'] }),
    ];

    assert.deepEqual(
        names(visibleGames(games, 'po', { systems: ['gba'], tags: ['RPG'] }, 'name')),
        ['Pokemon'],
    );
});

test('only the orders with two ends can be turned round', () => {
    // "Recently played" means most-recent-first: oldest-first is the same list
    // read backwards, so there is no arrow for it and nothing to store.
    assert.equal(isReversible('recent'), false);
    for (const key of SORT_KEYS) {
        if (key !== 'recent') {
            assert.equal(isReversible(key), true, key);
        }
    }

    assert.equal(directionOf('recent', 'asc'), 'desc');
    assert.equal(directionOf('recent', 'desc'), 'desc');
    assert.equal(directionOf('name', 'desc'), 'desc');
    assert.equal(directionOf('size', 'asc'), 'asc');
});

test('a saved direction for a one-ended order is ignored, not obeyed', () => {
    const games = [
        game('Alpha', { lastPlayedAt: 10 }),
        game('Zulu', { lastPlayedAt: 30 }),
    ];

    // The alphabetical order is the reverse of the recent one, so the two are
    // told apart by the result rather than by luck.
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'recent', 'desc')), ['Zulu', 'Alpha']);
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'recent', 'asc')), ['Zulu', 'Alpha']);

    // The order that does have two ends still obeys what it is given.
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'name', 'desc')), ['Zulu', 'Alpha']);
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'name', 'asc')), ['Alpha', 'Zulu']);
});

test('each order starts at the end its name means', () => {
    assert.deepEqual(DEFAULT_DIRECTION, {
        recent: 'desc',
        added: 'desc',
        name: 'asc',
        playtime: 'desc',
        size: 'desc',
    });
});

// The list of orders is shared with the main process, which validates the
// remembered choice against it. A key missing from one side is a setting that
// silently falls back to the default, and a key missing from the other is a
// chip with nothing to compare.
test('the shared list of orders, the directions and the default agree', () => {
    assert.deepEqual([...SORT_KEYS].sort(), Object.keys(DEFAULT_DIRECTION).sort());
    assert.deepEqual([...SORT_KEYS].sort(), Object.keys(DEFAULT_LIBRARY_VIEW.directions).sort());
    assert.deepEqual(DEFAULT_LIBRARY_VIEW.directions, DEFAULT_DIRECTION);
    assert.ok(SORT_KEYS.includes(DEFAULT_LIBRARY_VIEW.sort));
    // And every order can actually be applied: a comparator that forgot one
    // would leave that chip sorting by nothing.
    const games = [game('A', { addedAt: 1 }), game('B', { addedAt: 2 })];
    for (const key of SORT_KEYS) {
        assert.equal(visibleGames(games, '', NO_FILTER, key).length, 2);
    }
});

test('the direction is the one asked for, and it is per order', () => {
    const games = [
        game('A', { addedAt: 10, playSeconds: 5, size: 300 }),
        game('B', { addedAt: 30, playSeconds: 1, size: 100 }),
        game('C', { addedAt: 20, playSeconds: 90, size: 200 }),
    ];

    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'name', 'desc')), ['C', 'B', 'A']);
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'size', 'asc')), ['B', 'C', 'A']);
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'playtime', 'asc')), ['B', 'A', 'C']);

    // Newest addition first is the default; oldest first is what the arrow is
    // for, and it is the case that made the arrow worth having.
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'added')), ['B', 'C', 'A']);
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'added', 'asc')), ['A', 'C', 'B']);

    // A direction asked for and not given is the order's own.
    assert.deepEqual(
        names(visibleGames(games, '', NO_FILTER, 'added')),
        names(visibleGames(games, '', NO_FILTER, 'added', DEFAULT_DIRECTION.added)),
    );
});

test('the pin outranks the direction, not just the order', () => {
    const games = [
        game('A', { addedAt: 10 }),
        game('Z', { addedAt: 30, pinned: true }),
    ];

    // Oldest first would put Z last if the pin were part of the comparison; it
    // stays first, because the pin is not an entry in the sort.
    assert.deepEqual(names(visibleGames(games, '', NO_FILTER, 'added', 'asc')), ['Z', 'A']);
});

// -- what is new ------------------------------------------------------------

test('a new game is one that has never been played, newest addition first', () => {
    const games = [
        game('Old', { addedAt: 100, playCount: 3, lastPlayedAt: 900 }),
        game('Fresh', { addedAt: 500 }),
        game('Older', { addedAt: 200 }),
        game('Played once', { addedAt: 800, playCount: 1, lastPlayedAt: 850 }),
    ];

    assert.deepEqual(names(newGames(games)), ['Fresh', 'Older']);
});

test('two games added in the same millisecond are ordered by name', () => {
    const games = [game('Zelda', { addedAt: 5 }), game('Mario', { addedAt: 5 })];
    assert.deepEqual(names(newGames(games)), ['Mario', 'Zelda']);
});

test('nothing new when everything has been played, or when there is nothing', () => {
    assert.deepEqual(newGames([]), []);
    assert.deepEqual(newGames([game('A', { playCount: 1, lastPlayedAt: 1 })]), []);
});

// -- the small pieces --------------------------------------------------------

test('the chips list what the games actually carry, in a stable order', () => {
    const games = [
        game('A', { tags: ['zebra', 'RPG'] }),
        game('B', { path: '/library/B.gb', tags: ['rpg'] }),
    ];

    // Alphabetically, once per word rather than once per game -- and the two
    // spellings of "rpg" in the fixtures are one chip, because they are one
    // word as far as the library is concerned.
    assert.deepEqual(tagsIn(games), ['RPG', 'zebra']);
    // Consoles in the order the settings screen lists them, and only the ones
    // that are actually there.
    assert.deepEqual(systemsIn(games), ['nes', 'gb']);
});

test('a chip says which console, and is clearable', () => {
    assert.equal(systemLabel('nes'), 'NES');
    assert.equal(systemLabel('gba'), 'GBA');
    assert.equal(emulatorLabel(game('Mario')), 'NES');
    assert.equal(emulatorLabel(game('Pokemon', { path: '/library/Pokemon.gba' })), 'GBA');
    assert.equal(emulatorLabel(game('Tetris', { path: '/library/Tetris.gbc' })), 'GB');

    // An extension nothing knows is a NES game, which keeps the old behaviour
    // for a file with no extension at all.
    assert.equal(emulatorLabel(game('Weird', { path: '/library/Weird' })), 'NES');
});

test('pressing a chip adds it, and pressing it again takes it out', () => {
    assert.deepEqual(toggled([], 'nes'), ['nes']);
    assert.deepEqual(toggled(['nes'], 'gba'), ['nes', 'gba']);
    assert.deepEqual(toggled(['nes', 'gba'], 'nes'), ['gba']);
});

test('"is anything narrowing this list" is what the empty state needs', () => {
    assert.equal(isFiltered(NO_FILTER), false);
    assert.equal(isFiltered({ systems: ['nes'], tags: [] }), true);
    assert.equal(isFiltered({ systems: [], tags: ['RPG'] }), true);
});

test('a tag\'s colour comes from its name, always the same and always a colour', () => {
    assert.equal(tagHue('RPG'), tagHue('RPG'));
    assert.notEqual(tagHue('RPG'), tagHue('Puzzle'));

    for (const tag of ['RPG', 'rpg', '', '一个中文标签', 'x'.repeat(24)]) {
        const hue = tagHue(tag);
        assert.ok(hue >= 0 && hue < 360, `${tag} produced ${hue}`);
    }
});
