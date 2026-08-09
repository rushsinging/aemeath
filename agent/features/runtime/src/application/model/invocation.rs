//! model_invocation — 调 Provider、组装流、提取 tool_calls、记录 usage。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 职责：
//! - 调 `ProviderPort` 发起 LLM 调用
//! - 组装流式响应
//! - 提取 tool_calls
//! - 记录 `RawUsageSnapshot` -> 构造 `UsageRecord` 经 `RuntimeStreamEvent::Usage` 路径发出
//! - 退避重试：仅对 Retryable(超时/5xx/429/流中断) 指数退避重试
//! - Fatal(4xx) 直接失败；context 超限 -> compact
//! - 重试期 emit `ModelInvocationRetrying{attempt}`
//!
//! 状态：无（产出 `ModelInvocation` VO 交回 Run Step）
//! 消费：`ProviderPort`、`ReasoningPort`
//!
//! 实现由 #875 负责。

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use provider::{InvocationEvent, ProviderError, ProviderErrorKind};
use tokio_util::sync::CancellationToken;

use crate::application::context::coordination::ContextCoordinator;
use crate::application::loop_engine::chat::{
    ChatEventSinkHandle, InvocationEventReducer, InvocationResponse,
};
use crate::application::loop_engine::llm_strategy::{
    build_step_token_usage, extract_invocation_context,
};
use crate::application::loop_engine::{LoopEngineError, ModelStep, StepTokenUsage};
use crate::application::run::context::RuntimeContext;
use crate::application::run::execution_state::RunExecutionState;
use crate::application::tool::agent::ToolCall;
use crate::ports::{InvocationOptions, InvocationRequest};

/// One initial invocation plus at most ten retries.
const DEFAULT_MAX_ATTEMPTS: u32 = 11;
const INITIAL_BACKOFF: Duration = Duration::from_secs(10);
const MAX_BACKOFF: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryDecision {
    RetryAfter(Duration),
    Compact,
    Fail,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RetryPolicy {
    max_attempts: u32,
    max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            max_backoff: MAX_BACKOFF,
        }
    }
}

