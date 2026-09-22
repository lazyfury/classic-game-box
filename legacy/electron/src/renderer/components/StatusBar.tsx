// ---------------------------------------------------------------------------
// The status bar.
//
// Every number here is a reading, not a setting. It exists because this is an
// emulator and not a game console: when a game runs slowly, or silently, or
// with the d-pad stuck down, the status bar is where that becomes visible --
// and "audio 0.000" beside "60.1 fps" is a sentence somebody can act on.
//
// The right hand group is monospaced and tabular so the numbers do not jitter
// left and right as they change, which at four updates a second is the
// difference between a readout and a flicker.
//
// The two things the machine is wired up to -- the speaker and the pad -- are
// here too, as icon *and* words. They used to be a pair of dots at the bottom
// of the left rail, and a dot cannot answer the question a person actually
// has: is the pad connected, or merely expected? A dot that is off looks the
// same whether the source never started, found nothing, or was never asked
// for. The word is the whole point of moving them down here.
// ---------------------------------------------------------------------------

import { Gamepad2, Volume2, VolumeX } from 'lucide-react';
import type { ReactNode } from 'react';

import type { EngineStatus } from '../engineStatus';
import { formatCycles, formatHex16 } from '../format';

// ---------------------------------------------------------------------------
// What the speaker is doing, in one phrase
// ---------------------------------------------------------------------------

/**
 * Whether the speaker is wired up at all.
 *
 * Keyed on the error rather than on `status.audio`, because that field is only
 * written by the once-per-quarter-second status update inside the frame loop —
 * and that loop deliberately does not report while no cartridge is loaded. On
 * the library screen `status.audio` is null however healthy the pipeline is,
 * which would make a working speaker look broken. A failed pipeline, by
 * contrast, always sets `audioError`.
 */
function audioIsReady(status: EngineStatus): boolean {
    return status.audioError === null;
}

/** The one phrase beside the speaker icon. */
function audioLabel(status: EngineStatus): string {
    if (status.audioError !== null) {
        return '音频不可用';
    }
    if (status.audio === null) {
        return '音频就绪';
    }
    return status.audio.state === 'running' ? '音频已连接' : `音频 ${status.audio.state}`;
}

/** Everything the speaker is doing, for the hover text. */
function audioDetail(status: EngineStatus): string {
    if (status.audioError !== null) {
        return status.audioError;
    }
    if (status.audio === null) {
        return '音频管线已建立，尚未开始播放';
    }
    const { state, fill, targetFill, underruns, dropped } = status.audio;
    return `${state} · 缓冲 ${fill}/${targetFill} · 欠载 ${underruns} · 丢弃 ${dropped}`;
}

// ---------------------------------------------------------------------------
// What the pad is doing, in one phrase
// ---------------------------------------------------------------------------

/** The one phrase beside the pad icon. Short: the name goes in the tooltip. */
function gamepadLabel(status: EngineStatus, enabled: boolean): string {
    if (!enabled) {
        return '手柄未启用';
    }
    const count = status.gamepad.pads.length;
    if (count === 0) {
        return '手柄未检测到';
    }
    return count === 1 ? '手柄已连接' : `手柄 ${count} 个`;
}

/** The device names, or the reason there are none. */
function gamepadDetail(status: EngineStatus, enabled: boolean, native: boolean): string {
    if (!enabled) {
        return '没有手柄来源在运行';
    }
    const pads = status.gamepad.pads;
    if (pads.length === 0) {
        return native
            ? '原生 GameController 助手已启动，但未检测到手柄'
            : '浏览器 Gamepad API 已启动，但未检测到手柄';
    }
    const path = native ? 'native' : 'browser';
    return pads
        .map((pad) => `#${pad.index} ${pad.id} · ${pad.port < 0 ? '未分配' : `玩家 ${pad.port + 1}`} · ${path}`)
        .join('；');
}

/** The detailed audio readout the right hand group has always shown. */
function describeAudio(status: EngineStatus): string {
    if (status.audioError !== null) {
        return '音频关闭';
    }
    if (status.audio === null) {
        return '音频 —';
    }
    const { state, fill, targetFill, underruns, dropped } = status.audio;
    return `${state} ${fill}/${targetFill} u${underruns} d${dropped}`;
}

function audioTrouble(status: EngineStatus): boolean {
    if (status.audioError !== null) {
        return true;
    }
    return status.audio !== null && (status.audio.underruns > 0 || status.audio.dropped > 0);
}

/**
 * One peripheral: an icon and a word.
 *
 * `on` is the difference between "wired up" and "there" -- the speaker running
 * or the pad connected. Everything else is dim, because a device that is
 * merely absent is not an error.
 */
function Device({ on, label, detail, children }: {
    on: boolean;
    label: string;
    detail: string;
    children: ReactNode;
}) {
    return (
        <span className={on ? 'status-device status-device-on' : 'status-device'} title={detail}>
            <span className="status-device-icon" aria-hidden="true">{children}</span>
            {label}
        </span>
    );
}

interface StatusBarProps {
    status: EngineStatus;
    /** Whether a gamepad source is running at all. */
    gamepadEnabled: boolean;
    /** Whether that source is the native helper rather than the browser API.
     *  The two fail differently, so the tooltip says which one it is. */
    gamepadNative: boolean;
}

export default function StatusBar({ status, gamepadEnabled, gamepadNative }: StatusBarProps) {
    return (
        <footer className="statusbar">
            {status.message !== null && <span className="status-message">{status.message}</span>}

            {status.error !== null && <span className="status-error">{status.error}</span>}

            {status.state === 'loading' && <span className="status-dim">正在载入核心…</span>}
            {status.state === 'halted' && <span className="status-error">CPU 已停机</span>}

            {status.state === 'running' && status.romPath === null && (
                <span className="status-dim">未插入卡带 · 从游戏库选择一个</span>
            )}

            <span className="status-devices">
                <Device
                    on={audioIsReady(status)}
                    label={audioLabel(status)}
                    detail={audioDetail(status)}
                >
                    {audioIsReady(status) ? <Volume2 size={13} /> : <VolumeX size={13} />}
                </Device>
                <Device
                    on={gamepadEnabled && status.gamepad.pads.length > 0}
                    label={gamepadLabel(status, gamepadEnabled)}
                    detail={gamepadDetail(status, gamepadEnabled, gamepadNative)}
                >
                    <Gamepad2 size={13} />
                </Device>
            </span>

            <span className="status-spacer" />

            {status.state === 'running' && status.romPath !== null && (
                <>
                    <span className="status-num">
                        {status.held.length > 0 ? status.held.join(' ') : '—'}
                    </span>
                    {status.rewind !== null && (
                        <span className={status.rewinding ? 'status-rewind' : 'status-num'}>
                            {status.rewinding ? '◀◀ ' : ''}
                            {status.rewind.seconds.toFixed(1)}s
                        </span>
                    )}
                    <span className="status-num">{status.fps.toFixed(1)} fps</span>
                    <span className="status-num">帧 {status.frameCount}</span>
                    <span className="status-num">{formatCycles(status.totalCycles)} cyc</span>
                    <span className="status-num">PC {formatHex16(status.cpuPc)}</span>
                    <span className={status.audioPeak > 0.01 ? 'status-live' : 'status-num'}>
                        ♪ {status.audioPeak.toFixed(3)}
                    </span>
                    <span
                        className={audioTrouble(status) ? 'status-error' : 'status-num'}
                        title={status.audioError ?? undefined}
                    >
                        {describeAudio(status)}
                    </span>
                </>
            )}
        </footer>
    );
}
