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
        coverage_end: None,
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
            coverage_end: None,
            deviation_texts: None,
            suggested_memories: None,
        }
    );
    let json = serde_json::to_string(&record.safe_summary()).unwrap();
    assert!(!json.contains("secret"));
}

/// 可选内容投影：`safe_summary()` 默认不携带内容（Safe 边界保留）；
/// `safe_summary_with_content()` 显式携带偏差文本与建议内容（仅本地
/// /reflect 查询使用）。两条路径的计数与状态字段必须一致。
#[test]
fn safe_summary_with_content_carries_texts_and_suggestions() {
    let record = ReflectionRecord {
        id: "reflection-c".into(),
        timestamp: 43,
        trigger: ReflectionTrigger::Manual,
        status: ReflectionStatus::Succeeded,
        output: Some(ReflectionOutput {
            deviations: vec!["deviation one".into(), "deviation two".into()],
            suggested_memories: vec![MemorySuggestion {
                layer: MemoryLayer::Project,
                category: MemoryCategory::Decision,
                content: "memory content".into(),
                tags: vec!["tag-a".into()],
                reason: "why".into(),
                supersedes: vec![],
                synthesizes: Vec::new(),
            }],
            outdated_memories: vec![],
        }),
        apply_result: None,
        error_category: None,
        token_usage: None,
        duration_ms: 7,
        coverage_end: None,
    };

    let plain = record.safe_summary();
    assert_eq!(plain.deviations, 2);
    assert_eq!(plain.suggestions, 1);
    assert!(
        plain.deviation_texts.is_none(),
        "默认摘要不得携带偏差文本（Safe 边界）"
    );
    assert!(
        plain.suggested_memories.is_none(),
        "默认摘要不得携带建议内容（Safe 边界）"
    );
    let json = serde_json::to_string(&plain).unwrap();
    assert!(!json.contains("deviation one"));
    assert!(!json.contains("memory content"));

    let with_content = record.safe_summary_with_content();
    assert_eq!(with_content.deviations, 2);
    assert_eq!(with_content.suggestions, 1);
    assert_eq!(
        with_content.deviation_texts.as_deref(),
        Some(&["deviation one".to_string(), "deviation two".to_string()][..])
    );
    let suggestions = with_content
        .suggested_memories
        .as_ref()
        .expect("内容投影必须携带建议");
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].content, "memory content");
    assert_eq!(suggestions[0].category, MemoryCategory::Decision);
    assert_eq!(suggestions[0].layer, MemoryLayer::Project);
    assert_eq!(suggestions[0].reason, "why");
}

/// 无 output 的记录（Failed/Running）：内容投影同样不携带内容字段。
#[test]
fn safe_summary_with_content_without_output_stays_empty() {
    let record = ReflectionRecord::failed(
        "reflection-f",
        44,
        ReflectionTrigger::Interval,
        ReflectionErrorCategory::LlmCall,
        3,
    );
    let with_content = record.safe_summary_with_content();
    assert!(with_content.deviation_texts.is_none());
    assert!(with_content.suggested_memories.is_none());
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

// ── 反思游标（coverage_end）─────────────────────────────────────

/// 游标字段的序列化兼容：旧记录（无 coverage_end）反序列化为 None；
/// 新记录往返保持；safe summary 携带游标（元数据非内容，Safe 边界允许）。
#[test]
fn coverage_end_is_optional_and_round_trips() {
    let mut record = ReflectionRecord::running("r-1", 10, ReflectionTrigger::Interval);
    assert_eq!(record.coverage_end, None);

    // 旧数据（JSON 无 coverage_end）必须可反序列化。
    let legacy = serde_json::json!({
        "id": "r-0",
        "timestamp": 1,
        "trigger": "interval",
        "status": "succeeded",
        "apply_result": null,
        "duration_ms": 0
    });
    let restored: ReflectionRecord = serde_json::from_value(legacy).expect("旧记录必须可反序列化");
    assert_eq!(restored.coverage_end, None);

    record.coverage_end = Some(42);
    let json = serde_json::to_string(&record).unwrap();
    let round_trip: ReflectionRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.coverage_end, Some(42));
    assert_eq!(record.safe_summary().coverage_end, Some(42));
}
