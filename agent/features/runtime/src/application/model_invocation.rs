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

use provider::{ProviderError, ProviderErrorKind};
use tokio_util::sync::CancellationToken;

const DEFAULT_MAX_ATTEMPTS: u32 = 10;
const MAX_BACKOFF: Duration = Duration::from_secs(300);

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
        if visible_delta || !error.retryable || attempt >= self.max_attempts {
            return RetryDecision::Fail;
        }

        let exponential = if attempt == 1 {
            Duration::ZERO
        } else {
            Duration::from_secs(
                1u64.checked_shl(attempt.saturating_sub(2))
                    .unwrap_or(u64::MAX),
            )
        };
        let delay = exponential
            .saturating_add(Duration::from_millis(jitter_millis))
            .max(error.retry_after.unwrap_or(Duration::ZERO))
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

    fn retryable(kind: ProviderErrorKind) -> ProviderError {
        ProviderError::retryable(kind, "safe")
    }

    #[test]
    fn first_retry_is_immediate_then_backoff_uses_jitter_and_retry_after() {
        let policy = RetryPolicy::default();
        assert_eq!(
            policy.decide(1, false, &retryable(ProviderErrorKind::Timeout), 0),
            RetryDecision::RetryAfter(Duration::ZERO)
        );
        assert_eq!(
            policy.decide(3, false, &retryable(ProviderErrorKind::Network), 250),
            RetryDecision::RetryAfter(Duration::from_millis(2_250))
        );

        let mut rate_limited = retryable(ProviderErrorKind::RateLimited);
        rate_limited.retry_after = Some(Duration::from_secs(9));
        assert_eq!(
            policy.decide(2, false, &rate_limited, 0),
            RetryDecision::RetryAfter(Duration::from_secs(9))
        );
    }

    #[test]
    fn retry_policy_clamps_wait_and_attempt_count() {
        let policy = RetryPolicy::default();
        let mut error = retryable(ProviderErrorKind::RateLimited);
        error.retry_after = Some(Duration::from_secs(900));
        assert_eq!(
            policy.decide(9, false, &error, 999),
            RetryDecision::RetryAfter(Duration::from_secs(300))
        );
        assert_eq!(policy.decide(10, false, &error, 0), RetryDecision::Fail);
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
        assert_eq!(retries.into_inner(), vec![(2, Duration::ZERO)]);
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
