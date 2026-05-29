use super::*;
use share::memory::MemoryEntry;
use storage::memory::MemoryStore;

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

#[test]
fn test_apply_suggestions_adds_llm_project_memory() {
    let (mut store, dir) = temp_store(10);
    let output = ReflectionOutput {
        deviations: Vec::new(),
        suggested_memories: vec![MemorySuggestion {
            category: share::memory::MemoryCategory::Decision,
            content: "Reflection 使用真实 LLM 调用".to_string(),
            tags: vec!["reflection".to_string()],
            reason: "用户选择方案 B".to_string(),
        }],
        outdated_memories: Vec::new(),
        user_alert: None,
    };

    let added =
        ReflectionEngine::apply_suggestions(&output.suggested_memories, &mut store).unwrap();
    let memories = store
        .list(Some(share::memory::MemoryLayer::Project))
        .unwrap();

    assert_eq!(added, 1);
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0].source, share::memory::MemorySource::Llm);
    assert_eq!(memories[0].tags, vec!["reflection"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_apply_output_marks_outdated_and_adds_suggestions() {
    let (mut store, dir) = temp_store(10);
    let existing = MemoryEntry::new(
        share::memory::MemoryLayer::Project,
        share::memory::MemoryCategory::Fact,
        "旧事实",
        share::memory::MemorySource::User,
    );
    let existing_id = existing.id.clone();
    store.add(existing).unwrap();
    let output = ReflectionOutput {
        deviations: Vec::new(),
        suggested_memories: vec![MemorySuggestion {
            category: share::memory::MemoryCategory::Pattern,
            content: "先写测试再实现".to_string(),
            tags: Vec::new(),
            reason: String::new(),
        }],
        outdated_memories: vec![existing_id.clone()],
        user_alert: None,
    };

    let applied = ReflectionEngine::apply_output(&output, &mut store).unwrap();
    let memories = store
        .list(Some(share::memory::MemoryLayer::Project))
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
    use share::message::Message;
    let msg = Message::user("hello world");
    let result = ReflectionEngine::recent_messages_summary(&[msg], 2000);
    assert!(result.contains("[User]: hello world"));
}
