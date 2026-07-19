use memory::api::{
    MemoryLayer, MemoryPort, ReflectionApplyResult, ReflectionErrorCategory,
    ReflectionHistoryStore, ReflectionMessage, ReflectionOutput, ReflectionPromptPort,
    ReflectionRecord, ReflectionStatus, ReflectionTokenUsage, ReflectionTrigger,
};
use share::i18n::runtime::reflection as t;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflectionRunMode {
    Interval { turn_count: usize },
    Forced,
}

#[derive(Debug, Clone)]
pub struct CompleteReflectionResult {
    pub output: ReflectionOutput,
    pub formatted_content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub auto_applied: bool,
    pub apply_result: Option<ReflectionApplyResult>,
    pub error_category: Option<ReflectionErrorCategory>,
    /// Persistence identifier; never contains model-generated text.
    pub record_id: Option<String>,
}

#[derive(Debug, Error)]
pub enum ReflectionError {
    #[error("reflection LLM call failed: {0}")]
    LlmCall(String),
    #[error("reflection LLM returned an empty response")]
    EmptyResponse,
    #[error("reflection response could not be parsed: {0}")]
    Unparseable(String),
    #[error("reflection response contains an invalid suggestion")]
    InvalidSuggestion,
}

pub type ReflectionResult<T> = Result<T, ReflectionError>;

impl ReflectionError {
    fn category(&self) -> ReflectionErrorCategory {
        match self {
            Self::LlmCall(detail) if detail == "reflection history write failed" => {
                ReflectionErrorCategory::History
            }
            Self::LlmCall(_) => ReflectionErrorCategory::LlmCall,
            Self::EmptyResponse => ReflectionErrorCategory::EmptyResponse,
            Self::Unparseable(_) => ReflectionErrorCategory::Parse,
            Self::InvalidSuggestion => ReflectionErrorCategory::InvalidSuggestion,
        }
    }
}

fn safe_log_line(
    event: &str,
    trigger: ReflectionTaskTrigger,
    status: &str,
    metadata: Option<&ReflectionTaskMetadata>,
) -> String {
    let metadata = metadata.cloned().unwrap_or(ReflectionTaskMetadata {
        error_category: None,
        input_tokens: 0,
        output_tokens: 0,
        deviations: 0,
        suggestions: 0,
        outdated: 0,
        duration_ms: 0,
        record_id: None,
    });
    format!(
        "[reflection_{event}] trigger={} status={status} error_category={} input_tokens={} output_tokens={} deviations={} suggestions={} outdated={} duration_ms={} record_id={}",
        trigger.label(),
        metadata.error_category.map(error_category_label).unwrap_or("none"),
        metadata.input_tokens,
        metadata.output_tokens,
        metadata.deviations,
        metadata.suggestions,
        metadata.outdated,
        metadata.duration_ms,
        metadata.record_id.as_deref().unwrap_or("none"),
    )
}

fn error_category_label(category: ReflectionErrorCategory) -> &'static str {
    match category {
        ReflectionErrorCategory::LlmCall => "llm",
        ReflectionErrorCategory::EmptyResponse => "empty",
        ReflectionErrorCategory::Parse => "parse",
        ReflectionErrorCategory::InvalidSuggestion => "parse",
        ReflectionErrorCategory::Apply => "apply",
        ReflectionErrorCategory::History => "history",
        ReflectionErrorCategory::Cancelled => "cancel",
        ReflectionErrorCategory::TimedOut => "timeout",
    }
}

/// An owned message captured when a reflection task is submitted.
pub type ReflectionInputMessage = share::message::Message;

/// The complete result produced by a reflection executor.
pub type ReflectionResultPayload = CompleteReflectionResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflectionTaskTrigger {
    Interval { turn_count: usize },
    PreCompact,
    Manual,
}

impl ReflectionTaskTrigger {
    fn run_mode(self) -> ReflectionRunMode {
        match self {
            Self::Interval { turn_count } => ReflectionRunMode::Interval { turn_count },
            Self::PreCompact | Self::Manual => ReflectionRunMode::Forced,
        }
    }

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
    /// Safe execution metadata only. Reflection output text is deliberately absent.
    pub metadata: Option<ReflectionTaskMetadata>,
}

struct ReflectionPersistence {
    running: ReflectionRecord,
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

/// A one-slot background task adapter used to keep reflection off the caller's path.
///
/// The slot is claimed with `try_lock`, so submission never waits behind another
/// submitter or a finishing task. Completion, cancellation, and timeout all clear
/// that same slot before notifying drainers.
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