impl RetryPolicy {
    pub(crate) fn decide(
        &self,
        attempt: u32,
        _visible_delta: bool,
        error: &ProviderError,
        jitter_millis: u64,
    ) -> RetryDecision {
        if error.kind == ProviderErrorKind::ContextTooLong {
            return RetryDecision::Compact;
        }
        if error.kind == ProviderErrorKind::RateLimited
            || !error.retryable
            || attempt >= self.max_attempts
        {
            return RetryDecision::Fail;
        }

        let exponential = INITIAL_BACKOFF.saturating_mul(
            1u32.checked_shl(attempt.saturating_sub(1))
                .unwrap_or(u32::MAX),
        );
        let base_delay = error.retry_after.unwrap_or(exponential);
        let delay = base_delay
            .saturating_add(Duration::from_millis(jitter_millis))
            .min(self.max_backoff);
        RetryDecision::RetryAfter(delay)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryStep {
    Retry { attempt: u32, delay: Duration },
    Compact,
    Fail,
    Cancelled,
}

pub(crate) trait ModelInvocationSource: Send {
    fn runtime_context(&self) -> &RuntimeContext;
    fn role(&self) -> &str;
    fn request_log_context(&self, parent: &logging::LogContext) -> logging::LogContext;
    fn context_size(&self, execution: &RunExecutionState) -> usize;
    fn committed_delta(&self) -> bool;
    fn build_reducer(&self) -> InvocationEventReducer<ChatEventSinkHandle>;
    /// #1494：边流边执行句柄（默认无；Main observer 装配后提供）。
    fn streaming_tool(
        &self,
    ) -> Option<&Arc<crate::application::loop_engine::chat::streaming_tool::StreamingToolExecutor>>
    {
        None
    }
    fn extract_tool_calls(&self, response: &InvocationResponse) -> Vec<ToolCall>;
}

pub(crate) struct ModelInvocationContext<'a, O> {
    observer: &'a mut O,
}

impl<'a, O> ModelInvocationContext<'a, O>
where
    O: ModelInvocationObserver,
{
    pub(crate) fn new(observer: &'a mut O) -> Self {
        Self { observer }
    }

    pub(crate) async fn invoke(
        self,
        execution: &mut RunExecutionState,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        invocation_id: &sdk::ModelInvocationId,
        cancel: &CancellationToken,
    ) -> Result<(ModelStep, StepTokenUsage), LoopEngineError> {
        invoke_model_impl(
            self.observer,
            execution,
            run_id,
            step_id,
            invocation_id,
            cancel,
        )
        .await
    }
}

#[async_trait]
pub(crate) trait ModelInvocationObserver: ModelInvocationSource {
    async fn on_window(&mut self, _execution: &RunExecutionState) {}
    async fn pump_while_invoking<T: Send>(
        &mut self,
        invocation: impl std::future::Future<Output = T> + Send,
    ) -> T {
        invocation.await
    }
    async fn on_retry(&mut self, attempt: u32, delay: Duration);
    async fn on_retry_cancelled(&mut self) {}
    async fn on_response(
        &mut self,
        _execution: &mut RunExecutionState,
        _response: &InvocationResponse,
        _elapsed_secs: f64,
    ) {
    }
    async fn classify_terminal(
        &mut self,
        execution: &mut RunExecutionState,
        response: &InvocationResponse,
        calls: Vec<ToolCall>,
        usage: StepTokenUsage,
    ) -> Result<(ModelStep, StepTokenUsage), LoopEngineError>;
}

pub(crate) async fn orchestrate_model_invocation(
    observer: &mut impl ModelInvocationObserver,
    execution: &mut RunExecutionState,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    invocation_id: &sdk::ModelInvocationId,
    cancel: &CancellationToken,
) -> Result<(ModelStep, StepTokenUsage), LoopEngineError> {
    ModelInvocationContext::new(observer)
        .invoke(execution, run_id, step_id, invocation_id, cancel)
        .await
}

async fn invoke_model_impl(
    observer: &mut impl ModelInvocationObserver,
    execution: &mut RunExecutionState,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    invocation_id: &sdk::ModelInvocationId,
    cancel: &CancellationToken,
) -> Result<(ModelStep, StepTokenUsage), LoopEngineError> {
    // #1494：每次 invoke 开头重置边流边执行缓冲——上次 invoke（retry / compact）
    // 残留的旁路结果丢弃：异常时 step 作废，重试请求不带已执行工具结果。
    if let Some(executor) = observer.streaming_tool() {
        executor.reset_for_invocation(step_id, cancel.clone()).await;
    }
    if execution.context_window().is_none() {
        if let Some(request) = execution.context_request() {
            let coordinator = ContextCoordinator::new(observer.runtime_context().context());
            let window = coordinator
                .build_window(request)
                .await
                .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
            *execution.context_window_mut() = Some(window);
        }
    }
    let window = execution
        .context_window()
        .cloned()
        .ok_or_else(|| LoopEngineError::Adapter("ContextWindow 尚未构建".to_string()))?;
    observer.on_window(execution).await;
    let invocation_context = extract_invocation_context(&window);
    let mapping_summary =
        crate::application::loop_engine::llm_strategy::invocation_mapping_log_summary(
            &invocation_context,
        );
    log::debug!(
        target: crate::LOG_TARGET,
        "context_window_mapped_to_invocation messages={} system_blocks={} tool_schemas={} reminder_messages={}",
        mapping_summary.messages,
        mapping_summary.system_blocks,
        mapping_summary.tool_schemas,
        mapping_summary.reminder_messages,
    );
    crate::application::loop_engine::llm_log::log_llm_input(
        &invocation_context.messages_for_api,
        window.messages.len(),
        &invocation_context.system_blocks,
        &invocation_context.tool_schemas,
        observer.role(),
    );

    let binding = observer.runtime_context().provider();
    let reasoning = *observer
        .runtime_context()
        .reasoning()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let started = Instant::now();
    let mut coordinator = ModelInvocationCoordinator::new();
    let response = loop {
        let request_context = observer.request_log_context(&logging::capture());
        let mut reducer = observer.build_reducer();
        let provider = binding.provider.clone();
        let model = binding.model.clone();
        let max_tokens = binding.max_tokens;
        let messages = invocation_context.messages_for_api.clone();
        let system = invocation_context.system_blocks.clone();
        let tools = window.tool_schemas.clone();
        let stream_cancel = cancel.clone();
        let committed_delta = observer.committed_delta();
        let invocation = async {
            let mut request = InvocationRequest::new(
                model,
                messages,
                InvocationOptions::new(max_tokens, reasoning),
            );
            request.system = system;
            request.tools = tools;
            request.cancellation = stream_cancel.clone();
            log::debug!(
                target: crate::LOG_TARGET,
                "provider_invocation_request_ready model={} messages={} system_blocks={} tool_schemas={}",
                request.model.model,
                request.messages.len(),
                request.system.len(),
                request.tools.len(),
            );
            let stream = provider
                .invoke(request, &stream_cancel)
                .await
                .map_err(|error| (error, false))?;
            coordinator
                .pull_stream(stream, &stream_cancel, committed_delta, |event| {
                    reducer.apply(event)
                })
                .await
        };
        let result =
            logging::instrument(request_context, observer.pump_while_invoking(invocation)).await;
        match result {
            Ok((response, _)) => break response,
            Err((error, _)) if error.is_cancelled() || cancel.is_cancelled() => {
                return Err(LoopEngineError::Cancelled);
            }
            Err((error, visible_delta)) => {
                match coordinator
                    .handle_failure(&error, visible_delta, cancel)
                    .await
                {
                    RetryStep::Retry { attempt, delay } => {
                        observer.on_retry(attempt, delay).await;
                    }
                    RetryStep::Cancelled => {
                        observer.on_retry_cancelled().await;
                        return Err(LoopEngineError::Cancelled);
                    }
                    RetryStep::Compact => {
                        return Err(LoopEngineError::NeedsCompaction(error.to_string()));
                    }
                    RetryStep::Fail => return Err(LoopEngineError::Adapter(error.to_string())),
                }
            }
        }
    };
    let elapsed_secs = started.elapsed().as_secs_f64();
    let runtime_context = observer.runtime_context();
    record_successful_usage(
        runtime_context.usage_sink().as_ref(),
        crate::application::model::usage::UsageRecordContext {
            session_id: sdk::SessionId::new(runtime_context.skill_load_session_id()),
            run_id: run_id.clone(),
            run_step_id: step_id.clone(),
            model_invocation_id: invocation_id.clone(),
            model: binding.model.clone(),
        },
        &response,
        unix_timestamp_millis,
    );
    observer
        .runtime_context()
        .usage()
        .update(crate::application::model::token_usage::normalized_total_tokens(&response.usage));
    let usage = build_step_token_usage(
        &response,
        observer.context_size(execution) as u64,
        window.token_estimation.system_tokens,
        window.token_estimation.tool_schema_tokens,
        window.token_estimation.message_tokens,
    );
    execution.append_message(response.assistant_message.clone());
    execution.record_step_message(response.assistant_message.clone());
    observer
        .on_response(execution, &response, elapsed_secs)
        .await;
    let calls = observer.extract_tool_calls(&response);
    crate::application::loop_engine::llm_log::log_llm_output_and_tool_calls(
        binding.model.provider.as_str(),
        &response,
        &calls,
        elapsed_secs,
        observer.role(),
    );
    observer
        .classify_terminal(execution, &response, calls, usage)
        .await
}

#[derive(Debug, Default)]
pub(crate) struct ModelInvocationCoordinator {
    policy: RetryPolicy,
    attempt: u32,
}

impl ModelInvocationCoordinator {
    pub(crate) fn new() -> Self {
        Self {
            policy: RetryPolicy::default(),
            attempt: 1,
        }
    }

