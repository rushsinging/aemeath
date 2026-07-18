pub mod apply;
pub mod format;
pub mod prompt;
pub mod runner;
pub mod store;
pub mod types;

pub use runner::{run_complete_reflection, CompleteReflectionResult, ReflectionRunMode};
pub use types::{
    MemorySuggestion, ReflectionApplyResult, ReflectionEngine, ReflectionError, ReflectionOutput,
    ReflectionResult,
};

impl ReflectionEngine {
    pub fn parse_output(json: &str) -> ReflectionResult<ReflectionOutput> {
        let source = Self::extract_json_object(json).unwrap_or(json);
        serde_json::from_str(source).map_err(|e| {
            let preview: String = source.chars().take(200).collect();
            log::warn!(
                target: "aemeath:agent:runtime",
                "reflection JSON parse failed: {e}, first 200 chars: {preview}"
            );
            ReflectionError::Parse(e)
        })
    }

    pub fn extract_json_object(text: &str) -> Option<&str> {
        let fenced = text
            .split("```json")
            .nth(1)
            .and_then(|rest| rest.split("```").next())
            .map(str::trim);
        if fenced.is_some() {
            return fenced;
        }

        let start = text.find('{')?;
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        for (offset, ch) in text[start..].char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' if in_string => escaped = true,
                '"' => in_string = !in_string,
                '{' if !in_string => depth += 1,
                '}' if !in_string => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(&text[start..start + offset + ch.len_utf8()]);
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub async fn apply_suggestions(
        suggestions: &[MemorySuggestion],
        port: &dyn memory::MemoryPort,
    ) -> ReflectionResult<usize> {
        apply::apply_suggestions_via_port(suggestions, port).await
    }

    pub async fn apply_output(
        output: &ReflectionOutput,
        port: &dyn memory::MemoryPort,
    ) -> ReflectionResult<ReflectionApplyResult> {
        apply::apply_output_via_port(output, port).await
    }

    pub async fn apply_outdated(
        ids: &[String],
        port: &dyn memory::MemoryPort,
    ) -> ReflectionResult<usize> {
        apply::apply_outdated_via_port(ids, port).await
    }

    pub fn format_output(output: &ReflectionOutput, lang: &str) -> String {
        format::format_output(output, lang)
    }

    pub fn memory_summary(entries: &[memory::MemoryEntry]) -> String {
        store::memory_summary(entries)
    }

    pub fn build_prompt(project_memory: &str, recent_summary: &str, lang: &str) -> String {
        format::build_prompt(project_memory, recent_summary, lang)
    }

    pub fn recent_messages_summary(
        messages: &[share::message::Message],
        max_chars: usize,
    ) -> String {
        format::recent_messages_summary(messages, max_chars)
    }
}

#[cfg(test)]
mod tests;
