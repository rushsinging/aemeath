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
                "head_limit": 3,
                "output_mode": "content"
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
                "head_limit": 3,
                "output_mode": "content"
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
                "output_mode": "content"
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
                "head_limit": 1000,
                "output_mode": "content"
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

/// 默认模式输出文件级索引：只给「文件 + 匹配数」，不返回匹配行内容。
/// 索引是发现性信息——LLM 据此决定下一步读哪个文件。
#[tokio::test]
async fn test_grep_default_mode_returns_file_index() {
    let dir = tempfile::tempdir().unwrap();
    tokio::fs::write(dir.path().join("a.txt"), "match_me\nmatch_me\nmatch_me\n")
        .await
        .unwrap();
    tokio::fs::write(dir.path().join("b.txt"), "match_me\n")
        .await
        .unwrap();
    let ctx = test_ctx(dir.path().to_path_buf());
    let tool = GrepTool;

    let result = tool
        .call(
            serde_json::json!({
                "pattern": "match_me",
                "path": dir.path().to_string_lossy()
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error, "grep should succeed: {}", result.text);
    let data = result.data.expect("data should be present");
    assert!(data.matches.is_empty(), "索引模式不返回逐行匹配");
    assert_eq!(data.total_matches, 4, "总匹配数");
    assert_eq!(data.total_files, 2, "命中文件数");
    assert_eq!(data.files.len(), 2, "索引列表含两个文件");
    assert_eq!(data.files[0].match_count, 3, "按匹配数降序：a.txt 在前");
    assert!(
        result.text.contains("in 2 files"),
        "text 汇总文件数: {}",
        result.text
    );
}

/// 索引文本受字符预算约束：超出时截断文件列表并给出可操作的收窄提示，
/// 保证索引结果落在通用落盘阈值（最严 2,000 字符）之内、始终完整可见。
#[tokio::test]
async fn test_grep_index_output_respects_char_budget() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..60 {
        // 较长文件名：每行索引开销足够大，使 60 个文件必然超出字符预算。
        tokio::fs::write(
            dir.path()
                .join(format!("budget_probe_long_file_name_{index:03}.txt")),
            "match_me\n",
        )
        .await
        .unwrap();
    }
    let ctx = test_ctx(dir.path().to_path_buf());
    let tool = GrepTool;

    let result = tool
        .call(
            serde_json::json!({
                "pattern": "match_me",
                "path": dir.path().to_string_lossy()
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error, "grep should succeed: {}", result.text);
    let data = result.data.expect("data should be present");
    assert_eq!(data.total_files, 60, "总文件数完整保留");
    assert!(data.files.len() < 60, "文件列表被预算截断");
    assert!(
        result.text.chars().count() <= 2_000,
        "索引文本不超过通用落盘阈值: {}",
        result.text.chars().count()
    );
    assert!(
        result.text.contains("refine pattern"),
        "截断时必须给出可操作提示: {}",
        result.text
    );
}

/// 索引模式路径相对 workspace root：避免长绝对路径吃掉字符预算。
#[tokio::test]
async fn test_grep_index_paths_are_workspace_relative() {
    let dir = tempfile::tempdir().unwrap();
    tokio::fs::write(dir.path().join("a.txt"), "match_me\n")
        .await
        .unwrap();
    let ctx = test_ctx(dir.path().to_path_buf());
    let tool = GrepTool;

    let result = tool
        .call(
            serde_json::json!({
                "pattern": "match_me",
                "path": dir.path().to_string_lossy()
            }),
            &ctx,
        )
        .await;

    let data = result.data.expect("data should be present");
    let path = &data.files[0].file_path;
    assert!(!path.starts_with('/'), "索引路径应为相对路径: {path}");
    assert!(path.ends_with("a.txt"), "索引路径保留文件名: {path}");
}

/// 显式 content 模式：逐行匹配行为与既有语义一致。
#[tokio::test]
async fn test_grep_content_mode_returns_line_matches() {
    let dir = make_match_dir(10).await;
    let ctx = test_ctx(dir.path().to_path_buf());
    let tool = GrepTool;

    let result = tool
        .call(
            serde_json::json!({
                "pattern": "match_me",
                "path": dir.path().to_string_lossy(),
                "output_mode": "content"
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error, "grep should succeed: {}", result.text);
    let data = result.data.expect("data should be present");
    assert_eq!(data.matches.len(), 10, "content 模式返回逐行匹配");
    assert!(result.text.contains("10 matches"), "text 保留总数");
    assert!(data.files.is_empty(), "content 模式不填充文件索引");
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
#[ignore = "spawn 真实 rg + 进程组清理与 CI runner 进程管理交互，普通 cargo test 与 coverage 均触发 job 取消（同 bash 取消测试模式，见 #1508）；本地验证：cargo test -p tools -- --ignored grep_returns_cancelled"]
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
                "output_mode": "content"
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
#[ignore = "spawn 真实 rg 子进程；与 CI runner 进程管理交互的风险同 bash 取消测试，本地验证"]
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
                "output_mode": "content"
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

#[tokio::test]
async fn search_when_workspace_root_deleted_returns_cwd_attribution() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().to_path_buf();
    let ctx = test_ctx(root.clone());
    let tool = GrepTool;

    // 模拟事故：会话运行中工作目录（worktree）被清理后仍发起搜索。
    drop(workspace);

    let result = tool
        .call(serde_json::json!({ "pattern": "match_me" }), &ctx)
        .await;

    assert!(
        result.is_error,
        "工作目录缺失时搜索必须失败：{}",
        result.text
    );
    assert!(
        result.text.contains("工作目录已不存在"),
        "错误必须包含 cwd 归因，实际：{}",
        result.text
    );
    assert!(
        result.text.contains(&root.display().to_string()),
        "错误必须包含缺失路径，实际：{}",
        result.text
    );
}
