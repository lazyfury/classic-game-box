// ---------------------------------------------------------------------------
// The settings panes -- the play column, in the 设置 section.
//
// One pane per group of settings, shown one at a time by TabsView: the index
// in the middle column chooses which. Everything that was here is still here,
// moved from a single scroll to four tabs, because a value wants room and the
// play column is the only column with any.
//
// The console's own pane is one of the tabs while this section is open (see
// App), so a player can flip back to the picture to see what a change did --
// which is the whole reason display settings are worth having next to the
// game rather than in a dialog.
// ---------------------------------------------------------------------------
// Settings -- the middle column, in the 设置 section.
//
// There are no preferences to edit, and pretending otherwise would be a lie:
// the NES has no settings, and every number below is a reading from the
// machine rather than a knob. What the panel is for is the two things a
// player can actually change -- how the picture is filtered and whether it is
// drawn at all -- plus a straight answer to "is it running right".
//
// So it is a report, laid out the way a macOS inspector lays one out: grouped
// rows, the label on the left and the value on the right in tabular figures,
// so the eye can compare a column of numbers down the page.
// ---------------------------------------------------------------------------

import type { ReactNode } from 'react';
import * as Switch from '@radix-ui/react-switch';
import { FolderCog } from 'lucide-react';

import type { CoreSelection, InputSettings, Library } from '../../shared/api';
import InputPanel, { Segment } from './InputPanel';
import { SYSTEMS, coresFor } from '../systems';
import type { EngineStatus } from '../engineStatus';
import { settingsGroup, type SettingsGroupId } from '../settings';
import type { PixelScale } from '../usePixelScale';
import { formatCycles, formatHex16 } from '../format';

interface SettingsPanelProps {
    /** Which group of settings this pane is. */
    group: SettingsGroupId;
    status: EngineStatus;
    picture: PixelScale;
    scanlines: boolean;
    onScanlines: (on: boolean) => void;
    /** Keyboard mode, key bindings and pad assignments. */
    input: InputSettings;
    /** Change the input settings and remember them. */
    onInput: (next: InputSettings) => void;
    /** Which core to run each console on. Empty means the defaults. */
    cores: CoreSelection;
    /** Change the core for one console and remember it. Changing the core
     *  under a running game restarts that game on the new machine. */
    onCores: (next: CoreSelection) => void;
    gamepadEnabled: boolean;
    /** Whether that source is the native GameController helper rather than
     *  the browser's Gamepad API. The two fail differently, so the panel says
     *  which one is running. */
    gamepadNative: boolean;
    /** The library, for the folder and the row counts. */
    library: Library;
    /** How many pictures are in it, which lives outside the library's own
     *  type because it is a separate list. */
    screenshots: number;
    onChooseDirectory: () => void;
}

/** The audio numbers, in the order a person checks them. */
function audioRows(status: EngineStatus): [string, string][] {
    if (status.audioError !== null) {
        return [['状态', '不可用'], ['原因', status.audioError]];
    }
    if (status.audio === null) {
        return [['状态', '尚未建立']];
    }
    const { state, fill, targetFill, underruns, dropped } = status.audio;
    return [
        ['状态', state],
        ['缓冲', `${fill} / ${targetFill}`],
        ['欠载', String(underruns)],
        ['丢弃', String(dropped)],
    ];
}

function Group({ title, children }: { title: string; children: ReactNode }) {
    return (
        <section className="group">
            <h3>{title}</h3>
            {children}
        </section>
    );
}

function Rows({ rows }: { rows: [string, string][] }) {
    return (
        <dl className="rows">
            {rows.map(([label, value]) => (
                <div className="row" key={label}>
                    <dt>{label}</dt>
                    <dd>{value}</dd>
                </div>
            ))}
        </dl>
    );
}

