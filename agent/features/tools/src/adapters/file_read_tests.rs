use super::*;
use crate::adapters::test_support_tests::production_execution_context;
use crate::domain::TypedTool;

#[tokio::test]
async fn reads_only_the_requested_text_window() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("large.txt");
    let mut source = String::new();
    for line_number in 1..=20_000 {
        source.push_str(&format!("line-{line_number}\n"));
    }
    tokio::fs::write(&path, source)
        .await
        .expect("write fixture");
    let context = production_execution_context(temp.path().to_path_buf());

    let result = FileReadTool
        .call(
            serde_json::json!({
                "file_path": path,
                "offset": 10_000,
                "limit": 3,
            }),
            &context,
        )
        .await;

    assert!(!result.is_error);
    let data = result.data.expect("typed read result");
    assert_eq!(data.start_line, 10_000);
    assert_eq!(data.line_count, 3);
    assert_eq!(data.total_lines, None);
    assert_eq!(
        data.content,
        "10001  line-10001\n10002  line-10002\n10003  line-10003"
    );
}

#[tokio::test]
async fn offset_past_end_reports_consumed_line_count_without_full_file_buffer() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("short.txt");
    tokio::fs::write(&path, "one\ntwo\n")
        .await
        .expect("write fixture");
    let context = production_execution_context(temp.path().to_path_buf());

    let result = FileReadTool
        .call(
            serde_json::json!({
                "file_path": path,
                "offset": 10,
                "limit": 3,
            }),
            &context,
        )
        .await;

    assert!(!result.is_error);
    assert_eq!(result.text, "(empty file)");
    let data = result.data.expect("typed read result");
    assert_eq!(data.start_line, 0);
    assert_eq!(data.line_count, 0);
    assert_eq!(data.total_lines, Some(2));
}
