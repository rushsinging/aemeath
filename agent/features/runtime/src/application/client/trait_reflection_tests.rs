use super::*;

#[test]
fn safe_sdk_view_contains_only_metadata_and_counts() {
    let view = summary_to_sdk(ReflectionSafeSummary {
        id: "reflection-1".into(),
        timestamp: 42,
        trigger: ReflectionTrigger::PreCompact,
        status: ReflectionStatus::Succeeded,
        deviations: 1,
        suggestions: 2,
        outdated: 3,
        apply_status: ReflectionApplyStatus::Applied,
        error_category: None,
        token_usage: Some(memory::api::reflection::ReflectionTokenUsage {
            input_tokens: 10,
            output_tokens: 20,
        }),
        duration_ms: 30,
        deviation_texts: None,
        suggested_memories: None,
    });
    assert_eq!(view.trigger, ReflectionTriggerView::PreCompact);
    assert_eq!(view.suggestions, 2);
    assert_eq!(view.token_usage.unwrap().input_tokens, 10);
    assert!(
        view.deviation_texts.is_empty(),
        "无内容投影时 SDK view 内容字段为空 vec"
    );
    assert!(view.suggested_memories.is_empty());
}

/// 内容投影映射：deviation 文本与建议内容（含 layer/category 枚举映射、tags/reason）
/// 完整跨边界，NEVER 截断或改写内容。
#[test]
fn sdk_view_maps_content_projection_with_layer_and_category() {
    let view = summary_to_sdk(ReflectionSafeSummary {
        id: "reflection-2".into(),
        timestamp: 43,
        trigger: ReflectionTrigger::Manual,
        status: ReflectionStatus::Succeeded,
        deviations: 1,
        suggestions: 1,
        outdated: 0,
        apply_status: ReflectionApplyStatus::NotApplied,
        error_category: None,
        token_usage: None,
        duration_ms: 5,
        deviation_texts: Some(vec!["deviation one".into(), "deviation two".into()]),
        suggested_memories: Some(vec![memory::api::MemorySuggestion {
            layer: memory::api::MemoryLayer::Project,
            category: memory::api::MemoryCategory::Pitfall,
            content: "memory content".into(),
            tags: vec!["tag-a".into(), "tag-b".into()],
            reason: "why".into(),
            supersedes: vec![],
            synthesizes: vec![],
        }]),
    });

    assert_eq!(view.deviation_texts, vec!["deviation one", "deviation two"]);
    assert_eq!(view.suggested_memories.len(), 1);
    let suggestion = &view.suggested_memories[0];
    assert_eq!(suggestion.content, "memory content");
    assert_eq!(suggestion.layer, sdk::MemoryLayerView::Project);
    assert_eq!(suggestion.category, sdk::MemoryCategoryView::Pitfall);
    assert_eq!(suggestion.tags, vec!["tag-a", "tag-b"]);
    assert_eq!(suggestion.reason, "why");
}

/// 枚举映射穷举：Global layer 与各 category 都有对应 view 变体（防新增枚举漏映射）。
#[test]
fn sdk_view_maps_all_layer_and_category_variants() {
    use memory::api::MemorySuggestion;
    use memory::api::{MemoryCategory, MemoryLayer};

    for (layer, expected_layer) in [
        (MemoryLayer::Global, sdk::MemoryLayerView::Global),
        (MemoryLayer::Project, sdk::MemoryLayerView::Project),
    ] {
        for (category, expected_category) in [
            (MemoryCategory::Fact, sdk::MemoryCategoryView::Fact),
            (MemoryCategory::Decision, sdk::MemoryCategoryView::Decision),
            (
                MemoryCategory::Preference,
                sdk::MemoryCategoryView::Preference,
            ),
            (MemoryCategory::Pattern, sdk::MemoryCategoryView::Pattern),
            (MemoryCategory::Pitfall, sdk::MemoryCategoryView::Pitfall),
        ] {
            let mapped = suggestion_to_sdk(MemorySuggestion {
                layer,
                category,
                content: "c".into(),
                tags: vec![],
                reason: "r".into(),
                supersedes: vec![],
                synthesizes: vec![],
            });
            assert_eq!(mapped.layer, expected_layer);
            assert_eq!(mapped.category, expected_category);
        }
    }
}
