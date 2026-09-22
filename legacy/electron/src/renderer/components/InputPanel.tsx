// ---------------------------------------------------------------------------
// Input settings -- which keys and which pads drive which player.
//
// The console has two controller ports, and what this panel decides is how
// the two things a person can hold reach them:
//
//   the keyboard, as one player or two
//   every connected pad, as player 1, player 2, or nothing
//
// The keyboard's two-player default is a convention, not a guess the
// application can make: one hand on WASD and the other on the arrows is the
// only arrangement that lets two people share one keyboard. In one-player mode
// the same bindings are read as one person, so both clusters work -- which is
// exactly what the keyboard did before there was a second port to think about.
//
// Rebinding captures the next key the player presses. The capture is a
// window listener in the *capture* phase, which is what keeps the key from
// reaching the game: attachKeyboard listens in the bubble phase, so
// stopImmediatePropagation here means "W" does not walk Mario while it is being
// assigned.
// ---------------------------------------------------------------------------

import { useCallback, useEffect, useState } from 'react';
import { RotateCcw, X } from 'lucide-react';

import {
    COMMAND_ORDER, EXTRA_PAD_BUTTONS, commandBindings,
    type CommandBinding, type CommandKey, type CommandName, type ExtraPadButtonName,
    type HeldCommandName, type InputSettings, type KeyBinding, type PadCombo,
} from '../../shared/api';
import type { PadSummary } from '../gamepad';
import PadTester from './PadTester';
import { NO_CAPTURE, captureDone, captureUpdate, type ComboCapture } from '../commands';
import { activeBindings, padPort } from '../bindings';

interface InputPanelProps {
    input: InputSettings;
    /** Change the settings and remember them. */
    onInput: (next: InputSettings) => void;
    /** Every pad that is connected right now. */
    pads: PadSummary[];
    gamepadEnabled: boolean;
    gamepadNative: boolean;
}

/**
 * Everything a switch binding may not take: the keys the commands are on.
 *
 * The commands are sorted out below, so this is only about the *switch* rows:
 * a key that pauses the game should not also be the one that walks left, and
 * the row would otherwise accept it and make the command unreachable.
 */
function commandKeysInUse(input: InputSettings): Set<string> {
    const keys = new Set<string>();
    for (const binding of commandBindings(input)) {
        if (binding.key !== null) {
            keys.add(binding.key.code);
        }
    }
    return keys;
}

/** What a command is called on screen. */
const COMMAND_LABELS: Record<CommandName | HeldCommandName, string> = {
    pause: '暂停 / 继续',
    screenshot: '截图',
    'screenshot-cover': '截图并设为封面',
    quicksave: '快速存档',
    quickload: '快速读档',
    reset: '重置',
    save1: '存档到槽 1',
    save2: '存档到槽 2',
    save3: '存档到槽 3',
    load1: '读取槽 1',
    load2: '读取槽 2',
    load3: '读取槽 3',
    rewind: '倒带（按住）',
};

/** How a pad button reads on a chip. */
const PAD_BUTTON_LABELS: Record<ExtraPadButtonName, string> = {
    L1: 'L1', R1: 'R1', L2: 'L2', R2: 'R2', L3: 'L3', R3: 'R3',
    FACE_X: 'X', FACE_Y: 'Y', GUIDE: '⌂',
};

/** A key and its Shift, as one keycap reads. */
function keycapLabel(key: CommandKey): string {
    return key.shift ? `Shift+${keyLabel(key.code)}` : keyLabel(key.code);
}

/** How a key code reads on screen. */
function keyLabel(code: string): string {
    const special: Record<string, string> = {
        ArrowUp: '↑',
        ArrowDown: '↓',
        ArrowLeft: '←',
        ArrowRight: '→',
        Space: '空格',
        Enter: '回车',
        NumpadEnter: '小键盘回车',
        Tab: 'Tab',
        ShiftLeft: '左 Shift',
        ShiftRight: '右 Shift',
        Backspace: 'Backspace',
        Escape: 'Esc',
    };
    if (special[code] !== undefined) {
        return special[code];
    }
    if (code.startsWith('Key')) {
        return code.slice(3);
    }
    if (code.startsWith('Digit')) {
        return code.slice(5);
    }
    if (code.startsWith('Numpad')) {
        return `小键盘${code.slice(6)}`;
    }
    return code;
}

