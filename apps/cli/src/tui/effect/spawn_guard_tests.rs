use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[tokio::test]
async fn test_spawn_guarded_runs_normal_future() {
    let flag = Arc::new(AtomicBool::new(false));
    let f = flag.clone();
    spawn_guarded("normal", async move {
        f.store(true, Ordering::SeqCst);
    });
    // 让出执行权等待 task 完成
    tokio::task::yield_now().await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(flag.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_spawn_guarded_swallows_panic() {
    // panic 的 task 不应导致测试进程崩溃；spawn_guarded 返回后主流程继续。
    let started = Arc::new(AtomicBool::new(false));
    let s = started.clone();
    spawn_guarded("boom", async move {
        s.store(true, Ordering::SeqCst);
        panic!("intentional");
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    // started=true 证明 task 已运行并 panic，但 panic 未传播到此测试线程
    //（否则测试线程会被 abort，执行不到此断言）。
    assert!(started.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_spawn_guarded_propagates_captured_logging_context() {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let expected = composition::delivery_logging::LogContext {
        session_id: Some("frontend-session".to_string()),
        ..composition::delivery_logging::LogContext::default()
    };

    composition::delivery_logging::instrument(expected.clone(), async move {
        spawn_guarded("context", async move {
            tx.send(composition::delivery_logging::capture()).unwrap();
        });
    })
    .await;

    assert_eq!(rx.await.unwrap(), expected);
}

#[test]
fn production_spawn_is_instrumented_at_creation() {
    let source = include_str!("spawn_guard.rs");
    let production = source.split("#[cfg(test)]").next().unwrap();
    assert!(production.contains("composition::delivery_logging::spawn_instrumented("));
    assert!(production.contains("composition::delivery_logging::capture(),"));
    assert!(!production.contains("tokio::spawn("));
}

#[tokio::test]
async fn test_spawn_guarded_normal_after_panic_task() {
    // 边界：先 spawn 一个 panic task，再 spawn 正常 task，正常 task 仍执行。
    spawn_guarded("boom", async move { panic!("x") });
    let flag = Arc::new(AtomicBool::new(false));
    let f = flag.clone();
    spawn_guarded("ok", async move {
        f.store(true, Ordering::SeqCst);
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(flag.load(Ordering::SeqCst));
}
