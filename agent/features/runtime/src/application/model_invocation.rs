//! model_invocation — 调 Provider、组装流、提取 tool_calls、记录 usage。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 职责：
//! - 调 `ProviderPort` 发起 LLM 调用
//! - 组装流式响应
//! - 提取 tool_calls
//! - 记录 `RawUsageSnapshot` -> 构造 `UsageRecord` 经 `UsageSink.try_record`
//! - 退避重试：仅对 Retryable(超时/5xx/429/流中断) 指数退避重试
//! - Fatal(4xx) 直接失败；context 超限 -> compact
//! - 重试期 emit `ModelInvocationRetrying{attempt}`
//!
//! 状态：无（产出 `ModelInvocation` VO 交回 Run Step）
//! 消费：`ProviderPort`、`ReasoningPort`、`UsageSink`
//!
//! 实现由 #875 负责。

#![allow(dead_code)]

use std::future::Future;
use std::time::Duration;

use audit::UsageRecord;
use futures::{Stream, StreamExt};
use provider::{InvocationEvent, ProviderError, ProviderErrorKind, RawUsageSnapshot};
use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};
use tokio_util::sync::CancellationToken;

/// One initial invocation plus at most ten retries.
const DEFAULT_MAX_ATTEMPTS: u32 = 11;
const INITIAL_BACKOFF: Duration = Duration::from_secs(10);
const MAX_BACKOFF: Duration = Duration::from_secs(120);

/// 一次 Provider attempt 的 usage 归属信息。
///
/// 每次调用 [`Self::new`] 都生成新的 `ModelInvocationId`；该类型只负责纯映射，
/// 不向 `UsageSink` 写入，避免在尚无 `RunStepId` 的 legacy 调用链上伪接生产链。
#[derive(Debug, Clone)]
pub(crate) struct ModelInvocationUsageContext {
    session_id: SessionId,
    run_id: RunId,
    run_step_id: RunStepId,
    model_invocation_id: ModelInvocationId,
    provider: String,
    model: String,
}

impl ModelInvocationUsageContext {
    pub(crate) fn new(
        session_id: SessionId,
        run_id: RunId,
        run_step_id: RunStepId,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            session_id,
            run_id,
            run_step_id,
            model_invocation_id: ModelInvocationId::new_v7(),
            provider: provider.into(),
            model: model.into(),
        }
    }

    pub(crate) fn model_invocation_id(&self) -> &ModelInvocationId {
        &self.model_invocation_id
    }

    /// 将 Provider 原始 usage 映射为 Audit 记录。
    ///
    /// Audit 的输入、输出 token 是必填值，因此任一字段未报告（`None`）时都明确
    /// 返回“不完整，不记录”。真实零值 `Some(0)` 则被原样记录。其余可选 token
    /// 字段同样保留“未报告”和“报告为零”的差异。
    pub(crate) fn map_usage(
        &self,
        recorded_at_unix_ms: u64,
        usage: RawUsageSnapshot,
    ) -> UsageRecordMapping {
        let (Some(input_tokens), Some(output_tokens)) = (usage.input_tokens, usage.output_tokens)
        else {
            let reason = match (usage.input_tokens.is_none(), usage.output_tokens.is_none()) {
                (true, true) => IncompleteUsageReason::Both,
                (true, false) => IncompleteUsageReason::Input,
                (false, true) => IncompleteUsageReason::Output,
                (false, false) => unreachable!("已完整的 usage 不应进入不完整分支"),
            };
            return UsageRecordMapping::NotRecordedIncomplete(reason);
        };

        UsageRecordMapping::Recorded(Box::new(UsageRecord {
            recorded_at_unix_ms,
            session_id: self.session_id.clone(),
            run_id: self.run_id.clone(),
            run_step_id: self.run_step_id.clone(),
            model_invocation_id: self.model_invocation_id.clone(),
            provider: self.provider.clone(),
            model: self.model.clone(),
            input_tokens: u64::from(input_tokens),
            output_tokens: u64::from(output_tokens),
            cache_write_tokens: usage.cache_write_tokens.map(u64::from),
            cache_read_tokens: usage.cache_read_tokens.map(u64::from),
            reasoning_tokens: usage.reasoning_tokens.map(u64::from),
        }))
    }
}