    /// Pull and reduce one provider attempt.
    ///
    /// `delta_is_committed` is deliberately supplied by the caller: main-chat
    /// deltas are already projected to a user-visible sink, while sub-agent deltas
    /// go to a no-op sink. The resulting flag is diagnostic (and may inform future
    /// presentation policy), but does not determine retry eligibility.
    /// A stream ending without a reducer-produced terminal value is a protocol failure.
    pub(crate) async fn pull_stream<T, S, Apply>(
        &self,
        mut stream: S,
        cancel: &CancellationToken,
        delta_is_committed: bool,
        mut apply: Apply,
    ) -> Result<(T, bool), (ProviderError, bool)>
    where
        S: Stream<Item = InvocationEvent> + Unpin,
        Apply: FnMut(InvocationEvent) -> Result<Option<T>, ProviderError>,
    {
        let mut committed_delta = false;
        loop {
            let event = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    // Reducers own stream cleanup (for example closing an
                    // active text/thinking block), so cancellation must pass through
                    // the same terminal failure path before control returns.
                    let error = ProviderError::cancelled();
                    let _ = apply(InvocationEvent::Failed(error.clone()));
                    return Err((error, committed_delta));
                }
                event = stream.next() => event,
            };

            let Some(event) = event else {
                let error = missing_terminal_error();
                // A raw EOF has no provider terminal event, so synthesize the
                // retryable failure through the reducer before returning to the
                // coordinator. This lets stream consumers close any active
                // text/thinking block before the next attempt starts.
                let _ = apply(InvocationEvent::Failed(error.clone()));
                return Err((error, committed_delta));
            };
            let terminal_event = matches!(
                event,
                InvocationEvent::Completed(_) | InvocationEvent::Failed(_)
            );
            if matches!(event, InvocationEvent::Delta(_)) && delta_is_committed {
                committed_delta = true;
            }
            match apply(event) {
                Ok(Some(value)) if terminal_event => return Ok((value, committed_delta)),
                Ok(Some(_)) => return Err((non_terminal_value_error(), committed_delta)),
                Err(error) => return Err((error, committed_delta)),
                Ok(None) if terminal_event => {
                    return Err((missing_terminal_error(), committed_delta));
                }
                Ok(None) => {}
            }
        }
    }

    pub(crate) async fn handle_failure(
        &mut self,
        error: &ProviderError,
        visible_delta: bool,
        cancel: &CancellationToken,
    ) -> RetryStep {
        let jitter_millis = deterministic_jitter_millis(self.attempt);
        match self
            .policy
            .decide(self.attempt, visible_delta, error, jitter_millis)
        {
            RetryDecision::Compact => RetryStep::Compact,
            RetryDecision::Fail => RetryStep::Fail,
            RetryDecision::RetryAfter(delay) => {
                self.attempt += 1;
                let attempt = self.attempt;
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => RetryStep::Cancelled,
                    _ = tokio::time::sleep(delay) => RetryStep::Retry { attempt, delay },
                }
            }
        }
    }
}

