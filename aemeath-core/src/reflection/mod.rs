pub mod prompt;

use crate::memory::{
    AddResult, MemoryCategory, MemoryEntry, MemoryLayer, MemorySource, MemoryStore,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemorySuggestion {
    pub category: MemoryCategory,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Why this memory is suggested — helps user decide whether to accept.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReflectionOutput {
    #[serde(default)]
    pub deviations: Vec<String>,
    #[serde(default)]
    pub suggested_memories: Vec<MemorySuggestion>,
    #[serde(default)]
    pub outdated_memories: Vec<String>,
    #[serde(default)]
    pub user_alert: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReflectionApplyResult {
    pub suggestions_added: usize,
    pub outdated_marked: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ReflectionError {
    #[error("Reflection 输出不是有效 JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("标记过时记忆失败: {0}")]
    Memory(String),
}

pub type ReflectionResult<T> = Result<T, ReflectionError>;

pub struct ReflectionEngine;

impl ReflectionEngine {
    pub fn parse_output(json: &str) -> ReflectionResult<ReflectionOutput> {
        let extracted = Self::extract_json_object(json).unwrap_or(json);
        serde_json::from_str(extracted).map_err(ReflectionError::InvalidJson)
    }

    pub fn extract_json_object(text: &str) -> Option<&str> {
        let trimmed = text.trim();
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            return Some(trimmed);
        }

        if let Some(fence_start) = trimmed.find("```") {
            let after_start = &trimmed[fence_start + 3..];
            let after_lang = after_start
                .strip_prefix("json")
                .or_else(|| after_start.strip_prefix("JSON"))
                .unwrap_or(after_start)
                .trim_start_matches(['\r', '\n']);
            if let Some(fence_end) = after_lang.find("```") {
                let fenced = after_lang[..fence_end].trim();
                if fenced.starts_with('{') && fenced.ends_with('}') {
                    return Some(fenced);
                }
            }
        }

        let start = trimmed.find('{')?;
        let end = trimmed.rfind('}')?;
        if start < end {
            Some(trimmed[start..=end].trim())
        } else {
            None
        }
    }

    pub fn apply_suggestions(
        output: &ReflectionOutput,
        store: &mut MemoryStore,
    ) -> ReflectionResult<usize> {
        let mut added = 0;
        for suggestion in &output.suggested_memories {
            let mut entry = MemoryEntry::new(
                MemoryLayer::Project,
                suggestion.category,
                suggestion.content.clone(),
                MemorySource::Llm,
            )
            .with_tags(suggestion.tags.clone());
            if !suggestion.reason.trim().is_empty() {
                entry = entry.with_source_ref(format!("reflection: {}", suggestion.reason));
            } else {
                entry = entry.with_source_ref("reflection");
            }
            match store
                .add(entry)
                .map_err(|error| ReflectionError::Memory(error.to_string()))?
            {
                AddResult::Added | AddResult::Merged { .. } => added += 1,
                AddResult::NeedsEviction { .. } => {
                    return Err(ReflectionError::Memory(
                        "Memory 已满，请先执行 /memory compact 后再应用反思建议".to_string(),
                    ));
                }
            }
        }
        Ok(added)
    }

    pub fn apply_output(
        output: &ReflectionOutput,
        store: &mut MemoryStore,
    ) -> ReflectionResult<ReflectionApplyResult> {
        let outdated_marked = Self::apply_outdated(output, store)?;
        let suggestions_added = Self::apply_suggestions(output, store)?;
        Ok(ReflectionApplyResult {
            suggestions_added,
            outdated_marked,
        })
    }

    pub fn apply_outdated(
        output: &ReflectionOutput,
        store: &mut MemoryStore,
    ) -> ReflectionResult<usize> {
        let mut marked = 0;
        for id in &output.outdated_memories {
            store
                .mark_outdated(id)
                .map_err(|error| ReflectionError::Memory(error.to_string()))?;
            marked += 1;
        }
        Ok(marked)
    }

    pub fn format_output(output: &ReflectionOutput) -> String {
        let mut text = String::from("─── Reflection ───\n");
        text.push_str("偏差检测：\n");
        if output.deviations.is_empty() {
            text.push_str("  - 暂无明显偏差\n");
        } else {
            for deviation in &output.deviations {
                text.push_str(&format!("  - {deviation}\n"));
            }
        }

        text.push_str("\n建议记忆：\n");
        if output.suggested_memories.is_empty() {
            text.push_str("  - 暂无建议\n");
        } else {
            for suggestion in &output.suggested_memories {
                let reason = if suggestion.reason.is_empty() {
                    String::new()
                } else {
                    format!(" {}", suggestion.reason)
                };
                text.push_str(&format!(
                    "  - [{:?}] {}{} (+)\n",
                    suggestion.category, suggestion.content, reason
                ));
            }
        }

        text.push_str("\n过时记忆：\n");
        if output.outdated_memories.is_empty() {
            text.push_str("  - 暂无\n");
        } else {
            for id in &output.outdated_memories {
                text.push_str(&format!("  - {id}\n"));
            }
        }

        if let Some(alert) = &output.user_alert {
            text.push_str(&format!("\n用户提示：{alert}\n"));
        }
        text.push_str("────────────────");
        text
    }

    pub fn memory_summary(entries: &[MemoryEntry]) -> String {
        entries
            .iter()
            .map(|entry| format!("- {} [{:?}] {}", entry.id, entry.category, entry.content))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Build the full reflection prompt from project memory and recent conversation summary.
    pub fn build_prompt(project_memory: &str, recent_summary: &str) -> String {
        prompt::build_reflection_prompt(project_memory, recent_summary)
    }

    /// Extract a text summary of the most recent messages for the reflection prompt.
    /// Truncates to at most `max_chars` characters.
    pub fn recent_messages_summary(
        messages: &[crate::message::Message],
        max_chars: usize,
    ) -> String {
        use crate::message::Role;
        let mut summary = String::new();
        for msg in messages.iter().rev().take(20).rev() {
            let role = match msg.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
            };
            let content = msg.text_content();
            let truncated: String = if content.chars().count() > 500 {
                let idx = content
                    .char_indices()
                    .nth(500)
                    .map(|(i, _)| i)
                    .unwrap_or(content.len());
                format!("{}…", &content[..idx])
            } else {
                content
            };
            if truncated.trim().is_empty() {
                continue;
            }
            let line = format!("[{role}]: {truncated}\n");
            if summary.len() + line.len() > max_chars {
                break;
            }
            summary.insert_str(0, &line);
        }
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(max_entries: usize) -> (MemoryStore, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("aemeath-reflection-test-{}", uuid::Uuid::new_v4()));
        let store = MemoryStore::new(&dir, "project", max_entries, 0.9).unwrap();
        (store, dir)
    }

    #[test]
    fn test_parse_output_valid_json() {
        let json = r#"{
            "deviations": ["偏离约定"],
            "suggested_memories": [{"category":"decision","content":"使用中文回复","reason":"用户偏好"}],
            "outdated_memories": ["abc"],
            "user_alert": "需要确认"
        }"#;
        let output = ReflectionEngine::parse_output(json).unwrap();

        assert_eq!(output.deviations, vec!["偏离约定"]);
        assert_eq!(output.suggested_memories.len(), 1);
        assert_eq!(output.suggested_memories[0].reason, "用户偏好");
        assert_eq!(output.outdated_memories, vec!["abc"]);
    }

    #[test]
    fn test_parse_output_reason_optional() {
        let json = r#"{
            "suggested_memories": [{"category":"pattern","content":"测试"}]
        }"#;
        let output = ReflectionEngine::parse_output(json).unwrap();

        assert_eq!(output.suggested_memories[0].reason, "");
    }

    #[test]
    fn test_parse_output_malformed_json_error() {
        let result = ReflectionEngine::parse_output("not json");

        assert!(matches!(result, Err(ReflectionError::InvalidJson(_))));
    }

    #[test]
    fn test_parse_output_extracts_fenced_json() {
        let text = r#"这里是反思结果：
```json
{"deviations":["偏差"],"suggested_memories":[],"outdated_memories":[],"user_alert":null}
```
"#;

        let output = ReflectionEngine::parse_output(text).unwrap();

        assert_eq!(output.deviations, vec!["偏差"]);
    }

    #[test]
    fn test_parse_output_extracts_object_from_prose() {
        let text = r#"反思如下：{"deviations":["遗漏测试"],"suggested_memories":[]}请确认。"#;

        let output = ReflectionEngine::parse_output(text).unwrap();

        assert_eq!(output.deviations, vec!["遗漏测试"]);
    }

    #[test]
    fn test_apply_suggestions_adds_llm_project_memory() {
        let (mut store, dir) = temp_store(10);
        let output = ReflectionOutput {
            deviations: Vec::new(),
            suggested_memories: vec![MemorySuggestion {
                category: crate::memory::MemoryCategory::Decision,
                content: "Reflection 使用真实 LLM 调用".to_string(),
                tags: vec!["reflection".to_string()],
                reason: "用户选择方案 B".to_string(),
            }],
            outdated_memories: Vec::new(),
            user_alert: None,
        };

        let added = ReflectionEngine::apply_suggestions(&output, &mut store).unwrap();
        let memories = store
            .list(Some(crate::memory::MemoryLayer::Project))
            .unwrap();

        assert_eq!(added, 1);
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].source, crate::memory::MemorySource::Llm);
        assert_eq!(memories[0].tags, vec!["reflection"]);
        assert!(memories[0]
            .source_ref
            .as_deref()
            .unwrap()
            .contains("用户选择方案 B"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_apply_output_marks_outdated_and_adds_suggestions() {
        let (mut store, dir) = temp_store(10);
        let existing = MemoryEntry::new(
            crate::memory::MemoryLayer::Project,
            crate::memory::MemoryCategory::Fact,
            "旧事实",
            crate::memory::MemorySource::User,
        );
        let existing_id = existing.id.clone();
        store.add(existing).unwrap();
        let output = ReflectionOutput {
            deviations: Vec::new(),
            suggested_memories: vec![MemorySuggestion {
                category: crate::memory::MemoryCategory::Pattern,
                content: "先写测试再实现".to_string(),
                tags: Vec::new(),
                reason: String::new(),
            }],
            outdated_memories: vec![existing_id.clone()],
            user_alert: None,
        };

        let applied = ReflectionEngine::apply_output(&output, &mut store).unwrap();
        let memories = store
            .list(Some(crate::memory::MemoryLayer::Project))
            .unwrap();
        let outdated = memories
            .iter()
            .find(|entry| entry.id == existing_id)
            .unwrap();

        assert_eq!(applied.suggestions_added, 1);
        assert_eq!(applied.outdated_marked, 1);
        assert!(outdated.outdated);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_format_output_empty_sections() {
        let output = ReflectionOutput {
            deviations: Vec::new(),
            suggested_memories: Vec::new(),
            outdated_memories: Vec::new(),
            user_alert: None,
        };
        let text = ReflectionEngine::format_output(&output);

        assert!(text.contains("Reflection"));
        assert!(text.contains("暂无明显偏差"));
        assert!(text.contains("暂无建议"));
    }

    #[test]
    fn test_build_prompt_contains_memory_and_summary() {
        let prompt = ReflectionEngine::build_prompt(
            "- [Decision] 使用 JSON 存储",
            "User: 你好\nAssistant: 你好！\n",
        );
        assert!(prompt.contains("使用 JSON 存储"));
        assert!(prompt.contains("最近对话摘要"));
    }

    #[test]
    fn test_recent_messages_summary_empty() {
        let result = ReflectionEngine::recent_messages_summary(&[], 2000);
        assert!(result.is_empty());
    }

    #[test]
    fn test_recent_messages_summary_truncates() {
        use crate::message::Message;
        let msg = Message::user("hello world");
        let result = ReflectionEngine::recent_messages_summary(&[msg], 2000);
        assert!(result.contains("[User]: hello world"));
    }
}
