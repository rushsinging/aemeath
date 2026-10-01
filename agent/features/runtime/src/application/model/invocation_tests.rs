use super::*;
use provider::InvocationDeltaData;

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
    let events = futures::stream::iter(vec![InvocationEventData::Delta(
        InvocationDeltaData::Text("shown".to_string()),
    )]);

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
    let events = futures::stream::iter(vec![InvocationEventData::Delta(
        InvocationDeltaData::Text("partial".to_string()),
    )]);
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
            InvocationEventData::Delta(InvocationDeltaData::Text(_)),
            InvocationEventData::Failed(ProviderError {
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
    let events = futures::stream::iter(vec![InvocationEventData::Delta(
        InvocationDeltaData::Text("not projected".to_string()),
    )]);

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
    let events = futures::stream::iter(vec![InvocationEventData::Failed(ProviderError::fatal(
        ProviderErrorKind::Authentication,
        "denied",
    ))]);

    let outcome = coordinator
        .pull_stream(events, &cancel, true, |event| match event {
            InvocationEventData::Failed(error) => Err(error),
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
    let events = futures::stream::iter(vec![InvocationEventData::Delta(
        InvocationDeltaData::Text("partial".to_string()),
    )])
    .chain(futures::stream::pending());
    let reducer_events = std::cell::RefCell::new(Vec::new());
    let streaming_block_active = std::cell::Cell::new(false);

    let outcome = coordinator
        .pull_stream(events, &cancel, true, |event| {
            match &event {
                InvocationEventData::Delta(_) => {
                    streaming_block_active.set(true);
                    // Force cancellation after the reducer has opened a streaming block.
                    cancel.cancel();
                }
                InvocationEventData::Failed(error) if error.is_cancelled() => {
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
            InvocationEventData::Delta(InvocationDeltaData::Text(_)),
            InvocationEventData::Failed(ProviderError {
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
    let events = futures::stream::iter(vec![InvocationEventData::Delta(
        InvocationDeltaData::Thinking {
            thinking: "partial thought".to_string(),
            signature: None,
        },
    )])
    .chain(futures::stream::pending());
    let reducer_events = std::cell::RefCell::new(Vec::new());
    let streaming_block_active = std::cell::Cell::new(false);

    let outcome = coordinator
        .pull_stream(events, &cancel, true, |event| {
            match &event {
                InvocationEventData::Delta(InvocationDeltaData::Thinking { .. }) => {
                    streaming_block_active.set(true);
                    cancel.cancel();
                }
                InvocationEventData::Failed(error) if error.is_cancelled() => {
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
            InvocationEventData::Delta(InvocationDeltaData::Thinking { .. }),
            InvocationEventData::Failed(ProviderError {
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
    let events = futures::stream::iter(vec![InvocationEventData::Delta(
        InvocationDeltaData::Text("invalid terminal".to_string()),
    )]);

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
