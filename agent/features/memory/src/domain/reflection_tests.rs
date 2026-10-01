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
        .parse_output(r#"{"deviations":null,"suggested_memories":null,"outdated_memories":null}"#)
        .unwrap();
    assert_eq!(output, ReflectionOutput::default());

    let output = engine()
        .parse_output(r#"{"suggested_memories":[{"category":"fact","content":"x","tags":null}]}"#)
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
    let result =
        engine().parse_output(r#"{"suggested_memories":[{"category":"decision","content":"  "}]}"#);
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
