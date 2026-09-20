use super::*;
use crate::domain::{CancellationSignal, ToolExecutionContext, TypedTool};
use async_trait::async_trait;
use std::sync::Arc;

fn test_ctx(root: std::path::PathBuf) -> ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(root).build()
}

/// 永远报告“已取消”的 signal：Grep 必须在 spawn 前后立即观察到它。
struct AlreadyCancelled;

#[async_trait]
impl CancellationSignal for AlreadyCancelled {
    fn is_cancelled(&self) -> bool {
        true
    }

    async fn cancelled(&self) {}

    fn child_signal(&self) -> Arc<dyn CancellationSignal> {
        Arc::new(Self)
    }
}

/// 可中途触发的取消 signal：运行中取消场景使用。
struct SharedCancellation {
    cancelled: std::sync::atomic::AtomicBool,
    notify: tokio::sync::Notify,
}

#[async_trait]
impl CancellationSignal for SharedCancellation {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }

    async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }

    fn child_signal(&self) -> Arc<dyn CancellationSignal> {
        Arc::new(Self {
            cancelled: std::sync::atomic::AtomicBool::new(self.is_cancelled()),
            notify: tokio::sync::Notify::new(),
        })
    }
}

/// 创建包含 N 个匹配行的临时目录，每个文件一行 `match_me_{i}`。
async fn make_match_dir(count: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..count {
        let path = dir.path().join(format!("file_{i}.txt"));
        tokio::fs::write(&path, format!("match_me_{i}\n"))
            .await
            .unwrap();
    }
    dir
}

#[tokio::test]
async fn test_grep_head_limit_narrows_results() {
    let dir = make_match_dir(10).await;
    let ctx = test_ctx(dir.path().to_path_buf());
    let tool = GrepTool;

    let result = tool
        .call(
            serde_json::json!({
                "pattern": "match_me",
                "path": dir.path().to_string_lossy(),
                "head_limit": 3
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error, "grep should succeed: {}", result.text);
    let data = result.data.expect("data should be present");
    assert_eq!(data.matches.len(), 3, "head_limit=3 → shown=3");
    assert_eq!(data.shown, 3, "shown field = 3");
    assert_eq!(data.total_matches, 10, "total_matches = real total 10");
}

#[tokio::test]
async fn test_grep_head_limit_text_shows_truncation_hint() {
    // shown < total 时，text 应包含截断提示。
    let dir = make_match_dir(10).await;
    let ctx = test_ctx(dir.path().to_path_buf());
    let tool = GrepTool;

    let result = tool
        .call(
            serde_json::json!({
                "pattern": "match_me",
                "path": dir.path().to_string_lossy(),
                "head_limit": 3
            }),
            &ctx,
        )
        .await;

    assert!(result.text.contains("showing first 3"), "text 应含截断提示");
    assert!(result.text.contains("10 matches"), "text 应含真实总数");
}

#[tokio::test]
async fn test_grep_without_head_limit_returns_all() {
    // 不设 head_limit 时返回全部匹配，无隐式截断。
    let dir = make_match_dir(10).await;
    let ctx = test_ctx(dir.path().to_path_buf());
    let tool = GrepTool;

    let result = tool
        .call(
            serde_json::json!({
                "pattern": "match_me",
                "path": dir.path().to_string_lossy(),
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error, "grep should succeed: {}", result.text);
    let data = result.data.expect("data should be present");
    assert_eq!(data.matches.len(), 10, "无 head_limit → 全部 10 条");
    assert_eq!(data.shown, 10);
    assert_eq!(data.total_matches, 10);
    assert!(
        !result.text.contains("showing first"),
        "无截断时 text 不应含截断提示"
    );
}

#[tokio::test]
async fn test_grep_head_limit_exceeds_actual_returns_all_no_truncation_hint() {
    // head_limit > 实际匹配数时返回全部，不触发截断提示。
    let dir = make_match_dir(5).await;
    let ctx = test_ctx(dir.path().to_path_buf());
    let tool = GrepTool;

    let result = tool
        .call(
            serde_json::json!({
                "pattern": "match_me",
                "path": dir.path().to_string_lossy(),
                "head_limit": 1000
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error, "grep should succeed: {}", result.text);
    let data = result.data.expect("data should be present");
    assert_eq!(data.matches.len(), 5, "head_limit > 实际 → 全部 5 条");
    assert_eq!(data.shown, 5);
    assert_eq!(data.total_matches, 5);
    assert!(
        !result.text.contains("showing first"),
        "head_limit > 实际时不应有截断提示"
    );
}

#[test]
fn grep_declares_cooperative_cancellation() {
    // supervisor 只有在工具声明 Cooperative 时才会给 grace 并尝试确认清理；
    // NonCooperative 声明会让 deadline 终态永远 CancellationUnconfirmed。
    use crate::domain::published_language::CancellationDeclaration;
    assert_eq!(
        GrepTool.cancellation(),
        CancellationDeclaration::Cooperative
    );
}

#[tokio::test]
async fn grep_returns_cancelled_result_when_signal_already_cancelled() {
    // 取消 signal 已置位时，Grep 不得返回搜索成功结果，必须返回可见的取消文本。
    let dir = make_match_dir(3).await;
    let ctx = test_ctx(dir.path().to_path_buf()).with_cancellation(Arc::new(AlreadyCancelled));
    let tool = GrepTool;

    let result = tool
        .call(
            serde_json::json!({
                "pattern": "match_me",
                "path": dir.path().to_string_lossy(),
            }),
            &ctx,
        )
        .await;

    assert!(result.is_error, "已取消时必须返回 error 结果");
    assert!(
        result.text.to_lowercase().contains("cancel"),
        "结果文本应说明搜索被取消，实际：{}",
        result.text
    );
}

#[tokio::test]
#[ignore = "spawn 真实 rg 子进程；与 CI runner 进程管理交互的风险同 bash 取消测试（#1508），本地验证"]
async fn grep_running_search_observes_cancellation_and_terminates_child() {
    // 运行中取消：搜索必须在有界时间内收敛为取消结果，子进程不得继续存活。
    let dir = make_match_dir(3).await;
    let cancellation = Arc::new(SharedCancellation {
        cancelled: std::sync::atomic::AtomicBool::new(false),
        notify: tokio::sync::Notify::new(),
    });
    let ctx = test_ctx(dir.path().to_path_buf()).with_cancellation(cancellation.clone());
    let tool = GrepTool;

    let execution = tokio::time::timeout(std::time::Duration::from_secs(5), async move {
        let call = tool.call(
            serde_json::json!({
                "pattern": "match_me",
                "path": dir.path().to_string_lossy(),
            }),
            &ctx,
        );
        tokio::pin!(call);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        cancellation
            .cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        cancellation.notify.notify_waiters();
        call.await
    })
    .await
    .expect("运行中的 Grep 必须观察到取消并在有界时间内返回");

    assert!(execution.is_error, "运行中取消必须返回 error 结果");
    assert!(
        execution.text.to_lowercase().contains("cancel"),
        "结果文本应说明搜索被取消，实际：{}",
        execution.text
    );
}
