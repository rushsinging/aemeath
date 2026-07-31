use super::*;
use async_trait::async_trait;
use std::sync::Mutex;

use crate::ports::{ToolResultBlobError, ToolResultBlobRef};

#[derive(Default)]
struct FakeBlobPort {
    writes: Mutex<Vec<(String, String, Vec<u8>)>>,
    failure: Mutex<Option<ToolResultBlobError>>,
}

#[async_trait]
impl ToolResultBlobPort for FakeBlobPort {
    async fn write_once(
        &self,
        session_id: &str,
        tool_use_id: &str,
        bytes: &[u8],
    ) -> Result<ToolResultBlobRef, ToolResultBlobError> {
        if let Some(error) = self.failure.lock().unwrap().clone() {
            return Err(error);
        }
        self.writes.lock().unwrap().push((
            session_id.to_string(),
            tool_use_id.to_string(),
            bytes.to_vec(),
        ));
        Ok(ToolResultBlobRef::new(format!(
            "tool-result://{session_id}/{tool_use_id}"
        )))
    }
}

#[tokio::test]
async fn output_at_threshold_remains_inline_without_blob_write() {
    let blobs = Arc::new(FakeBlobPort::default());
    let materializer =
        ToolResultMaterializer::new(blobs.clone(), ToolResultMaterializationPolicy::new(4, 2, 1));

    let output = materializer
        .materialize("session", "tool", "四个字符")
        .await;

    assert_eq!(output.text(), "四个字符");
    assert!(!output.persisted());
    assert!(blobs.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn oversized_unicode_output_writes_full_bytes_and_formats_character_preview() {
    let blobs = Arc::new(FakeBlobPort::default());
    let materializer =
        ToolResultMaterializer::new(blobs.clone(), ToolResultMaterializationPolicy::new(4, 2, 1));

    let output = materializer
        .materialize("session", "tool", "甲乙丙丁戊")
        .await;

    assert!(output.persisted());
    assert!(output.text().contains("甲乙"));
    assert!(output.text().contains("戊"));
    assert!(output.text().contains("2 chars omitted"));
    assert!(output.text().contains("tool-result://session/tool"));
    assert_eq!(
        blobs.writes.lock().unwrap().as_slice(),
        &[(
            "session".into(),
            "tool".into(),
            "甲乙丙丁戊".as_bytes().to_vec()
        )]
    );
}

#[tokio::test]
async fn oversized_output_when_blob_write_fails_keeps_bounded_projection() {
    let blobs = Arc::new(FakeBlobPort::default());
    *blobs.failure.lock().unwrap() = Some(ToolResultBlobError::write("磁盘不可写"));
    let materializer =
        ToolResultMaterializer::new(blobs, ToolResultMaterializationPolicy::new(4, 2, 1));

    let output = materializer
        .materialize("session", "tool", "甲乙丙丁戊")
        .await;

    assert!(!output.persisted());
    assert!(output.text().contains("甲乙"));
    assert!(output.text().contains("戊"));
    assert!(output.text().contains("2 chars omitted"));
    assert!(!output.text().contains("甲乙丙丁戊"));
    assert!(output.warning().is_some());
}

#[tokio::test]
async fn oversized_output_projection_reports_exact_size_and_persisted_locator() {
    let blobs = Arc::new(FakeBlobPort::default());
    let materializer =
        ToolResultMaterializer::new(blobs, ToolResultMaterializationPolicy::new(4, 2, 1));

    let output = materializer
        .materialize("session", "tool", "甲乙丙丁戊")
        .await;

    assert_eq!(output.original_chars(), 5);
    assert_eq!(output.original_bytes(), 15);
    assert_eq!(output.omitted_chars(), 2);
    assert_eq!(output.blob_locator(), Some("tool-result://session/tool"));
    assert_eq!(output.degradation_reason(), None);
}

#[tokio::test]
async fn oversized_output_projection_reports_unavailable_blob_without_locator() {
    let blobs = Arc::new(FakeBlobPort::default());
    *blobs.failure.lock().unwrap() = Some(ToolResultBlobError::write("磁盘不可写"));
    let materializer =
        ToolResultMaterializer::new(blobs, ToolResultMaterializationPolicy::new(4, 2, 1));

    let output = materializer
        .materialize("session", "tool", "甲乙丙丁戊")
        .await;

    assert_eq!(output.original_chars(), 5);
    assert_eq!(output.original_bytes(), 15);
    assert_eq!(output.omitted_chars(), 2);
    assert_eq!(output.blob_locator(), None);
    assert_eq!(output.degradation_reason(), Some("write_failed"));
}

#[tokio::test]
async fn provider_text_and_session_content_share_one_bounded_projection() {
    let blobs = Arc::new(FakeBlobPort::default());
    let materializer =
        ToolResultMaterializer::new(blobs, ToolResultMaterializationPolicy::new(4, 2, 1));
    let original = "甲乙丙丁戊";

    let message = materializer
        .materialize_provider_results(
            "session",
            vec![(
                "tool".to_string(),
                original.to_string(),
                serde_json::json!({"text": original, "nested": original}),
                false,
                Vec::new(),
            )],
        )
        .await;

    let [share::message::ContentBlock::ToolResult { content, text, .. }] =
        message.content.as_slice()
    else {
        panic!("expected one tool result");
    };
    let text = text.as_deref().expect("provider projection must exist");
    let encoded = content.to_string();
    assert!(!text.contains(original));
    assert!(!encoded.contains(original));
    assert_eq!(
        content.get("text").and_then(serde_json::Value::as_str),
        Some(text)
    );
    assert_eq!(
        content
            .get("original_chars")
            .and_then(serde_json::Value::as_u64),
        Some(5)
    );
    assert_eq!(
        content
            .get("original_bytes")
            .and_then(serde_json::Value::as_u64),
        Some(15)
    );
}

#[tokio::test]
async fn display_and_session_consumers_share_the_same_projection() {
    let display_blobs = Arc::new(FakeBlobPort::default());
    let session_blobs = Arc::new(FakeBlobPort::default());
    let policy = ToolResultMaterializationPolicy::new(4, 2, 1);
    let display = ToolResultMaterializer::new(display_blobs, policy);
    let session = ToolResultMaterializer::new(session_blobs, policy);
    let original = "甲乙丙丁戊";
    let original_content = serde_json::json!({"text": original});

    let (display_output, display_content) = display
        .materialize_display_result("session", "tool", original, &original_content)
        .await;
    let message = session
        .materialize_provider_results(
            "session",
            vec![(
                "tool".to_string(),
                original.to_string(),
                original_content,
                false,
                Vec::new(),
            )],
        )
        .await;
    let [share::message::ContentBlock::ToolResult { content, text, .. }] =
        message.content.as_slice()
    else {
        panic!("expected one tool result");
    };

    assert_eq!(text.as_deref(), Some(display_output.as_str()));
    assert_eq!(content, &display_content);
    assert!(!display_output.contains(original));
    assert!(!display_content.to_string().contains(original));
}

#[tokio::test]
async fn main_and_sub_paths_share_the_same_tool_result_projection() {
    let main_blobs = Arc::new(FakeBlobPort::default());
    let sub_blobs = Arc::new(FakeBlobPort::default());
    let policy = ToolResultMaterializationPolicy::new(4, 2, 1);
    let main = ToolResultMaterializer::new(main_blobs, policy);
    let sub = ToolResultMaterializer::new(sub_blobs, policy);
    let result = (
        "tool".to_string(),
        "甲乙丙丁戊".to_string(),
        serde_json::json!({"text": "甲乙丙丁戊"}),
        false,
        Vec::new(),
    );

    let main_message = main
        .materialize_provider_results("session", vec![result.clone()])
        .await;
    let sub_message = sub
        .materialize_provider_results("session", vec![result])
        .await;

    assert_eq!(
        serde_json::to_value(&main_message).unwrap(),
        serde_json::to_value(&sub_message).unwrap()
    );
    let encoded = serde_json::to_string(&main_message).unwrap();
    assert!(!encoded.contains("甲乙丙丁戊"));
    assert!(encoded.contains("tool-result://session/tool"));
}
