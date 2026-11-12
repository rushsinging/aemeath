//! 纯值常量（#1146 placement 归位）。

/// prompt 摘要的最大字符数（超出截断加省略号）。
/// prompt 摘要的最大字符数（超出截断加省略号）。
pub(crate) const PROMPT_SUMMARY_MAX_CHARS: usize = 60;

/// 等待系统打开器退出的上限。macOS `open` 立即返回；部分 Linux 的 `xdg-open`
/// 会跟随应用生命周期，超时视为「已发起」而不是失败（#2196）。
pub(crate) const OPEN_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