    /// Builds a production adapter. Production submissions provide all execution
    /// dependencies per submission so the current model client is captured rather
    /// than permanently binding the session's initial client.
    pub fn production(timeout: std::time::Duration) -> Self {
        Self::new(timeout, |_request, _cancel| async {
            Err(ReflectionError::LlmCall(
                "production reflection requires submit_complete".into(),
            ))
        })
    }

    pub fn for_complete_reflection(
        timeout: std::time::Duration,
        config: share::config::MemoryConfig,
        client: std::sync::Arc<provider::LlmClient>,
        system_prompt_text: impl Into<String>,
        lang: impl Into<String>,
        memory: std::sync::Arc<dyn MemoryPort>,
        reflection: std::sync::Arc<dyn ReflectionPromptPort>,
        history: std::sync::Arc<dyn ReflectionHistoryStore>,
    ) -> Self {
        let config = std::sync::Arc::new(config);
        let submissions_enabled =
            config.enabled && config.reflection.enabled && config.reflection.interval_turns > 0;
        let system_prompt_text = std::sync::Arc::new(system_prompt_text.into());
        let lang = std::sync::Arc::new(lang.into());
        let mut adapter = Self::new(timeout, move |request, _cancel| {
            let config = std::sync::Arc::clone(&config);
            let client = std::sync::Arc::clone(&client);
            let system_prompt_text = std::sync::Arc::clone(&system_prompt_text);
            let lang = std::sync::Arc::clone(&lang);
            let memory = std::sync::Arc::clone(&memory);
            let reflection = std::sync::Arc::clone(&reflection);
            let history = std::sync::Arc::clone(&history);
            async move {
                let id = uuid::Uuid::now_v7().to_string();
                let timestamp = chrono::Utc::now().timestamp().max(0) as u64;
                let running = ReflectionRecord::running(
                    id.clone(),
                    timestamp,
                    request.trigger.memory_trigger(),
                );
                history.append(&running).await.map_err(|_| {
                    ReflectionError::LlmCall("reflection history write failed".into())
                })?;
                execute_and_record(
                    request,
                    &config,
                    &client,
                    &system_prompt_text,
                    &lang,
                    memory.as_ref(),
                    reflection.as_ref(),
                    history.as_ref(),
                    id,
                    timestamp,
                )
                .await
            }
        });
        adapter.submissions_enabled = submissions_enabled;
        adapter
    }

    pub fn submit(&self, request: ReflectionTaskRequest) -> ReflectionTaskSubmitOutcome {
        if !self.submissions_enabled {
            return ReflectionTaskSubmitOutcome::DisabledSkipped;
        }
        let trigger = request.trigger;
        let executor = std::sync::Arc::clone(&self.executor);
        self.submit_future(trigger, move |cancel| executor(request, cancel))
    }

    /// Submits a task whose future is built only after the shared slot is claimed.
    /// This is the production seam used to capture the current model and an owned
    /// message snapshot at each trigger.
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

