use super::*;
use crate::constants::EVENT_SCHEMA_VERSION;
use crate::domain::{MemoryCategory, MemoryEntry, MemoryId, MemoryLayer, MemorySource};
use std::collections::HashMap;

fn entry(content: &str) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        1_760_000_000,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        content,
        MemorySource::User,
    )
    .unwrap()
}

fn sample_event() -> MemoryEvent {
    let mut event = MemoryEvent::new(
        "evt-01890f3c",
        1_760_000_000_123,
        MemoryEventOp::WriteAdd,
        EventOutcome::Succeeded,
        "corr-01890f3c",
        EventActor::Service,
        EventChange::Write {
            before: vec![entry("旧记忆内容")],
            after: vec![entry("新记忆内容")],
        },
        EventContext {
            query: Some("用户写入记忆".to_string()),
            trigger_summary: Some("write 命令提交成功".to_string()),
            coverage_range: None,
        },
        ConfigFingerprint {
            scoring_enabled: true,
            reflection_model: Some("reflection-model".to_string()),
            reflection_model_revision: Some("rev-3".to_string()),
            similarity_threshold: Some(0.8),
            inject_token_budget: Some(4096),
            event_retention_days: Some(30),
        },
    );
    event.session_id = Some("session-42".to_string());
    event.run_ordinal = Some(7);
    event.step_ordinal = Some(12);
    event.tool_call_id = Some("call-9".to_string());
    event.layer = Some(MemoryLayer::Project);
    event.corpus = Some("project".to_string());
    event.commit_revision = Some("rev-abc".to_string());
    event
}

/// op 分组归属：穷尽 match 保证新增/删除变体必须同步本测试。
fn op_group(op: MemoryEventOp) -> &'static str {
    match op {
        MemoryEventOp::RetrieveForInject
        | MemoryEventOp::Search
        | MemoryEventOp::PerMessageRecall
        | MemoryEventOp::ListStats => "read",
        MemoryEventOp::WriteAdd
        | MemoryEventOp::Update
        | MemoryEventOp::Delete
        | MemoryEventOp::Pin
        | MemoryEventOp::MarkOutdated
        | MemoryEventOp::ArchiveRestore
        | MemoryEventOp::Compact
        | MemoryEventOp::SupersedeSynthesis => "write",
        MemoryEventOp::ReflectionTriggered
        | MemoryEventOp::ReflectionApplied
        | MemoryEventOp::ReflectionCost => "reflection",
        MemoryEventOp::OpenLoad
        | MemoryEventOp::CommitCas
        | MemoryEventOp::AssemblyFingerprint
        | MemoryEventOp::EvictionWatermark => "lifecycle",
    }
}

#[test]
fn memory_event_round_trips_through_serde_json() {
    let event = sample_event();
    let encoded = serde_json::to_string(&event).unwrap();
    let decoded: MemoryEvent = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, event);
}

#[test]
fn memory_event_op_covers_exactly_nineteen_operations() {
    // 监控操作面映射表以测试常量固化：读 4 / 写 8 / 反思 3 / 生命周期 4 = 19。
    let all_ops = [
        (MemoryEventOp::RetrieveForInject, "retrieve_for_inject"),
        (MemoryEventOp::Search, "search"),
        (MemoryEventOp::PerMessageRecall, "per_message_recall"),
        (MemoryEventOp::ListStats, "list_stats"),
        (MemoryEventOp::WriteAdd, "write_add"),
        (MemoryEventOp::Update, "update"),
        (MemoryEventOp::Delete, "delete"),
        (MemoryEventOp::Pin, "pin"),
        (MemoryEventOp::MarkOutdated, "mark_outdated"),
        (MemoryEventOp::ArchiveRestore, "archive_restore"),
        (MemoryEventOp::Compact, "compact"),
        (MemoryEventOp::SupersedeSynthesis, "supersede_synthesis"),
        (MemoryEventOp::ReflectionTriggered, "reflection_triggered"),
        (MemoryEventOp::ReflectionApplied, "reflection_applied"),
        (MemoryEventOp::ReflectionCost, "reflection_cost"),
        (MemoryEventOp::OpenLoad, "open_load"),
        (MemoryEventOp::CommitCas, "commit_cas"),
        (MemoryEventOp::AssemblyFingerprint, "assembly_fingerprint"),
        (MemoryEventOp::EvictionWatermark, "eviction_watermark"),
    ];
    assert_eq!(all_ops.len(), 19);

    let mut group_counts: HashMap<&'static str, usize> = HashMap::new();
    for (op, expected_name) in all_ops {
        let value = serde_json::to_value(op).unwrap();
        assert_eq!(
            value,
            serde_json::Value::String(expected_name.to_string()),
            "op 序列化必须是 snake_case"
        );
        let decoded: MemoryEventOp = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, op);
        *group_counts.entry(op_group(op)).or_insert(0) += 1;
    }
    assert_eq!(group_counts.get("read"), Some(&4));
    assert_eq!(group_counts.get("write"), Some(&8));
    assert_eq!(group_counts.get("reflection"), Some(&3));
    assert_eq!(group_counts.get("lifecycle"), Some(&4));
}

