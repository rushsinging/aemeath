use std::sync::Arc;

#[cfg(test)]
use async_trait::async_trait;
#[cfg(test)]
use std::sync::Mutex;

use crate::ports::ToolResultBlobPort;
#[cfg(test)]
use crate::ports::{ToolResultBlobError, ToolResultBlobRef};
use share::message::Message;
use share::tool::ImageData;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolResultMaterializationPolicy {
    threshold_chars: usize,
    preview_head_chars: usize,
    preview_tail_chars: usize,
}

impl ToolResultMaterializationPolicy {
    pub fn new(
        threshold_chars: usize,
        preview_head_chars: usize,
        preview_tail_chars: usize,
    ) -> Self {
        assert!(
            threshold_chars > 0,
            "tool result threshold must be positive"
        );
        assert!(
            preview_head_chars + preview_tail_chars <= threshold_chars,
            "tool result previews must fit within the threshold"
        );
        Self {
            threshold_chars,
            preview_head_chars,
            preview_tail_chars,
        }
    }
}

pub struct ToolResultMaterialization {
    text: String,
    persisted: bool,
    warning: Option<String>,
}

impl ToolResultMaterialization {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn persisted(&self) -> bool {
        self.persisted
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }
}

pub struct ToolResultMaterializer {
    blobs: Arc<dyn ToolResultBlobPort>,
    policy: ToolResultMaterializationPolicy,
}

impl ToolResultMaterializer {
    pub fn new(
        blobs: Arc<dyn ToolResultBlobPort>,
        policy: ToolResultMaterializationPolicy,
    ) -> Self {
        Self { blobs, policy }
    }

    pub async fn materialize_provider_results(
        &self,
        session_id: &str,
        results: Vec<(String, String, serde_json::Value, bool, Vec<ImageData>)>,
    ) -> Message {
        let mut materialized = Vec::with_capacity(results.len());
        for (tool_use_id, output, mut content, is_error, images) in results {
            let result = self.materialize(session_id, &tool_use_id, &output).await;
            if let Some(warning) = result.warning() {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "tool result blob persistence failed: {warning}"
                );
            }
            let text = result.text().to_string();
            if result.persisted() {
                content = serde_json::json!({ "text": text, "persisted": true });
            }
            materialized.push((tool_use_id, text, content, is_error, images));
        }
        Message::tool_results_rich(materialized)
    }

    pub async fn materialize(
        &self,
        session_id: &str,
        tool_use_id: &str,
        output: &str,
    ) -> ToolResultMaterialization {
        let character_count = output.chars().count();
        if character_count <= self.policy.threshold_chars {
            return ToolResultMaterialization {
                text: output.to_string(),
                persisted: false,
                warning: None,
            };
        }

        let blob = match self
            .blobs
            .write_once(session_id, tool_use_id, output.as_bytes())
            .await
        {
            Ok(blob) => blob,
            Err(error) => {
                return ToolResultMaterialization {
                    text: output.to_string(),
                    persisted: false,
                    warning: Some(error.to_string()),
                };
            }
        };
        let head: String = output
            .chars()
            .take(self.policy.preview_head_chars)
            .collect();
        let tail: String = output
            .chars()
            .skip(character_count - self.policy.preview_tail_chars)
            .collect();
        let omitted = character_count - head.chars().count() - tail.chars().count();
        let text = format!(
            "<persisted-output>\nOutput too large. Full output saved to: {}\n\n--- head ({} chars) ---\n{}\n\n[... {} chars omitted ...]\n\n--- tail ({} chars) ---\n{}\n</persisted-output>",
            blob.locator(),
            head.chars().count(),
            head,
            omitted,
            tail.chars().count(),
            tail,
        );
        ToolResultMaterialization {
            text,
            persisted: true,
            warning: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let materializer = ToolResultMaterializer::new(
            blobs.clone(),
            ToolResultMaterializationPolicy::new(4, 2, 1),
        );

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
        let materializer = ToolResultMaterializer::new(
            blobs.clone(),
            ToolResultMaterializationPolicy::new(4, 2, 1),
        );

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
    async fn blob_failure_keeps_complete_output_inline() {
        let blobs = Arc::new(FakeBlobPort::default());
        *blobs.failure.lock().unwrap() = Some(ToolResultBlobError::write("磁盘不可写"));
        let materializer =
            ToolResultMaterializer::new(blobs, ToolResultMaterializationPolicy::new(4, 2, 1));

        let output = materializer
            .materialize("session", "tool", "甲乙丙丁戊")
            .await;

        assert_eq!(output.text(), "甲乙丙丁戊");
        assert!(!output.persisted());
        assert!(output.warning().is_some());
    }
}