    fn submit_future_inner<F, Fut>(
        &self,
        trigger: ReflectionTaskTrigger,
        persistence: Option<std::sync::Arc<ReflectionPersistence>>,
        build: F,
    ) -> ReflectionTaskSubmitOutcome
    where
        F: FnOnce(tokio_util::sync::CancellationToken) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ReflectionResult<ReflectionResultPayload>>
            + Send
            + 'static,
    {
        let Ok(mut slot) = self.slot.try_lock() else {
            log::info!(target: crate::LOG_TARGET, "{}", safe_log_line("busy", trigger, "busy", None));
            return ReflectionTaskSubmitOutcome::BusySkipped;
        };
        if slot.running.is_some() {
            log::info!(target: crate::LOG_TARGET, "{}", safe_log_line("busy", trigger, "busy", None));
            return ReflectionTaskSubmitOutcome::BusySkipped;
        }

        let cancel = tokio_util::sync::CancellationToken::new();
        slot.running = Some(cancel.clone());

        let task_cancel = cancel.clone();
        let timeout = self.timeout;
        let task_slot = std::sync::Arc::clone(&self.slot);
        let changed = std::sync::Arc::clone(&self.changed);
        log::info!(target: crate::LOG_TARGET, "{}", safe_log_line("accepted", trigger, "accepted", None));
        tokio::spawn(async move {
            let started = std::time::Instant::now();
            // Establish the durable fact before cancellation, timeout, LLM, or
            // Memory apply can win. A failed initial write prevents execution.
            if let Some(persistence) = &persistence {
                if persistence
                    .history
                    .append(&persistence.running)
                    .await
                    .is_err()
                {
                    let metadata = Some(terminal_metadata(
                        ReflectionErrorCategory::History,
                        started.elapsed(),
                    ));
                    let mut slot = task_slot.lock().await;
                    slot.running = None;
                    slot.completions.push(ReflectionTaskCompletion {
                        trigger,
                        status: ReflectionTaskCompletionStatus::Failed,
                        metadata,
                    });
                    drop(slot);
                    changed.notify_waiters();
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
                    let mut terminal = ReflectionRecord::failed(
                        persistence.running.id.clone(),
                        persistence.running.timestamp,
                        persistence.running.trigger,
                        category,
                        started.elapsed().as_millis() as u64,
                    );
                    terminal.status = ReflectionStatus::Failed;
                    if persistence.history.upsert(&terminal).await.is_err() {
                        status = ReflectionTaskCompletionStatus::Failed;
                        metadata = Some(terminal_metadata(
                            ReflectionErrorCategory::History,
                            started.elapsed(),
                        ));
                    } else if let Some(metadata) = &mut metadata {
                        metadata.record_id = Some(terminal.id);
                    }
                }
            }
            if let Some(metadata) = &mut metadata {
                metadata.duration_ms = started.elapsed().as_millis() as u64;
            }
            let event = match metadata.as_ref().and_then(|item| item.error_category) {
                None => "succeeded",
                Some(category) => error_category_label(category),
            };
            log::info!(target: crate::LOG_TARGET, "{}", safe_log_line(event, trigger, completion_status_label(status), metadata.as_ref()));

            let mut slot = task_slot.lock().await;
            slot.running = None;
            slot.completions.push(ReflectionTaskCompletion {
                trigger,
                status,
                metadata,
            });
            drop(slot);
            changed.notify_waiters();
        });

        ReflectionTaskSubmitOutcome::Accepted
    }

    #[allow(clippy::too_many_arguments)]
    pub fn submit_complete(
        &self,
        request: ReflectionTaskRequest,
        config: share::config::MemoryConfig,
        client: std::sync::Arc<provider::LlmClient>,
        system_prompt_text: String,
        lang: String,
        memory: std::sync::Arc<dyn MemoryPort>,
        reflection: std::sync::Arc<dyn ReflectionPromptPort>,
        history: std::sync::Arc<dyn ReflectionHistoryStore>,
    ) -> ReflectionTaskSubmitOutcome {
        if !config.enabled || !config.reflection.enabled || config.reflection.interval_turns == 0 {
            return ReflectionTaskSubmitOutcome::DisabledSkipped;
        }
        let trigger = request.trigger;
        let id = uuid::Uuid::now_v7().to_string();
        let timestamp = chrono::Utc::now().timestamp().max(0) as u64;
        let running = ReflectionRecord::running(id.clone(), timestamp, trigger.memory_trigger());
        let terminal_history = std::sync::Arc::clone(&history);
        self.submit_persisted_future(
            trigger,
            running,
            terminal_history,
            move |_cancel| async move {
                execute_and_record(
                    request,
                    &config,
                    client.as_ref(),
                    &system_prompt_text,
                    &lang,
                    memory.as_ref(),
                    reflection.as_ref(),
                    history.as_ref(),
                    id,
                    timestamp,
                )
                .await
            },
        )
    }

    fn submit_persisted_future<F, Fut>(
        &self,
        trigger: ReflectionTaskTrigger,
        running: ReflectionRecord,
        history: std::sync::Arc<dyn ReflectionHistoryStore>,
        build: F,
    ) -> ReflectionTaskSubmitOutcome
    where
        F: FnOnce(tokio_util::sync::CancellationToken) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ReflectionResult<ReflectionResultPayload>>
            + Send
            + 'static,
    {
        let persistence = std::sync::Arc::new(ReflectionPersistence { running, history });
        self.submit_future_inner(trigger, Some(persistence), build)
    }

    pub async fn cancel(&self) {
        let slot = self.slot.lock().await;
        if let Some(cancel) = &slot.running {
            cancel.cancel();
        }
    }