fn non_terminal_value_error() -> ProviderError {
    ProviderError::fatal(
        ProviderErrorKind::Protocol,
        "invocation reducer returned a terminal value for a non-terminal event",
    )
}

fn missing_terminal_error() -> ProviderError {
    ProviderError::retryable(
        ProviderErrorKind::StreamTruncated,
        "provider stream ended without terminal event",
    )
}

fn deterministic_jitter_millis(attempt: u32) -> u64 {
    if attempt <= 1 {
        0
    } else {
        u64::from(attempt.wrapping_mul(73) % 251)
    }
}

fn record_successful_usage(
    sink: &dyn crate::ports::UsageSink,
    context: crate::application::model::usage::UsageRecordContext,
    response: &InvocationResponse,
    clock: impl Fn() -> u64,
) {
    let factory = crate::application::model::usage::UsageRecordFactory::new(clock);
    if let Some(record) = factory.build_from_raw_usage(context, response.usage.clone()) {
        let _ = sink.try_record(record);
    }
}

fn unix_timestamp_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "invocation_usage_tests.rs"]
mod usage_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use provider::InvocationDelta;

    fn retryable(kind: ProviderErrorKind) -> ProviderError {
        ProviderError::retryable(kind, "safe")
    }

    #[test]
    fn retry_policy_rejects_rate_limits_and_uses_retry_after_or_capped_exponential_backoff() {
        let policy = RetryPolicy::default();
        assert_eq!(
            policy.decide(1, false, &retryable(ProviderErrorKind::RateLimited), 0),
            RetryDecision::Fail
        );
        assert_eq!(
            policy.decide(1, false, &retryable(ProviderErrorKind::Timeout), 250),
            RetryDecision::RetryAfter(Duration::from_millis(10_250))
        );
        assert_eq!(
            policy.decide(4, false, &retryable(ProviderErrorKind::Network), 0),
            RetryDecision::RetryAfter(Duration::from_secs(80))
        );
        assert_eq!(
            policy.decide(8, false, &retryable(ProviderErrorKind::Network), 999),
            RetryDecision::RetryAfter(Duration::from_secs(120))
        );

        let mut retry_after = retryable(ProviderErrorKind::Timeout);
        retry_after.retry_after = Some(Duration::from_secs(30));
        assert_eq!(
            policy.decide(4, false, &retry_after, 250),
            RetryDecision::RetryAfter(Duration::from_millis(30_250))
        );
    }

    #[test]
    fn retry_policy_clamps_retry_after_and_allows_ten_retries_after_first_attempt() {
        let policy = RetryPolicy::default();
        let mut error = retryable(ProviderErrorKind::Timeout);
        error.retry_after = Some(Duration::from_secs(900));
        assert_eq!(
            policy.decide(10, false, &error, 999),
            RetryDecision::RetryAfter(Duration::from_secs(120))
        );
        assert_eq!(policy.decide(11, false, &error, 0), RetryDecision::Fail);
    }

    #[test]
    fn visible_delta_does_not_disable_structurally_retryable_error() {
        let policy = RetryPolicy::default();
        assert_eq!(
            policy.decide(1, true, &retryable(ProviderErrorKind::StreamTruncated), 0,),
            RetryDecision::RetryAfter(Duration::from_secs(10))
        );
    }

    #[test]
    fn fatal_error_still_fails_after_visible_delta() {
        let policy = RetryPolicy::default();
        assert_eq!(
            policy.decide(
                1,
                true,
                &ProviderError::fatal(ProviderErrorKind::Authentication, "safe"),
                0,
            ),
            RetryDecision::Fail
        );
    }

    #[tokio::test]
    async fn main_committed_delta_remains_diagnostic_but_can_retry() {
        let coordinator = ModelInvocationCoordinator::new();
        let cancel = CancellationToken::new();
        let events = futures::stream::iter(vec![InvocationEvent::Delta(InvocationDelta::Text(
            "shown".to_string(),
        ))]);

        let outcome = coordinator
            .pull_stream(events, &cancel, true, |_| {
                Ok::<Option<()>, ProviderError>(None)
            })
            .await;

        let Err((error, committed_delta)) = outcome else {
            panic!("unterminated stream must fail");
        };
        assert_eq!(error.kind, ProviderErrorKind::StreamTruncated);
        assert!(committed_delta);
        assert_eq!(
            coordinator.policy.decide(1, committed_delta, &error, 0),
            RetryDecision::RetryAfter(Duration::from_secs(10))
        );
    }

    #[tokio::test]
    async fn raw_eof_dispatches_retryable_failure_through_reducer_for_stream_cleanup() {
        let coordinator = ModelInvocationCoordinator::new();
        let cancel = CancellationToken::new();
        let events = futures::stream::iter(vec![InvocationEvent::Delta(InvocationDelta::Text(
            "partial".to_string(),
        ))]);
        let reducer_events = std::cell::RefCell::new(Vec::new());

        let outcome = coordinator
            .pull_stream(events, &cancel, true, |event| {
                reducer_events.borrow_mut().push(event);
                Ok::<Option<()>, ProviderError>(None)
            })
            .await;

        assert!(matches!(
            outcome,
            Err((
                ProviderError {
                    kind: ProviderErrorKind::StreamTruncated,
                    retryable: true,
                    ..
                },
                true
            ))
        ));
        assert!(matches!(
            reducer_events.borrow().as_slice(),
            [
                InvocationEvent::Delta(InvocationDelta::Text(_)),
                InvocationEvent::Failed(ProviderError {
                    kind: ProviderErrorKind::StreamTruncated,
                    retryable: true,
                    ..
                })
            ]
        ));
    }

    #[tokio::test]
    async fn sub_agent_uncommitted_delta_can_retry() {
        let coordinator = ModelInvocationCoordinator::new();
        let cancel = CancellationToken::new();
        let events = futures::stream::iter(vec![InvocationEvent::Delta(InvocationDelta::Text(
            "not projected".to_string(),
        ))]);

        let Err((error, committed_delta)) = coordinator
            .pull_stream(events, &cancel, false, |_| {
                Ok::<Option<()>, ProviderError>(None)
            })
            .await
        else {
            panic!("unterminated stream must fail");
        };

        assert_eq!(error.kind, ProviderErrorKind::StreamTruncated);
        assert!(error.retryable);
        assert!(!committed_delta);
        assert_eq!(
            coordinator.policy.decide(1, committed_delta, &error, 0),
            RetryDecision::RetryAfter(Duration::from_secs(10))
        );
    }

    #[tokio::test]
    async fn pull_stream_returns_terminal_value() {
        let coordinator = ModelInvocationCoordinator::new();
        let cancel = CancellationToken::new();
        let events = futures::stream::iter(vec![InvocationEvent::Failed(ProviderError::fatal(
            ProviderErrorKind::Authentication,
            "denied",
        ))]);

        let outcome = coordinator
            .pull_stream(events, &cancel, true, |event| match event {
                InvocationEvent::Failed(error) => Err(error),
                _ => Ok(None::<()>),
            })
            .await;

        assert!(matches!(
            outcome,
            Err((
                ProviderError {
                    kind: ProviderErrorKind::Authentication,
                    ..
                },
                false
            ))
        ));
    }

    #[tokio::test]
    async fn cancellation_calls_reducer_failure_for_streaming_cleanup() {
        let coordinator = ModelInvocationCoordinator::new();
        let cancel = CancellationToken::new();
        let events = futures::stream::iter(vec![InvocationEvent::Delta(InvocationDelta::Text(
            "partial".to_string(),
        ))])
        .chain(futures::stream::pending());
        let reducer_events = std::cell::RefCell::new(Vec::new());
        let streaming_block_active = std::cell::Cell::new(false);

        let outcome = coordinator
            .pull_stream(events, &cancel, true, |event| {
                match &event {
                    InvocationEvent::Delta(_) => {
                        streaming_block_active.set(true);
                        // Force cancellation after the reducer has opened a streaming block.
                        cancel.cancel();
                    }
                    InvocationEvent::Failed(error) if error.is_cancelled() => {
                        streaming_block_active.set(false);
                    }
                    _ => {}
                }
                reducer_events.borrow_mut().push(event);
                Ok::<Option<()>, ProviderError>(None)
            })
            .await;

        assert!(matches!(
            outcome,
            Err((
                ProviderError {
                    kind: ProviderErrorKind::Cancelled,
                    ..
                },
                true
            ))
        ));
        assert!(!streaming_block_active.get());
        assert!(matches!(
            reducer_events.borrow().as_slice(),
            [
                InvocationEvent::Delta(InvocationDelta::Text(_)),
                InvocationEvent::Failed(ProviderError {
                    kind: ProviderErrorKind::Cancelled,
                    ..
                })
            ]
        ));
    }

    #[tokio::test]
    async fn thinking_cancellation_calls_reducer_failure_for_streaming_cleanup() {
        let coordinator = ModelInvocationCoordinator::new();
        let cancel = CancellationToken::new();
        let events =
            futures::stream::iter(vec![InvocationEvent::Delta(InvocationDelta::Thinking {
                thinking: "partial thought".to_string(),
                signature: None,
            })])
            .chain(futures::stream::pending());
        let reducer_events = std::cell::RefCell::new(Vec::new());
        let streaming_block_active = std::cell::Cell::new(false);

        let outcome = coordinator
            .pull_stream(events, &cancel, true, |event| {
                match &event {
                    InvocationEvent::Delta(InvocationDelta::Thinking { .. }) => {
                        streaming_block_active.set(true);
                        cancel.cancel();
                    }
                    InvocationEvent::Failed(error) if error.is_cancelled() => {
                        streaming_block_active.set(false);
                    }
                    _ => {}
                }
                reducer_events.borrow_mut().push(event);
                Ok::<Option<()>, ProviderError>(None)
            })
            .await;

        assert!(matches!(
            outcome,
            Err((
                ProviderError {
                    kind: ProviderErrorKind::Cancelled,
                    ..
                },
                true
            ))
        ));
        assert!(!streaming_block_active.get());
        assert!(matches!(
            reducer_events.borrow().as_slice(),
            [
                InvocationEvent::Delta(InvocationDelta::Thinking { .. }),
                InvocationEvent::Failed(ProviderError {
                    kind: ProviderErrorKind::Cancelled,
                    ..
                })
            ]
        ));
    }

    #[tokio::test]
    async fn reducer_value_from_delta_is_protocol_failure() {
        let coordinator = ModelInvocationCoordinator::new();
        let cancel = CancellationToken::new();
        let events = futures::stream::iter(vec![InvocationEvent::Delta(InvocationDelta::Text(
            "invalid terminal".to_string(),
        ))]);

        let outcome = coordinator
            .pull_stream(events, &cancel, true, |_| {
                Ok::<Option<()>, ProviderError>(Some(()))
            })
            .await;

        assert!(matches!(
            outcome,
            Err((
                ProviderError {
                    kind: ProviderErrorKind::Protocol,
                    retryable: false,
                    ..
                },
                true
            ))
        ));
    }

    #[test]
    fn context_too_long_requests_compaction_instead_of_retry() {
        let policy = RetryPolicy::default();
        assert_eq!(
            policy.decide(
                1,
                false,
                &ProviderError::fatal(ProviderErrorKind::ContextTooLong, "safe"),
                0,
            ),
            RetryDecision::Compact
        );
    }
}
