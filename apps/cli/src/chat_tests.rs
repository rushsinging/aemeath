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

/// no-TUI 启动提醒渲染：每条 notice 恰好输出一行文本（含手动下载命令提示），
/// 空列表不产生任何输出。
#[test]
fn render_startup_notices_for_no_tui_prints_each_notice_once() {
    let notices = vec![composition::systemone::StartupNotice {
        message: "System One 模型未安装，评分功能不可用；执行 `aemeath systemone download` 安装"
            .to_string(),
    }];
    let rendered = render_startup_notices_for_no_tui(&notices);
    assert_eq!(
        rendered.matches("aemeath systemone download").count(),
        1,
        "命令提示应恰好出现一次"
    );
    assert_eq!(rendered.lines().count(), 1);

    assert_eq!(
        render_startup_notices_for_no_tui(&[]),
        "",
        "无提醒时不应产生输出"
    );
}

/// TUI 启动提醒渲染：每条 notice 恰好进入一个 System block，渲染后不重复。
#[test]
fn apply_startup_notices_to_tui_appends_each_notice_once() {
    let mut app = crate::tui::App::new(
        "sess-startup-notice".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    let notices = vec![composition::systemone::StartupNotice {
        message: "System One 模型未安装，评分功能不可用；执行 `aemeath systemone download` 安装"
            .to_string(),
    }];
    apply_startup_notices_to_tui(&mut app, &notices);
    let notice_texts: Vec<&String> = app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            crate::tui::model::output_timeline::OutputTimelineItem::System { text, .. } => {
                Some(text)
            }
            _ => None,
        })
        .collect();
    // App 启动自带 banner 等 System block，只断言提醒文本恰好出现一次。
    assert_eq!(
        notice_texts
            .iter()
            .filter(|text| text.contains("aemeath systemone download"))
            .count(),
        1,
        "启动提醒应恰好渲染为一个 System block"
    );

    apply_startup_notices_to_tui(&mut app, &[]);
    let notice_count_after_empty = app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            crate::tui::model::output_timeline::OutputTimelineItem::System { text, .. } => {
                Some(text)
            }
            _ => None,
        })
        .filter(|text| text.contains("aemeath systemone download"))
        .count();
    assert_eq!(
        notice_count_after_empty, 1,
        "空列表重复调用不得追加额外 block"
    );
}
