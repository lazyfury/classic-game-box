// ---------------------------------------------------------------------------
// The game library -- the middle column.
//
// A grid of cards. There is no cover art on the internet to fetch -- a .nes
// contains no title, no publisher and no date that can be read without running
// it, and scraping a database for artwork would mean this application talked
// to the network, which it otherwise never does -- so a card's picture comes
// from one place: a screenshot the player took of it.
//
// Until they take one, the card is a block of colour with the game's name
// clipped into it. The colour is derived from the name, so it is stable across
// runs and different between neighbours, and the name is the only label there
// has ever been.
//
// The name is still the search key, because for ROMs it is surprisingly
// informative -- "Super Mario Bros. (Japan, USA)" is a better label than most
// title screens.
//
// A card now says three more things the database learned to hold: which
// console the file is for, how long it has been played, and what the player
// tagged it with. The badge is derived from the extension, the play time is
// counted by the emulator and stored in seconds, and the tags are words the
// player typed -- drawn as a coloured square and the word, which is the whole
// of this application's colour-coding.
//
// Three controls narrow the list: the search box, the emulator filter, the tag
// filter. They are chips because they are the same gesture as the pin on a
// card -- press to say something, press again to take it back -- and the two
// filters share one appearance because they are the same kind of thing. The
// decisions behind them live in libraryView.ts, where a test can reach them.
//
// A card carries four controls besides "open": a tag editor, a pin, which is
// the one thing the database remembers that a folder listing could not, and a
// delete, which is the only control in the application that destroys something
// and therefore goes through a native confirmation in the main process. They
// sit outside the card's open button because a button inside a button is not a
// thing.
// ---------------------------------------------------------------------------

import * as ToggleGroup from '@radix-ui/react-toggle-group';
import {
    ArrowDown, ArrowUp, Camera, FolderCog, FolderOpen, Pin, Play, Plus, Search, Tag, Trash2, X,
} from 'lucide-react';
import { useId, useState, type CSSProperties } from 'react';

import { libraryAssetUrl, type GameEntry } from '../../shared/api';
import { formatDuration, formatSize, formatWhen } from '../format';
import {
    NO_FILTER, emulatorLabel, isFiltered, isReversible, systemLabel, systemsIn, tagHue, tagsIn,
    toggled, visibleGames, type LibraryFilter, type SortDirection, type SortKey,
} from '../libraryView';

const SORTS: readonly { key: SortKey; label: string }[] = [
    { key: 'recent', label: '最近游玩' },
    { key: 'added', label: '添加时间' },
    { key: 'name', label: '名称a-z' },
    { key: 'playtime', label: '游玩时长' },
    { key: 'size', label: '体积大小' },
];

/**
 * How many tag words a card has room for. The rest are counted.
 *
 * One, because a tile is wide enough for about one word: at two, a card with
 * two Chinese tags wrapped onto a second line and made every cell in its row
 * 17px taller -- and a grid whose rows are all slightly different heights
 * looks broken rather than compact. The full list is in the editor, and the
 * count carries it in the tooltip.
 */
const CARD_TAG_SLOTS = 1;

/**
 * A colour for a game with no screenshot.
 *
 * A hash of the name, folded into a hue. Two things matter and neither is
 * taste: it has to be the same colour every time the application starts, which
 * rules out anything random, and two games side by side have to look
 * different, which is what the hash is for. Saturation and lightness are fixed
 * so that every one of them has enough contrast for white text.
 */
function coverHue(name: string): number {
    let hash = 0;
    for (let i = 0; i < name.length; i += 1) {
        hash = (hash * 31 + name.codePointAt(i)!) % 360;
    }
    return hash;
}

