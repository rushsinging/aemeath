use super::*;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

fn test_ctx(root: std::path::PathBuf, read_file: String) -> ToolExecutionContext {
    let mut read_files = HashSet::new();
    if !read_file.is_empty() {
        read_files.insert(read_file);
    }
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
async fn file_edit_without_prior_read_is_rejected_even_when_allow_all() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.rs");
    tokio::fs::write(&path, "one\ntwo\n").await.unwrap();
    let mut ctx = test_ctx(dir.path().to_path_buf(), String::new());
    ctx.resources.allow_all = true;

    let result = FileEditTool
        .call(
            serde_json::json!({
                "file_path": path,
                "old_string": "two",
                "new_string": "TWO"
            }),
            &ctx,
        )
        .await;

    assert!(result.is_error);
    assert!(result.text.contains("must read"));
    assert_eq!(
        tokio::fs::read_to_string(&path).await.unwrap(),
        "one\ntwo\n"
    );
}

#[tokio::test]
async fn test_file_edit_text_excludes_diff_data_carries_structured_diff() {
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
    // text 不含 diff 标记（diff 已移到 data 通道，LLM 不再看到 diff）
    assert!(
        !result.text.contains("---DIFF"),
        "text should NOT contain diff markers, got: {}",
        result.text
    );
    assert!(
        result.text.contains("Replaced 1 occurrence(s)"),
        "text should contain confirmation line, got: {}",
        result.text
    );
    // data 含结构化 diff 字段
    let data = result.data.expect("data should be present");
    assert_eq!(data.old, "two", "data.old should be the matched old text");
    assert_eq!(data.new, "TWO", "data.new should be the actual new text");
    assert_eq!(data.start_line, 2, "data.start_line should be line 2");
    // 落盘验证：确认文件内容真的被改写
    let persisted = tokio::fs::read_to_string(&path).await.unwrap();
    assert_eq!(
        persisted, "one\nTWO\nthree\n",
        "文件应已落盘新内容: {persisted}"
    );
}
