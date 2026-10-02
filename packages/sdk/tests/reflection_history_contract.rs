//! `ReflectionHistoryView` 内容投影的 wire 契约：新字段序列化往返稳定、
//! 缺失字段反序列化为空 vec（旧数据兼容）、schema 暴露新类型。

use sdk::{
    MemoryCategoryView, MemoryLayerView, MemorySuggestionView, ReflectionApplyStatusView,
    ReflectionHistoryView, ReflectionStatusView, ReflectionTriggerView,
};

fn sample_view() -> ReflectionHistoryView {
    ReflectionHistoryView {
        id: "r-1".to_string(),
        timestamp: 42,
        trigger: ReflectionTriggerView::Manual,
        status: ReflectionStatusView::Succeeded,
        deviations: 1,
        suggestions: 1,
        outdated: 0,
        apply_status: ReflectionApplyStatusView::Applied,
        error_category: None,
        token_usage: None,
        duration_ms: 7,
        deviation_texts: vec!["deviation text".to_string()],
        suggested_memories: vec![MemorySuggestionView {
            layer: MemoryLayerView::Project,
            category: MemoryCategoryView::Decision,
            content: "memory content".to_string(),
            tags: vec!["tag-a".to_string()],
            reason: "why".to_string(),
        }],
    }
}

#[test]
fn reflection_history_view_content_fields_round_trip() {
    let view = sample_view();
    let json = serde_json::to_string(&view).expect("serialize");
    let restored: ReflectionHistoryView = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored, view);

    // snake_case 枚举序列化与 Memory BC 对齐（跨边界反序列化兼容）。
    assert!(json.contains("\"layer\":\"project\""));
    assert!(json.contains("\"category\":\"decision\""));
    assert!(json.contains("\"trigger\":\"manual\""));
}

#[test]
fn reflection_history_view_without_content_fields_deserializes_to_empty() {
    // 旧数据/默认摘要（无内容字段）必须反序列化为空 vec（serde default）。
    let json = serde_json::json!({
        "id": "r-2",
        "timestamp": 1,
        "trigger": "interval",
        "status": "succeeded",
        "deviations": 0,
        "suggestions": 0,
        "outdated": 0,
        "apply_status": "not_applied",
        "error_category": null,
        "token_usage": null,
        "duration_ms": 0
    });
    let view: ReflectionHistoryView =
        serde_json::from_value(json).expect("旧摘要不带内容字段也必须可反序列化");
    assert!(view.deviation_texts.is_empty());
    assert!(view.suggested_memories.is_empty());
}

#[test]
fn wire_schema_exposes_suggestion_view_types() {
    let document = sdk::wire::components_document();
    let schemas = document["$defs"].as_object().expect("$defs must exist");
    assert!(schemas.contains_key("MemorySuggestionView"));
    assert!(schemas.contains_key("MemoryLayerView"));
    assert!(schemas.contains_key("MemoryCategoryView"));
}
