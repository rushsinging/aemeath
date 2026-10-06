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
    /// guidance 来源变更：paths 为变更文件（guidance / instruction 前缀），
    /// reload_policy 决定渲染分支——Remind（specs/3.9 §155 规定形态）带
    /// 路径与 Read 引导；Inject / Confirm 尚未实现，按 Remind 兜底并 warn。
    pub(crate) fn guidance_sources_changed(
        paths: Vec<String>,
        reload_policy: share::config::domain::config::GuidanceReloadPolicy,
    ) -> Self {
        if !matches!(
            reload_policy,
            share::config::domain::config::GuidanceReloadPolicy::Remind
        ) {
            log::warn!(
                target: crate::LOG_TARGET,
                "guidance_reload_policy 未实现（policy={reload_policy:?}），按 Remind 兜底渲染",
            );
        }
        Self {
            kind: ReminderKind::new("guidance_sources_changed"),
            data: context::InvocationReminderData::guidance_sources_changed(paths),
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

/// per-message 记忆主动召回 source（#1834）：`refresh` 预物化（async）+
/// `build` 读缓存（sync）。
///
/// `ReminderSource::build` 签名同步，而召回需要 async 评分——故职责分离：
/// turn 边界（accept_step_input 发现新用户消息）先 `refresh` 物化快照，
/// pipeline 的同步 build 只读已物化结果。任何失败（评分不可用/词法零命中/
/// 低于阈值）都清空缓存 = 本 turn 静默缺席，NEVER 阻断 turn。
pub(crate) struct MemoryRecallReminderSource {
    memory: Arc<dyn memory::api::MemoryPort>,
    scorer: Arc<dyn systemone::ScoringPort>,
    cache: std::sync::Mutex<Option<String>>,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl MemoryRecallReminderSource {
    pub(crate) fn new(
        memory: Arc<dyn memory::api::MemoryPort>,
        scorer: Arc<dyn systemone::ScoringPort>,
    ) -> Self {
        Self::with_clock(memory, scorer, Arc::new(system_now_seconds))
    }

    pub(crate) fn with_clock(
        memory: Arc<dyn memory::api::MemoryPort>,
        scorer: Arc<dyn systemone::ScoringPort>,
        now: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        Self {
            memory,
            scorer,
            cache: std::sync::Mutex::new(None),
            now,
        }
    }

    /// 用户消息到达时预物化召回快照；不达标则清空（本 turn 不注入）。
    pub(crate) async fn refresh(&self, message_text: &str) {
        let next = self.recall(message_text).await;
        *self.cache.lock().expect("recall cache lock poisoned") = next;
    }

    async fn recall(&self, message_text: &str) -> Option<String> {
        let recalled = memory::api::search::recall_relevant(
            self.memory.as_ref(),
            self.scorer.as_ref(),
            message_text,
            (self.now)(),
            crate::application::constants::MEMORY_RECALL_RECALL_LIMIT,
            // 预算减条需要候选冗余：多召回一倍供截断时递补。
            crate::application::constants::MEMORY_RECALL_TOP_K * 2,
        )
        .await
        .ok()?;
        // 相关性阈值门：top1 概率不足则本 turn 不注入（防噪音稀释上下文）。
        if recalled.first()?.probability < crate::application::constants::MEMORY_RECALL_THRESHOLD {
            return None;
        }
        // token 预算（字符近似）：逐条累加，超预算依次减条——宁可不注入不超预算。
        let mut entries = Vec::new();
        let mut used_chars = 0usize;
        for item in recalled {
            let preview: String = item
                .entry
                .content
                .chars()
                .take(crate::application::constants::MEMORY_RECALL_PREVIEW_CHARS)
                .collect();
            let line_chars = preview.chars().count() + item.entry.id.to_string().len() + 4;
            if used_chars + line_chars > crate::application::constants::MEMORY_RECALL_BUDGET_CHARS {
                break;
            }
            used_chars += line_chars;
            entries.push(context::MemoryRecallEntryData {
                id: item.entry.id.to_string(),
                content_preview: preview,
            });
        }
        if entries.is_empty() {
            return None;
        }
        serde_json::to_string(&context::InvocationReminderData::MemoryRecall { entries }).ok()
    }
}

impl ReminderSource for MemoryRecallReminderSource {
    fn kind(&self) -> ReminderKind {
        ReminderKind::memory_recall()
    }

    fn policy(&self) -> ReminderPolicy {
        ReminderPolicy {
            refresh: context::RefreshTrigger::OnUserMessage,
            placement: ReminderPlacement::TailUserMessage,
            inject: InjectBehavior {
                dedup: ReminderDedup::SkipIfUnchanged,
                priority: ReminderPriority::memory_recall(),
            },
            compact: CompactBehavior::Rebuild,
        }
    }

    fn build(&self) -> Option<ReminderSnapshot> {
        self.cache
            .lock()
            .expect("recall cache lock poisoned")
            .clone()
            .map(|data| ReminderSnapshot { data })
    }

    fn render(&self, snapshot: &ReminderSnapshot, language: &str) -> String {
        match serde_json::from_str::<context::InvocationReminderData>(&snapshot.data) {
            Ok(data) => context::render_invocation_reminder_body(&data, language),
            Err(error) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "reminder 快照反序列化失败 kind=memory_recall error={error}"
                );
                String::new()
            }
        }
    }
}

fn system_now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
