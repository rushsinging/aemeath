use super::*;
use crate::api::{ToolExecutionContext, TypedTool};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

fn test_ctx(root: std::path::PathBuf) -> ToolExecutionContext {
    let read_files = HashSet::new();
    ToolExecutionContext {
        workspace: project::api::WorkspaceService::new(root),
        cancel: tokio_util::sync::CancellationToken::new(),
        read_files: Arc::new(Mutex::new(read_files)),
        resources: crate::api::ToolResources {
            agent_runner: None,
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
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
    assert_eq!(
        data.matches.len(),
        3,
        "head_limit=3 should narrow to 3 matches"
    );
}

#[tokio::test]
async fn test_grep_without_head_limit_defaults_to_250() {
    // 创建 10 个匹配（< 250），不设 head_limit 时应全部返回。
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
    assert_eq!(
        data.matches.len(),
        10,
        "without head_limit, all 10 matches should be returned"
    );
}
