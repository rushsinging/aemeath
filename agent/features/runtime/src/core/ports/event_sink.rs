//! EventSink — 事件出口端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2。
//! 细化由 #874 负责。

use crate::business::agent_run::RunDomainEvent;

/// 事件出口端口——领域事件 → SDK ChatEvent 的路由。
///
/// Main Run → TUI 消费。
/// Sub Run → 父 Run 消费（#612 Main/Sub 路由）。
///
/// #874 将补 `agent_id`（#612 缺口）。
pub trait EventSink: Send + Sync {
    /// 提交一批领域事件。
    fn emit(&self, events: Vec<RunDomainEvent>);
}
