// ---------------------------------------------------------------------------
// Which games the library shows, and in what order.
//
// A module of its own rather than the top of the panel that draws it, for the
// same reason systems.ts is not inside the settings screen: this is a set of
// decisions -- is this game visible, which order do these come in, what colour
// is this word -- and a decision can be tested without a window. The panel
// stays markup and event handlers.
//
// Four things narrow the list: the section (the library, the pinned games, the
// recently played), the search box, the emulator filter and the tag filter.
// The first is applied by App -- it is which list you are looking at, not a
// filter of a list -- and the other three meet here, in one function, so the
// count in the header and the cards below it cannot disagree about how many
// games there are.
// ---------------------------------------------------------------------------

// The explicit `.ts` is what lets a Node test import this file directly; see
// the same note at the top of systems.ts.
import { systemForExtension, SYSTEMS, type SystemId } from './systems.ts';
import {
    DEFAULT_DIRECTION, directionOf, isReversible,
    type LibraryViewSettings, type SortDirection, type SortKey,
} from '../shared/api.ts';
import type { GameEntry } from '../shared/api.ts';

// The orders themselves live in shared/api.ts, because the main process has to
// validate the remembered one against the same list. Re-exported here so that
// the panel and App go on importing them from the module that decides what
// they mean.
export { DEFAULT_DIRECTION, directionOf, isReversible };
export type { LibraryViewSettings, SortDirection, SortKey };

/** Everything the player has narrowed the list to, apart from the search box. */
export interface LibraryFilter {
    /** Emulator types to show, or empty for all of them. */
    systems: readonly SystemId[];
    /** Tags to show, or empty for all of them. A game matches when it has
     *  any one of them -- see visibleGames. */
    tags: readonly string[];
}

/** The filter with nothing selected, which is also what a fresh start uses. */
export const NO_FILTER: LibraryFilter = Object.freeze({ systems: [], tags: [] });

/** True when anything at all is narrowing the list, so the empty state can
 *  tell "there are no games" apart from "your filter hides them". */
export function isFiltered(filter: LibraryFilter): boolean {
    return filter.systems.length > 0 || filter.tags.length > 0;
}

/**
 * A stable colour for a tag, as a hue in degrees.
 *
 * The same fold as the blank cover: a hash of the name into the colour wheel.
 * Two things matter and neither is taste -- the same word has to be the same
 * colour in every run, which rules out anything random, and two words next to
 * each other should not be, which is what the hash is for. Saturation and
 * lightness are fixed by the stylesheet, so the hue is the whole colour.
 */
export function tagHue(name: string): number {
    let hash = 0;
    for (let i = 0; i < name.length; i += 1) {
        hash = (hash * 31 + name.codePointAt(i)!) % 360;
    }
    return hash;
}

/** The console a game is for, derived from its extension. Never stored: the
 *  extension is part of the file name, and the folder is the truth. */
export function systemOf(game: GameEntry): SystemId {
    return systemForExtension(game.path);
}

/** What the badge on a card says: "NES", "GBA", "GB". */
export function systemLabel(system: SystemId): string {
    return SYSTEMS.find((known) => known.id === system)?.short ?? system.toUpperCase();
}

/** The badge for a game, without the caller having to go through two steps. */
export function emulatorLabel(game: GameEntry): string {
    return systemLabel(systemOf(game));
}

/**
 * The emulator types the games actually use, in the order the settings screen
 * lists consoles.
 *
 * Derived from the games rather than from the core table, because a filter
 * that offers a console nothing in the library is for is a filter that can
 * only ever empty the list.
 */
export function systemsIn(games: readonly GameEntry[]): SystemId[] {
    const present = new Set(games.map(systemOf));
    return SYSTEMS.map((known) => known.id).filter((id) => present.has(id));
}

/**
 * Every tag the games carry, alphabetically and without duplicates.
 *
 * Case-insensitively deduplicated, like the database itself: the model stores
 * one spelling per word (COLLATE NOCASE), so two games cannot disagree about
 * the case of a tag, and the chips should not be able to show one word twice
 * even if they did -- a chip that filters by "rpg" next to one that filters by
 * "RPG" would be two controls for one thing.
 *
 * Sorted the way SQLite's COLLATE NOCASE sorts, so the chips and the tags on
 * the cards are in the same order.
 */
