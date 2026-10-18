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

#[tokio::test]
async fn image_file_returns_image_attachment_with_detected_media_type() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("photo.png");
    // 最小 PNG 签名即可被 magic 探测识别（#1992 锁定工具层图片附件产出）。
    let png_header = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x01];
    std::fs::write(&path, png_header).expect("write fixture");
    let context = production_execution_context(temp.path().to_path_buf());

    let result = FileReadTool
        .call(serde_json::json!({ "file_path": path }), &context)
        .await;

    assert!(!result.is_error, "{}", result.text);
    assert_eq!(result.images.len(), 1, "{:?}", result.images);
    assert_eq!(result.images[0].media_type, "image/png");
    assert!(!result.images[0].base64.is_empty());
}

#[tokio::test]
async fn image_over_size_budget_is_rejected_without_attachment() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("huge.png");
    // 5MB base64 上限 → 原始字节超过 3.75MB 即超限；头 3 字节走 JPEG 探测分支。
    let payload = vec![0x89; 4 * 1024 * 1024];
    std::fs::write(&path, payload).expect("write fixture");
    let context = production_execution_context(temp.path().to_path_buf());

    let result = FileReadTool
        .call(serde_json::json!({ "file_path": path }), &context)
        .await;

    assert!(result.is_error, "{}", result.text);
    assert!(result.text.contains("image too large"), "{}", result.text);
    assert!(result.images.is_empty());
}