export default function SettingsPanes({
    group,
    status,
    picture,
    scanlines,
    onScanlines,
    input,
    onInput,
    cores,
    onCores,
    gamepadEnabled,
    gamepadNative,
    library,
    screenshots,
    onChooseDirectory,
}: SettingsPanelProps) {
    const pinned = library.games.filter((game) => game.pinned).length;

    const content = (() => {
        switch (group) {
            case 'input':
                return (
                    <Group title="输入">
                        <InputPanel
                            input={input}
                            onInput={onInput}
                            pads={status.gamepad.pads}
                            gamepadEnabled={gamepadEnabled}
                            gamepadNative={gamepadNative}
                        />
                    </Group>
                );
            case 'display':
                return (
                    <>
                <Group title="显示">
                    <dl className="rows">
                        <div className="row">
                            <dt>画面</dt>
                            <dd>256 × 240</dd>
                        </div>
                        <div className="row">
                            <dt>缩放</dt>
                            <dd>{picture.scale.toFixed(2)} ×</dd>
                        </div>
                    </dl>
                    <div className="switch-row">
                        <Switch.Root
                            id="fc-scanlines"
                            className="switch-root"
                            checked={scanlines}
                            onCheckedChange={onScanlines}
                        >
                            <Switch.Thumb className="switch-thumb" />
                        </Switch.Root>
                        <label className="switch-label" htmlFor="fc-scanlines">
                            扫描线
                        </label>
                    </div>
                </Group>

                <Group title="音频">
                    <Rows rows={audioRows(status)} />
                </Group>

                <Group title="运行">
                    <Rows
                        rows={[
                            ['帧率', `${status.fps.toFixed(1)} fps`],
                            ['帧数', status.frameCount.toLocaleString('en-US')],
                            ['周期', formatCycles(status.totalCycles)],
                            ['PC', formatHex16(status.cpuPc)],
                            ['卡带', status.romSummary === '' ? '—' : status.romSummary],
                        ]}
                    />
                </Group>

                    </>
                );
            case 'cores':
                return (
                <Group title="模拟器核心">
                    {SYSTEMS.map((system) => {
                        const available = coresFor(system.id);
                        if (available.length === 0) {
                            return null;
                        }
                        const chosen = cores[system.id] ?? available[0].id;
                        return (
                            <div className="setting-row" key={system.id}>
                                <span className="setting-label">{system.name}</span>
                                <Segment
                                    value={chosen}
                                    options={available.map((core) => ({
                                        value: core.id,
                                        label: core.name,
                                    }))}
                                    onChange={(id) => onCores({ ...cores, [system.id]: id })}
                                />
                            </div>
                        );
                    })}
                    <p className="prose">
                        同一个机种可以选不同的模拟器核心。切换核心会拆掉当前机器，
                        再用新核心把卡带重新装进去 —— 卡带没变，只是换了硬件。
                    </p>
                </Group>

                );
            case 'library':
                return (
                <Group title="游戏库">
                    <Rows
                        rows={[
                            ['目录', library.directory === '' ? '—' : library.directory],
                            ['数据库', library.database === '' ? '—' : 'library.sqlite'],
                            ['游戏', `${library.games.length} 个`],
                            ['置顶', `${pinned} 个`],
                            ['截图', `${screenshots} 张`],
                        ]}
                    />
                    <div className="panel-actions">
                        <button type="button" className="button" onClick={onChooseDirectory}>
                            <FolderCog size={13} />
                            选择游戏库文件夹
                        </button>
                    </div>
                    <p className="prose">
                        游戏库是一个文件夹：ROM、截图
                        （<code>screenshots/</code>）、它们的元数据
                        （<code>library.sqlite</code>）都在里面。换一个文件夹就是换一个游戏库。
                    </p>
                </Group>

                );
        }
    })();

    return (
        <section className="play settings-pane" aria-label="设置">
            <header className="play-head">
                <div className="play-titles">
                    <span className="play-name">{settingsGroup(group).label}</span>
                    <span className="play-summary">设置</span>
                </div>
            </header>

            <div className="settings-body">{content}</div>
        </section>
    );
}
