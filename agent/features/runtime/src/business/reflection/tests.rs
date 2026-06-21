use super::*;
use share::memory::MemoryEntry;
use storage::api::MemoryStore;

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
            layer: share::memory::MemoryLayer::Project,
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
    assert_eq!(memories[0].layer, share::memory::MemoryLayer::Project);
    assert_eq!(memories[0].tags, vec!["reflection"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_apply_suggestions_adds_llm_global_memory() {
    let (mut store, dir) = temp_store(10);
    let suggestion = MemorySuggestion {
        layer: share::memory::MemoryLayer::Global,
        category: share::memory::MemoryCategory::Preference,
        content: "始终使用中文回复".to_string(),
        tags: Vec::new(),
        reason: String::new(),
    };

    let added = ReflectionEngine::apply_suggestions(&[suggestion], &mut store).unwrap();
    let global = store
        .list(Some(share::memory::MemoryLayer::Global))
        .unwrap();
    let project = store
        .list(Some(share::memory::MemoryLayer::Project))
        .unwrap();

    assert_eq!(added, 1);
    assert_eq!(global.len(), 1);
    assert_eq!(global[0].content, "始终使用中文回复");
    assert!(project.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_apply_suggestions_evicts_when_store_is_full() {
    let (mut store, dir) = temp_store(1);
    let existing = MemoryEntry::new(
        "memory-1",
        100,
        share::memory::MemoryLayer::Project,
        share::memory::MemoryCategory::Fact,
        "旧记忆需要被淘汰",
        share::memory::MemorySource::User,
    );
    store.add(existing).unwrap();
    let suggestion = MemorySuggestion {
        layer: share::memory::MemoryLayer::Project,
        category: share::memory::MemoryCategory::Decision,
        content: "新的 reflection 记忆".to_string(),
        tags: Vec::new(),
        reason: String::new(),
    };

    let added = ReflectionEngine::apply_suggestions(&[suggestion], &mut store).unwrap();
    let active = store
        .list(Some(share::memory::MemoryLayer::Project))
        .unwrap();
    let archived = store.search("旧记忆", 10).unwrap();

    assert_eq!(added, 1);
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].content, "新的 reflection 记忆");
    assert_eq!(archived.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_apply_suggestions_returns_error_when_eviction_cannot_free_space() {
    let (mut store, dir) = temp_store(1);
    let mut existing = MemoryEntry::new(
        "memory-1",
        100,
        share::memory::MemoryLayer::Project,
        share::memory::MemoryCategory::Fact,
        "被 pin 的旧记忆",
        share::memory::MemorySource::User,
    );
    existing.pinned = true;
    store.add(existing).unwrap();
    let suggestion = MemorySuggestion {
        layer: share::memory::MemoryLayer::Project,
        category: share::memory::MemoryCategory::Decision,
        content: "无法写入的新记忆".to_string(),
        tags: Vec::new(),
        reason: String::new(),
    };

    let result = ReflectionEngine::apply_suggestions(&[suggestion], &mut store);

    assert!(matches!(result, Err(ReflectionError::Apply(_))));
    assert_eq!(
        store
            .list(Some(share::memory::MemoryLayer::Project))
            .unwrap()
            .len(),
        1
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_apply_output_marks_outdated_and_adds_suggestions() {
    let (mut store, dir) = temp_store(10);
    let existing = MemoryEntry::new(
        "memory-1",
        100,
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
            layer: share::memory::MemoryLayer::Project,
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