/** The card's picture: the cover, or the coloured block that stands in for it. */
function Cover({ game }: { game: GameEntry }) {
    if (game.cover !== null) {
        return (
            <img
                className="cover-image"
                src={libraryAssetUrl(game.cover)}
                alt=""
                loading="lazy"
                draggable={false}
            />
        );
    }
    return (
        <span
            className="cover-blank"
            style={{ '--cover-hue': coverHue(game.name) } as CSSProperties}
        >
            <span className="cover-blank-name">{game.name}</span>
        </span>
    );
}

/**
 * A tag's colour, which is the tag's name.
 *
 * The square is the label: a word on its own is a word, a word beside a square
 * is the same thing in a second channel, and the second channel is what a
 * glance can sort through. Shared by the chips, the cards and the editor so
 * that one tag is one colour everywhere on the screen.
 */
function TagSquare({ name }: { name: string }) {
    return <span className="tag-square" style={{ '--tag-hue': tagHue(name) } as CSSProperties} />;
}

/**
 * The tag editor, drawn inside the card's cell while the tag button is on.
 *
 * Plain React rather than a popover: this application carries three Radix
 * primitives and none of them is one, and this is a text field and a list of
 * words. It replaces the whole list on every change, which is what the main
 * process stores -- so what is on screen and what is in the database are one
 * value, and the card that redraws under the editor is already the new one.
 */
function TagEditor({ game, allTags, onSetTags, onClose }: {
    game: GameEntry;
    /** Every tag in the library, for the suggestions the field offers. */
    allTags: readonly string[];
    onSetTags: (path: string, tags: string[]) => void;
    onClose: () => void;
}) {
    const [draft, setDraft] = useState('');
    const suggestions = useId();

    const add = (): void => {
        const name = draft.trim();
        if (name === '') {
            return;
        }
        onSetTags(game.path, [...game.tags, name]);
        setDraft('');
    };

    return (
        <div className="tag-editor">
            <div className="tag-editor-row">
                {game.tags.map((tag) => (
                    <span key={tag} className="tag-chip">
                        <TagSquare name={tag} />
                        {tag}
                        <button
                            type="button"
                            onClick={() => onSetTags(
                                game.path,
                                game.tags.filter((other) => other !== tag),
                            )}
                            title={`移除标签 ${tag}`}
                            aria-label={`移除标签 ${tag}`}
                        >
                            <X size={10} />
                        </button>
                    </span>
                ))}
                {game.tags.length === 0 && (
                    <span className="tag-editor-empty">还没有标签</span>
                )}
            </div>

            <div className="tag-editor-input">
                <input
                    value={draft}
                    onChange={(event) => setDraft(event.target.value)}
                    onKeyDown={(event) => {
                        if (event.key === 'Enter') {
                            event.preventDefault();
                            add();
                        } else if (event.key === 'Escape') {
                            onClose();
                        }
                        // Nothing is stopped from propagating to the game:
                        // the input manager ignores anything typed into a
                        // field (see input.ts), and the library is on screen
                        // precisely because nothing should be playing.
                    }}
                    placeholder="输入标签，回车添加"
                    list={suggestions}
                    maxLength={24}
                    spellCheck={false}
                    autoComplete="off"
                    aria-label="添加标签"
                    autoFocus
                />
                <button
                    type="button"
                    className="icon-button"
                    onClick={add}
                    title="添加标签"
                    aria-label="添加标签"
                >
                    <Plus size={12} />
                </button>
            </div>

            <datalist id={suggestions}>
                {allTags.map((tag) => <option key={tag} value={tag} />)}
            </datalist>
        </div>
    );
}

/**
 * The first card in the grid, when there is a game nobody has played yet.
 *
 * It exists because the gap between "I copied this in" and "I am playing
 * it" is otherwise a search through a grid for a name you just typed. It is
 * a card rather than a dialog because it is not a question: nothing is asked
 * and nothing has to be dismissed.
 *
 * The newest addition is the one offered; the rest sit beside it as small
 * covers, because a folder dropped on the window can be ten games and the
 * card should not become a list. It counts as a card in the grid (it is an
 * `<li>` with the cards) but not as a card in the count: the header counts
 * games.
 */
