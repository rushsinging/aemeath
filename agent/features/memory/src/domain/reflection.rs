use super::{MemoryCategory, MemoryEntry, MemoryError, MemoryLayer};
use crate::{ReflectionApplyResult, ReflectionPromptPort};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

fn null_as_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<Vec<T>>::deserialize(deserializer).map(Option::unwrap_or_default)
}

fn default_memory_layer() -> MemoryLayer {
    MemoryLayer::Project
}

/// A candidate memory produced by Reflection, before it becomes a `MemoryEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySuggestion {
    #[serde(default = "default_memory_layer")]
    pub layer: MemoryLayer,
    pub category: MemoryCategory,
    pub content: String,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub reason: String,
}

/// The complete published-language response expected from a Reflection model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionOutput {
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub deviations: Vec<String>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub suggested_memories: Vec<MemorySuggestion>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub outdated_memories: Vec<String>,
    #[serde(default)]
    pub user_alert: Option<String>,
}

// Value-namespace compatibility for callers of the former unit placeholder.
#[allow(non_upper_case_globals)]
pub const ReflectionOutput: ReflectionOutput = ReflectionOutput {
    deviations: Vec::new(),
    suggested_memories: Vec::new(),
    outdated_memories: Vec::new(),
    user_alert: None,
};

#[derive(Debug, Error)]
pub enum ReflectionError {
    #[error("failed to parse reflection JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("reflection response could not be parsed as JSON (first 200 chars): {0}")]
    Unparseable(String),
    #[error("invalid reflection memory suggestion: {0}")]
    InvalidSuggestion(String),
    #[error(transparent)]
    Memory(#[from] MemoryError),
}

pub type ReflectionResult<T> = Result<T, ReflectionError>;

/// A provider-independent message projection used by the pure Reflection service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionMessage {
    pub role: String,
    pub text: String,
}

