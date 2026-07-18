use super::*;
use crate::domain::{ToolExecutionContext, TypedTool};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

fn test_ctx(root: std::path::PathBuf) -> ToolExecutionContext {
    let read_files = HashSet::new();
    ToolExecutionContext {
        workspace: project::wire_production_workspace(root)
            .expect("workspace 初始化成功")
            .into_views(),
        run_id: "test-run".to_string(),
        cancel: tokio_util::sync::CancellationToken::new(),
        read_files: Arc::new(Mutex::new(read_files)),
        resources: crate::domain::ToolResources {
            agent_runner: None,
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
            memory_source: crate::domain::memory_source::test_memory_source(),
            lang: "en".to_string(),
            allow_all: false,
        },
        session_reminders: None,
        plan_mode: None,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
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