function NewGamesCard({ games, onPick }: {
    /** Never-played games, newest addition first. Never empty. */
    games: readonly GameEntry[];
    onPick: (path: string) => void;
}) {
    const [newest, ...rest] = games;

    return (
        <li className="card-new">
            <span className="card-new-cover">
                <Cover game={newest} />
            </span>

            <span className="card-new-body">
                <span className="card-new-tag">新添加</span>
                <span className="card-new-name" title={newest.path}>{newest.name}</span>
                <span className="card-new-when">
                    {rest.length === 0
                        ? `${formatWhen(newest.addedAt)}添加 · ${emulatorLabel(newest)}`
                        : `共 ${games.length} 个新游戏 · 添加于${formatWhen(newest.addedAt)}`}
                </span>
            </span>

            {rest.length > 0 && (
                <span className="card-new-others">
                    {rest.slice(0, 3).map((game) => (
                        <button
                            key={game.path}
                            type="button"
                            className="card-new-other"
                            onClick={() => onPick(game.path)}
                            title={`立即游玩 ${game.name}`}
                            aria-label={`立即游玩 ${game.name}`}
                        >
                            <Cover game={game} />
                        </button>
                    ))}
                    {rest.length > 3 && (
                        <span className="card-new-more">+{rest.length - 3}</span>
                    )}
                </span>
            )}

            <button
                type="button"
                className="button button-primary card-new-play"
                onClick={() => onPick(newest.path)}
            >
                <Play size={13} />
                立即游玩
            </button>
        </li>
    );
}

interface LibraryPanelProps {
    title: string;
    directory: string;
    games: GameEntry[];
    loading: boolean;
    query: string;
    onQuery: (query: string) => void;
    sort: SortKey;
    onSort: (sort: SortKey) => void;
    /** The direction of every order. Only the selected order's is applied;
     *  the rest are remembered so that coming back to an order finds it the
     *  way it was left. */
    directions: Record<SortKey, SortDirection>;
    /** Turn one order round, by pressing the chip that is already on. */
    onFlip: (sort: SortKey) => void;
    /** The emulator and tag filters, and how to change them. */
    filter: LibraryFilter;
    onFilter: (filter: LibraryFilter) => void;
    /** Never-played games, newest first, for the card at the top of the grid.
     *  Empty everywhere except the whole-library section: "new" is a fact
     *  about the library, not about the list somebody is looking at. */
    fresh: GameEntry[];
    /** The cartridge in the slot, so the list can mark the card. */
    activePath: string | null;
    onPick: (path: string) => void;
    onAdd: () => void;
    onChooseDirectory: () => void;
    onOpenFolder: () => void;
    onSetTags: (path: string, tags: string[]) => void;
    onTogglePinned: (path: string, pinned: boolean) => void;
    onRemove: (path: string) => void;
    /** What to say when there is nothing to show. Different sections are empty
     *  for different reasons -- no games at all, or none pinned. */
    emptyTitle: string;
}