export function tagsIn(games: readonly GameEntry[]): string[] {
    const seen = new Map<string, string>();
    for (const game of games) {
        for (const tag of game.tags) {
            const key = tag.toLocaleLowerCase();
            if (!seen.has(key)) {
                seen.set(key, tag);
            }
        }
    }

    return [...seen.values()].sort((a, b) => {
        const left = a.toLocaleLowerCase();
        const right = b.toLocaleLowerCase();
        return left < right ? -1 : left > right ? 1 : 0;
    });
}

/**
 * The games that have never been started, newest addition first.
 *
 * “New” is defined as “never played” rather than “added in the last week”:
 * there is no other place in the model that remembers having *seen* a game, so
 * a window of days would be a second, weaker copy of something the library
 * already knows, and it would still be wrong the moment somebody adds a game
 * and goes to bed. A game stops being new when it is played, which is exactly
 * when the card offering to play it has done its job.
 */
export function newGames(games: readonly GameEntry[]): GameEntry[] {
    return games
        .filter((game) => game.playCount === 0 && game.lastPlayedAt === 0)
        .sort((a, b) => b.addedAt - a.addedAt || a.name.localeCompare(b.name));
}

/** Add a value to a filter list, or take it out: what tapping a chip does. */
export function toggled<T>(list: readonly T[], value: T): T[] {
    return list.includes(value) ? list.filter((item) => item !== value) : [...list, value];
}

/** Case-insensitive, and on the name only -- matching the whole path would
 *  make every entry in a folder with a Chinese name match every query. */
function matchesQuery(game: GameEntry, needle: string): boolean {
    return game.name.toLocaleLowerCase().includes(needle);
}

/** A game matches the tags when it carries any one of them. Any rather than
 *  all, because the chips are a way of asking for "these kinds of game": with
 *  two pressed, wanting only the games that are both is the rarer question. */
function matchesTags(game: GameEntry, wanted: readonly string[]): boolean {
    if (wanted.length === 0) {
        return true;
    }
    const carried = new Set(game.tags.map((tag) => tag.toLocaleLowerCase()));
    return wanted.some((tag) => carried.has(tag.toLocaleLowerCase()));
}

/**
 * The games to draw, in the order to draw them.
 *
 * Pinned first, always: the pin is the player saying "this one at the top",
 * and a sort that could move it is a sort that overrules them -- in either
 * direction, which is why the pin is settled before the direction is applied
 * to anything. Then the order that was asked for, with the name as the
 * tie-break so that two games with nothing else to compare keep a stable
 * place between renders.
 *
 * Each order is written once, ascending, and the direction multiplies the
 * result. Five reversed comparators would be five places to get the sign
 * wrong; this is one. Ties are not reversed -- they fall through to the name,
 * always A to Z -- because a tie is not a direction.
 */
export function visibleGames(
    games: readonly GameEntry[],
    query: string,
    filter: LibraryFilter,
    sort: SortKey,
    direction: SortDirection = DEFAULT_DIRECTION[sort],
): GameEntry[] {
    const needle = query.trim().toLocaleLowerCase();

    const shown = games.filter((game) => (
        (needle === '' || matchesQuery(game, needle))
        && (filter.systems.length === 0 || filter.systems.includes(systemOf(game)))
        && matchesTags(game, filter.tags)
    ));

    const byName = (a: GameEntry, b: GameEntry): number => a.name.localeCompare(b.name);

    /** The order asked for, ascending, before the direction is applied. */
    const ascending = (a: GameEntry, b: GameEntry): number => {
        switch (sort) {
        case 'recent':
            return a.lastPlayedAt - b.lastPlayedAt;
        case 'name':
            return byName(a, b);
        case 'playtime':
            return a.playSeconds - b.playSeconds;
        case 'added':
            return a.addedAt - b.addedAt;
        case 'size':
            return a.size - b.size;
        }
    };

    /**
     * The direction actually applied: an order with only one end keeps it, no
     * matter what was handed in. See `directionOf` -- the rule lives there so
     * that the list, the saved value and the control cannot disagree about
     * whether "oldest first" is a thing that can be asked for.
     */
    const sign = directionOf(sort, direction) === 'asc' ? 1 : -1;

    shown.sort((a, b) => {
        if (a.pinned !== b.pinned) {
            return Number(b.pinned) - Number(a.pinned);
        }
        const order = ascending(a, b);
        return order !== 0 ? sign * order : byName(a, b);
    });

    return shown;
}
