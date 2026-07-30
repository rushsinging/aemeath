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
