use super::{ReflectionEngine, ReflectionOutput};
use std::path::{Path, PathBuf};
use storage::api::MemoryStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflectionRunMode {
    Interval { turn_count: usize },
    Forced,
}

#[derive(Debug, Clone)]
pub struct CompleteReflectionResult {
    pub output: ReflectionOutput,
    pub formatted_content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub auto_applied: bool,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_complete_reflection(
    mode: ReflectionRunMode,
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    cwd: &Path,
    client: &provider::api::LlmClient,
    system_prompt_text: &str,
) -> Option<CompleteReflectionResult> {
    run_complete_reflection_with_base_dir(
        mode,
        config,
        messages,
        cwd,
        client,
        system_prompt_text,
        storage::api::memory_base_dir(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_complete_reflection_with_base_dir(
    mode: ReflectionRunMode,
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    cwd: &Path,
    client: &provider::api::LlmClient,
    system_prompt_text: &str,
    base_dir: PathBuf,
) -> Option<CompleteReflectionResult> {
    if !should_run_reflection(mode, config) {
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

    let (full_response, input_tokens, output_tokens) =
        call_llm_for_reflection(client, &prompt, system_prompt_text).await?;

    let output = ReflectionEngine::parse_output(&full_response).ok()?;

    let mut formatted_content = ReflectionEngine::format_output(&output);
    let mut auto_applied = false;
    if config.reflection.auto_apply_suggestions {
        match ReflectionEngine::apply_output(&output, &mut store) {
            Ok(result) => {
                formatted_content.push_str(&format!(
                    "\n已自动应用 Reflection：新增/合并 {} 条记忆，标记 {} 条过时记忆。",
                    result.suggestions_added, result.outdated_marked
                ));
                auto_applied = true;
            }
            Err(error) => {
                log::warn!("Reflection auto apply failed: {error}");
            }
        }
    }

    Some(CompleteReflectionResult {
        output,
        formatted_content,
        input_tokens,
        output_tokens,
        auto_applied,
    })
}

fn should_run_reflection(mode: ReflectionRunMode, config: &share::config::MemoryConfig) -> bool {
    if !config.enabled || !config.reflection.enabled || config.reflection.interval_turns == 0 {
        return false;
    }
    match mode {
        ReflectionRunMode::Interval { turn_count } => {
            turn_count.is_multiple_of(config.reflection.interval_turns)
        }
        ReflectionRunMode::Forced => true,
    }
}

async fn call_llm_for_reflection(
    client: &provider::api::LlmClient,
    prompt: &str,
    system_prompt_text: &str,
) -> Option<(String, u32, u32)> {
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
        fn on_tool_use_start(&mut self, _name: &str, _provider_id: Option<&str>, _index: usize) {}
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
        Ok(resp) => {
            let text = handler.text.trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some((
                    extract_json(&text).unwrap_or(text),
                    resp.usage.input_tokens,
                    resp.usage.output_tokens,
                ))
            }
        }
        Err(e) => {
            log::debug!("Reflection LLM call failed: {e}");
            None
        }
    }
}

fn extract_json(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim().to_string());
        }
    }
    if text.starts_with("```") && text.ends_with("```") {
        let inner = &text[3..text.len() - 3];
        if inner.trim().starts_with('{') {
            return Some(inner.trim().to_string());
        }
    }
    if text.starts_with('{') {
        return Some(text.to_string());
    }
    None
}
