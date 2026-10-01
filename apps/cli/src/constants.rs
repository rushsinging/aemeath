//! CLI crate 身份常量（#1146 双轨归位）。

pub(crate) const LOG_TARGET: &str = "aemeath:tui";
/// 终端恢复转义序列：LeaveAlternateScreen + DisableMouseCapture + DisableBracketedPaste + show cursor。
/// 与 TerminalGuard::drop 的恢复语义保持一致（此处为 panic hook 的最后兜底，不依赖 crossterm execute）。
/// 终端恢复转义序列：LeaveAlternateScreen + DisableMouseCapture + DisableBracketedPaste + show cursor。
/// 与 TerminalGuard::drop 的恢复语义保持一致（此处为 panic hook 的最后兜底，不依赖 crossterm execute）。
pub(crate) const TERMINAL_RESTORE_SEQ: &[u8] = b"\x1b[?1049l\x1b[?1000l\x1b[?2004l\x1b[?25h";
