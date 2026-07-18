use super::*;
use memory::{InMemoryMemory, MemoryEntry, MemoryId, MemoryLayer, MemoryPolicy, MemoryPort};
use std::sync::Arc;

fn in_memory_port(max_entries: usize) -> Arc<InMemoryMemory> {
    Arc::new(
        InMemoryMemory::new(MemoryPolicy {
            max_entries,
            similarity_threshold: 0.9,
        })
        .unwrap(),
    )
}

fn project_entry(content: &str, category: memory::MemoryCategory) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        100,
        MemoryLayer::Project,
        category,
        content,
        memory::MemorySource::User,
    )
    .unwrap()
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
fn test_parse_output_null_arrays_as_empty() {
    let json = r#"{
            "deviations": null,
            "suggested_memories": null,
            "outdated_memories": null,
            "user_alert": null
        }"#;
    let output = ReflectionEngine::parse_output(json).unwrap();

    assert!(output.deviations.is_empty());
    assert!(output.suggested_memories.is_empty());
    assert!(output.outdated_memories.is_empty());
    assert!(output.user_alert.is_none());
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

    assert!(matches!(result, Err(ReflectionError::Parse(_))));
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

#[tokio::test]
async fn test_apply_suggestions_adds_llm_project_memory_via_port() {
    let port = in_memory_port(10);
    let output = ReflectionOutput {
        deviations: Vec::new(),
        suggested_memories: vec![MemorySuggestion {
            layer: share::memory::MemoryLayer::Project,
            category: share::memory::MemoryCategory::Decision,
            content: "Reflection 使用真实 LLM 调用".to_string(),
            tags: vec!["reflection".to_string()],
            reason: "用户选择方案 B".to_string(),
        }],
        outdated_memories: Vec::new(),
        user_alert: None,
    };

    let added = ReflectionEngine::apply_suggestions(&output.suggested_memories, port.as_ref())
        .await
        .unwrap();
    let memories = port.list(Some(MemoryLayer::Project));

    assert_eq!(added, 1);
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0].source, memory::MemorySource::Llm);
    assert_eq!(memories[0].layer, MemoryLayer::Project);
    assert_eq!(memories[0].tags, vec!["reflection"]);
}

#[tokio::test]
async fn test_apply_suggestions_adds_llm_global_memory_via_port() {
    let port = in_memory_port(10);
    let suggestion = MemorySuggestion {
        layer: share::memory::MemoryLayer::Global,
        category: share::memory::MemoryCategory::Preference,
        content: "始终使用中文回复".to_string(),
        tags: Vec::new(),
        reason: String::new(),
    };

    let added = ReflectionEngine::apply_suggestions(&[suggestion], port.as_ref())
        .await
        .unwrap();
    let global = port.list(Some(MemoryLayer::Global));
    let project = port.list(Some(MemoryLayer::Project));

    assert_eq!(added, 1);
    assert_eq!(global.len(), 1);
    assert_eq!(global[0].content, "始终使用中文回复");
    assert!(project.is_empty());
}

#[tokio::test]
async fn test_apply_suggestions_evicts_when_port_is_full() {
    let port = in_memory_port(1);
    port.write(project_entry(
        "旧记忆需要被淘汰",
        memory::MemoryCategory::Fact,
    ))
    .await
    .unwrap();
    let suggestion = MemorySuggestion {
        layer: share::memory::MemoryLayer::Project,
        category: share::memory::MemoryCategory::Decision,
        content: "新的 reflection 记忆".to_string(),
        tags: Vec::new(),
        reason: String::new(),
    };

    let added = ReflectionEngine::apply_suggestions(&[suggestion], port.as_ref())
        .await
        .unwrap();
    let active = port.list(Some(MemoryLayer::Project));

    assert_eq!(added, 1);
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].content, "新的 reflection 记忆");
}

#[tokio::test]
async fn test_apply_suggestions_returns_error_when_eviction_cannot_free_space() {
    let port = in_memory_port(1);
    let mut existing = project_entry("被 pin 的旧记忆", memory::MemoryCategory::Fact);
    existing.pinned = true;
    port.write(existing).await.unwrap();
    let suggestion = MemorySuggestion {
        layer: share::memory::MemoryLayer::Project,
        category: share::memory::MemoryCategory::Decision,
        content: "无法写入的新记忆".to_string(),
        tags: Vec::new(),
        reason: String::new(),
    };

    let result = ReflectionEngine::apply_suggestions(&[suggestion], port.as_ref()).await;

    assert!(matches!(result, Err(ReflectionError::Apply(_))));
    assert_eq!(port.list(Some(MemoryLayer::Project)).len(), 1);
}

#[tokio::test]
async fn test_apply_output_marks_outdated_and_adds_suggestions_via_port() {
    let port = in_memory_port(10);
    let existing = project_entry("旧事实", memory::MemoryCategory::Fact);
    let existing_id = existing.id;
    port.write(existing).await.unwrap();
    let output = ReflectionOutput {
        deviations: Vec::new(),
        suggested_memories: vec![MemorySuggestion {
            layer: share::memory::MemoryLayer::Project,
            category: share::memory::MemoryCategory::Pattern,
            content: "先写测试再实现".to_string(),
            tags: Vec::new(),
            reason: String::new(),
        }],
        outdated_memories: vec![existing_id.to_string()],
        user_alert: None,
    };

    let applied = ReflectionEngine::apply_output(&output, port.as_ref())
        .await
        .unwrap();
    let memories = port.list(Some(MemoryLayer::Project));
    let outdated = memories
        .iter()
        .find(|entry| entry.id == existing_id)
        .unwrap();

    assert_eq!(applied.suggestions_added, 1);
    assert_eq!(applied.outdated_marked, 1);
    assert!(outdated.outdated);
}

#[test]
fn test_format_output_empty_sections() {
    let output = ReflectionOutput {
        deviations: Vec::new(),
        suggested_memories: Vec::new(),
        outdated_memories: Vec::new(),
        user_alert: None,
    };
    let text = ReflectionEngine::format_output(&output, "zh");

    assert!(text.contains("Reflection"));
    assert!(text.contains("暂无明显偏差"));
    assert!(text.contains("暂无建议"));
}

#[test]
fn test_build_prompt_contains_memory_and_summary() {
    let prompt = ReflectionEngine::build_prompt(
        "- [Decision] 使用 JSON 存储",
        "User: 你好\nAssistant: 你好！\n",
        "zh",
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
    use share::message::Message;
    let msg = Message::user("hello world");
    let result = ReflectionEngine::recent_messages_summary(&[msg], 2000);
    assert!(result.contains("[User]: hello world"));
}

#[tokio::test]
async fn test_project_memory_summary_reads_from_port() {
    let port = in_memory_port(10);
    port.write(project_entry(
        "端口里的项目记忆",
        memory::MemoryCategory::Fact,
    ))
    .await
    .unwrap();

    let summary = ReflectionEngine::memory_summary(&port.list(Some(MemoryLayer::Project)));

    assert!(summary.contains("端口里的项目记忆"));
}