    pub async fn drain(&self) -> Vec<ReflectionTaskCompletion> {
        loop {
            // Register before inspecting the slot, preventing a completion
            // notification from being lost between inspection and awaiting.
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

fn terminal_metadata(
    error_category: ReflectionErrorCategory,
    duration: std::time::Duration,
) -> ReflectionTaskMetadata {
    ReflectionTaskMetadata {
        error_category: Some(error_category),
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

#[allow(clippy::too_many_arguments)]
async fn execute_and_record(
    request: ReflectionTaskRequest,
    config: &share::config::MemoryConfig,
    client: &provider::LlmClient,
    system_prompt_text: &str,
    lang: &str,
    memory: &dyn MemoryPort,
    reflection: &dyn ReflectionPromptPort,
    history: &dyn ReflectionHistoryStore,
    id: String,
    timestamp: u64,
) -> ReflectionResult<CompleteReflectionResult> {
    let started = std::time::Instant::now();
    let trigger = request.trigger.memory_trigger();
    match run_complete_reflection(
        request.trigger.run_mode(),
        config,
        &request.messages,
        client,
        system_prompt_text,
        lang,
        memory,
        reflection,
    )
    .await
    {
        Ok(Some(mut result)) => {
            let record = ReflectionRecord {
                id: id.clone(),
                timestamp,
                trigger,
                status: if result.error_category == Some(ReflectionErrorCategory::Apply) {
                    ReflectionStatus::Failed
                } else {
                    ReflectionStatus::Succeeded
                },
                output: Some(result.output.clone()),
                apply_result: result.apply_result.clone(),
                error_category: result.error_category,
                token_usage: Some(ReflectionTokenUsage {
                    input_tokens: result.input_tokens,
                    output_tokens: result.output_tokens,
                }),
                duration_ms: started.elapsed().as_millis() as u64,
            };
            history
                .upsert(&record)
                .await
                .map_err(|_| ReflectionError::LlmCall("reflection history write failed".into()))?;
            result.record_id = Some(id);
            Ok(result)
        }
        Ok(None) => {
            let record = ReflectionRecord::failed(
                id,
                timestamp,
                trigger,
                ReflectionErrorCategory::EmptyResponse,
                started.elapsed().as_millis() as u64,
            );
            history
                .upsert(&record)
                .await
                .map_err(|_| ReflectionError::LlmCall("reflection history write failed".into()))?;
            Err(ReflectionError::EmptyResponse)
        }
        Err(error) => {
            let record = ReflectionRecord::failed(
                id,
                timestamp,
                trigger,
                error.category(),
                started.elapsed().as_millis() as u64,
            );
            if history.upsert(&record).await.is_err() {
                return Err(ReflectionError::LlmCall(
                    "reflection history write failed".into(),
                ));
            }
            Err(error)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run_complete_reflection(
    mode: ReflectionRunMode,
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    client: &provider::LlmClient,
    system_prompt_text: &str,
    lang: &str,
    memory: &dyn MemoryPort,
    reflection: &dyn ReflectionPromptPort,
) -> ReflectionResult<Option<CompleteReflectionResult>> {
    if !should_run_reflection(mode, config) {
        return Ok(None);
    }

    let entries = memory.list(Some(MemoryLayer::Project));
    let project_memory = reflection.format_memory_summary(&entries);
    let reflection_messages = messages
        .iter()
        .map(|message| {
            let role = match message.role {
                share::message::Role::User => "user",
                share::message::Role::Assistant => "assistant",
            };
            ReflectionMessage::new(role, message.text_content())
        })
        .collect::<Vec<_>>();
    let recent_summary = reflection.recent_messages_summary(&reflection_messages, usize::MAX);
    let prompt = reflection.build_prompt(&project_memory, &recent_summary, lang);

    let (full_response, input_tokens, output_tokens) =
        call_llm_for_reflection(client, &prompt, system_prompt_text).await?;

    let output = reflection
        .parse_output(&full_response)
        .map_err(|error| match error {
            memory::api::ReflectionError::InvalidSuggestion(_) => {
                ReflectionError::InvalidSuggestion
            }
            _ => ReflectionError::Unparseable("invalid reflection response".to_string()),
        })?;

    let mut formatted_content = reflection.format_output(&output, lang);
    let mut auto_applied = false;
    let mut apply_result = None;
    let mut error_category = None;
    if config.reflection.auto_apply_suggestions {
        match memory.apply_reflection(&output).await {
            Ok(result) => {
                formatted_content.push_str(&t::auto_apply_summary(
                    lang,
                    result.suggestions_added,
                    result.outdated_marked,
                ));
                auto_applied = true;
                apply_result = Some(result);
            }
            Err(memory::api::MemoryError::PartialApply {
                result_attempted,
                result_completed,
                suggestions_added,
                outdated_marked,
            }) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "Reflection auto apply partially failed"
                );
                apply_result = Some(ReflectionApplyResult {
                    attempted: result_attempted,
                    completed: result_completed,
                    suggestions_added,
                    outdated_marked,
                });
                error_category = Some(ReflectionErrorCategory::Apply);
            }
            Err(error) => {
                log::warn!(target: crate::LOG_TARGET, "Reflection auto apply failed: {error}");
                error_category = Some(ReflectionErrorCategory::Apply);
            }
        }
    }

    Ok(Some(CompleteReflectionResult {
        output,
        formatted_content,
        input_tokens,
        output_tokens,
        auto_applied,
        apply_result,
        error_category,
        record_id: None,
    }))
}

fn should_run_reflection(mode: ReflectionRunMode, config: &share::config::MemoryConfig) -> bool {
    if !config.enabled || !config.reflection.enabled || config.reflection.interval_turns == 0 {
        return false;
    }
    match mode {
        ReflectionRunMode::Interval { turn_count } => {
            turn_count.is_multiple_of(config.reflection.interval_turns)
        }
        ReflectionRunMode::Forced => true,
    }
}

async fn call_llm_for_reflection(
    client: &provider::LlmClient,
    prompt: &str,
    system_prompt_text: &str,
) -> ReflectionResult<(String, u32, u32)> {
    use futures::StreamExt;
    use provider::SystemBlock;

    let system_blocks = vec![SystemBlock::dynamic(system_prompt_text.to_string())];
    let messages = vec![share::message::Message::user(prompt)];
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut stream = client
        .invocation_stream(
            client.default_scope(),
            &system_blocks,
            &messages,
            &[],
            &cancel,
        )
        .await
        .map_err(|error| ReflectionError::LlmCall(error.to_string()))?;
    while let Some(event) = stream.next().await {
        match event {
            provider::InvocationEvent::Completed(completion) => {
                let text = completion
                    .output
                    .iter()
                    .filter_map(|block| match block {
                        provider::ProviderContentBlock::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>()
                    .trim()
                    .to_string();
                if text.is_empty() {
                    return Err(ReflectionError::EmptyResponse);
                }
                let usage = completion.usage.unwrap_or_default();
                return Ok((
                    text,
                    usage.input_tokens.unwrap_or(0),
                    usage.output_tokens.unwrap_or(0),
                ));
            }
            provider::InvocationEvent::Failed(error) => {
                return Err(ReflectionError::LlmCall(error.to_string()));
            }
            provider::InvocationEvent::Delta(_) => {}
        }
    }
    Err(ReflectionError::LlmCall(
        "provider stream ended without terminal event".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::text_completion_stream;
    use async_trait::async_trait;
    use memory::api::{NoOpMemory, ReflectionEngine};
    use provider::{InvocationStream, LlmProvider, ProviderError, SystemBlock};
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    struct StaticProvider {
        response: String,
        input_tokens: u32,
        output_tokens: u32,
    }

    #[async_trait]
    impl LlmProvider for StaticProvider {
        async fn invocation_stream(
            &self,
            _scope: &provider::InvocationScope,
            _system: &[SystemBlock],
            _messages: &[share::message::Message],
            _tool_schemas: &[serde_json::Value],
            _cancel: &CancellationToken,
        ) -> Result<InvocationStream, ProviderError> {
            Ok(text_completion_stream(
                self.response.clone(),
                self.input_tokens,
                self.output_tokens,
            ))
        }

        fn model_name(&self) -> &str {
            "reflection-test-model"
        }

        fn provider_name(&self) -> &str {
            "reflection-test-provider"
        }
    }

    fn client(response: &str) -> provider::LlmClient {
        provider::LlmClient::from_provider(Arc::new(StaticProvider {
            response: response.to_string(),
            input_tokens: 11,
            output_tokens: 22,
        }))
    }

    #[tokio::test]
    async fn disabled_reflection_does_not_call_or_parse_provider() {
        let mut config = share::config::MemoryConfig::default();
        config.reflection.enabled = false;
        let result = run_complete_reflection(
            ReflectionRunMode::Forced,
            &config,
            &[],
            &client("not json"),
            "system",
            "en",
            &NoOpMemory,
            &ReflectionEngine,
        )
        .await
        .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn forced_reflection_uses_memory_pl_and_preserves_usage() {
        let config = share::config::MemoryConfig::default();
        let result = run_complete_reflection(
            ReflectionRunMode::Forced,
            &config,
            &[share::message::Message::user("reflect")],
            &client(r#"{"deviations":["drift"],"suggested_memories":[]}"#),
            "system",
            "en",
            &NoOpMemory,
            &ReflectionEngine,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.output.deviations, ["drift"]);
        assert_eq!((result.input_tokens, result.output_tokens), (11, 22));
        assert!(result.formatted_content.contains("drift"));
        assert!(!result.auto_applied);
    }

    #[tokio::test]
    async fn auto_apply_is_dispatched_through_memory_port() {
        let mut config = share::config::MemoryConfig::default();
        config.reflection.auto_apply_suggestions = true;
        let result = run_complete_reflection(
            ReflectionRunMode::Forced,
            &config,
            &[],
            &client(r#"{"suggested_memories":[]}"#),
            "system",
            "en",
            &NoOpMemory,
            &ReflectionEngine,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(result.auto_applied);
    }

    #[tokio::test]
    async fn malformed_response_is_wrapped_as_local_execution_error() {
        let config = share::config::MemoryConfig::default();
        let error = run_complete_reflection(
            ReflectionRunMode::Forced,
            &config,
            &[],
            &client("not json"),
            "system",
            "en",
            &NoOpMemory,
            &ReflectionEngine,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, ReflectionError::Unparseable(_)));
        let rendered = error.to_string();
        assert!(!rendered.contains("not json"));
        assert!(!rendered.contains("first 200"));
    }

    #[test]
    fn safe_log_helper_never_contains_raw_model_text() {
        let secret = "SECRET-provider-raw-response";
        let error = ReflectionError::Unparseable("invalid reflection response".into());
        let metadata = ReflectionTaskMetadata {
            error_category: Some(error.category()),
            input_tokens: 3,
            output_tokens: 4,
            deviations: 0,
            suggestions: 0,
            outdated: 0,
            duration_ms: 5,
            record_id: Some("safe-id".into()),
        };
        let line = safe_log_line(
            "parse",
            ReflectionTaskTrigger::Manual,
            "failed",
            Some(&metadata),
        );
        assert!(!error.to_string().contains(secret));
        assert!(!line.contains(secret));
        assert_eq!(line.matches("safe-id").count(), 1);
    }
}

#[cfg(test)]
mod task_adapter_tests {
    use super::{
        ReflectionError, ReflectionTaskAdapter, ReflectionTaskCompletion,
        ReflectionTaskCompletionStatus, ReflectionTaskRequest, ReflectionTaskSubmitOutcome,
        ReflectionTaskTrigger,
    };
    use std::{future::pending, sync::Arc, time::Duration};
    use tokio::sync::{Mutex, Notify};
    use tokio_util::sync::CancellationToken;

    fn successful_payload() -> super::CompleteReflectionResult {
        super::CompleteReflectionResult {
            output: memory::api::ReflectionOutput::default(),
            formatted_content: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            auto_applied: false,
            apply_result: None,
            error_category: None,
            record_id: None,
        }
    }

    fn pre_compact_request(messages: Vec<share::message::Message>) -> ReflectionTaskRequest {
        ReflectionTaskRequest::new(ReflectionTaskTrigger::PreCompact, messages)
    }

    fn assert_one_completion(
        completions: Vec<ReflectionTaskCompletion>,
        expected: ReflectionTaskCompletionStatus,
    ) {
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].status, expected);
    }

    struct PanicHistory;

    #[async_trait::async_trait]
    impl memory::api::ReflectionHistoryQuery for PanicHistory {
        async fn list(
            &self,
            _limit: usize,
        ) -> Result<Vec<memory::api::ReflectionRecord>, memory::api::MemoryError> {
            panic!("disabled reflection must not query history")
        }
    }

    #[async_trait::async_trait]
    impl memory::api::ReflectionHistoryStore for PanicHistory {
        async fn append(
            &self,
            _record: &memory::api::ReflectionRecord,
        ) -> Result<(), memory::api::MemoryError> {
            panic!("disabled reflection must not append Running")
        }

        async fn upsert(
            &self,
            _record: &memory::api::ReflectionRecord,
        ) -> Result<(), memory::api::MemoryError> {
            panic!("disabled reflection must not upsert a terminal record")
        }
    }

    struct PanicProvider;

    #[async_trait::async_trait]
    impl provider::LlmProvider for PanicProvider {
        async fn invocation_stream(
            &self,
            _scope: &provider::InvocationScope,
            _system: &[provider::SystemBlock],
            _messages: &[share::message::Message],
            _tool_schemas: &[serde_json::Value],
            _cancel: &CancellationToken,
        ) -> Result<provider::InvocationStream, provider::ProviderError> {
            panic!("disabled reflection must not invoke provider")
        }

        fn model_name(&self) -> &str {
            "disabled-test"
        }

        fn provider_name(&self) -> &str {
            "disabled-test"
        }
    }

    #[tokio::test]
    async fn disabled_submit_skips_before_claiming_slot_or_writing_history() {
        let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));
        let mut config = share::config::MemoryConfig::default();
        config.reflection.enabled = false;
        let outcome = adapter.submit_complete(
            pre_compact_request(vec![share::message::Message::user("ignored")]),
            config,
            Arc::new(provider::LlmClient::from_provider(Arc::new(PanicProvider))),
            String::new(),
            "en".to_string(),
            Arc::new(memory::api::NoOpMemory),
            Arc::new(memory::api::ReflectionEngine),
            Arc::new(PanicHistory),
        );

        assert_eq!(outcome, ReflectionTaskSubmitOutcome::DisabledSkipped);
        assert!(adapter.drain().await.is_empty());
        assert_eq!(
            adapter.submit(pre_compact_request(Vec::new())),
            ReflectionTaskSubmitOutcome::Accepted,
            "disabled submission must not occupy the slot"
        );
        adapter.cancel().await;
        let _ = adapter.drain().await;
    }

    #[tokio::test]
    async fn submit_accepts_first_task_and_busy_skips_second_without_blocking() {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            move |_request: ReflectionTaskRequest, _cancel: CancellationToken| {
                let started = Arc::clone(&started);
                let release = Arc::clone(&release);
                async move {
                    started.notify_one();
                    release.notified().await;
                    Ok(successful_payload())
                }
            }
        });

        assert_eq!(
            adapter.submit(ReflectionTaskRequest::new(
                ReflectionTaskTrigger::Interval { turn_count: 10 },
                vec![],
            )),
            ReflectionTaskSubmitOutcome::Accepted
        );
        started.notified().await;
        let submitted_at = std::time::Instant::now();
        assert_eq!(
            adapter.submit(pre_compact_request(vec![])),
            ReflectionTaskSubmitOutcome::BusySkipped,
            "interval and precompact must contend for the same slot"
        );
        assert!(
            submitted_at.elapsed() < Duration::from_millis(100),
            "busy precompact submission must not await interval completion"
        );

        release.notify_one();
        assert_one_completion(
            adapter.drain().await,
            ReflectionTaskCompletionStatus::Succeeded,
        );
    }

    #[tokio::test]
    async fn submit_future_captures_dependencies_at_each_submission() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));

        for current_client_marker in ["initial", "switched"] {
            let observed = Arc::clone(&observed);
            let marker = current_client_marker.to_string();
            assert_eq!(
                adapter.submit_future(
                    ReflectionTaskTrigger::PreCompact,
                    move |_cancel| async move {
                        observed.lock().await.push(marker);
                        Ok(successful_payload())
                    }
                ),
                ReflectionTaskSubmitOutcome::Accepted
            );
            assert_one_completion(
                adapter.drain().await,
                ReflectionTaskCompletionStatus::Succeeded,
            );
        }

        assert_eq!(*observed.lock().await, ["initial", "switched"]);
    }

    #[tokio::test]
    async fn successful_and_failed_tasks_both_release_the_single_slot() {
        let attempts = Arc::new(Mutex::new(0_u8));
        let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), {
            let attempts = Arc::clone(&attempts);
            move |_request: ReflectionTaskRequest, _cancel: CancellationToken| {
                let attempts = Arc::clone(&attempts);
                async move {
                    let mut attempts = attempts.lock().await;
                    *attempts += 1;
                    match *attempts {
                        1 | 3 => Ok(successful_payload()),
                        2 => Err(ReflectionError::LlmCall("expected failure".into())),
                        _ => unreachable!(),
                    }
                }
            }
        });

        assert_eq!(
            adapter.submit(pre_compact_request(vec![])),
            ReflectionTaskSubmitOutcome::Accepted
        );
        assert_one_completion(
            adapter.drain().await,
            ReflectionTaskCompletionStatus::Succeeded,
        );
        assert_eq!(
            adapter.submit(pre_compact_request(vec![])),
            ReflectionTaskSubmitOutcome::Accepted,
            "a successful task must release the slot"
        );
        assert_one_completion(
            adapter.drain().await,
            ReflectionTaskCompletionStatus::Failed,
        );
        assert_eq!(
            adapter.submit(pre_compact_request(vec![])),
            ReflectionTaskSubmitOutcome::Accepted,
            "a failed task must release the slot"
        );
        assert_one_completion(
            adapter.drain().await,
            ReflectionTaskCompletionStatus::Succeeded,
        );
    }

    #[tokio::test]
    async fn pre_compact_request_owns_the_snapshot_used_by_the_background_task() {
        let observed = Arc::new(Mutex::new(Vec::<String>::new()));
        let release = Arc::new(Notify::new());
        let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), {
            let observed = Arc::clone(&observed);
            let release = Arc::clone(&release);
            move |request: ReflectionTaskRequest, _cancel: CancellationToken| {
                let observed = Arc::clone(&observed);
                let release = Arc::clone(&release);
                async move {
                    release.notified().await;
                    *observed.lock().await = request
                        .messages
                        .iter()
                        .map(share::message::Message::text_content)
                        .collect();
                    Ok(successful_payload())
                }
            }
        });

        let mut live_messages = vec![share::message::Message::user("before compact")];
        let frozen_request = pre_compact_request(live_messages.clone());
        assert_eq!(
            adapter.submit(frozen_request),
            ReflectionTaskSubmitOutcome::Accepted
        );
        live_messages.push(share::message::Message::user("after submit"));
        drop(live_messages);
        release.notify_one();
        assert_one_completion(
            adapter.drain().await,
            ReflectionTaskCompletionStatus::Succeeded,
        );
        assert_eq!(*observed.lock().await, ["before compact"]);
    }

