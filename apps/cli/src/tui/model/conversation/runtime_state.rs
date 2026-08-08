//! 会话运行态——UI 基础设施关注点。
//!
//! 与 `ConversationAggregate`（核心域：对话内容）分离。
//! 对话域产出的 `ConversationChange` 经映射层翻译为 `RuntimeState` 方法调用。

use super::processing_job::{ProcessingJob, ProcessingStatus};
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
    pub processing_jobs: Vec<ProcessingJob>,
    pub status_notice: StatusNotice,
    pub graph_phase: Option<String>,
    pub transient_notice_expiry: Option<Instant>,
}

// ── 只读访问器（供 view assembler 读取） ──

impl RuntimeState {
    pub fn usage(&self) -> &UsageSummary {
        &self.usage
    }
    pub fn live_tps(&self) -> Option<f64> {
        self.live_tps
    }
    pub fn task_status(&self) -> &TaskStatusSnapshot {
        &self.task_status
    }
    pub fn processing_jobs(&self) -> &[ProcessingJob] {
        &self.processing_jobs
    }
    pub fn status_notice(&self) -> &StatusNotice {
        &self.status_notice
    }
    pub fn graph_phase(&self) -> Option<&str> {
        self.graph_phase.as_deref()
    }
}

// ── 临时 notice 过期逻辑 ──

impl RuntimeState {
    pub(crate) fn notice_from_phase(phase: Option<&str>) -> StatusNotice {
        match phase {
            None | Some("idle") => StatusNotice::success("Ready"),
            Some(p) => StatusNotice::running(p.to_string()),
        }
    }

    /// 检查临时 notice 是否过期；过期则回退到 graph_phase 派生的持久态。
    pub fn expire_transient_notice(&mut self, now: Instant) -> bool {
        if self.transient_notice_expiry.is_some_and(|exp| now >= exp) {
            self.transient_notice_expiry = None;
            self.status_notice = Self::notice_from_phase(self.graph_phase.as_deref());
            return true;
        }
        false
    }
}

// ── 运行态 intent 的直接字段操作（纯运行态 intent 不经过对话域 change 映射） ──

impl RuntimeState {
    pub fn record_usage(
        &mut self,
        input_tokens: u64,
        output_tokens: u64,
        last_input_tokens: u64,
        cost_usd: f64,
    ) -> (u64, u64, f64) {
        self.usage.input_tokens += input_tokens;
        self.usage.output_tokens += output_tokens;
        self.usage.last_input_tokens = last_input_tokens;
        self.usage.api_calls += 1;
        self.usage.cost_usd += cost_usd;
        (
            self.usage.input_tokens,
            self.usage.output_tokens,
            self.usage.cost_usd,
        )
    }

    pub fn update_last_input_tokens(&mut self, tokens: u64) -> (u64, u64, f64) {
        self.usage.last_input_tokens = tokens;
        (
            self.usage.input_tokens,
            self.usage.output_tokens,
            self.usage.cost_usd,
        )
    }

    pub fn set_live_tps(&mut self, tps: f64) {
        self.live_tps = Some(tps);
    }

    pub fn set_task_status(&mut self, total: usize, completed: usize, in_progress: usize) {
        self.task_status = TaskStatusSnapshot {
            total,
            completed,
            in_progress,
            lines: std::mem::take(&mut self.task_status.lines),
            ..TaskStatusSnapshot::default()
        };
    }

    pub fn set_task_lines(&mut self, lines: Vec<String>) {
        self.task_status.lines = lines;
    }

    pub fn start_processing_job(&mut self, id: String, chat_id: Option<String>) {
        self.processing_jobs.push(ProcessingJob {
            id,
            chat_id,
            status: ProcessingStatus::Running,
        });
    }

    pub fn finish_processing_job(&mut self, id: &str, success: bool) {
        if let Some(job) = self.processing_jobs.iter_mut().find(|job| job.id == id) {
            job.status = if success {
                ProcessingStatus::Finished
            } else {
                ProcessingStatus::Failed
            };
        }
    }

    pub fn set_status_notice(&mut self, notice: StatusNotice) {
        self.status_notice = notice;
        self.transient_notice_expiry = None;
    }

    pub fn set_transient_status_notice(&mut self, notice: StatusNotice, expires_at: Instant) {
        self.status_notice = notice;
        self.transient_notice_expiry = Some(expires_at);
    }

    pub fn set_graph_phase(&mut self, phase: Option<String>) {
        self.graph_phase = phase.clone();
        if self.transient_notice_expiry.is_none() {
            self.status_notice = Self::notice_from_phase(phase.as_deref());
        }
    }

    pub fn clear_compact_runtime(&mut self) {}
}

#[cfg(test)]
#[path = "runtime_state_tests.rs"]
mod tests;
