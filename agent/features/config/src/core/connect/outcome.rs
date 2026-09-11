//! Connect 向导的终态 outcome。

/// Connect session 的业务终态。
///
/// 一个 session 至多产生一次 outcome；重复进入返回 typed error 且无副作用。
/// 出现在 [`crate::connect::ConnectView::terminal`] 中供 SDK / TUI 渲染。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectOutcome {
    /// connect 成功提交：新全局配置已 durable 写回。
    ///
    /// `applied_revision` 是 [`crate::ports::ConnectCommitPort`] 返回的
    /// receipt 投影；下一次 committed snapshot 读取应能取到一致值。
    Completed { applied_revision: u64 },
    /// 用户主动或被动取消；session 内的 draft 全部丢弃，没有持久化副作用。
    Cancelled,
}

impl ConnectOutcome {
    pub fn kind_label(&self) -> &'static str {
        match self {
            ConnectOutcome::Completed { .. } => "completed",
            ConnectOutcome::Cancelled => "cancelled",
        }
    }
}

#[cfg(test)]
#[path = "outcome_tests.rs"]
mod outcome_tests;
