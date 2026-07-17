#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

/// 从 ExitStatus 提取 (exit_code, failure_detail)。
///
/// - 正常退出：`exit_code` 为实际码，`failure_detail` 为 `"exit code N"`
/// - 信号终止（Unix）：`exit_code` 为 `-1`，`failure_detail` 为 `"signal N (SIGNAME)"`
/// - 信号终止（非 Unix）：`exit_code` 为 `-1`，`failure_detail` 为 `"unknown (no exit code)"`
pub(super) fn exit_status_description(status: &std::process::ExitStatus) -> (i32, String) {
    if let Some(code) = status.code() {
        return (code, format!("exit code {code}"));
    }
    // 进程没有正常退出码 → 被信号终止
    #[cfg(unix)]
    {
        let signal = status.signal().unwrap_or(0);
        let sig_name = signal_name(signal);
        (-1, format!("signal {signal} ({sig_name})"))
    }
    #[cfg(not(unix))]
    {
        (-1, "unknown (no exit code)".to_string())
    }
}

/// 将常见 Unix signal 编号映射为可读名称（覆盖最常见值，未知返回 "UNKNOWN"）。
pub(super) fn signal_name(sig: i32) -> &'static str {
    match sig {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        6 => "SIGABRT",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => "UNKNOWN",
    }
}

/// 截断字符串到 PREVIEW_MAX 字节（按 char boundary），超长时附加截断标记。
/// 用于日志预览，避免大输出把日志刷爆。
pub(super) const PREVIEW_MAX: usize = 512;
pub(super) fn preview(s: &str) -> String {
    if s.len() <= PREVIEW_MAX {
        s.to_string()
    } else {
        let cut = s
            .char_indices()
            .take_while(|(i, _)| *i < PREVIEW_MAX)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(PREVIEW_MAX);
        format!("{}...[truncated {} bytes]", &s[..cut], s.len() - cut)
    }
}