impl ReflectionMessage {
    pub fn new(role: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionTokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionTrigger {
    Interval,
    PreCompact,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionErrorCategory {
    LlmCall,
    EmptyResponse,
    Parse,
    InvalidSuggestion,
    Apply,
    History,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionApplyStatus {
    NotApplied,
    Applied,
    PartiallyApplied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionSafeSummary {
    pub id: String,
    pub timestamp: u64,
    pub trigger: ReflectionTrigger,
    pub status: ReflectionStatus,
    pub deviations: usize,
    pub suggestions: usize,
    pub outdated: usize,
    pub apply_status: ReflectionApplyStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_category: Option<ReflectionErrorCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<ReflectionTokenUsage>,
    pub duration_ms: u64,
}

/// One completed Reflection result. Persistence is supplied by a separate adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionRecord {
    pub id: String,
    pub timestamp: u64,
    pub trigger: ReflectionTrigger,
    pub status: ReflectionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<ReflectionOutput>,
    pub apply_result: Option<ReflectionApplyResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_category: Option<ReflectionErrorCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<ReflectionTokenUsage>,
    pub duration_ms: u64,
}

impl ReflectionRecord {
    pub fn running(id: impl Into<String>, timestamp: u64, trigger: ReflectionTrigger) -> Self {
        Self {
            id: id.into(),
            timestamp,
            trigger,
            status: ReflectionStatus::Running,
            output: None,
            apply_result: None,
            error_category: None,
            token_usage: None,
            duration_ms: 0,
        }
    }

    pub fn failed(
        id: impl Into<String>,
        timestamp: u64,
        trigger: ReflectionTrigger,
        error_category: ReflectionErrorCategory,
        duration_ms: u64,
    ) -> Self {
        Self {
            id: id.into(),
            timestamp,
            trigger,
            status: ReflectionStatus::Failed,
            output: None,
            apply_result: None,
            error_category: Some(error_category),
            token_usage: None,
            duration_ms,
        }
    }

    pub fn safe_summary(&self) -> ReflectionSafeSummary {
        let (deviations, suggestions, outdated) = self
            .output
            .as_ref()
            .map(|output| {
                (
                    output.deviations.len(),
                    output.suggested_memories.len(),
                    output.outdated_memories.len(),
                )
            })
            .unwrap_or_default();
        let apply_status = match &self.apply_result {
            None => ReflectionApplyStatus::NotApplied,
            Some(result) if result.completed < result.attempted => {
                ReflectionApplyStatus::PartiallyApplied
            }
            Some(_) => ReflectionApplyStatus::Applied,
        };
        ReflectionSafeSummary {
            id: self.id.clone(),
            timestamp: self.timestamp,
            trigger: self.trigger,
            status: self.status,
            deviations,
            suggestions,
            outdated,
            apply_status,
            error_category: self.error_category,
            token_usage: self.token_usage,
            duration_ms: self.duration_ms,
        }
    }
}

/// Stateless implementation of the Memory Reflection domain service.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReflectionEngine;

impl ReflectionEngine {
    fn preview(text: &str) -> String {
        text.chars().take(200).collect()
    }

    fn extract_json_object(text: &str) -> Option<&str> {
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

    fn prompt_template(lang: &str) -> &'static str {
        if lang == "zh" {
            r#"你是 Aemeath 的 Reflection 引擎。请根据当前项目记忆和最近对话摘要，检查行为偏差、提出应写入的长期记忆、识别过时记忆。

要求：
- 只输出 JSON，不要输出 Markdown。
- suggested_memories[].layer 只能是 project 或 global，默认优先使用 project。
- suggested_memories[].category 只能是 fact、decision、preference、pattern、pitfall。
- outdated_memories 使用已有 memory id。
- 没有内容时输出空数组。

JSON 格式：
{{
    "deviations": ["偏差描述"],
    "suggested_memories": [{{"layer":"project","category":"decision","content":"记忆内容","tags":["可选标签"],"reason":"为什么建议添加"}}],
    "outdated_memories": ["memory-id"],
    "user_alert": "可选用户提示"
}}

# 当前项目记忆
{project_memory}

# 最近对话摘要
{recent_summary}"#
        } else {
            r#"You are the Aemeath Reflection engine. Based on the current project memory and recent conversation summary, detect behavioral deviations, suggest long-term memories to write, and identify outdated memories.

Requirements:
- Output JSON only, no Markdown.
- suggested_memories[].layer must be project or global; prefer project by default.
- suggested_memories[].category must be fact, decision, preference, pattern, or pitfall.
- outdated_memories uses existing memory ids.
- Output empty arrays when there is nothing.

JSON format:
{{
    "deviations": ["deviation description"],
    "suggested_memories": [{{"layer":"project","category":"decision","content":"memory content","tags":["optional tag"],"reason":"why this is suggested"}}],
    "outdated_memories": ["memory-id"],
    "user_alert": "optional user alert"
}}

# Current project memory
{project_memory}

# Recent conversation summary
{recent_summary}"#
        }
    }

    fn labels(
        lang: &str,
    ) -> (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    ) {
        if lang == "zh" {
            (
                "Reflection",
                "偏差：暂无明显偏差",
                "偏差：\n- ",
                "记忆建议：暂无建议",
                "记忆建议：\n",
                "过期记忆：",
            )
        } else {
            (
                "Reflection",
                "Deviations: no significant deviations",
                "Deviations:\n- ",
                "Memory suggestions: none",
                "Memory suggestions:\n",
                "Outdated memories: ",
            )
        }
    }
}

impl ReflectionPromptPort for ReflectionEngine {
    fn build_prompt(&self, project_memory: &str, recent_summary: &str, lang: &str) -> String {
        Self::prompt_template(lang)
            .replace("{project_memory}", project_memory)
            .replace("{recent_summary}", recent_summary)
    }

    fn parse_output(&self, raw: &str) -> ReflectionResult<ReflectionOutput> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ReflectionError::Unparseable(String::new()));
        }

        let fenced = trimmed
            .split_once("```json")
            .and_then(|(_, rest)| rest.split_once("```").map(|(json, _)| json.trim()));
        let source = if let Some(json) = fenced {
            json
        } else if let Some(json) = Self::extract_json_object(trimmed) {
            json
        } else if trimmed.starts_with('{') {
            // Preserve serde's structural error for JSON-looking, incomplete input.
            trimmed
        } else {
            return Err(ReflectionError::Unparseable(Self::preview(trimmed)));
        };

        let output: ReflectionOutput = serde_json::from_str(source)?;
        for (index, suggestion) in output.suggested_memories.iter().enumerate() {
            if suggestion.content.trim().is_empty() {
                return Err(ReflectionError::InvalidSuggestion(format!(
                    "suggested_memories[{index}].content must not be empty"
                )));
            }
        }
        Ok(output)
    }

    fn format_output(&self, output: &ReflectionOutput, lang: &str) -> String {
        let (
            title,
            deviations_empty,
            deviations_header,
            suggestions_empty,
            suggestions_header,
            outdated_header,
        ) = Self::labels(lang);
        let mut sections = vec![title.to_string()];
        if output.deviations.is_empty() {
            sections.push(deviations_empty.to_string());
        } else {
            sections.push(format!(
                "{deviations_header}{}",
                output.deviations.join("\n- ")
            ));
        }
        if output.suggested_memories.is_empty() {
            sections.push(suggestions_empty.to_string());
        } else {
            let suggestions = output
                .suggested_memories
                .iter()
                .map(|suggestion| format!("- [{:?}] {}", suggestion.category, suggestion.content))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("{suggestions_header}{suggestions}"));
        }
        if !output.outdated_memories.is_empty() {
            sections.push(format!(
                "{outdated_header}{}",
                output.outdated_memories.join(", ")
            ));
        }
        if let Some(alert) = &output.user_alert {
            let header = if lang == "zh" {
                "用户提醒："
            } else {
                "User alert: "
            };
            sections.push(format!("{header}{alert}"));
        }
        sections.join("\n\n")
    }

