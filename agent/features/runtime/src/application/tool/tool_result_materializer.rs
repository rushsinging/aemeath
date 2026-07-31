use std::sync::Arc;

use crate::ports::ToolResultBlobPort;
use share::message::Message;
use tools::ImageData;

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
    original_chars: usize,
    original_bytes: usize,
    omitted_chars: usize,
    blob_locator: Option<String>,
    degradation_reason: Option<&'static str>,
    warning: Option<String>,
}

impl ToolResultMaterialization {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn persisted(&self) -> bool {
        self.persisted
    }

    pub fn original_chars(&self) -> usize {
        self.original_chars
    }

    pub fn original_bytes(&self) -> usize {
        self.original_bytes
    }

    pub fn omitted_chars(&self) -> usize {
        self.omitted_chars
    }

    pub fn blob_locator(&self) -> Option<&str> {
        self.blob_locator.as_deref()
    }

    pub fn degradation_reason(&self) -> Option<&str> {
        self.degradation_reason
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    fn content_metadata(&self) -> serde_json::Value {
        let blob = match (&self.blob_locator, self.degradation_reason) {
            (Some(locator), _) => serde_json::json!({
                "status": "persisted",
                "locator": locator,
            }),
            (None, Some(reason)) => serde_json::json!({
                "status": "unavailable",
                "reason": reason,
            }),
            (None, None) => serde_json::Value::Null,
        };
        serde_json::json!({
            "text": self.text,
            "truncated": self.omitted_chars > 0,
            "original_chars": self.original_chars,
            "original_bytes": self.original_bytes,
            "omitted_chars": self.omitted_chars,
            "blob": blob,
        })
    }
}

#[derive(Clone)]
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

    pub(crate) async fn materialize_display_result(
        &self,
        session_id: &str,
        tool_use_id: &str,
        output: &str,
        content: &serde_json::Value,
    ) -> (String, serde_json::Value) {
        let result = self.materialize(session_id, tool_use_id, output).await;
        if let Some(warning) = result.warning() {
            log::warn!(
                target: crate::LOG_TARGET,
                "tool result blob persistence failed: {warning}"
            );
        }
        let output = result.text().to_string();
        let content = if result.omitted_chars() > 0 {
            result.content_metadata()
        } else {
            content.clone()
        };
        (output, content)
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
            if result.omitted_chars() > 0 {
                content = result.content_metadata();
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
                original_chars: character_count,
                original_bytes: output.len(),
                omitted_chars: 0,
                blob_locator: None,
                degradation_reason: None,
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
                let text = bounded_tool_result_text(output, character_count, self.policy, None);
                return ToolResultMaterialization {
                    text,
                    persisted: false,
                    original_chars: character_count,
                    original_bytes: output.len(),
                    omitted_chars: omitted_chars(character_count, self.policy),
                    blob_locator: None,
                    degradation_reason: Some("write_failed"),
                    warning: Some(error.to_string()),
                };
            }
        };
        let text =
            bounded_tool_result_text(output, character_count, self.policy, Some(blob.locator()));
        ToolResultMaterialization {
            text,
            persisted: true,
            original_chars: character_count,
            original_bytes: output.len(),
            omitted_chars: omitted_chars(character_count, self.policy),
            blob_locator: Some(blob.locator().to_string()),
            degradation_reason: None,
            warning: None,
        }
    }
}

fn omitted_chars(character_count: usize, policy: ToolResultMaterializationPolicy) -> usize {
    character_count - policy.preview_head_chars - policy.preview_tail_chars
}

fn bounded_tool_result_text(
    output: &str,
    character_count: usize,
    policy: ToolResultMaterializationPolicy,
    locator: Option<&str>,
) -> String {
    let head: String = output.chars().take(policy.preview_head_chars).collect();
    let tail: String = output
        .chars()
        .skip(character_count - policy.preview_tail_chars)
        .collect();
    let omitted = omitted_chars(character_count, policy);
    let location = locator
        .map(|value| format!("Full output saved to: {value}"))
        .unwrap_or_else(|| "Full output unavailable because persistence failed.".to_string());
    format!(
        "<persisted-output>\nOutput too large. {location}\n\n--- head ({} chars) ---\n{}\n\n[... {} chars omitted ...]\n\n--- tail ({} chars) ---\n{}\n</persisted-output>",
        head.chars().count(),
        head,
        omitted,
        tail.chars().count(),
        tail,
    )
}

#[cfg(test)]
#[path = "tool_result_materializer_tests.rs"]
mod tests;