/** What a switch is called on the console. */
const SWITCH_LABELS: Record<string, string> = {
    A: 'A', B: 'B', SELECT: 'Select', START: 'Start',
    UP: '上', DOWN: '下', LEFT: '左', RIGHT: '右',
};

const PORT_LABELS = ['1P', '2P', '关闭'];

function playerName(port: number): string {
    return port === 0 ? '玩家 1' : port === 1 ? '玩家 2' : '不使用';
}

/**
 * One binding row, with its own capture state.
 *
 * The capture is per row rather than one for the whole panel, because the
 * thing being edited is the row and a panel-wide "who is capturing" flag is a
 * second piece of state that can disagree with it.
 */
function BindingRow({ binding, reserved, onRebind }: {
    binding: KeyBinding;
    /** Keys the commands are on. See commandKeysInUse. */
    reserved: ReadonlySet<string>;
    onRebind: (code: string) => void;
}) {
    const [capturing, setCapturing] = useState(false);

    useEffect(() => {
        if (!capturing) {
            return;
        }
        const onKey = (event: KeyboardEvent): void => {
            event.preventDefault();
            // Stop the game from seeing it. attachKeyboard is a bubble phase
            // listener, so this capture phase one runs first and can end it.
            event.stopImmediatePropagation();

            if (event.code === 'Escape') {
                setCapturing(false);
                return;
            }
            if (reserved.has(event.code)) {
                // That key fires a command. Keep capturing rather than binding
                // it away, and rather than making the command unreachable: the
                // switches are sorted out down here, the commands up there.
                return;
            }
            onRebind(event.code);
            setCapturing(false);
        };
        window.addEventListener('keydown', onKey, true);
        return () => window.removeEventListener('keydown', onKey, true);
    }, [capturing, reserved, onRebind]);

    return (
        <div className="row binding-row">
            <dt>
                <span className={`port-tag port-${binding.port}`}>{PORT_LABELS[binding.port]}</span>
                {SWITCH_LABELS[binding.button]}
            </dt>
            <dd>
                <button
                    type="button"
                    className={capturing ? 'keycap keycap-capturing' : 'keycap'}
                    onClick={() => setCapturing(true)}
                    title="点击后按下新的按键"
                >
                    {capturing ? '按下按键…' : keyLabel(binding.code)}
                </button>
            </dd>
        </div>
    );
}

/**
 * One command: what it is, the key that fires it, and the pad buttons that do.
 *
 * The capture is the same trick as the switch rows above -- a window listener
 * in the capture phase, so the key does not reach the game while it is being
 * assigned -- with one difference: `Shift` is captured as *part* of the key.
 * F1 saves and Shift+F1 loads, and the two are separate bindings rather than a
 * key and a modifier.
 */
