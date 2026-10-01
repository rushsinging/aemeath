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
/// (`chat`, `run_step`, request, model, provider, and role) are deliberately reset.
pub fn create_session_scope(parent: LogContext, session_id: impl Into<String>) -> LogContext {
    parent.patched(logging::LogContextPatch {
        session_id: logging::FieldPatch::Set(session_id.into()),
        chat_id: logging::FieldPatch::Clear,
        run_step: logging::FieldPatch::Clear,
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
#[path = "delivery_logging_tests.rs"]
mod tests;
