use super::*;
use crate::domain::{FieldPatch, LogContext, LogContextPatch};
use std::future::pending;
use tokio::sync::Barrier;

fn patch(role: &str, turn: usize) -> LogContextPatch {
    LogContextPatch {
        role: FieldPatch::Set(role.to_string()),
        turn: FieldPatch::Set(turn),
        ..LogContextPatch::default()
    }
}

#[tokio::test]
async fn nested_scope_restores_parent_after_normal_completion() {
    within(patch("parent", 0), async {
        assert_eq!(capture().role.as_deref(), Some("parent"));
        assert_eq!(capture().turn, Some(0));

        within(patch("child", 1), async {
            assert_eq!(capture().role.as_deref(), Some("child"));
        })
        .await;

        assert_eq!(capture().role.as_deref(), Some("parent"));
        assert_eq!(capture().turn, Some(0));
    })
    .await;

    assert_eq!(capture(), LogContext::default());
}

#[tokio::test]
async fn concurrent_scopes_do_not_overwrite_each_other() {
    let barrier = std::sync::Arc::new(Barrier::new(2));
    let first_barrier = barrier.clone();
    let second_barrier = barrier.clone();

    let first = tokio::spawn(within(patch("first", 1), async move {
        first_barrier.wait().await;
        capture()
    }));
    let second = tokio::spawn(within(patch("second", 2), async move {
        second_barrier.wait().await;
        capture()
    }));

    let first = first.await.expect("first task");
    let second = second.await.expect("second task");
    assert_eq!(first.role.as_deref(), Some("first"));
    assert_eq!(first.turn, Some(1));
    assert_eq!(second.role.as_deref(), Some("second"));
    assert_eq!(second.turn, Some(2));
}

#[tokio::test]
async fn cancelled_scope_does_not_change_parent() {
    within(patch("parent", 3), async {
        let child = tokio::spawn(within(patch("child", 4), pending::<()>()));
        tokio::task::yield_now().await;
        child.abort();
        assert!(child.await.expect_err("cancelled").is_cancelled());
        assert_eq!(capture().role.as_deref(), Some("parent"));
        assert_eq!(capture().turn, Some(3));
    })
    .await;
}

#[tokio::test]
async fn panicking_scope_does_not_change_parent() {
    within(patch("parent", 5), async {
        let child = tokio::spawn(within(patch("child", 6), async {
            panic!("expected panic");
        }));
        assert!(child.await.expect_err("panic").is_panic());
        assert_eq!(capture().role.as_deref(), Some("parent"));
        assert_eq!(capture().turn, Some(5));
    })
    .await;
}

#[tokio::test]
async fn instrument_propagates_captured_context_explicitly() {
    within(patch("captured", 8), async {
        let captured = capture();
        let child = tokio::spawn(instrument(captured, async { capture() }));
        assert_eq!(
            child.await.expect("instrumented").role.as_deref(),
            Some("captured")
        );
    })
    .await;
}

#[tokio::test]
async fn spawn_instrumented_binds_context_before_task_creation() {
    let task = spawn_instrumented(
        LogContext {
            role: Some("spawned".to_string()),
            ..LogContext::default()
        },
        async { capture() },
    );

    assert_eq!(
        task.await.expect("spawned").role.as_deref(),
        Some("spawned")
    );
}

#[tokio::test]
async fn spawned_task_does_not_inherit_scope_implicitly() {
    within(patch("parent", 9), async {
        let child = tokio::spawn(async { capture() });
        assert_eq!(child.await.expect("plain spawn"), LogContext::default());
    })
    .await;
}