function CommandRow({ binding, down, onKey, onPads }: {
    binding: CommandBinding;
    /** What is down on the pads right now, for the capture below. */
    down: readonly ExtraPadButtonName[];
    onKey: (key: CommandKey | null) => void;
    onPads: (pads: PadCombo[]) => void;
}) {
    const [capturing, setCapturing] = useState(false);
    const [capturingPad, setCapturingPad] = useState<ComboCapture | null>(null);

    /**
     * Watch the pads until the player lets go, then bind what they held.
     *
     * A chord is what is down *at once*, so what is bound is the largest set
     * that was down together -- and the screen says which buttons those are
     * while it waits, so nobody has to guess what is about to be saved.
     */
    useEffect(() => {
        if (capturingPad === null) {
            return;
        }
        const next = captureUpdate(capturingPad, down);
        if (next !== capturingPad) {
            setCapturingPad(next);
        }
        const done = captureDone(next);
        if (done !== null) {
            onPads([done]);
            setCapturingPad(null);
        }
    }, [capturingPad, down, onPads]);

    useEffect(() => {
        if (!capturing) {
            return;
        }
        const onKeyDown = (event: KeyboardEvent): void => {
            event.preventDefault();
            event.stopImmediatePropagation();
            if (event.code === 'Escape' && !event.shiftKey) {
                // Escape on its own is the way out of the capture -- which is
                // also the default pause key, so binding it would leave no way
                // to cancel. Shift+Escape is still assignable.
                setCapturing(false);
                return;
            }
            onKey({ code: event.code, shift: event.shiftKey });
            setCapturing(false);
        };
        window.addEventListener('keydown', onKeyDown, true);
        return () => window.removeEventListener('keydown', onKeyDown, true);
    }, [capturing, onKey]);

    /** The chord the chips edit. The file may hold more than one; the screen
     *  edits the first, which is what "the chord for this command" means. */
    const chord = binding.pads[0] ?? [];

    const togglePad = (button: ExtraPadButtonName): void => {
        const next = chord.includes(button)
            ? chord.filter((other) => other !== button)
            : [...chord, button];
        onPads(next.length === 0 ? [] : [[...next].sort()]);
    };

    return (
        <div className="row command-row">
            <dt>{COMMAND_LABELS[binding.command]}</dt>
            <dd className="command-bindings">
                <span className="keycap-wrap">
                    <button
                        type="button"
                        className={capturing ? 'keycap keycap-capturing' : 'keycap'}
                        onClick={() => setCapturing(true)}
                        title="点击后按下新的按键；Shift 会一起记下来"
                    >
                        {capturing
                            ? '按下按键…'
                            : (binding.key === null ? '未绑定' : keycapLabel(binding.key))}
                    </button>
                    {binding.key !== null && !capturing && (
                        <button
                            type="button"
                            className="keycap-clear"
                            onClick={() => onKey(null)}
                            title="取消这个键"
                            aria-label={`取消 ${COMMAND_LABELS[binding.command]} 的按键`}
                        >
                            <X size={10} />
                        </button>
                    )}
                </span>

                <span className="pad-chips">
                    {EXTRA_PAD_BUTTONS.map((button) => (
                        <button
                            key={button}
                            type="button"
                            className="pad-chip"
                            aria-pressed={chord.includes(button)}
                            onClick={() => togglePad(button)}
                            title={`按住手柄的 ${PAD_BUTTON_LABELS[button]}${
                                chord.length > 0 ? '，与其它已选键一起组成组合键' : ''
                            }`}
                        >
                            {PAD_BUTTON_LABELS[button]}
                        </button>
                    ))}
                </span>

                {/* Capturing from the pad itself: press what you want, let go,
                    done. The chips above do the same thing one click at a
                    time, which is fine for one button and tedious for three. */}
                <button
                    type="button"
                    className={capturingPad === null ? 'pad-capture' : 'pad-capture pad-capture-on'}
                    onClick={() => setCapturingPad(capturingPad === null ? NO_CAPTURE : null)}
                    title="按手柄上的键来绑：按住要用的键，然后全部松开"
                >
                    {capturingPad === null
                        ? '按下来绑'
                        : (capturingPad.best.length === 0
                            ? '请按键…'
                            : `已按 ${capturingPad.best.map((b) => PAD_BUTTON_LABELS[b]).join('+')}`)}
                </button>
            </dd>
        </div>
    );
}

/** A small segmented control: the buttons that pick one of a few values.
 *  Exported because the settings screen uses the same control for the
 *  emulator-core choice, and two implementations would drift. */
export function Segment<T extends string | number>({ value, options, onChange }: {
    value: T;
    options: { value: T; label: string }[];
    onChange: (next: T) => void;
}) {
    return (
        <div className="segmented" role="group">
            {options.map((option) => (
                <button
                    key={String(option.value)}
                    type="button"
                    className={option.value === value ? 'segment segment-on' : 'segment'}
                    aria-pressed={option.value === value}
                    onClick={() => onChange(option.value)}
                >
                    {option.label}
                </button>
            ))}
        </div>
    );
}