export default function LibraryPanel({
    title,
    directory,
    games,
    loading,
    query,
    onQuery,
    sort,
    onSort,
    directions,
    onFlip,
    filter,
    onFilter,
    fresh,
    activePath,
    onPick,
    onAdd,
    onChooseDirectory,
    onOpenFolder,
    onSetTags,
    onTogglePinned,
    onRemove,
    emptyTitle,
}: LibraryPanelProps) {
    /** The card whose tag editor is open, by path, or null. One at a time:
     *  a grid full of open text fields is not a thing anybody asked for. */
    const [editingTags, setEditingTags] = useState<string | null>(null);

    // Named by the visible labels rather than by a second copy of the same
    // words in `aria-label`: see where they are used.
    const filterLabelId = useId();
    const sortLabelId = useId();

    // Only the selected order's direction is in effect; the rest are just
    // remembered by the control.
    const shown = visibleGames(games, query, filter, sort, directions[sort]);

    // The chips come from the games, so a console or a word nothing in the
    // library carries is not offered. Two exceptions, both of which are the
    // same rule: a control that is doing something has to stay on screen.
    // A console is only worth offering while more than one is present, but a
    // console that is *selected* stays even after the last game of the other
    // one is deleted, or the filter would be stuck on with nothing to unpress.
    // And a selected tag is added back for exactly that reason.
    const present = systemsIn(games);
    const systemChips = [...new Set([...(present.length > 1 ? present : []), ...filter.systems])];
    const tagChips = [...new Set([...tagsIn(games), ...filter.tags])];

    return (
        <section className="panel" aria-label={title}>
            <header className="panel-head">
                <div className="panel-title">
                    <h2>{title}</h2>
                    <span className="panel-count">
                        {loading ? '读取中…' : `${shown.length} / ${games.length}`}
                    </span>
                    <span className="panel-title-actions">
                        <button
                            type="button"
                            className="icon-button"
                            onClick={onAdd}
                            title="添加游戏（拷贝到游戏库）"
                            aria-label="添加游戏"
                        >
                            <Plus size={14} />
                        </button>
                        <button
                            type="button"
                            className="icon-button"
                            onClick={onChooseDirectory}
                            title="选择游戏库文件夹"
                            aria-label="选择游戏库文件夹"
                        >
                            <FolderCog size={14} />
                        </button>
                    </span>
                </div>

                <div className="panel-tools">
                    <div className="search">
                        <Search size={13} aria-hidden="true" />
                        <input
                            type="search"
                            value={query}
                            onChange={(event) => onQuery(event.target.value)}
                            placeholder="搜索游戏"
                            spellCheck={false}
                            autoComplete="off"
                        />
                        {query !== '' && (
                            <button type="button" onClick={() => onQuery('')} aria-label="清除搜索">
                                <X size={12} />
                            </button>
                        )}
                    </div>

                    {/* The two filters, drawn as the same chip twice. A chip is
                        a toggle, so it is a button with aria-pressed rather
                        than a checkbox: it does not submit, and it has no
                        label to point at.

                        The word and the chips are children of the *same*
                        flex row, not a label with a chip container beside it.
                        That is what makes the group wrap as one thing: a
                        second line of chips starts under the word, at the left
                        edge of the row, instead of starting under the first
                        chip and reading as a second block.

                        The group is named by that word through
                        aria-labelledby rather than by a copy of it in
                        aria-label: one string, so the screen reader and the
                        screen cannot say different things. */}
                    {(systemChips.length > 0 || tagChips.length > 0) && (
                        <div
                            className="tool-row filters"
                            role="group"
                            aria-labelledby={filterLabelId}
                        >
                            <span className="tool-label" id={filterLabelId}>筛选</span>
                            {systemChips.map((system) => (
                                <button
                                    key={system}
                                    type="button"
                                    className="filter-chip"
                                    aria-pressed={filter.systems.includes(system)}
                                    onClick={() => onFilter({
                                        ...filter,
                                        systems: toggled(filter.systems, system),
                                    })}
                                >
                                    {systemLabel(system)}
                                </button>
                            ))}
                            {tagChips.map((tag) => (
                                <button
                                    key={`tag:${tag}`}
                                    type="button"
                                    className="filter-chip"
                                    aria-pressed={filter.tags.includes(tag)}
                                    onClick={() => onFilter({
                                        ...filter,
                                        tags: toggled(filter.tags, tag),
                                    })}
                                    title={`筛选标签 ${tag}`}
                                >
                                    <TagSquare name={tag} />
                                    {tag}
                                </button>
                            ))}
                            {isFiltered(filter) && (
                                <button
                                    type="button"
                                    className="filter-chip filter-clear"
                                    onClick={() => onFilter(NO_FILTER)}
                                >
                                    清除筛选
                                </button>
                            )}
                        </div>
                    )}

                    {/* The orders, as chips that wrap rather than as one bar.
                        Five orders with an arrow each do not fit on one line
                        of the middle column, and a control that cannot wrap
                        either clips its labels or shrinks them. There is no
                        card around the row: the chip that is in effect is the
                        only one that needs a background, and the other four
                        are things you might pick.

                        It is still a single-select group, so exactly one
                        order is ever in effect. Each chip carries its own
                        arrow, and pressing the chip that is already on turns
                        that one round: the direction belongs to the order, so
                        it is set where the order is chosen.

                        The label is a child of the group -- the one child that
                        is not a chip -- so that the word and the chips wrap as
                        a single row and a wrapped chip lands at the left edge
                        of that row. A radiogroup with a span in it is not
                        perfectly pure, and the alternative is a second block
                        of chips under the first, misaligned with the word. */}
                    <ToggleGroup.Root
                        type="single"
                        value={sort}
                        onValueChange={(value) => {
                            // Empty is how Radix reports "the selected item was
                            // pressed again", which a single-select group must
                            // not treat as "no order": for an order with two
                            // ends it means the opposite one, and for one with
                            // a single end it means nothing to do.
                            if (value === '') {
                                if (isReversible(sort)) {
                                    onFlip(sort);
                                }
                                return;
                            }
                            onSort(value as SortKey);
                        }}
                        className="tool-row sort-chips"
                        aria-labelledby={sortLabelId}
                    >
                        <span className="tool-label" id={sortLabelId}>排序</span>
                        {SORTS.map((option) => {
                            const active = option.key === sort;
                            const up = directions[option.key] === 'asc';
                            // "Recently played" has one end, so it gets no
                            // arrow and no second title: there is nothing to
                            // say about a direction that cannot be chosen.
                            const reversible = isReversible(option.key);
                            return (
                                <ToggleGroup.Item
                                    key={option.key}
                                    value={option.key}
                                    className="sort-chip"
                                    title={!reversible
                                        ? '最近玩过的排在前面，顺序固定'
                                        : active
                                            ? `当前${up ? '升' : '降'}序，再点一次改为${up ? '降' : '升'}序`
                                            : `${up ? '升' : '降'}序排列`}
                                >
                                    {option.label}
                                    {reversible && (
                                        <span
                                            className="sort-chip-arrow"
                                            data-active={active}
                                            aria-hidden="true"
                                        >
                                            {up ? <ArrowUp size={10} /> : <ArrowDown size={10} />}
                                        </span>
                                    )}
                                </ToggleGroup.Item>
                            );
                        })}
                    </ToggleGroup.Root>
                </div>
            </header>

            <div className="panel-scroll">
                {!loading && games.length === 0 && (
                    <div className="empty">
                        <p>{emptyTitle}</p>
                        <p className="empty-path">{directory}</p>
                        <button type="button" className="button" onClick={onAdd}>
                            <Plus size={14} />
                            添加游戏
                        </button>
                        <button type="button" className="button" onClick={onOpenFolder}>
                            <FolderOpen size={14} />
                            在访达中打开
                        </button>
                        <p className="empty-hint">
                            添加会把手选的 ROM <strong>拷贝</strong>进游戏库，
                            原文件留在原处。也可以把文件或文件夹直接拖进窗口。
                        </p>
                    </div>
                )}

                {!loading && games.length > 0 && shown.length === 0 && (
                    <div className="empty">
                        <p>
                            {query.trim() !== ''
                                ? `没有匹配「${query}」的游戏。`
                                : isFiltered(filter)
                                    ? '没有符合当前筛选的游戏。'
                                    : emptyTitle}
                        </p>
                    </div>
                )}

                <ul className="cards">
                    {fresh.length > 0 && query.trim() === '' && !isFiltered(filter) && (
                        <NewGamesCard games={fresh} onPick={onPick} />
                    )}

                    {shown.map((game) => {
                        const playing = game.path === activePath;
                        const editing = editingTags === game.path;
                        const classes = ['card-cell'];
                        if (playing) {
                            classes.push('card-playing');
                        }
                        if (game.pinned) {
                            classes.push('card-pinned');
                        }
                        if (editing) {
                            classes.push('card-editing-tags');
                        }
                        return (
                            <li key={game.path} className={classes.join(' ')}>
                                <button
                                    type="button"
                                    className="card"
                                    onClick={() => onPick(game.path)}
                                    title={game.path}
                                >
                                    <span className="card-cover">
                                        <Cover game={game} />
                                        <span className="card-system">
                                            {emulatorLabel(game)}
                                        </span>
                                    </span>
                                    <span className="card-name">{game.name}</span>
                                    {game.tags.length > 0 && (
                                        <span className="card-tags">
                                            {game.tags.slice(0, CARD_TAG_SLOTS).map((tag) => (
                                                <span key={tag} className="card-tag" title={tag}>
                                                    <TagSquare name={tag} />
                                                    {tag}
                                                </span>
                                            ))}
                                            {/* The rest are counted, not listed: a
                                                tile is wide enough for about one
                                                word, and a second line of tags
                                                makes every cell in the row
                                                taller. The editor is where the
                                                full list lives. */}
                                            {game.tags.length > CARD_TAG_SLOTS && (
                                                <span
                                                    className="card-tag card-tag-more"
                                                    title={game.tags.join('、')}
                                                >
                                                    +{game.tags.length - CARD_TAG_SLOTS}
                                                </span>
                                            )}
                                        </span>
                                    )}
                                    <span className="card-meta">
                                        {formatSize(game.size)}
                                        {game.playSeconds > 0 && (
                                            <span
                                                className="card-playtime"
                                                title={`累计游玩 ${formatDuration(game.playSeconds)}`}
                                            >
                                                {formatDuration(game.playSeconds)}
                                            </span>
                                        )}
                                        <span className="card-when">{formatWhen(game.lastPlayedAt)}</span>
                                    </span>
                                </button>

                                <span className="card-actions">
                                    {game.screenshots > 0 && (
                                        <span className="card-shots" title={`${game.screenshots} 张截图`}>
                                            <Camera size={10} />
                                            {game.screenshots}
                                        </span>
                                    )}
                                    <button
                                        type="button"
                                        className="icon-button"
                                        aria-pressed={editing}
                                        onClick={() => setEditingTags(editing ? null : game.path)}
                                        title={editing ? '收起标签' : '编辑标签'}
                                        aria-label={editing ? '收起标签' : '编辑标签'}
                                    >
                                        <Tag size={12} />
                                    </button>
                                    <button
                                        type="button"
                                        className="icon-button"
                                        aria-pressed={game.pinned}
                                        onClick={() => onTogglePinned(game.path, !game.pinned)}
                                        title={game.pinned ? '取消置顶' : '置顶'}
                                        aria-label={game.pinned ? '取消置顶' : '置顶'}
                                    >
                                        <Pin size={12} />
                                    </button>
                                    <button
                                        type="button"
                                        className="icon-button"
                                        onClick={() => onRemove(game.path)}
                                        title="从游戏库删除"
                                        aria-label="从游戏库删除"
                                    >
                                        <Trash2 size={12} />
                                    </button>
                                </span>

                                {editing && (
                                    <TagEditor
                                        game={game}
                                        allTags={tagChips}
                                        onSetTags={onSetTags}
                                        onClose={() => setEditingTags(null)}
                                    />
                                )}
                            </li>
                        );
                    })}
                </ul>

                {directory !== '' && (
                    <p className="panel-foot" title={directory}>
                        {directory}
                    </p>
                )}
            </div>
        </section>
    );
}
