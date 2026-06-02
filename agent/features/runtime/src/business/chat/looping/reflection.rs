//! Shared reflection utilities used by both TUI and REPL paths.

use crate::business::reflection::ReflectionEngine;
use std::path::{Path, PathBuf};
use storage::api::MemoryStore;

/// Build the reflection context (memory + recent messages), call LLM, parse result.
///
/// Returns `Some(formatted_text)` if reflection was triggered and produced output,
/// or `None` if reflection is disabled, not due yet, or failed silently.
pub async fn run_reflection(
    config: &share::config::MemoryConfig,
    turn_count: usize,
    messages: &[share::message::Message],
    cwd: &Path,
    client: &provider::api::LlmClient,
    system_prompt_text: &str,
) -> Option<String> {
    run_reflection_with_base_dir(
        config,
        turn_count,
        messages,
        cwd,
        client,
        system_prompt_text,
        storage::api::memory_base_dir(),
    )
    .await
}

async fn run_reflection_with_base_dir(
    config: &share::config::MemoryConfig,
    turn_count: usize,
    messages: &[share::message::Message],
    cwd: &Path,
    client: &provider::api::LlmClient,
    system_prompt_text: &str,
    base_dir: PathBuf,
) -> Option<String> {
    if !config.enabled || !config.reflection.enabled || config.reflection.interval_turns == 0 {
        return None;
    }
    if !turn_count.is_multiple_of(config.reflection.interval_turns) {
        return None;
    }

    let mut store = MemoryStore::new(
        base_dir.clone(),
        storage::api::project_file_name_from_path(cwd),
        config.max_entries,
        config.similarity_threshold,
    )
    .ok()?;

    let entries = store
        .list(Some(share::memory::MemoryLayer::Project))
        .ok()
        .unwrap_or_default();

    let project_memory = ReflectionEngine::memory_summary(&entries);
    let recent_summary = ReflectionEngine::recent_messages_summary(messages, 4000);
    let prompt = ReflectionEngine::build_prompt(&project_memory, &recent_summary);

    // Call LLM with reflection prompt
    let full_response = call_llm_for_reflection(client, &prompt, system_prompt_text).await?;

    // Parse the JSON response
    let output = match ReflectionEngine::parse_output(&full_response) {
        Ok(output) => output,
        Err(_) => {
            // Fall back to lightweight if LLM parsing fails
            return lightweight_reflection_text_with_base_dir(
                config, turn_count, messages, cwd, base_dir,
            )
            .await;
        }
    };

    if config.reflection.auto_apply_suggestions {
        return match ReflectionEngine::apply_output(&output, &mut store) {
            Ok(result) => {
                let mut text = ReflectionEngine::format_output(&output);
                text.push_str(&format!(
                    "\n已自动应用 Reflection：新增/合并 {} 条记忆，标记 {} 条过时记忆。",
                    result.suggestions_added, result.outdated_marked
                ));
                Some(text)
            }
            Err(error) => {
                log::warn!("Reflection auto apply failed: {error}");
                Some(ReflectionEngine::format_output(&output))
            }
        };
    }

    Some(ReflectionEngine::format_output(&output))
}

/// Call LLM with a simple prompt and return the full text response.
async fn call_llm_for_reflection(
    client: &provider::api::LlmClient,
    prompt: &str,
    system_prompt_text: &str,
) -> Option<String> {
    use provider::api::StreamHandler;
    use provider::api::SystemBlock;

    let system_blocks = vec![SystemBlock::dynamic(system_prompt_text.to_string())];
    let messages = vec![share::message::Message::user(prompt)];

    struct CollectHandler {
        text: String,
    }
    impl StreamHandler for CollectHandler {
        fn on_text(&mut self, text: &str) {
            self.text.push_str(text);
        }
        fn on_tool_use_start(&mut self, _name: &str, _index: usize) {}
        fn on_error(&mut self, _error: &str) {}
    }

    let mut handler = CollectHandler {
        text: String::new(),
    };
    let cancel = tokio_util::sync::CancellationToken::new();

    match client
        .stream_message(&system_blocks, &messages, &[], &mut handler, &cancel)
        .await
    {
        Ok(_resp) => {
            let text = handler.text.trim().to_string();
            if text.is_empty() {
                None
            } else {
                // Extract JSON from possible markdown code blocks
                Some(extract_json(&text).unwrap_or(text))
            }
        }
        Err(e) => {
            log::debug!("Reflection LLM call failed: {e}");
            None
        }
    }
}

/// Extract JSON object from a response that may be wrapped in ```json ... ``` blocks.
fn extract_json(text: &str) -> Option<String> {
    let text = text.trim();
    // Try to find JSON between ```json and ```
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim().to_string());
        }
    }
    // Try to find JSON between ``` and ```
    if text.starts_with("```") && text.ends_with("```") {
        let inner = &text[3..text.len() - 3];
        if inner.trim().starts_with('{') {
            return Some(inner.trim().to_string());
        }
    }
    // If the text itself starts with {, return as-is
    if text.starts_with('{') {
        return Some(text.to_string());
    }
    None
}

