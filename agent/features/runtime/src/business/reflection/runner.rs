use super::types::{ReflectionError, ReflectionResult};
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
) -> ReflectionResult<Option<CompleteReflectionResult>> {
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
) -> ReflectionResult<Option<CompleteReflectionResult>> {
    if !should_run_reflection(mode, config) {
        return Ok(None);
    }

    let mut store = MemoryStore::new(
        base_dir.clone(),
        storage::api::project_file_name_from_path(cwd),
        config.max_entries,
        config.similarity_threshold,
    )
    .map_err(|e| ReflectionError::StoreInit(e.to_string()))?;

    let entries = store
        .list(Some(share::memory::MemoryLayer::Project))
        .ok()
        .unwrap_or_default();

    let project_memory = ReflectionEngine::memory_summary(&entries);
    let recent_summary = ReflectionEngine::recent_messages_summary(messages, usize::MAX);
    let prompt = ReflectionEngine::build_prompt(&project_memory, &recent_summary);

    let (full_response, input_tokens, output_tokens) =
        call_llm_for_reflection(client, &prompt, system_prompt_text).await?;

    let output = ReflectionEngine::parse_output(&full_response).map_err(|e| {
        ReflectionError::Unparseable(format!("{e}: {}", truncate_200(&full_response)))
    })?;

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
                log::warn!(target: "runtime::reflection", "Reflection auto apply failed: {error}");
            }
        }
    }

    Ok(Some(CompleteReflectionResult {
        output,
        formatted_content,
        input_tokens,
        output_tokens,
        auto_applied,
    }))
}

fn truncate_200(s: &str) -> String {
    s.chars().take(200).collect()
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
) -> ReflectionResult<(String, u32, u32)> {
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
                Err(ReflectionError::EmptyResponse)
            } else {
                Ok((text, resp.usage.input_tokens, resp.usage.output_tokens))
            }
        }
        Err(e) => Err(ReflectionError::LlmCall(e.to_string())),
    }
}
