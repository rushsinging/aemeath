use super::*;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

fn test_ctx(root: std::path::PathBuf, read_file: String) -> ToolExecutionContext {
    let mut read_files = HashSet::new();
    read_files.insert(read_file);
    ToolExecutionContext {
        cwd: root.clone(),
        workspace: project::api::WorkspaceService::new(root),
        cancel: tokio_util::sync::CancellationToken::new(),
        read_files: Arc::new(Mutex::new(read_files)),
        agent_runner: None,
        session_reminders: None,
        memory_config: share::config::MemoryConfig::default(),
        plan_mode: None,
        allow_all: false,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
        registry: None,
    }
}

#[test]
fn test_start_line_of_match_normal_path() {
    let content = "one\ntwo\nthree\n";

    assert_eq!(start_line_of_match(content, "two\nthree"), Some(2));
}

#[test]
fn test_start_line_of_match_boundary_first_line() {
    assert_eq!(start_line_of_match("one\ntwo\n", "one"), Some(1));
}

#[test]
fn test_start_line_of_match_error_when_missing() {
    assert_eq!(start_line_of_match("one\ntwo\n", "missing"), None);
}

#[tokio::test]
async fn test_file_edit_success_diff_marker_includes_real_line_number() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.rs");
    tokio::fs::write(&path, "one\ntwo\nthree\n").await.unwrap();
    let file_path = path.to_string_lossy().to_string();
    let ctx = test_ctx(dir.path().to_path_buf(), file_path.clone());
    let tool = FileEditTool;

    let result = tool
        .call(
            serde_json::json!({
                "file_path": file_path,
                "old_string": "two",
                "new_string": "TWO"
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error, "edit should succeed: {}", result.text);
    assert!(
        result.text.contains("---DIFF:LINE:2---"),
        "diff marker should include real line number, got: {}",
        result.text
    );
    // 落盘验证：确认文件内容真的被改写（此前无任何测试读回文件，回归盲区）
    let persisted = tokio::fs::read_to_string(&path).await.unwrap();
    assert_eq!(
        persisted, "one\nTWO\nthree\n",
        "文件应已落盘新内容: {persisted}"
    );
}
