//! Small text formatters shared by the pages: file sizes, play durations and
//! "how long ago" captions.

/// Bytes into the short string a card has room for.
pub(super) fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    if bytes < KIB {
        format!("{bytes} B")
    } else if bytes < MIB {
        format!("{} KB", (bytes as f64 / KIB as f64).round() as u64)
    } else {
        format!("{:.1} MB", bytes as f64 / MIB as f64)
    }
}

/// A total play time in the units an interface has room for.
pub(super) fn format_duration(seconds: i64) -> String {
    let whole = seconds.max(0);
    if whole < 60 {
        return format!("{whole} 秒");
    }
    let minutes = whole / 60;
    if minutes < 60 {
        return format!("{minutes} 分");
    }
    let hours = minutes / 60;
    let rest = minutes % 60;
    if hours >= 100 || rest == 0 {
        format!("{hours} 时")
    } else {
        format!("{hours} 时 {rest} 分")
    }
}

/// A short "how long ago" for a screenshot caption.
pub(super) fn format_when(created_ms: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let seconds = ((now - created_ms) / 1000).max(0);
    if seconds < 60 {
        "刚刚".to_string()
    } else if seconds < 3600 {
        format!("{} 分钟前", seconds / 60)
    } else if seconds < 86_400 {
        format!("{} 小时前", seconds / 3600)
    } else {
        format!("{} 天前", seconds / 86_400)
    }
}
