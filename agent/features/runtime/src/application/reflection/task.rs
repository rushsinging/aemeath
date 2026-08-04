use super::execution::{
    execute_reflection, CompleteReflectionResult, ReflectionExecutionError,
    ReflectionExecutionResultType, ReflectionInvocation,
};
use crate::ports::ProviderPort;
use memory::api::{
    MemoryPort, ReflectionErrorCategory, ReflectionExecutionIdentity, ReflectionHistoryStore,
    ReflectionTrigger, ReflectionWorkflow,
};

pub type ReflectionResultPayload = CompleteReflectionResult;
pub type ReflectionError = ReflectionExecutionError;
pub type ReflectionResult<T> = ReflectionExecutionResultType<T>;
pub type ReflectionInputMessage = share::message::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflectionTaskTrigger {
    Interval { step_count: usize },
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
}

impl ReflectionTaskRequest {
    pub fn new(trigger: ReflectionTaskTrigger, messages: Vec<ReflectionInputMessage>) -> Self {
        Self { trigger, messages }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflectionTaskSubmitOutcome {
    Accepted,
    BusySkipped,
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
    pub deviations: usize,
    pub suggestions: usize,
    pub outdated: usize,
    pub duration_ms: u64,
    pub record_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionTaskCompletion {
    pub trigger: ReflectionTaskTrigger,
    pub status: ReflectionTaskCompletionStatus,
    pub metadata: Option<ReflectionTaskMetadata>,
}

struct ReflectionPersistence {
    identity: ReflectionExecutionIdentity,
    history: std::sync::Arc<dyn ReflectionHistoryStore>,
}

struct ReflectionTaskSlot {
    running: Option<tokio_util::sync::CancellationToken>,
    completions: Vec<ReflectionTaskCompletion>,
}

type ReflectionTaskFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = ReflectionResult<ReflectionResultPayload>> + Send>,
>;
type ReflectionTaskExecutor = dyn Fn(ReflectionTaskRequest, tokio_util::sync::CancellationToken) -> ReflectionTaskFuture
    + Send
    + Sync;

#[derive(Clone)]
pub struct ReflectionTaskAdapter {
    timeout: std::time::Duration,
    submissions_enabled: bool,
    executor: std::sync::Arc<ReflectionTaskExecutor>,
    slot: std::sync::Arc<tokio::sync::Mutex<ReflectionTaskSlot>>,
    changed: std::sync::Arc<tokio::sync::Notify>,
}

impl ReflectionTaskAdapter {
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
            submissions_enabled: true,
            executor: std::sync::Arc::new(move |request, cancel| {
                Box::pin(executor(request, cancel))
            }),
            slot: std::sync::Arc::new(tokio::sync::Mutex::new(ReflectionTaskSlot {
                running: None,
                completions: Vec::new(),
            })),
            changed: std::sync::Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub fn production(timeout: std::time::Duration) -> Self {
        Self::new(timeout, |_request, _cancel| async {
            Err(ReflectionError::LlmCall)
        })
    }

    pub fn submit(&self, request: ReflectionTaskRequest) -> ReflectionTaskSubmitOutcome {
        if !self.submissions_enabled {
            return ReflectionTaskSubmitOutcome::DisabledSkipped;
        }
        let trigger = request.trigger;
        let executor = std::sync::Arc::clone(&self.executor);
        self.submit_future(trigger, move |cancel| executor(request, cancel))
    }

    pub fn submit_future<F, Fut>(
        &self,
        trigger: ReflectionTaskTrigger,
        build: F,
    ) -> ReflectionTaskSubmitOutcome
    where
        F: FnOnce(tokio_util::sync::CancellationToken) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ReflectionResult<ReflectionResultPayload>>
            + Send
            + 'static,
    {
        self.submit_future_inner(trigger, None, build)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn submit_complete(
        &self,
        request: ReflectionTaskRequest,
        config: share::config::MemoryConfig,
        provider: std::sync::Arc<dyn ProviderPort>,
        model: provider::ModelId,
        max_tokens: u32,
        requested_reasoning: provider::ReasoningLevel,
        system_prompt_text: String,
        lang: String,
        memory: std::sync::Arc<dyn MemoryPort>,
        history: std::sync::Arc<dyn ReflectionHistoryStore>,
    ) -> ReflectionTaskSubmitOutcome {
        if !config.enabled
            || !config.reflection.enabled
            || config.reflection.interval_run_steps == 0
        {
            return ReflectionTaskSubmitOutcome::DisabledSkipped;
        }
        let trigger = request.trigger;
        let identity = ReflectionExecutionIdentity {
            id: uuid::Uuid::now_v7().to_string(),
            timestamp: chrono::Utc::now().timestamp().max(0) as u64,
            trigger: trigger.memory_trigger(),
        };
        self.submit_future_inner(
            trigger,
            Some(ReflectionPersistence {
                identity: identity.clone(),
                history: std::sync::Arc::clone(&history),
            }),
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
    }

    fn submit_future_inner<F, Fut>(
        &self,
        trigger: ReflectionTaskTrigger,
        persistence: Option<ReflectionPersistence>,
        build: F,
    ) -> ReflectionTaskSubmitOutcome
    where
        F: FnOnce(tokio_util::sync::CancellationToken) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ReflectionResult<ReflectionResultPayload>>
            + Send
            + 'static,
    {
        let Ok(mut slot) = self.slot.try_lock() else {
            log_completion("busy", trigger, "busy", None);
            return ReflectionTaskSubmitOutcome::BusySkipped;
        };
        if slot.running.is_some() {
            log_completion("busy", trigger, "busy", None);
            return ReflectionTaskSubmitOutcome::BusySkipped;
        }
        let cancel = tokio_util::sync::CancellationToken::new();
        slot.running = Some(cancel.clone());
        let task_cancel = cancel.clone();
        let timeout = self.timeout;
        let task_slot = std::sync::Arc::clone(&self.slot);
        let changed = std::sync::Arc::clone(&self.changed);
        log_completion("accepted", trigger, "accepted", None);
        tokio::spawn(async move {
            let started = std::time::Instant::now();
            if let Some(persistence) = &persistence {
                if ReflectionWorkflow::append_running(
                    persistence.history.as_ref(),
                    &persistence.identity,
                )
                .await
                .is_err()
                {
                    finish_slot(
                        task_slot,
                        changed,
                        ReflectionTaskCompletion {
                            trigger,
                            status: ReflectionTaskCompletionStatus::Failed,
                            metadata: Some(terminal_metadata(
                                ReflectionErrorCategory::History,
                                started.elapsed(),
                            )),
                        },
                    )
                    .await;
                    return;
                }
            }
            let execution = build(task_cancel.clone());
            tokio::pin!(execution);
            let (mut status, mut metadata) = tokio::select! {
                biased;
                _ = task_cancel.cancelled() => (
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
                ReflectionTaskCompletionStatus::Cancelled
                    | ReflectionTaskCompletionStatus::TimedOut
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
            finish_slot(
                task_slot,
                changed,
                ReflectionTaskCompletion {
                    trigger,
                    status,
                    metadata,
                },
            )
            .await;
        });
        ReflectionTaskSubmitOutcome::Accepted
    }

    pub async fn cancel(&self) {
        if let Some(cancel) = &self.slot.lock().await.running {
            cancel.cancel();
        }
    }

    pub async fn shutdown(&self, deadline: std::time::Duration) -> Vec<ReflectionTaskCompletion> {
        match tokio::time::timeout(deadline, self.drain()).await {
            Ok(completions) => completions,
            Err(_) => {
                self.cancel().await;
                self.drain().await
            }
        }
    }

    pub async fn drain(&self) -> Vec<ReflectionTaskCompletion> {
        loop {
            let notified = self.changed.notified();
            let mut slot = self.slot.lock().await;
            if slot.running.is_none() {
                return std::mem::take(&mut slot.completions);
            }
            drop(slot);
            notified.await;
        }
    }
}

async fn finish_slot(
    slot: std::sync::Arc<tokio::sync::Mutex<ReflectionTaskSlot>>,
    changed: std::sync::Arc<tokio::sync::Notify>,
    completion: ReflectionTaskCompletion,
) {
    let mut slot = slot.lock().await;
    slot.running = None;
    slot.completions.push(completion);
    drop(slot);
    changed.notify_waiters();
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
        duration_ms: duration.as_millis() as u64,
        record_id: None,
    }
}

fn result_metadata(
    result: &CompleteReflectionResult,
    duration: std::time::Duration,
) -> ReflectionTaskMetadata {
    ReflectionTaskMetadata {
        error_category: result.error_category,
        input_tokens: result.input_tokens,
        output_tokens: result.output_tokens,
        deviations: result.output.deviations.len(),
        suggestions: result.output.suggested_memories.len(),
        outdated: result.output.outdated_memories.len(),
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
    }
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
