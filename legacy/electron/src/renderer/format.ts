// ---------------------------------------------------------------------------
// Turning numbers into the short strings an interface has room for.
//
// Kept out of the components because two of them format the same things --
// the library and the status bar both count bytes and both count cycles -- and
// because "how long ago" is a decision, not a detail: a game played this
// morning should say "today", not a timestamp.
// ---------------------------------------------------------------------------

export function baseName(path: string): string {
    const parts = path.split(/[/\\]/);
    return parts[parts.length - 1] ?? path;
}

/** The file name without its extension: the closest thing a .nes has to a
 *  title that can be read without running the game. */
export function gameTitle(path: string): string {
    return baseName(path).replace(/\.nes$/i, '');
}

export function formatSize(bytes: number): string {
    if (bytes < 1024) {
        return `${bytes} B`;
    }
    if (bytes < 1024 * 1024) {
        return `${Math.round(bytes / 1024)} KB`;
    }
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatWhen(timestamp: number): string {
    if (timestamp === 0) {
        return '未玩过';
    }
    const days = Math.floor((Date.now() - timestamp) / 86_400_000);
    if (days === 0) {
        return '今天';
    }
    if (days === 1) {
        return '昨天';
    }
    if (days < 30) {
        return `${days} 天前`;
    }
    return new Date(timestamp).toLocaleDateString();
}

/**
 * A total play time, in the units an interface has room for.
 *
 * Seconds only under a minute -- "0 秒" for a game that was opened and closed
 * is honest, and a rounded "1 分" would not be -- then minutes, then hours and
 * minutes, then hours alone once the minutes are noise.
 */
export function formatDuration(seconds: number): string {
    const whole = Math.max(0, Math.floor(seconds));
    if (whole < 60) {
        return `${whole} 秒`;
    }

    const minutes = Math.floor(whole / 60);
    if (minutes < 60) {
        return `${minutes} 分`;
    }

    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    if (hours >= 100 || rest === 0) {
        return `${hours} 时`;
    }
    return `${hours} 时 ${rest} 分`;
}

export function formatCycles(cycles: number): string {
    if (cycles < 1_000_000) {
        return cycles.toLocaleString('en-US');
    }
    return `${(cycles / 1_000_000).toFixed(1)}M`;
}

export function formatHex16(value: number): string {
    return `$${value.toString(16).toUpperCase().padStart(4, '0')}`;
}