export default function InputPanel({
    input,
    onInput,
    pads,
    gamepadEnabled,
    gamepadNative,
}: InputPanelProps) {
    const bindings = activeBindings(input);

    /** Replace the binding list, keeping every other field. */
    const setBindings = useCallback((next: KeyBinding[]): void => {
        onInput({ ...input, bindings: next });
    }, [input, onInput]);

    /**
     * Point one binding at a new key, swapping with whoever had it.
     *
     * Swapping rather than duplicating keeps every key doing exactly one
     * thing: the resolver takes the first binding for a key and drops the
     * rest, so two bindings on one key would silently make one of them dead.
     */
    const rebind = useCallback((index: number, code: string): void => {
        const next = bindings.map((binding) => ({ ...binding }));
        const previous = next[index];
        if (previous === undefined) {
            return;
        }
        const oldCode = previous.code;
        const clash = next.findIndex((binding, position) => (
            position !== index && binding.code === code
        ));
        next[index] = { ...previous, code };
        if (clash !== -1) {
            next[clash] = { ...next[clash], code: oldCode };
        }
        setBindings(next);
    }, [bindings, setBindings]);

    /**
     * Assign a pad to a port, or to nothing.
     *
     * The array is extended with -1 rather than with defaults, because the
     * slots before this pad's index have no pad in them: an explicit "unused"
     * is the honest value for a hole.
     */
    const assignPad = useCallback((index: number, port: number): void => {
        const padPorts = [...input.padPorts];
        while (padPorts.length <= index) {
            padPorts.push(-1);
        }
        padPorts[index] = port;
        onInput({ ...input, padPorts });
    }, [input, onInput]);

    const reserved = commandKeysInUse(input);

    const bindingRows = bindings.map((binding, index) => (
        <BindingRow
            key={`${binding.port}-${binding.button}-${index}`}
            binding={binding}
            reserved={reserved}
            onRebind={(code) => rebind(index, code)}
        />
    ));

    /**
     * The commands, in the order the settings screen lists them, with the
     * binding the player has for each -- or the default, which is what a fresh
     * install has and what "恢复默认" puts back.
     */
    const commands = commandBindings(input);
    const setCommands = useCallback((next: CommandBinding[]): void => {
        onInput({ ...input, commands: next });
    }, [input, onInput]);

    /**
     * Change one command, and collapse it to a single binding.
     *
     * The defaults put 暂停 on two keys (Escape and P) because both are what
     * hands reach for, and the screen has one row per command -- so it shows
     * the first. Editing has to mean "this is the binding now", not "and also
     * the one you cannot see": without the collapse, rebinding 暂停 would leave
     * two bindings on the *same* new key, and the file would collect rows
     * nobody can see or delete.
     */
    const replaceCommand = useCallback(
        (command: CommandName | HeldCommandName, change: (binding: CommandBinding) => CommandBinding): void => {
            const first = commands.find((binding) => binding.command === command);
            if (first === undefined) {
                return;
            }
            setCommands([
                ...commands.filter((binding) => binding.command !== command),
                change(first),
            ]);
        },
        [commands, setCommands],
    );

    /**
     * Every extra button that is down on any connected pad.
     *
     * Any pad rather than the one that owns player 1: a command is something
     * the application does, not something a player does, and the settings
     * screen is where somebody holds the controller in one hand and clicks
     * with the other. A pad assigned to nothing still reports its buttons.
     */
    const downOnPads = [...new Set(pads.flatMap((pad) => (
        EXTRA_PAD_BUTTONS.filter((button) => pad.buttons[button])
    )))];

    const commandRows = COMMAND_ORDER.map((command) => {
        const binding = commands.find((candidate) => candidate.command === command);
        if (binding === undefined) {
            return null;
        }
        return (
            <CommandRow
                key={command}
                binding={binding}
                down={downOnPads}
                onKey={(key) => replaceCommand(command, (current) => ({ ...current, key }))}
                onPads={(pads) => replaceCommand(command, (current) => ({ ...current, pads }))}
            />
        );
    });

    return (
        <>
            <section className="group">
                <h3>键盘</h3>
                <div className="setting-row">
                    <span className="setting-label">模式</span>
                    <Segment
                        value={input.keyboard}
                        options={[
                            { value: '1p' as const, label: '单人' },
                            { value: '2p' as const, label: '双人' },
                        ]}
                        onChange={(keyboard) => onInput({ ...input, keyboard })}
                    />
                </div>
                {input.keyboard === '1p' ? (
                    <div className="setting-row">
                        <span className="setting-label">作为</span>
                        <Segment
                            value={input.keyboardPlayer}
                            options={[
                                { value: 0, label: '玩家 1' },
                                { value: 1, label: '玩家 2' },
                            ]}
                            onChange={(keyboardPlayer) => onInput({ ...input, keyboardPlayer })}
                        />
                    </div>
                ) : null}
                <p className="prose">
                    {input.keyboard === '1p'
                        ? '一个人用：方向键和 WASD 都是同一位玩家，两套按键都可用。'
                        : '两个人用：WASD 一侧是玩家 1，方向键一侧是玩家 2。下面可以改成任意按键。'}
                </p>
            </section>

            <section className="group">
                <h3>按键绑定</h3>
                <dl className="rows binding-list">{bindingRows}</dl>
                <div className="panel-actions">
                    <button
                        type="button"
                        className="button"
                        onClick={() => onInput({ ...input, bindings: null })}
                    >
                        <RotateCcw size={13} />
                        恢复默认
                    </button>
                </div>
                <p className="prose">
                    点击键帽，再按下新按键。已经被下面「命令」用掉的键不会被接受 ——
                    两个动作共用一个键，只会有一个生效。
                </p>
            </section>

            <section className="group">
                <h3>命令</h3>
                <dl className="rows binding-list">{commandRows}</dl>
                <div className="panel-actions">
                    <button
                        type="button"
                        className="button"
                        onClick={() => onInput({ ...input, commands: null })}
                    >
                        <RotateCcw size={13} />
                        恢复默认
                    </button>
                </div>
                <p className="prose">
                    暂停、截图、存档、读档这些是「命令」，不是手柄上的开关：按键点一下再按新键
                    （Shift 会一起记下来），右边的小方块是手柄上的按钮 —— 肩膀键、扳机键、摇杆按下
                    和 X / Y，都是主机用不到的键，所以绑在这里不会误触游戏。主机的八个开关在
                    上面单独绑。
                </p>
                <p className="prose">
                    一个命令绑一个键（默认的「暂停」在 Esc 和 P 上，编辑后会合并成你按下的那一个），
                    但可以绑一组手柄组合键：同时按下的键才算组合，三个键的组合优先于两个键，
                    而两个键的组合会先等一下 —— 否则按 L1+R1 时 L1 会先把「暂停」触发掉。
                </p>
            </section>

            <section className="group">
                <h3>手柄</h3>
                <PadTester pads={pads} />
                <dl className="rows">
                    <div className="row">
                        <dt>来源</dt>
                        <dd>{!gamepadEnabled
                            ? '未启用'
                            : (gamepadNative ? '原生 GameController 助手' : '浏览器 Gamepad API')}</dd>
                    </div>
                </dl>

                {pads.length === 0 ? (
                    <p className="prose">
                        {gamepadEnabled
                            ? (gamepadNative
                                ? '还没有检测到手柄。先按一下手柄上的键，确认它已配对；如果还是没有，去系统设置 → 隐私与安全性 → 输入监控，勾上本应用后重新启动。'
                                : '手柄要用过一次才会出现在浏览器里：先按一下它上面的键。如果按了还是没有，可能需要在系统设置 → 隐私与安全性 → 输入监控里允许本应用。')
                            : '没有手柄来源在运行。'}
                    </p>
                ) : (
                    <>
                        <ul className="pad-list">
                            {pads.map((pad) => (
                                <li className="pad-row" key={pad.index}>
                                    <div className="pad-name">
                                        <span className="pad-slot">#{pad.index}</span>
                                        <span className="pad-id" title={pad.id}>{pad.id}</span>
                                        {pad.mapping !== 'standard' && pad.mapping !== 'native' && (
                                            <span className="pad-warn">布局未知</span>
                                        )}
                                    </div>
                                    <Segment
                                        value={padPort(input, pad.index)}
                                        options={[
                                            { value: 0, label: '1P' },
                                            { value: 1, label: '2P' },
                                            { value: -1, label: '关闭' },
                                        ]}
                                        onChange={(port) => assignPad(pad.index, port)}
                                    />
                                </li>
                            ))}
                        </ul>
                        <div className="panel-actions">
                            <button
                                type="button"
                                className="button"
                                onClick={() => onInput({ ...input, padPorts: [] })}
                            >
                                <RotateCcw size={13} />
                                自动分配
                            </button>
                        </div>
                        <p className="prose">
                            默认第一个手柄是玩家 1，第二个是玩家 2。当前：{
                                pads.map((pad) => `#${pad.index} ${playerName(padPort(input, pad.index))}`).join('、')
                            }。
                        </p>
                        <p className="prose">
                            上面的灯就是手柄现在按下的键：上一排是主机的八个开关（游戏读的），
                            下一排是命令用来绑定的九个（主机用不到）。按下手柄上的键，对上是哪一个，
                            再去上面「命令」那一段点「按下来绑」。
                        </p>
                    </>
                )}
            </section>
        </>
    );
}