/// usage 映射结果；不完整数据不会构造可误写入 Audit 的 `UsageRecord`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UsageRecordMapping {
    Recorded(Box<UsageRecord>),
    /// Provider usage 不完整，因此不记录。
    NotRecordedIncomplete(IncompleteUsageReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IncompleteUsageReason {
    Input,
    Output,
    Both,
}

impl std::fmt::Display for IncompleteUsageReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Input => "usage 不完整，不记录：Provider 未报告 input_tokens",
            Self::Output => "usage 不完整，不记录：Provider 未报告 output_tokens",
            Self::Both => "usage 不完整，不记录：Provider 未报告 input_tokens 和 output_tokens",
        };
        formatter.write_str(message)
    }
}

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
        visible_delta: bool,
        error: &ProviderError,
        jitter_millis: u64,
    ) -> RetryDecision {
        if error.kind == ProviderErrorKind::ContextTooLong {
            return RetryDecision::Compact;
        }
        if error.kind == ProviderErrorKind::RateLimited
            || visible_delta
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
    /// deltas are projected to a user-visible sink and cannot be rolled back, while
    /// sub-agent deltas go to a no-op sink and remain safe to retry. A stream ending
    /// without a reducer-produced terminal value is a protocol failure.
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
                    // Reducers own stream projection cleanup (for example closing an
                    // active text/thinking block), so cancellation must pass through
                    // the same terminal failure path before control returns.
                    let error = ProviderError::cancelled();
                    let _ = apply(InvocationEvent::Failed(error.clone()));
                    return Err((error, committed_delta));
                }
                event = stream.next() => event,
            };

            let Some(event) = event else {
                return Err((missing_terminal_error(), committed_delta));
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

#[derive(Debug)]
pub(crate) enum InvocationAttempt<T> {
    Completed(T),
    Compact(ProviderError),
    Failed(ProviderError),
    Cancelled,
}

pub(crate) async fn coordinate<T, MakeAttempt, AttemptFuture, Wait, WaitFuture, Jitter, OnRetry>(
    policy: RetryPolicy,
    cancel: &CancellationToken,
    mut make_attempt: MakeAttempt,
    mut wait: Wait,
    mut jitter: Jitter,
    mut on_retry: OnRetry,
) -> InvocationAttempt<T>
where
    MakeAttempt: FnMut() -> AttemptFuture,
    AttemptFuture: Future<Output = Result<(T, bool), (ProviderError, bool)>>,
    Wait: FnMut(Duration) -> WaitFuture,
    WaitFuture: Future<Output = ()>,
    Jitter: FnMut(u32) -> u64,
    OnRetry: FnMut(u32, Duration),
{
    let mut attempt = 1;
    loop {
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => return InvocationAttempt::Cancelled,
            result = make_attempt() => result,
        };
        match result {
            Ok((value, _)) => return InvocationAttempt::Completed(value),
            Err((error, visible_delta)) => {
                match policy.decide(attempt, visible_delta, &error, jitter(attempt)) {
                    RetryDecision::Compact => return InvocationAttempt::Compact(error),
                    RetryDecision::Fail => return InvocationAttempt::Failed(error),
                    RetryDecision::RetryAfter(delay) => {
                        attempt += 1;
                        on_retry(attempt, delay);
                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => return InvocationAttempt::Cancelled,
                            _ = wait(delay) => {}
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use provider::InvocationDelta;

    fn usage_context() -> ModelInvocationUsageContext {
        ModelInvocationUsageContext::new(
            SessionId::new_v7(),
            RunId::new_v7(),
            RunStepId::new_v7(),
            "anthropic",
            "claude-test",
        )
    }

    #[test]
    fn every_attempt_gets_a_unique_model_invocation_id() {
        let session_id = SessionId::new_v7();
        let run_id = RunId::new_v7();
        let step_id = RunStepId::new_v7();
        let first = ModelInvocationUsageContext::new(
            session_id.clone(),
            run_id.clone(),
            step_id.clone(),
            "anthropic",
            "claude-test",
        );
        let second = ModelInvocationUsageContext::new(
            session_id,
            run_id,
            step_id,
            "anthropic",
            "claude-test",
        );

        assert_ne!(first.model_invocation_id(), second.model_invocation_id());
    }

    #[test]
    fn reported_zero_is_recorded_and_optional_usage_semantics_are_preserved() {
        let context = usage_context();
        let outcome = context.map_usage(
            927,
            RawUsageSnapshot {
                input_tokens: Some(0),
                output_tokens: Some(0),
                cache_read_tokens: Some(0),
                cache_write_tokens: None,
                reasoning_tokens: Some(0),
            },
        );

        let UsageRecordMapping::Recorded(record) = outcome else {
            panic!("Some(0) 是已报告的真实零值，应当记录");
        };
        assert_eq!(record.session_id, context.session_id);
        assert_eq!(record.run_id, context.run_id);
        assert_eq!(record.run_step_id, context.run_step_id);
        assert_eq!(record.model_invocation_id, *context.model_invocation_id());
        assert_eq!(record.recorded_at_unix_ms, 927);
        assert_eq!(record.provider, "anthropic");
        assert_eq!(record.model, "claude-test");
        assert_eq!(record.input_tokens, 0);
        assert_eq!(record.output_tokens, 0);
        assert_eq!(record.cache_read_tokens, Some(0));
        assert_eq!(record.cache_write_tokens, None);
        assert_eq!(record.reasoning_tokens, Some(0));
    }

    #[test]
    fn missing_required_usage_is_explicitly_not_recorded_instead_of_faked_as_zero() {
        let context = usage_context();
        let outcome = context.map_usage(
            927,
            RawUsageSnapshot {
                input_tokens: None,
                output_tokens: Some(0),
                cache_read_tokens: Some(12),
                cache_write_tokens: None,
                reasoning_tokens: None,
            },
        );

        assert_eq!(
            outcome,
            UsageRecordMapping::NotRecordedIncomplete(IncompleteUsageReason::Input)
        );
        let UsageRecordMapping::NotRecordedIncomplete(reason) = outcome else {
            panic!("缺失 input_tokens 时不应构造 UsageRecord");
        };
        assert_eq!(
            reason.to_string(),
            "usage 不完整，不记录：Provider 未报告 input_tokens"
        );

        assert_eq!(
            context.map_usage(927, RawUsageSnapshot::default()),
            UsageRecordMapping::NotRecordedIncomplete(IncompleteUsageReason::Both)
        );
    }

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

    #[tokio::test]
    async fn coordinator_never_starts_twelfth_attempt() {
        let cancel = CancellationToken::new();
        let attempts = std::cell::Cell::new(0);
        let outcome = coordinate::<(), _, _, _, _, _, _>(
            RetryPolicy::default(),
            &cancel,
            || {
                attempts.set(attempts.get() + 1);
                async { Err((retryable(ProviderErrorKind::Network), false)) }
            },
            |_| async {},
            |_| 0,
            |_, _| {},
        )
        .await;

        assert!(matches!(outcome, InvocationAttempt::Failed(_)));
        assert_eq!(attempts.get(), 11);
    }

    #[test]
    fn visible_delta_and_fatal_error_disable_retry() {
        let policy = RetryPolicy::default();
        assert_eq!(
            policy.decide(1, true, &retryable(ProviderErrorKind::Network), 0),
            RetryDecision::Fail
        );
        assert_eq!(
            policy.decide(
                1,
                false,
                &ProviderError::fatal(ProviderErrorKind::Authentication, "safe"),
                0,
            ),
            RetryDecision::Fail
        );
    }

    #[tokio::test]
    async fn coordinator_retries_without_visible_delta_and_reports_attempt() {
        let cancel = CancellationToken::new();
        let attempts = std::cell::Cell::new(0);
        let retries = std::cell::RefCell::new(Vec::new());
        let outcome = coordinate(
            RetryPolicy::default(),
            &cancel,
            || {
                let attempt = attempts.get() + 1;
                attempts.set(attempt);
                async move {
                    if attempt == 1 {
                        Err((retryable(ProviderErrorKind::Timeout), false))
                    } else {
                        Ok(("ok", false))
                    }
                }
            },
            |_| async {},
            |_| 0,
            |attempt, delay| retries.borrow_mut().push((attempt, delay)),
        )
        .await;

        assert!(matches!(outcome, InvocationAttempt::Completed("ok")));
        assert_eq!(attempts.get(), 2);
        assert_eq!(retries.into_inner(), vec![(2, Duration::from_secs(10))]);
    }

    #[tokio::test]
    async fn coordinator_cancellation_wins_during_backoff() {
        let cancel = CancellationToken::new();
        let cancel_for_wait = cancel.clone();
        let outcome = coordinate::<(), _, _, _, _, _, _>(
            RetryPolicy::default(),
            &cancel,
            || async { Err((retryable(ProviderErrorKind::Network), false)) },
            move |_| {
                let cancel = cancel_for_wait.clone();
                async move {
                    cancel.cancel();
                    std::future::pending::<()>().await;
                }
            },
            |_| 0,
            |_, _| {},
        )
        .await;

        assert!(matches!(outcome, InvocationAttempt::Cancelled));
    }

    #[tokio::test]
    async fn main_committed_delta_disables_retry() {
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
            coordinator.policy.decide(
                1,
                committed_delta,
                &retryable(ProviderErrorKind::Network),
                0
            ),
            RetryDecision::Fail
        );
    }

    #[tokio::test]
    async fn sub_agent_rollbackable_delta_can_retry() {
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