    fn format_memory_summary(&self, entries: &[MemoryEntry]) -> String {
        entries
            .iter()
            .map(|entry| {
                format!(
                    "- [{:?}][{}] {}",
                    entry.category,
                    entry.tags.join(","),
                    entry.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn recent_messages_summary(&self, messages: &[ReflectionMessage], max_chars: usize) -> String {
        if max_chars == 0 {
            return String::new();
        }

        let mut recent = Vec::new();
        for message in messages.iter().rev() {
            if message.text.trim().is_empty() {
                continue;
            }
            let role = if message.role.eq_ignore_ascii_case("user") {
                "User"
            } else if message.role.eq_ignore_ascii_case("assistant") {
                "Assistant"
            } else {
                message.role.as_str()
            };
            recent.push(format!("[{role}]: {}", message.text));
            let summary = recent.iter().rev().cloned().collect::<Vec<_>>().join("\n");
            if summary.chars().count() >= max_chars {
                return summary.chars().take(max_chars).collect();
            }
        }
        recent.into_iter().rev().collect::<Vec<_>>().join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryId, MemorySource};

    fn engine() -> ReflectionEngine {
        ReflectionEngine
    }

    #[test]
    fn reflection_record_summary_is_safe_and_deterministic() {
        let record = ReflectionRecord {
            id: "reflection-1".into(),
            timestamp: 42,
            trigger: ReflectionTrigger::PreCompact,
            status: ReflectionStatus::Succeeded,
            output: Some(ReflectionOutput {
                deviations: vec!["secret deviation".into()],
                suggested_memories: vec![MemorySuggestion {
                    layer: MemoryLayer::Project,
                    category: MemoryCategory::Decision,
                    content: "secret memory".into(),
                    tags: vec![],
                    reason: "secret reason".into(),
                }],
                outdated_memories: vec!["secret-id".into()],
                user_alert: None,
            }),
            apply_result: None,
            error_category: None,
            token_usage: Some(ReflectionTokenUsage {
                input_tokens: 10,
                output_tokens: 5,
            }),
            duration_ms: 12,
        };

        assert_eq!(
            record.safe_summary(),
            ReflectionSafeSummary {
                id: "reflection-1".into(),
                timestamp: 42,
                trigger: ReflectionTrigger::PreCompact,
                status: ReflectionStatus::Succeeded,
                deviations: 1,
                suggestions: 1,
                outdated: 1,
                apply_status: ReflectionApplyStatus::NotApplied,
                error_category: None,
                token_usage: Some(ReflectionTokenUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                }),
                duration_ms: 12,
            }
        );
        let json = serde_json::to_string(&record.safe_summary()).unwrap();
        assert!(!json.contains("secret"));
    }

    #[test]
    fn failed_reflection_record_has_typed_error_and_no_output() {
        let record = ReflectionRecord::failed(
            "reflection-2",
            43,
            ReflectionTrigger::Interval,
            ReflectionErrorCategory::LlmCall,
            9,
        );
        assert_eq!(record.status, ReflectionStatus::Failed);
        assert!(record.output.is_none());
        assert_eq!(
            record.safe_summary().error_category,
            Some(ReflectionErrorCategory::LlmCall)
        );
    }

    #[test]
    fn null_collections_deserialize_as_empty() {
        let output = engine()
            .parse_output(r#"{"deviations":null,"suggested_memories":null,"outdated_memories":null,"user_alert":null}"#)
            .unwrap();
        assert_eq!(output, ReflectionOutput::default());

        let output = engine()
            .parse_output(
                r#"{"suggested_memories":[{"category":"fact","content":"x","tags":null}]}"#,
            )
            .unwrap();
        assert!(output.suggested_memories[0].tags.is_empty());
    }

    #[test]
    fn extracts_fenced_and_prose_json() {
        let fenced = engine()
            .parse_output("answer:\n```json\n{\"deviations\":[\"fenced\"]}\n```")
            .unwrap();
        let prose = engine()
            .parse_output("answer: {\"deviations\":[\"prose\"]} done")
            .unwrap();
        assert_eq!(fenced.deviations, ["fenced"]);
        assert_eq!(prose.deviations, ["prose"]);
    }

    #[test]
    fn distinguishes_empty_unparseable_and_malformed_json() {
        assert!(matches!(
            engine().parse_output("  "),
            Err(ReflectionError::Unparseable(_))
        ));
        assert!(matches!(
            engine().parse_output("no json here"),
            Err(ReflectionError::Unparseable(_))
        ));
        assert!(matches!(
            engine().parse_output("{\"deviations\": [}"),
            Err(ReflectionError::Parse(_))
        ));
    }

    #[test]
    fn rejects_empty_suggestion_content() {
        let result = engine()
            .parse_output(r#"{"suggested_memories":[{"category":"decision","content":"  "}]}"#);
        assert!(matches!(result, Err(ReflectionError::InvalidSuggestion(_))));
    }

    #[test]
    fn prompt_and_format_are_bilingual() {
        let zh = engine().build_prompt("MEM", "SUMMARY", "zh");
        let en = engine().build_prompt("MEM", "SUMMARY", "en");
        assert!(zh.contains("只输出 JSON") && zh.contains("# 最近对话摘要"));
        assert!(en.contains("Output JSON only") && en.contains("# Recent conversation summary"));
        assert!(zh.contains("MEM") && en.contains("SUMMARY"));

        let empty = ReflectionOutput::default();
        assert!(engine()
            .format_output(&empty, "zh")
            .contains("暂无明显偏差"));
        assert!(engine()
            .format_output(&empty, "en")
            .contains("no significant deviations"));
    }

    #[test]
    fn formats_memory_summary() {
        let mut entry = MemoryEntry::new(
            MemoryId::now_v7(),
            1,
            MemoryLayer::Project,
            MemoryCategory::Decision,
            "keep Reflection in Memory",
            MemorySource::Llm,
        )
        .unwrap();
        entry.tags = vec!["ddd".into(), "reflection".into()];
        assert_eq!(
            engine().format_memory_summary(&[entry]),
            "- [Decision][ddd,reflection] keep Reflection in Memory"
        );
    }

    #[test]
    fn message_summary_keeps_recent_messages_and_truncates_by_char() {
        let messages = vec![
            ReflectionMessage::new("user", "old"),
            ReflectionMessage::new("assistant", "最新回复"),
        ];
        let full = engine().recent_messages_summary(&messages, usize::MAX);
        assert_eq!(full, "[User]: old\n[Assistant]: 最新回复");

        let truncated = engine().recent_messages_summary(&messages, 8);
        assert_eq!(truncated.chars().count(), 8);
        assert!(truncated.starts_with("[Assistant]".chars().take(8).collect::<String>().as_str()));
        assert_eq!(engine().recent_messages_summary(&messages, 0), "");
    }
}
