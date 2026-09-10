use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;

#[tokio::test]
async fn frontend_preserves_original_result_when_audit_drain_is_absent() {
    let client = Arc::new(NoChatClient);

    let result = run_frontend_with_audit_drain(client, None::<std::future::Ready<()>>, |_| async {
        Err::<(), sdk::SdkError>(sdk::SdkError::Internal("frontend failed".to_string()))
    })
    .await;

    assert!(matches!(
        result,
        Err(sdk::SdkError::Internal(ref message)) if message == "frontend failed"
    ));
}

#[tokio::test]
async fn frontend_success_runs_audit_drain_once_and_preserves_success() {
    let client = Arc::new(NoChatClient);
    let drain_calls = Arc::new(AtomicUsize::new(0));
    let drain_observer = drain_calls.clone();

    let result = run_frontend_with_audit_drain(
        client,
        Some(async move {
            drain_observer.fetch_add(1, Ordering::SeqCst);
        }),
        |_| async { Ok::<(), sdk::SdkError>(()) },
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(drain_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn frontend_failure_runs_audit_drain_once_and_preserves_original_error() {
    let client = Arc::new(NoChatClient);
    let drain_calls = Arc::new(AtomicUsize::new(0));
    let drain_observer = drain_calls.clone();

    let result = run_frontend_with_audit_drain(
        client,
        Some(async move {
            drain_observer.fetch_add(1, Ordering::SeqCst);
        }),
        |_| async {
            Err::<(), sdk::SdkError>(sdk::SdkError::Internal("frontend failed".to_string()))
        },
    )
    .await;

    assert!(matches!(
        result,
        Err(sdk::SdkError::Internal(ref message)) if message == "frontend failed"
    ));
    assert_eq!(drain_calls.load(Ordering::SeqCst), 1);
}

struct NoChatClient;

#[async_trait::async_trait]
impl sdk::AgentClient for NoChatClient {
    async fn chat(&self, _input: sdk::ChatRequest) -> Result<sdk::ChatStream, sdk::SdkError> {
        Err(sdk::SdkError::Internal("测试不发起 chat".to_string()))
    }
}

#[test]
fn test_should_emit_cli_frontend_started_log() {
    assert!(should_emit_cli_frontend_started_log());
}

#[test]
fn test_should_emit_quiet_cli_diagnostic_log_for_quiet_mode() {
    assert!(should_emit_quiet_cli_diagnostic_log(true));
}

#[test]
fn test_should_emit_quiet_cli_diagnostic_log_skips_tui_mode() {
    assert!(!should_emit_quiet_cli_diagnostic_log(false));
}

fn complete_context(session_id: &str) -> composition::delivery_logging::LogContext {
    composition::delivery_logging::LogContext {
        session_id: Some(session_id.to_string()),
        chat_id: Some("runtime-chat".to_string()),
        run_step: Some(7),
        request_id: Some("request-42".to_string()),
        model: Some("model-1".to_string()),
        provider: Some("provider-1".to_string()),
        role: Some("worker".to_string()),
    }
}

#[test]
fn tui_session_context_replaces_parent_with_session_only() {
    let context = composition::delivery_logging::create_session_scope(
        complete_context("parent-session"),
        "bootstrap-session",
    );

    assert_eq!(
        context,
        composition::delivery_logging::LogContext {
            session_id: Some("bootstrap-session".to_string()),
            ..composition::delivery_logging::LogContext::default()
        }
    );
}

#[tokio::test]
async fn concurrent_tui_session_scopes_do_not_leak() {
    composition::delivery_logging::instrument(complete_context("parent-session"), async {
        let first = tokio::spawn(composition::delivery_logging::instrument(
            composition::delivery_logging::create_session_scope(
                composition::delivery_logging::capture(),
                "session-a",
            ),
            async {
                tokio::task::yield_now().await;
                composition::delivery_logging::capture()
            },
        ));
        let second = tokio::spawn(composition::delivery_logging::instrument(
            composition::delivery_logging::create_session_scope(
                composition::delivery_logging::capture(),
                "session-b",
            ),
            async {
                tokio::task::yield_now().await;
                composition::delivery_logging::capture()
            },
        ));

        assert_eq!(
            first.await.unwrap(),
            composition::delivery_logging::LogContext {
                session_id: Some("session-a".to_string()),
                ..composition::delivery_logging::LogContext::default()
            }
        );
        assert_eq!(
            second.await.unwrap(),
            composition::delivery_logging::LogContext {
                session_id: Some("session-b".to_string()),
                ..composition::delivery_logging::LogContext::default()
            }
        );
        assert_eq!(
            composition::delivery_logging::capture(),
            complete_context("parent-session")
        );
    })
    .await;
}

#[tokio::test]
async fn tui_session_scope_exit_restores_complete_parent_scope() {
    let parent = complete_context("parent-session");
    composition::delivery_logging::instrument(parent.clone(), async {
        composition::delivery_logging::instrument(
            composition::delivery_logging::create_session_scope(
                composition::delivery_logging::capture(),
                "bootstrap-session",
            ),
            async {
                assert_eq!(
                    composition::delivery_logging::capture(),
                    composition::delivery_logging::LogContext {
                        session_id: Some("bootstrap-session".to_string()),
                        ..composition::delivery_logging::LogContext::default()
                    }
                );
            },
        )
        .await;

        assert_eq!(composition::delivery_logging::capture(), parent);
    })
    .await;
}
