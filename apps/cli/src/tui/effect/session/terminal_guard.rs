//! 终端 RAII guard：enter 进入 raw mode + alternate screen，Drop 时恢复。
//! 因 panic 策略为默认 unwind，Drop 在正常退出 / `?` 返回 / panic 展开三条路径都执行，
//! 保证终端不会卡在 raw + alternate screen。

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

/// 持有 TUI 终端句柄，析构时自动恢复终端状态。
pub struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    /// 进入 TUI：启用 raw mode、切到 alternate screen、开启括号粘贴与鼠标捕获，
    /// 并通知 panic hook TUI 已持有终端。
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            crossterm::event::EnableBracketedPaste,
            crossterm::event::EnableMouseCapture,
        )?;
        crate::panic_hook::set_tui_active(true);
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    /// 借出底层 ratatui Terminal 供事件循环渲染。
    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<io::Stdout>> {
        &mut self.terminal
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // 先告知 panic hook TUI 已退出，避免后续 panic 走 TUI 分支。
        crate::panic_hook::set_tui_active(false);
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            crossterm::event::DisableMouseCapture,
            crossterm::event::DisableBracketedPaste,
            LeaveAlternateScreen,
        );
        let _ = self.terminal.show_cursor();
    }
}
