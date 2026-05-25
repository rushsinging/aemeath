pub mod apply;
pub mod format;
pub mod prompt;
pub mod store;
pub mod types;

pub use types::{
    MemorySuggestion, ReflectionApplyResult, ReflectionEngine, ReflectionError, ReflectionOutput,
    ReflectionResult,
};

impl ReflectionEngine {
    pub fn parse_output(json: &str) -> ReflectionResult<ReflectionOutput> {
        let source = Self::extract_json_object(json).unwrap_or(json);
        Ok(serde_json::from_str(source)?)
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

    pub fn apply_suggestions(
        suggestions: &[MemorySuggestion],
        store: &mut aemeath_core::memory::MemoryStore,
    ) -> ReflectionResult<usize> {
        apply::apply_suggestions(suggestions, store)
    }

    pub fn apply_output(
        output: &ReflectionOutput,
        store: &mut aemeath_core::memory::MemoryStore,
    ) -> ReflectionResult<ReflectionApplyResult> {
        apply::apply_output(output, store)
    }

    pub fn apply_outdated(
        ids: &[String],
        store: &mut aemeath_core::memory::MemoryStore,
    ) -> ReflectionResult<usize> {
        apply::apply_outdated(ids, store)
    }

    pub fn format_output(output: &ReflectionOutput) -> String {
        format::format_output(output)
    }

    pub fn memory_summary(entries: &[aemeath_core::memory::MemoryEntry]) -> String {
        store::memory_summary(entries)
    }

    pub fn build_prompt(project_memory: &str, recent_summary: &str) -> String {
        format::build_prompt(project_memory, recent_summary)
    }

    pub fn recent_messages_summary(
        messages: &[aemeath_core::message::Message],
        max_chars: usize,
    ) -> String {
        format::recent_messages_summary(messages, max_chars)
    }
}

#[cfg(test)]
mod tests;
