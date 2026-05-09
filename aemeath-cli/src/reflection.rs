//! Shared reflection utilities used by both TUI and REPL paths.

use aemeath_core::memory::MemoryStore;
use aemeath_core::reflection::ReflectionEngine;
use std::path::PathBuf;

/// Build the reflection context (memory + recent messages), call LLM, parse result.
///
/// Returns `Some(formatted_text)` if reflection was triggered and produced output,
/// or `None` if reflection is disabled, not due yet, or failed silently.
pub async fn run_reflection(
    config: &aemeath_core::config::MemoryConfig,
    turn_count: usize,
    messages: &[aemeath_core::message::Message],
    cwd: &PathBuf,
    client: &aemeath_llm::client::LlmClient,
    system_prompt_text: &str,
) -> Option<String> {
    if !config.enabled || !config.reflection.enabled || config.reflection.interval_turns == 0 {
        return None;
    }
    if turn_count % config.reflection.interval_turns != 0 {
        return None;
    }

    let store = MemoryStore::new(
        aemeath_core::memory::memory_base_dir(),
        aemeath_core::memory::project_hash_from_path(cwd),
        config.max_entries,
        config.similarity_threshold,
    )
    .ok()?;

    let entries = store
        .list(Some(aemeath_core::memory::MemoryLayer::Project))
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
            return lightweight_reflection_text(config, turn_count, messages, cwd).await;
        }
    };

    Some(ReflectionEngine::format_output(&output))
}

/// Call LLM with a simple prompt and return the full text response.
async fn call_llm_for_reflection(
    client: &aemeath_llm::client::LlmClient,
    prompt: &str,
    system_prompt_text: &str,
) -> Option<String> {
    use aemeath_llm::provider::StreamHandler;
    use aemeath_llm::types::SystemBlock;

    let system_blocks = vec![SystemBlock::dynamic(system_prompt_text.to_string())];
    let messages = vec![aemeath_core::message::Message::user(prompt)];

    struct CollectHandler {
        text: String,
    }
    impl StreamHandler for CollectHandler {
        fn on_text(&mut self, text: &str) {
            self.text.push_str(text);
        }
        fn on_tool_use_start(&mut self, _name: &str) {}
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
async fn lightweight_reflection_text(
    config: &aemeath_core::config::MemoryConfig,
    turn_count: usize,
    messages: &[aemeath_core::message::Message],
    cwd: &PathBuf,
) -> Option<String> {
    if !config.enabled || !config.reflection.enabled || config.reflection.interval_turns == 0 {
        return None;
    }
    if turn_count % config.reflection.interval_turns != 0 {
        return None;
    }

    let store = MemoryStore::new(
        aemeath_core::memory::memory_base_dir(),
        aemeath_core::memory::project_hash_from_path(cwd),
        config.max_entries,
        config.similarity_threshold,
    )
    .ok()?;
    let entries = store
        .list(Some(aemeath_core::memory::MemoryLayer::Project))
        .ok()?;
    let mut output = aemeath_core::reflection::ReflectionOutput {
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