    #[tokio::test]
    async fn drain_waits_for_the_running_task_and_consumes_completed_results() {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let adapter = Arc::new(ReflectionTaskAdapter::new(Duration::from_secs(5), {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            move |_request: ReflectionTaskRequest, _cancel: CancellationToken| {
                let started = Arc::clone(&started);
                let release = Arc::clone(&release);
                async move {
                    started.notify_one();
                    release.notified().await;
                    Ok(successful_payload())
                }
            }
        }));

        assert_eq!(
            adapter.submit(pre_compact_request(vec![])),
            ReflectionTaskSubmitOutcome::Accepted
        );
        started.notified().await;
        let draining = tokio::spawn({
            let adapter = Arc::clone(&adapter);
            async move { adapter.drain().await }
        });
        tokio::task::yield_now().await;
        assert!(!draining.is_finished(), "drain must join the active task");
        release.notify_one();
        assert_one_completion(
            draining.await.unwrap(),
            ReflectionTaskCompletionStatus::Succeeded,
        );
        assert!(
            adapter.drain().await.is_empty(),
            "drain must consume returned completions"
        );
    }

    #[tokio::test]
    async fn cancel_stops_the_task_records_cancellation_and_releases_the_slot() {
        let started = Arc::new(Notify::new());
        let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), {
            let started = Arc::clone(&started);
            move |_request: ReflectionTaskRequest, cancel: CancellationToken| {
                let started = Arc::clone(&started);
                async move {
                    started.notify_one();
                    cancel.cancelled().await;
                    Ok(successful_payload())
                }
            }
        });

        assert_eq!(
            adapter.submit(pre_compact_request(vec![])),
            ReflectionTaskSubmitOutcome::Accepted
        );
        started.notified().await;
        adapter.cancel().await;
        assert_one_completion(
            adapter.drain().await,
            ReflectionTaskCompletionStatus::Cancelled,
        );
        assert_eq!(
            adapter.submit(pre_compact_request(vec![])),
            ReflectionTaskSubmitOutcome::Accepted,
            "a cancelled task must release the slot"
        );
        adapter.cancel().await;
        let _ = adapter.drain().await;
    }

    #[tokio::test]
    async fn timeout_stops_the_task_records_timeout_and_releases_the_slot() {
        let adapter = ReflectionTaskAdapter::new(
            Duration::from_millis(20),
            |_request: ReflectionTaskRequest, _cancel: CancellationToken| async move {
                pending::<()>().await;
                #[allow(unreachable_code)]
                Ok(successful_payload())
            },
        );

        assert_eq!(
            adapter.submit(pre_compact_request(vec![])),
            ReflectionTaskSubmitOutcome::Accepted
        );
        assert_one_completion(
            adapter.drain().await,
            ReflectionTaskCompletionStatus::TimedOut,
        );
        assert_eq!(
            adapter.submit(pre_compact_request(vec![])),
            ReflectionTaskSubmitOutcome::Accepted,
            "a timed-out task must release the slot"
        );
        adapter.cancel().await;
        let _ = adapter.drain().await;
    }
}
