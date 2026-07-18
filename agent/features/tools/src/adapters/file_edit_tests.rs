use super::*;

fn test_ctx(root: std::path::PathBuf, read_file: String) -> ToolExecutionContext {
    let builder = crate::domain::test_support::TestToolExecutionContextBuilder::new(root);
    if read_file.is_empty() {
        builder.build()
    } else {
        builder.read_file(read_file).build()
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
    let ctx =
        crate::domain::test_support::TestToolExecutionContextBuilder::new(dir.path().to_path_buf())
            .allow_all(true)
            .build();

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
