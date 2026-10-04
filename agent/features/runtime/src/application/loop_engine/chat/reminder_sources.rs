//! Runtime 侧 reminder source 实现（07-reminder-pipeline.md）：
//! 数据获取在 Runtime（task access、Run 启动冻结事实），
//! 渲染委托 Context 的 `render_invocation_reminder_body`（文案单一真相），
//! 快照载体为 `InvocationReminderData` 的 serde JSON（fingerprint 稳定）。

use std::sync::Arc;

use context::{
    CompactBehavior, InjectBehavior, ReminderDedup, ReminderKind, ReminderPlacement,
    ReminderPolicy, ReminderPriority, ReminderSnapshot, ReminderSource,
};

/// 任务进度 source：从 `TaskAccess` 读当前 batch 快照。
pub(crate) struct TaskProgressReminderSource {
    task: Arc<dyn task::TaskAccess>,
    max_lines: usize,
}

impl TaskProgressReminderSource {
    pub(crate) fn new(task: Arc<dyn task::TaskAccess>, max_lines: usize) -> Self {
        Self { task, max_lines }
    }
}

impl ReminderSource for TaskProgressReminderSource {
    fn kind(&self) -> ReminderKind {
        ReminderKind::task_progress()
    }

    fn policy(&self) -> ReminderPolicy {
        ReminderPolicy {
            refresh: context::RefreshTrigger::OnStepInterval(
                crate::application::constants::TASK_PROGRESS_REFRESH_INTERVAL_STEPS,
            ),
            placement: ReminderPlacement::TailUserMessage,
            inject: InjectBehavior {
                dedup: ReminderDedup::SkipIfUnchanged,
                priority: ReminderPriority::task_state(),
            },
            compact: CompactBehavior::Rebuild,
        }
    }

    fn build(&self) -> Option<ReminderSnapshot> {
        super::task_snapshot::build_task_reminder_intent(self.task.as_ref(), self.max_lines).map(
            |data| ReminderSnapshot {
                data: serde_json::to_string(&data).expect("reminder 快照序列化不可失败"),
            },
        )
    }

    fn render(&self, snapshot: &ReminderSnapshot, language: &str) -> String {
        match serde_json::from_str::<context::InvocationReminderData>(&snapshot.data) {
            Ok(data) => context::render_invocation_reminder_body(&data, language),
            Err(error) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "reminder 快照反序列化失败 kind=task_progress error={error}"
                );
                String::new()
            }
        }
    }
}

/// Run 启动冻结事实 source（guidance 变化 / 模型不匹配 / memory 更新）：
/// Run 启动生成一次，build 恒返回同一数据。
pub(crate) struct RunStartFactReminderSource {
    kind: ReminderKind,
    data: context::InvocationReminderData,
    policy: ReminderPolicy,
}

impl RunStartFactReminderSource {
    pub(crate) fn guidance_sources_changed() -> Self {
        Self {
            kind: ReminderKind::new("guidance_sources_changed"),
            data: context::InvocationReminderData::GuidanceSourcesChanged,
            policy: ReminderPolicy {
                refresh: context::RefreshTrigger::OnRunStart,
                placement: ReminderPlacement::SystemTail,
                inject: InjectBehavior {
                    dedup: ReminderDedup::SkipIfUnchanged,
                    priority: ReminderPriority::environment(),
                },
                compact: CompactBehavior::Reinstate,
            },
        }
    }

    pub(crate) fn model_guidance_mismatch(
        session_model_id: impl Into<String>,
        run_model_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: ReminderKind::new("model_guidance_mismatch"),
            data: context::InvocationReminderData::model_guidance_mismatch(
                session_model_id,
                run_model_id,
            ),
            policy: ReminderPolicy {
                refresh: context::RefreshTrigger::OnRunStart,
                placement: ReminderPlacement::SystemTail,
                inject: InjectBehavior {
                    dedup: ReminderDedup::SkipIfUnchanged,
                    priority: ReminderPriority::environment(),
                },
                compact: CompactBehavior::Reinstate,
            },
        }
    }

    /// memory 更新事实：Run 启动一次性注入后丢弃（reflection notice 在
    /// Run 边界被 take，非 Run 内事件流；OnEvent(memory) 留待 reflection
    /// 运行态演进时接入）。
    pub(crate) fn memory_updated(changed: usize) -> Self {
        Self {
            kind: ReminderKind::memory_updated(),
            data: context::InvocationReminderData::memory_updated(changed),
            policy: ReminderPolicy {
                refresh: context::RefreshTrigger::OnRunStart,
                placement: ReminderPlacement::TailUserMessage,
                inject: InjectBehavior {
                    dedup: ReminderDedup::SkipIfUnchanged,
                    priority: ReminderPriority::event(),
                },
                compact: CompactBehavior::Drop,
            },
        }
    }
}

impl ReminderSource for RunStartFactReminderSource {
    fn kind(&self) -> ReminderKind {
        self.kind.clone()
    }

    fn policy(&self) -> ReminderPolicy {
        self.policy.clone()
    }

    fn build(&self) -> Option<ReminderSnapshot> {
        Some(ReminderSnapshot {
            data: serde_json::to_string(&self.data).expect("reminder 快照序列化不可失败"),
        })
    }

    fn render(&self, snapshot: &ReminderSnapshot, language: &str) -> String {
        match serde_json::from_str::<context::InvocationReminderData>(&snapshot.data) {
            Ok(data) => context::render_invocation_reminder_body(&data, language),
            Err(error) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "reminder 快照反序列化失败 kind={} error={error}",
                    self.kind.as_str(),
                );
                String::new()
            }
        }
    }
}

#[cfg(test)]
#[path = "reminder_sources_tests.rs"]
mod tests;
