use super::{MemoryCategory, MemoryEntry, MemoryError, MemoryId, MemoryLayer};
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
    /// 本建议取代哪些已有记忆（apply 时建立 `superseded_by = 留存条目 id`）。
    /// 只由 apply 消费，不直接写库（#1774）。
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub supersedes: Vec<MemoryId>,
    /// 本建议归纳自哪些已有记忆（apply 后成为新条目的 `evidence`，且置
    /// `kind = Synthesized`）。少于两条来源不是归纳而是复制，M13 要求
    /// 降级为普通建议（#1776）。
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub synthesizes: Vec<MemoryId>,
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
}

#[derive(Debug, Error)]
pub enum ReflectionError {
    #[error("reflection response JSON is invalid")]
    Parse,
    #[error("reflection response could not be parsed as JSON")]
    Unparseable,
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

/// 申请结果值对象：Reflection 建议的应用完成度（原摆放于 ports，依赖方向回归 domain）。
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ReflectionApplyResult {
    /// Number of requested operations (suggestions plus outdated-memory marks).
    pub attempted: usize,
    /// Number of operations durably completed. This can be smaller than
    /// `attempted` when a cross-layer apply returns `MemoryError::PartialApply`.
    pub completed: usize,
    pub suggestions_added: usize,
    pub outdated_marked: usize,
    /// Supersede relations durably established (#1774). A relation rejected by
    /// the cycle guard (M9) counts in neither this nor `completed`, so
    /// `attempted - completed` stays the honest skipped-operation count.
    pub superseded: usize,
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
- suggested_memories[].synthesizes 只在**归纳多条已有记忆**时填这些 memory id。
  单条来源不是归纳（那只是改写），此时留空数组——不足两条来源的归纳 MUST NOT
  产出。结论被新证据修正时在正文里写「曾…现…」，不要输出置信度数字。
- suggested_memories[].supersedes 只在**新记忆明确取代某条已有记忆**时填该
  memory id（例如部署方式、端口、结论被新事实推翻）。两条记忆只是补充关系、或
  旧记忆仍然成立时，**必须留空数组**——误取代会让仍有价值的记忆停止注入。
- 没有内容时输出空数组。

JSON 格式：
{{
    "deviations": ["偏差描述"],
    "suggested_memories": [{{"layer":"project","category":"decision","content":"记忆内容","tags":["可选标签"],"reason":"为什么建议添加","supersedes":[],"synthesizes":[]}}],
    "outdated_memories": ["memory-id"]
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
- Fill suggested_memories[].synthesizes with the memory ids ONLY when this
  suggestion combines several existing memories into a new conclusion. A single
  source is not a synthesis — it is a restatement — so leave it empty; a
  synthesis over fewer than two sources must not be produced. When new evidence
  corrects a conclusion, phrase the change in the content ("used to …, now …")
  instead of emitting a confidence number.
- Fill suggested_memories[].supersedes with an existing memory id ONLY when the
  new memory explicitly replaces it (a changed deploy target, port, or reversed
  conclusion). Leave it empty when the two memories merely complement each
  other or the old one still holds — a wrong supersede stops a still-valuable
  memory from being injected.
- Output empty arrays when there is nothing.

JSON format:
{{
    "deviations": ["deviation description"],
    "suggested_memories": [{{"layer":"project","category":"decision","content":"memory content","tags":["optional tag"],"reason":"why this is suggested","supersedes":[],"synthesizes":[]}}],
    "outdated_memories": ["memory-id"]
}}

# Current project memory
{project_memory}

# Recent conversation summary
{recent_summary}"#
        }
    }
}

impl ReflectionEngine {
    pub fn build_prompt(&self, project_memory: &str, recent_summary: &str, lang: &str) -> String {
        Self::prompt_template(lang)
            .replace("{project_memory}", project_memory)
            .replace("{recent_summary}", recent_summary)
    }

    pub fn parse_output(&self, raw: &str) -> ReflectionResult<ReflectionOutput> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ReflectionError::Unparseable);
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
            return Err(ReflectionError::Unparseable);
        };

        let output: ReflectionOutput =
            serde_json::from_str(source).map_err(|_| ReflectionError::Parse)?;
        for (index, suggestion) in output.suggested_memories.iter().enumerate() {
            if suggestion.content.trim().is_empty() {
                return Err(ReflectionError::InvalidSuggestion(format!(
                    "suggested_memories[{index}].content must not be empty"
                )));
            }
        }
        Ok(output)
    }

    pub fn format_memory_summary(&self, entries: &[MemoryEntry]) -> String {
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

    pub fn recent_messages_summary(
        &self,
        messages: &[ReflectionMessage],
        max_chars: usize,
    ) -> String {
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
    use crate::domain::{MemoryId, MemorySource};

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
                    supersedes: vec![],
                    synthesizes: Vec::new(),
                }],
                outdated_memories: vec!["secret-id".into()],
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
            .parse_output(
                r#"{"deviations":null,"suggested_memories":null,"outdated_memories":null}"#,
            )
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
            Err(ReflectionError::Unparseable)
        ));
        assert!(matches!(
            engine().parse_output("no json here"),
            Err(ReflectionError::Unparseable)
        ));
        assert!(matches!(
            engine().parse_output("{\"deviations\": [}"),
            Err(ReflectionError::Parse)
        ));
    }

    #[test]
    fn rejects_empty_suggestion_content() {
        let result = engine()
            .parse_output(r#"{"suggested_memories":[{"category":"decision","content":"  "}]}"#);
        assert!(matches!(result, Err(ReflectionError::InvalidSuggestion(_))));
    }

    #[test]
    fn prompt_is_bilingual_without_user_alert() {
        let zh = engine().build_prompt("MEM", "SUMMARY", "zh");
        let en = engine().build_prompt("MEM", "SUMMARY", "en");
        assert!(zh.contains("只输出 JSON") && zh.contains("# 最近对话摘要"));
        assert!(en.contains("Output JSON only") && en.contains("# Recent conversation summary"));
        assert!(zh.contains("MEM") && en.contains("SUMMARY"));
        // user_alert 已随死代码清理移除：prompt 不再要求 LLM 产出该字段。
        assert!(!zh.contains("user_alert"));
        assert!(!en.contains("user_alert"));
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
