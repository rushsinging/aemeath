//! 会话运行态——UI 基础设施关注点。
//!
//! 与 `ConversationAggregate`（核心域：对话内容）分离。
//! 对话域产出的 `ConversationChange` 经映射层翻译为 `RuntimeState` 方法调用。

use super::status_notice::StatusNotice;
use super::task_status::TaskStatusSnapshot;
use super::usage::UsageSummary;
use std::time::Instant;

/// 会话运行态聚合——usage / workspace / status 等基础设施关注点。
///
/// TODO: 字段将逐步私有化，改为只经业务方法操作。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeState {
    pub usage: UsageSummary,
    pub live_tps: Option<f64>,
    pub runtime_status: Option<crate::tui::adapter::runtime_status::TuiRuntimeStatus>,
    pub task_status: TaskStatusSnapshot,
    pub status_notice: StatusNotice,
    pub transient_notice_expiry: Option<Instant>,
}

// ── 临时 notice 过期逻辑 ──

impl RuntimeState {
    /// 检查临时 notice 是否过期；过期则回退到 Ready 持久态。
    pub fn expire_transient_notice(&mut self, now: Instant) -> bool {
        if self.transient_notice_expiry.is_some_and(|exp| now >= exp) {
            self.transient_notice_expiry = None;
            self.status_notice = StatusNotice::success("Ready");
            return true;
        }
        false
    }
}

// ── 运行态 intent 的直接字段操作（纯运行态 intent 不经过对话域 change 映射） ──

impl RuntimeState {
    pub fn clear_compact_runtime(&mut self) {}
}
