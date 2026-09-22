// ---------------------------------------------------------------------------
// Cheats -- the middle column, in the 金手指 section.
//
// A cheat is a byte, at an address, put back. The panel is the "at an address"
// part: the player says where and what, and the emulator keeps writing it. It
// does not know what lives are, and this screen does not either -- the address
// is the player's to find.
//
// The readout next to every row is what makes finding one possible at all: you
// watch a number, lose a life, and watch it change. That is the whole method,
// done by eye; a search that does it automatically is the next layer on top of
// exactly this `peek`.
// ---------------------------------------------------------------------------

import { useEffect, useRef, useState } from 'react';
import { Plus, Trash2, Zap } from 'lucide-react';

import type { Cheat } from '../../shared/api';
import { emptyCheat, formatAddress, parseAddress, parseValue } from '../cheats';

interface CheatsPanelProps {
    /** The game's title, or null when there is no cartridge in the slot. */
    game: string | null;
    cheats: Cheat[];
    onCheats: (next: Cheat[]) => void;
    /** Read one byte of console RAM, or null with no machine. */
    peek: (address: number) => number | null;
    /** Write one byte, once. */
    poke: (address: number, value: number) => void;
}

/** How often the live readout is refreshed. Fast enough to watch a number
 *  change as the game does something, slow enough not to be a render storm. */
const READOUT_MS = 250;

function CheatRow({ cheat, onChange, onRemove, onPoke, peek }: {
    cheat: Cheat;
    onChange: (next: Cheat) => void;
    onRemove: () => void;
    onPoke: () => void;
    peek: (address: number) => number | null;
}) {
    // Text as typed, kept apart from the parsed value so that a half-typed
    // address is not thrown away underneath the cursor.
    const [address, setAddress] = useState(() => formatAddress(cheat.address));
    const [value, setValue] = useState(() => String(cheat.value));
    const [live, setLive] = useState<number | null>(null);

    // Follow the stored values when they change from elsewhere (a load, a
    // reset of the list).
    useEffect(() => { setAddress(formatAddress(cheat.address)); }, [cheat.address]);
    useEffect(() => { setValue(String(cheat.value)); }, [cheat.value]);

    // The readout. `peek` is rebuilt by the shell on every render, so it goes
    // in a ref: an interval that re-registers itself four times a second would
    // never actually fire on schedule.
    const peekRef = useRef(peek);
    peekRef.current = peek;
    useEffect(() => {
        const read = (): void => setLive(peekRef.current(cheat.address));
        read();
        const timer = window.setInterval(read, READOUT_MS);
        return () => window.clearInterval(timer);
    }, [cheat.address]);

    const onAddress = (text: string): void => {
        setAddress(text);
        const parsed = parseAddress(text);
        if (parsed !== null) {
            onChange({ ...cheat, address: parsed });
        }
    };

    const onValue = (text: string): void => {
        setValue(text);
        const parsed = parseValue(text);
        if (parsed !== null) {
            onChange({ ...cheat, value: parsed });
        }
    };

    return (
        <li className={cheat.enabled ? 'cheat-row' : 'cheat-row cheat-row-off'}>
            <div className="cheat-main">
                <input
                    className="cheat-label"
                    type="text"
                    value={cheat.label}
                    placeholder="备注"
                    spellCheck={false}
                    onChange={(event) => onChange({ ...cheat, label: event.target.value })}
                />
                <input
                    className="cheat-address"
                    type="text"
                    value={address}
                    spellCheck={false}
                    aria-label="地址"
                    onChange={(event) => onAddress(event.target.value)}
                    onBlur={() => setAddress(formatAddress(cheat.address))}
                />
                <input
                    className="cheat-value"
                    type="text"
                    value={value}
                    spellCheck={false}
                    aria-label="值"
                    onChange={(event) => onValue(event.target.value)}
                    onBlur={() => setValue(String(cheat.value))}
                />
                <span className="cheat-live" title="当前值">
                    {live === null ? '—' : live}
                </span>
            </div>

            <div className="cheat-actions">
                <label className="cheat-toggle" title="每帧重写">
                    <input
                        type="checkbox"
                        checked={cheat.freeze}
                        onChange={(event) => onChange({ ...cheat, freeze: event.target.checked })}
                    />
                    锁定
                </label>
                <label className="cheat-toggle" title="启用">
                    <input
                        type="checkbox"
                        checked={cheat.enabled}
                        onChange={(event) => onChange({ ...cheat, enabled: event.target.checked })}
                    />
                    启用
                </label>
                <button
                    type="button"
                    className="icon-button"
                    onClick={onPoke}
                    title="写入一次"
                    aria-label="写入一次"
                >
                    <Zap size={14} />
                </button>
                <button
                    type="button"
                    className="icon-button"
                    onClick={onRemove}
                    title="删除"
                    aria-label="删除"
                >
                    <Trash2 size={14} />
                </button>
            </div>
        </li>
    );
}

export default function CheatsPanel({ game, cheats, onCheats, peek, poke }: CheatsPanelProps) {
    const replace = (index: number, next: Cheat): void => {
        onCheats(cheats.map((cheat, position) => (position === index ? next : cheat)));
    };

    const remove = (index: number): void => {
        onCheats(cheats.filter((_, position) => position !== index));
    };

    return (
        <section className="panel" aria-label="金手指">
            <header className="panel-head">
                <div className="panel-title">
                    <h2>金手指</h2>
                    <span className="panel-count">
                        {game === null ? '未插入卡带' : `${cheats.length} 条`}
                    </span>
                </div>
            </header>

            <div className="panel-scroll">
                {game === null ? (
                    <p className="prose">
                        金手指改的是游戏在内存里的一个字节。先插入卡带，再添加一条：
                        填地址（十六进制，例如 <code>$075A</code>）和值，勾上「锁定」
                        让模拟器每帧都把它写回去。
                    </p>
                ) : (
                    <>
                        <div className="cheat-game">{game}</div>

                        {cheats.length === 0 ? (
                            <p className="prose">
                                还没有金手指。地址靠看：下面的「当前值」每 0.25 秒刷新一次，
                                在游戏里丢一条命，看哪个地址跟着变，就是它。
                            </p>
                        ) : (
                            <ul className="cheat-list">
                                {cheats.map((cheat, index) => (
                                    <CheatRow
                                        key={index}
                                        cheat={cheat}
                                        onChange={(next) => replace(index, next)}
                                        onRemove={() => remove(index)}
                                        onPoke={() => poke(cheat.address, cheat.value)}
                                        peek={peek}
                                    />
                                ))}
                            </ul>
                        )}

                        <div className="panel-actions">
                            <button
                                type="button"
                                className="button"
                                onClick={() => onCheats([...cheats, emptyCheat()])}
                            >
                                <Plus size={13} />
                                添加金手指
                            </button>
                        </div>

                        <p className="prose">
                            「锁定」是每帧写回，游戏的修改会在下一帧被覆盖；不勾就只在
                            点<i>写入一次</i>时写一次。金手指存在配置文件里，按卡带分开。
                        </p>
                    </>
                )}
            </div>
        </section>
    );
}
