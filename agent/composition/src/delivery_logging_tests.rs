use super::*;

fn complete_context(session_id: &str) -> LogContext {
    LogContext {
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
fn session_scope_replaces_session_and_clears_execution_fields() {
    assert_eq!(
        create_session_scope(complete_context("parent"), "frontend"),
        LogContext {
            session_id: Some("frontend".to_string()),
            ..LogContext::default()
        }
    );
}

#[tokio::test]
async fn concurrent_session_scopes_are_isolated_and_restore_parent() {
    let parent = complete_context("parent");
    instrument(parent.clone(), async {
        let first = spawn_instrumented(create_session_scope(capture(), "a"), async {
            tokio::task::yield_now().await;
            capture()
        });
        let second = spawn_instrumented(create_session_scope(capture(), "b"), async {
            tokio::task::yield_now().await;
            capture()
        });

        assert_eq!(first.await.unwrap().session_id.as_deref(), Some("a"));
        assert_eq!(second.await.unwrap().session_id.as_deref(), Some("b"));
        assert_eq!(capture(), parent);
    })
    .await;
}
