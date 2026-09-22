// ---------------------------------------------------------------------------
// Save states -- the middle column, in the 存档 section.
//
// Four slots, because that is what the keyboard has: F5/F6 for the quick slot
// and F1-F3 (with Shift to load) for the other three. The panel is arranged
// the other way round from how it is usually drawn -- what each slot is bound
// to comes first, and the buttons second -- because the bindings are the part
// a player has to learn and the buttons are the part they can see.
//
// The main process owns the files. This panel asks it which slots have
// something in them and never learns a path, which is why it can draw the
// whole thing from a list of integers.
// ---------------------------------------------------------------------------

import { Download, Eject, Upload } from 'lucide-react';

interface SlotInfo {
    slot: number;
    label: string;
    keys: string;
}

const SLOTS: readonly SlotInfo[] = [
    { slot: 0, label: '快速存档', keys: 'F5 / F6' },
    { slot: 1, label: '存档槽 1', keys: 'F1 / ⇧F1' },
    { slot: 2, label: '存档槽 2', keys: 'F2 / ⇧F2' },
    { slot: 3, label: '存档槽 3', keys: 'F3 / ⇧F3' },
];

interface SavesPanelProps {
    game: string | null;
    saves: number[];
    onSave: (slot: number) => void;
    onLoad: (slot: number) => void;
    onEject: () => void;
}

export default function SavesPanel({ game, saves, onSave, onLoad, onEject }: SavesPanelProps) {
    const hasGame = game !== null;

    return (
        <section className="panel" aria-label="存档">
            <header className="panel-head">
                <div className="panel-title">
                    <h2>存档</h2>
                    <span className="panel-count">{hasGame ? game : '没有卡带'}</span>
                </div>
            </header>

            <div className="panel-scroll">
                {!hasGame && (
                    <div className="empty">
                        <p>先选一个游戏，再存档。</p>
                        <p className="empty-hint">存档按卡带名分开放在应用数据目录里。</p>
                    </div>
                )}

                {hasGame && (
                    <>
                        <ul className="slots">
                            {SLOTS.map((info) => {
                                const filled = saves.includes(info.slot);
                                return (
                                    <li key={info.slot} className="slot">
                                        <span className={filled ? 'slot-badge slot-badge-full' : 'slot-badge'}>
                                            {info.slot}
                                        </span>
                                        <span className="slot-body">
                                            <span className="slot-label">{info.label}</span>
                                            <span className="slot-state">
                                                {filled ? '有存档' : '空'}
                                                <span className="slot-keys">{info.keys}</span>
                                            </span>
                                        </span>
                                        <span className="slot-actions">
                                            <button
                                                type="button"
                                                className="button"
                                                onClick={() => onSave(info.slot)}
                                                title={`保存到${info.label}`}
                                            >
                                                <Download size={13} />
                                                存档
                                            </button>
                                            <button
                                                type="button"
                                                className="button"
                                                onClick={() => onLoad(info.slot)}
                                                disabled={!filled}
                                                title={filled ? `读取${info.label}` : '这个槽是空的'}
                                            >
                                                <Upload size={13} />
                                                读取
                                            </button>
                                        </span>
                                    </li>
                                );
                            })}
                        </ul>

                        <div className="panel-actions">
                            <button type="button" className="button" onClick={onEject}>
                                <Eject size={13} />
                                弹出卡带
                            </button>
                        </div>
                    </>
                )}
            </div>
        </section>
    );
}
