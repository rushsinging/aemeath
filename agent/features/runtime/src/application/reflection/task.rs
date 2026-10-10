#[cfg(test)]
use super::execution::ReflectionExecutionError;
use super::execution::{
    execute_reflection, CompleteReflectionResult, ReflectionExecutionResultType,
    ReflectionInvocation,
};
use crate::ports::ProviderPort;
use memory::api::reflection::{
    ReflectionErrorCategory, ReflectionExecutionIdentity, ReflectionTrigger, ReflectionWorkflow,
};
use memory::api::{MemoryPort, ReflectionHistoryStore};

pub type ReflectionResultPayload = CompleteReflectionResult;
#[cfg(test)]
pub(crate) type ReflectionError = ReflectionExecutionError;
pub type ReflectionResult<T> = ReflectionExecutionResultType<T>;
pub type ReflectionInputMessage = share::message::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflectionTaskTrigger {
    Interval { run_ordinal: usize },
    PreCompact,
    Manual,
}

impl ReflectionTaskTrigger {
    pub fn memory_trigger(self) -> ReflectionTrigger {
        match self {
            Self::Interval { .. } => ReflectionTrigger::Interval,
            Self::PreCompact => ReflectionTrigger::PreCompact,
            Self::Manual => ReflectionTrigger::Manual,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Interval { .. } => "interval",
            Self::PreCompact => "pre_compact",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReflectionTaskRequest {
    pub trigger: ReflectionTaskTrigger,
    pub messages: Vec<ReflectionInputMessage>,
    /// 反思游标：本次快照覆盖到的 session active 历史终点（消息计数）。
    /// 由触发编排层（知道快照来源）填写；仅 Succeeded 落盘时推进。
    pub coverage_end: Option<u64>,
}

impl ReflectionTaskRequest {
    pub fn new(trigger: ReflectionTaskTrigger, messages: Vec<ReflectionInputMessage>) -> Self {
        Self {
            trigger,
            messages,
            coverage_end: None,
        }
    }
}

/// One run of the reflection stage, awaited by its caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReflectionRunOutcome {
    /// The stage ran to a terminal status; the completion carries its facts.
    Completed(ReflectionTaskCompletion),
    /// Configuration disabled reflection; nothing ran.
    DisabledSkipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflectionTaskCompletionStatus {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionTaskMetadata {
    pub error_category: Option<ReflectionErrorCategory>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Deviations the model reported.
    pub deviations: usize,
    /// Memories the model proposed to add (not necessarily applied).
    pub suggestions: usize,
    /// Memory ids the model proposed to mark outdated (not necessarily applied).
    pub outdated: usize,
    /// Entries the apply stage actually added.
    pub suggestions_added: usize,
    /// Entries the apply stage actually marked outdated.
    pub outdated_marked: usize,
    /// Supersede relations the apply stage actually established (#1774).
    pub superseded: usize,
    pub duration_ms: u64,
    pub record_id: Option<String>,
}

impl ReflectionTaskMetadata {
    /// Entries the apply stage actually changed. A partial apply still reports
    /// what it completed, and a run without apply (auto-apply off) reports zero.
    pub fn applied_changes(&self) -> usize {
        self.suggestions_added + self.outdated_marked + self.superseded
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionTaskCompletion {
    pub trigger: ReflectionTaskTrigger,
    pub status: ReflectionTaskCompletionStatus,
    pub metadata: Option<ReflectionTaskMetadata>,
}

/// Memory entries changed since the last notice, awaiting the next Run to deliver it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryUpdateNotice {
    pub changed: usize,
}

struct ReflectionPersistence {
    identity: ReflectionExecutionIdentity,
    history: std::sync::Arc<dyn ReflectionHistoryStore>,
}

#[cfg(test)]
type ReflectionTaskFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = ReflectionResult<ReflectionResultPayload>> + Send>,
>;
#[cfg(test)]
type ReflectionTaskExecutor = dyn Fn(ReflectionTaskRequest, tokio_util::sync::CancellationToken) -> ReflectionTaskFuture
    + Send
    + Sync;

/// Why a configuration check disabled reflection. Logged at `info` because each
/// cause is a one-off configuration decision, never a per-turn no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReflectionDisabledReason {
    MemoryOff,
    ReflectionOff,
    IntervalZero,
}

impl ReflectionDisabledReason {
    fn label(self) -> &'static str {
        match self {
            Self::MemoryOff => "memory_off",
            Self::ReflectionOff => "reflection_off",
            Self::IntervalZero => "interval_zero",
        }
    }
}

#[derive(Clone)]
pub struct ReflectionTaskAdapter {
    timeout: std::time::Duration,
    /// 仅测试注入的执行体：生产路径（`run_future` / `run_complete`）由调用方
    /// 提供 future，`production` 构造不携带执行体。
    #[cfg(test)]
    executor: std::sync::Arc<ReflectionTaskExecutor>,
    pending_memory_updates: std::sync::Arc<std::sync::Mutex<usize>>,
    /// 反思专用 provider binding（配置 `memory.reflection.model`）。
    /// 缓存位置与生命周期：session driver 装配期（run_launch 的
    /// `ReflectionTaskAdapter::production` 之后）解析一次后写入，随本 adapter
    /// 在整个 session driver 内存续（Interval/PreCompact/Manual 三个触发点拿到的
    /// 都是同一 session 级 adapter 的 clone，共享同一 `Arc`）；`None` 表示未配置
    /// 或解析失败，反思回退会话 binding。
    reflection_binding: Option<std::sync::Arc<crate::ports::ProviderBindingData>>,
}

impl ReflectionTaskAdapter {
    #[cfg(test)]
    pub fn new<F, Fut>(timeout: std::time::Duration, executor: F) -> Self
    where
        F: Fn(ReflectionTaskRequest, tokio_util::sync::CancellationToken) -> Fut
            + Send
            + Sync
            + 'static,
        Fut: std::future::Future<Output = ReflectionResult<ReflectionResultPayload>>
            + Send
            + 'static,
    {
        Self {
            timeout,
            executor: std::sync::Arc::new(move |request, cancel| {
                Box::pin(executor(request, cancel))
            }),
            pending_memory_updates: std::sync::Arc::new(std::sync::Mutex::new(0)),
            reflection_binding: None,
        }
    }

    pub fn production(timeout: std::time::Duration) -> Self {
        Self {
            timeout,
            #[cfg(test)]
            executor: std::sync::Arc::new(|_request, _cancel| {
                Box::pin(async { Err(ReflectionError::LlmCall) })
            }),
            pending_memory_updates: std::sync::Arc::new(std::sync::Mutex::new(0)),
            reflection_binding: None,
        }
    }

    /// 装配期写入反思专用 binding（`None` = 未配置/解析失败，回退会话 binding）。
    /// 见字段注释：缓存于 session 级 adapter，随 clone 共享。
    pub fn set_reflection_binding(
        &mut self,
        binding: Option<std::sync::Arc<crate::ports::ProviderBindingData>>,
    ) {
        self.reflection_binding = binding;
    }

    /// 读取缓存的反思专用 binding；`None` 时调用方回退会话 binding。
    pub fn reflection_binding(&self) -> Option<&std::sync::Arc<crate::ports::ProviderBindingData>> {
        self.reflection_binding.as_ref()
    }

    /// Run the stage without configuration gating, for callers that own their own
    /// trigger decision. `cancel` is the caller's Run token; the stage also
    /// enforces its own timeout.
    ///
    /// 仅测试使用的便捷入口（内部新建 detached token，不可取消）：生产路径
    /// 一律经 `run_future` / `run_complete` 传入 Run/step 谱系的 cancel token。
    #[cfg(test)]
    pub async fn run(&self, request: ReflectionTaskRequest) -> ReflectionRunOutcome {
        self.run_future(
            request.trigger,
            tokio_util::sync::CancellationToken::new(),
            move |cancel| {
                let executor = std::sync::Arc::clone(&self.executor);
                async move { executor(request, cancel).await }
            },
        )
        .await
    }

    /// Same as [`Self::run`] but the caller supplies the future, keeping the
    /// `cancel` token in the caller's hands for provider-level cancellation.
    pub async fn run_future<F, Fut>(
        &self,
        trigger: ReflectionTaskTrigger,
        cancel: tokio_util::sync::CancellationToken,
        build: F,
    ) -> ReflectionRunOutcome
    where
        F: FnOnce(tokio_util::sync::CancellationToken) -> Fut,
        Fut: std::future::Future<Output = ReflectionResult<ReflectionResultPayload>> + Send,
    {
        self.run_persisted(trigger, None, cancel, build).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run_complete(
        &self,
        request: ReflectionTaskRequest,
        config: share::config::MemoryConfig,
        provider: std::sync::Arc<dyn ProviderPort>,
        model: provider::ModelInfo,
        max_tokens: u32,
        requested_reasoning: share::reasoning::ReasoningLevel,
        system_prompt_text: String,
        lang: String,
        memory: std::sync::Arc<dyn MemoryPort>,
        history: std::sync::Arc<dyn ReflectionHistoryStore>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> ReflectionRunOutcome {
        if let Some(reason) = ReflectionDisabledReason::of(&config) {
            log::info!(
                target: crate::LOG_TARGET,
                "[reflection_disabled] trigger={} reason={}",
                request.trigger.label(),
                reason.label(),
            );
            return ReflectionRunOutcome::DisabledSkipped;
        }
        let trigger = request.trigger;
        // 收口上次进程退出留下的悬挂 Running 记录：阈值 = 本次任务超时的两倍
        // （至少 60s），并发中新启动的运行不会被误收口；失败只告警不阻断本次反思。
        let stale_after_secs = (self.timeout.as_secs().saturating_mul(2)).max(60);
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        match ReflectionWorkflow::reap_stale_running(history.as_ref(), now, stale_after_secs).await
        {
            Ok(0) => {}
            Ok(reaped) => log::info!(
                target: crate::LOG_TARGET,
                "[reflection_reaped] count={reaped} stale_after_secs={stale_after_secs}",
            ),
            Err(_) => log::warn!(
                target: crate::LOG_TARGET,
                "[reflection_reap_failed] 悬挂 running 记录收口失败，继续本次反思",
            ),
        }
        let identity = ReflectionExecutionIdentity {
            id: uuid::Uuid::now_v7().to_string(),
            timestamp: now,
            trigger: trigger.memory_trigger(),
            coverage_end: request.coverage_end,
        };
        self.run_persisted(
            trigger,
            Some(ReflectionPersistence {
                identity: identity.clone(),
                history: std::sync::Arc::clone(&history),
            }),
            cancel,
            move |cancel| async move {
                execute_reflection(
                    &request.messages,
                    &lang,
                    config.reflection.auto_apply_suggestions,
                    ReflectionInvocation {
                        provider: provider.as_ref(),
                        model: &model,
                        max_tokens,
                        requested_reasoning,
                        system_prompt_text: &system_prompt_text,
                    },
                    memory.as_ref(),
                    history.as_ref(),
                    &identity,
                    &cancel,
                )
                .await
            },
        )
        .await
    }

    /// Take the memory changes recorded since the last take. Returns `None` when
    /// nothing changed, so a Run never injects an empty notice.
    pub fn take_memory_update_notice(&self) -> Option<MemoryUpdateNotice> {
        let mut pending = self.pending_memory_updates.lock().ok()?;
        let changed = std::mem::take(&mut *pending);
        (changed > 0).then_some(MemoryUpdateNotice { changed })
    }

    async fn run_persisted<F, Fut>(
        &self,
        trigger: ReflectionTaskTrigger,
        persistence: Option<ReflectionPersistence>,
        cancel: tokio_util::sync::CancellationToken,
        build: F,
    ) -> ReflectionRunOutcome
    where
        F: FnOnce(tokio_util::sync::CancellationToken) -> Fut,
        Fut: std::future::Future<Output = ReflectionResult<ReflectionResultPayload>> + Send,
    {
        let timeout = self.timeout;
        let started = std::time::Instant::now();
        if let Some(persistence) = &persistence {
            if ReflectionWorkflow::append_running(
                persistence.history.as_ref(),
                &persistence.identity,
            )
            .await
            .is_err()
            {
                let metadata =
                    terminal_metadata(ReflectionErrorCategory::History, started.elapsed());
                log_completion("terminal", trigger, "failed", Some(&metadata));
                return ReflectionRunOutcome::Completed(ReflectionTaskCompletion {
                    trigger,
                    status: ReflectionTaskCompletionStatus::Failed,
                    metadata: Some(metadata),
                });
            }
        }
        log_completion("accepted", trigger, "accepted", None);
        let execution = build(cancel.clone());
        tokio::pin!(execution);
        let (mut status, mut metadata) = tokio::select! {
            biased;
            _ = cancel.cancelled() => (
                ReflectionTaskCompletionStatus::Cancelled,
                Some(terminal_metadata(ReflectionErrorCategory::Cancelled, started.elapsed())),
            ),
            _ = tokio::time::sleep(timeout) => (
                ReflectionTaskCompletionStatus::TimedOut,
                Some(terminal_metadata(ReflectionErrorCategory::TimedOut, started.elapsed())),
            ),
            result = &mut execution => match result {
                Ok(result) => (
                    if result.error_category.is_some() {
                        ReflectionTaskCompletionStatus::Failed
                    } else {
                        ReflectionTaskCompletionStatus::Succeeded
                    },
                    Some(result_metadata(&result, started.elapsed())),
                ),
                Err(error) => (
                    ReflectionTaskCompletionStatus::Failed,
                    Some(terminal_metadata(error.category(), started.elapsed())),
                ),
            },
        };
        if matches!(
            status,
            ReflectionTaskCompletionStatus::Cancelled | ReflectionTaskCompletionStatus::TimedOut
        ) {
            if let Some(persistence) = &persistence {
                let category = if status == ReflectionTaskCompletionStatus::Cancelled {
                    ReflectionErrorCategory::Cancelled
                } else {
                    ReflectionErrorCategory::TimedOut
                };
                if ReflectionWorkflow::record_failure(
                    persistence.history.as_ref(),
                    &persistence.identity,
                    category,
                    started.elapsed().as_millis() as u64,
                )
                .await
                .is_err()
                {
                    status = ReflectionTaskCompletionStatus::Failed;
                    metadata = Some(terminal_metadata(
                        ReflectionErrorCategory::History,
                        started.elapsed(),
                    ));
                } else if let Some(metadata) = &mut metadata {
                    metadata.record_id = Some(persistence.identity.id.clone());
                }
            }
        }
        if let Some(metadata) = &mut metadata {
            metadata.duration_ms = started.elapsed().as_millis() as u64;
        }
        log_completion(
            "terminal",
            trigger,
            completion_status_label(status),
            metadata.as_ref(),
        );
        if status == ReflectionTaskCompletionStatus::Succeeded {
            self.record_applied_changes(metadata.as_ref());
        }
        ReflectionRunOutcome::Completed(ReflectionTaskCompletion {
            trigger,
            status,
            metadata,
        })
    }

    fn record_applied_changes(&self, metadata: Option<&ReflectionTaskMetadata>) {
        let Some(metadata) = metadata else {
            return;
        };
        let applied_changes = metadata.applied_changes();
        if applied_changes == 0 {
            return;
        }
        if let Ok(mut pending) = self.pending_memory_updates.lock() {
            *pending += applied_changes;
        }
    }
}

impl ReflectionDisabledReason {
    fn of(config: &share::config::MemoryConfig) -> Option<Self> {
        if !config.enabled {
            return Some(Self::MemoryOff);
        }
        if !config.reflection.enabled {
            return Some(Self::ReflectionOff);
        }
        (config.reflection.interval_runs == 0).then_some(Self::IntervalZero)
    }
}

fn terminal_metadata(
    category: ReflectionErrorCategory,
    duration: std::time::Duration,
) -> ReflectionTaskMetadata {
    ReflectionTaskMetadata {
        error_category: Some(category),
        input_tokens: 0,
        output_tokens: 0,
        deviations: 0,
        suggestions: 0,
        outdated: 0,
        suggestions_added: 0,
        outdated_marked: 0,
        superseded: 0,
        duration_ms: duration.as_millis() as u64,
        record_id: None,
    }
}

fn result_metadata(
    result: &CompleteReflectionResult,
    duration: std::time::Duration,
) -> ReflectionTaskMetadata {
    let (suggestions_added, outdated_marked, superseded) = result
        .apply_result
        .as_ref()
        .map(|apply| {
            (
                apply.suggestions_added,
                apply.outdated_marked,
                apply.superseded,
            )
        })
        .unwrap_or_default();
    ReflectionTaskMetadata {
        error_category: result.error_category,
        input_tokens: result.input_tokens,
        output_tokens: result.output_tokens,
        deviations: result.output.deviations.len(),
        suggestions: result.output.suggested_memories.len(),
        outdated: result.output.outdated_memories.len(),
        suggestions_added,
        outdated_marked,
        superseded,
        duration_ms: duration.as_millis() as u64,
        record_id: result.record_id.clone(),
    }
}

fn completion_status_label(status: ReflectionTaskCompletionStatus) -> &'static str {
    match status {
        ReflectionTaskCompletionStatus::Succeeded => "succeeded",
        ReflectionTaskCompletionStatus::Failed => "failed",
        ReflectionTaskCompletionStatus::Cancelled => "cancelled",
        ReflectionTaskCompletionStatus::TimedOut => "timed_out",
    }
}

fn log_completion(
    event: &str,
    trigger: ReflectionTaskTrigger,
    status: &str,
    metadata: Option<&ReflectionTaskMetadata>,
) {
    let category = metadata
        .and_then(|item| item.error_category)
        .map(error_category_label)
        .unwrap_or("none");
    let record_id = metadata
        .and_then(|item| item.record_id.as_deref())
        .unwrap_or("none");
    log::info!(
        target: crate::LOG_TARGET,
        "[reflection_{event}] trigger={} status={status} error_category={category} record_id={record_id}",
        trigger.label(),
    );
    if event == "terminal" {
        log_terminal_facts(trigger, metadata);
    }
}

/// The apply counts and token usage only exist on a terminal that actually ran;
/// busy skips and rejected submissions never reach this point.
fn log_terminal_facts(trigger: ReflectionTaskTrigger, metadata: Option<&ReflectionTaskMetadata>) {
    let Some(metadata) = metadata else {
        return;
    };
    log::info!(
        target: crate::LOG_TARGET,
        "[reflection_tokens] trigger={} input_tokens={} output_tokens={}",
        trigger.label(),
        metadata.input_tokens,
        metadata.output_tokens,
    );
    if metadata.applied_changes() > 0 {
        log::info!(
            target: crate::LOG_TARGET,
            "[reflection_applied] trigger={} added={} superseded={} outdated={}",
            trigger.label(),
            metadata.suggestions_added,
            metadata.superseded,
            metadata.outdated_marked,
        );
    }
}

fn error_category_label(category: ReflectionErrorCategory) -> &'static str {
    match category {
        ReflectionErrorCategory::LlmCall => "llm",
        ReflectionErrorCategory::EmptyResponse => "empty",
        ReflectionErrorCategory::Parse | ReflectionErrorCategory::InvalidSuggestion => "parse",
        ReflectionErrorCategory::Apply => "apply",
        ReflectionErrorCategory::History => "history",
        ReflectionErrorCategory::Cancelled => "cancel",
        ReflectionErrorCategory::TimedOut => "timeout",
        ReflectionErrorCategory::Interrupted => "interrupted",
    }
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
