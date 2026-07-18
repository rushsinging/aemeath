//! Narrow delivery-layer facade for scoped logging context propagation.
//!
//! Delivery crates use this module instead of depending on the global logging
//! implementation directly. Context construction stays here so callers cannot
//! accidentally inherit Runtime-only fields into a frontend session.

use std::future::Future;

pub use logging::LogContext;

/// Capture the context currently bound to this task.
pub fn capture() -> LogContext {
    logging::capture()
}

/// Create a frontend session context from an explicit parent snapshot.
///
/// Only the session identifier crosses the delivery boundary. Runtime fields
/// (`chat`, `turn`, request, model, provider, and role) are deliberately reset.
pub fn create_session_scope(parent: LogContext, session_id: impl Into<String>) -> LogContext {
    parent.patched(logging::LogContextPatch {
        session_id: logging::FieldPatch::Set(session_id.into()),
        chat_id: logging::FieldPatch::Clear,
        turn: logging::FieldPatch::Clear,
        request_id: logging::FieldPatch::Clear,
        model: logging::FieldPatch::Clear,
        provider: logging::FieldPatch::Clear,
        role: logging::FieldPatch::Clear,
    })
}

/// Bind an explicit context to a future for the duration of its execution.
pub async fn instrument<T>(context: LogContext, future: impl Future<Output = T>) -> T {
    logging::instrument(context, future).await
}

/// Spawn a task with context bound before task creation.
pub fn spawn_instrumented<T>(
    context: LogContext,
    future: impl Future<Output = T> + Send + 'static,
) -> tokio::task::JoinHandle<T>
where
    T: Send + 'static,
{
    logging::spawn_instrumented(context, future)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_context(session_id: &str) -> LogContext {
        LogContext {
            session_id: Some(session_id.to_string()),
            chat_id: Some("runtime-chat".to_string()),
            turn: Some(7),
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
}