#[test]
fn missing_optional_fields_deserialize_as_none() {
    let json = serde_json::to_value(sample_event()).unwrap();
    let mut envelope = json.as_object().unwrap().clone();
    for key in [
        "session_id",
        "run_ordinal",
        "step_ordinal",
        "tool_call_id",
        "layer",
        "corpus",
        "commit_revision",
    ] {
        assert!(envelope.remove(key).is_some(), "样本应携带 {key}");
    }
    let mut context = envelope["context"].as_object().unwrap().clone();
    assert!(context.remove("query").is_some());
    envelope.insert("context".to_string(), serde_json::Value::Object(context));
    let mut fingerprint = envelope["config_fingerprint"].as_object().unwrap().clone();
    assert!(fingerprint.remove("similarity_threshold").is_some());
    envelope.insert(
        "config_fingerprint".to_string(),
        serde_json::Value::Object(fingerprint),
    );

    let decoded: MemoryEvent = serde_json::from_value(serde_json::Value::Object(envelope)).unwrap();
    assert_eq!(decoded.session_id, None);
    assert_eq!(decoded.run_ordinal, None);
    assert_eq!(decoded.step_ordinal, None);
    assert_eq!(decoded.tool_call_id, None);
    assert_eq!(decoded.layer, None);
    assert_eq!(decoded.corpus, None);
    assert_eq!(decoded.commit_revision, None);
    assert_eq!(decoded.context.query, None);
    assert_eq!(decoded.config_fingerprint.similarity_threshold, None);
}

#[test]
fn schema_version_is_written_and_mandatory() {
    let event = sample_event();
    assert_eq!(event.schema_version, EVENT_SCHEMA_VERSION);
    assert_eq!(EVENT_SCHEMA_VERSION, 1);
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["schema_version"], serde_json::json!(1));

    let mut without_version = json.as_object().unwrap().clone();
    without_version.remove("schema_version");
    assert!(
        serde_json::from_value::<MemoryEvent>(serde_json::Value::Object(without_version)).is_err(),
        "schema_version 是必填字段，缺省必须报错而非静默补默认值"
    );
}

#[test]
fn event_change_round_trips_all_discriminants() {
    let changes = [
        EventChange::Write {
            before: vec![entry("变更前全文")],
            after: vec![entry("变更后全文")],
        },
        EventChange::Read {
            candidates: vec![entry("候选正文")],
            hit_count: 3,
            limit: 5,
            layer_filter: Some(MemoryLayer::Global),
        },
        EventChange::Reflection {
            summary: "覆盖近 7 天的会话记忆".to_string(),
            affected: vec![entry("反思落地条目")],
        },
        EventChange::Lifecycle {
            stage: "eviction_watermark".to_string(),
            affected: vec![entry("满容淘汰候选")],
        },
    ];
    for change in changes {
        let encoded = serde_json::to_string(&change).unwrap();
        let decoded: EventChange = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, change);
    }
}

#[test]
fn event_outcome_and_actor_round_trip_with_snake_case() {
    let outcomes = [
        (EventOutcome::Succeeded, serde_json::json!("succeeded")),
        (
            EventOutcome::Failed {
                kind: "cas_conflict".to_string(),
            },
            serde_json::json!({"failed": {"kind": "cas_conflict"}}),
        ),
        (EventOutcome::Skipped, serde_json::json!("skipped")),
    ];
    for (outcome, expected) in outcomes {
        let value = serde_json::to_value(&outcome).unwrap();
        assert_eq!(value, expected);
        let decoded: EventOutcome = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, outcome);
    }

    let actors = [
        (EventActor::Service, "service"),
        (EventActor::ReflectionWorkflow, "reflection_workflow"),
        (EventActor::Opener, "opener"),
        (EventActor::RetentionGc, "retention_gc"),
    ];
    for (actor, expected) in actors {
        let value = serde_json::to_value(actor).unwrap();
        assert_eq!(value, serde_json::Value::String(expected.to_string()));
        let decoded: EventActor = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, actor);
    }
}