/// Lightweight reflection fallback: basic checks without LLM call.
async fn lightweight_reflection_text_with_base_dir(
    config: &share::config::MemoryConfig,
    turn_count: usize,
    messages: &[share::message::Message],
    cwd: &Path,
    base_dir: PathBuf,
) -> Option<String> {
    if !config.enabled || !config.reflection.enabled || config.reflection.interval_turns == 0 {
        return None;
    }
    if !turn_count.is_multiple_of(config.reflection.interval_turns) {
        return None;
    }

    let store = MemoryStore::new(
        base_dir,
        storage::api::project_file_name_from_path(cwd),
        config.max_entries,
        config.similarity_threshold,
    )
    .ok()?;
    let entries = store.list(Some(share::memory::MemoryLayer::Project)).ok()?;
    let mut output = crate::business::reflection::ReflectionOutput {
        deviations: Vec::new(),
        suggested_memories: Vec::new(),
        outdated_memories: entries
            .iter()
            .filter(|entry| entry.outdated)
            .map(|entry| entry.id.clone())
            .collect(),
        user_alert: None,
    };
    if entries.is_empty() && !messages.is_empty() {
        output
            .deviations
            .push("当前项目没有长期记忆，建议在关键决策后写入 Memory。".to_string());
    }
    Some(ReflectionEngine::format_output(&output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use provider::api::{LlmProvider, StreamHandler};
    use provider::api::{StopReason, StreamResponse, SystemBlock, Usage};
    use share::memory::{MemoryCategory, MemoryLayer, MemorySource};
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    struct StaticReflectionProvider {
        response: String,
    }

    #[async_trait]
    impl LlmProvider for StaticReflectionProvider {
        async fn stream_message(
            &self,
            _system: &[SystemBlock],
            _messages: &[share::message::Message],
            _tool_schemas: &[serde_json::Value],
            handler: &mut dyn StreamHandler,
            _cancel: &CancellationToken,
        ) -> Result<StreamResponse, provider::LlmError> {
            handler.on_text(&self.response);
            Ok(StreamResponse {
                assistant_message: share::message::Message::placeholder(
                    share::message::Role::Assistant,
                ),
                usage: Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                },
                stop_reason: StopReason::EndTurn,
            })
        }

        fn model_name(&self) -> &str {
            "test-reflection-model"
        }

        fn provider_name(&self) -> &str {
            "test-reflection-provider"
        }

        fn set_reasoning(&self, _enabled: bool) {}

        fn is_reasoning(&self) -> bool {
            false
        }
    }

    fn build_client(response: &str) -> provider::api::LlmClient {
        provider::api::LlmClient::from_provider(Arc::new(StaticReflectionProvider {
            response: response.to_string(),
        }))
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("aemeath-{name}-{}", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn test_run_reflection_auto_apply_suggestions_writes_memory() {
        let cwd = temp_dir("reflection-cwd");
        std::fs::create_dir_all(&cwd).unwrap();
        let base_dir = temp_dir("reflection-memory");
        let response = r#"{
            "suggested_memories": [
                {
                    "category": "decision",
                    "content": "后台 reflection 自动写入 memory",
                    "tags": ["reflection"],
                    "reason": "auto_apply_suggestions=true"
                }
            ]
        }"#;
        let client = build_client(response);
        let mut config = share::config::MemoryConfig::default();
        config.reflection.interval_turns = 2;
        config.reflection.auto_apply_suggestions = true;

        let text = run_reflection_with_base_dir(
            &config,
            2,
            &[share::message::Message::user("请记住这个决策")],
            &cwd,
            &client,
            "system prompt",
            base_dir.clone(),
        )
        .await
        .unwrap();
        let store = MemoryStore::new(
            &base_dir,
            storage::api::project_hash_from_path(&cwd),
            config.max_entries,
            config.similarity_threshold,
        )
        .unwrap();
        let entries = store.list(Some(MemoryLayer::Project)).unwrap();

        assert!(text.contains("后台 reflection 自动写入 memory"));
        assert!(text.contains("已自动应用 Reflection：新增/合并 1 条记忆，标记 0 条过时记忆。"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].category, MemoryCategory::Decision);
        assert_eq!(entries[0].content, "后台 reflection 自动写入 memory");
        assert_eq!(entries[0].source, MemorySource::Llm);
        let _ = std::fs::remove_dir_all(cwd);
        let _ = std::fs::remove_dir_all(base_dir);
    }

    #[tokio::test]
    async fn test_run_reflection_auto_apply_false_does_not_write_memory() {
        let cwd = temp_dir("reflection-cwd");
        std::fs::create_dir_all(&cwd).unwrap();
        let base_dir = temp_dir("reflection-memory");
        let response = r#"{
            "suggested_memories": [
                {
                    "category": "decision",
                    "content": "auto apply false 不写入",
                    "tags": ["reflection"],
                    "reason": "auto_apply_suggestions=false"
                }
            ]
        }"#;
        let client = build_client(response);
        let mut config = share::config::MemoryConfig::default();
        config.reflection.interval_turns = 2;
        config.reflection.auto_apply_suggestions = false;

        let text = run_reflection_with_base_dir(
            &config,
            2,
            &[share::message::Message::user("请只展示建议")],
            &cwd,
            &client,
            "system prompt",
            base_dir.clone(),
        )
        .await
        .unwrap();
        let store = MemoryStore::new(
            &base_dir,
            storage::api::project_hash_from_path(&cwd),
            config.max_entries,
            config.similarity_threshold,
        )
        .unwrap();
        let entries = store.list(Some(MemoryLayer::Project)).unwrap();

        assert!(text.contains("auto apply false 不写入"));
        assert!(!text.contains("已自动应用 Reflection"));
        assert!(entries.is_empty());
        let _ = std::fs::remove_dir_all(cwd);
        let _ = std::fs::remove_dir_all(base_dir);
    }
}
