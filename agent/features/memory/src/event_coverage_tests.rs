//! 19 项 MemoryEventOp 映射表契约：每个 op 至少被一条生产路径 emit 测试覆盖。
//! 本文件是「口头约定禁止」的固化点——增减 op 必须同步改 COVERED_OPS。

use crate::domain::event::MemoryEventOp;

/// (op, 覆盖该 op 的测试函数名)。长度必须为 19，且与 serde 映射表全集一致。
const COVERED_OPS: &[(MemoryEventOp, &str)] = &[
    (
        MemoryEventOp::RetrieveForInject,
        "event_read_ops_retrieve_for_inject_emits_candidates_with_content",
    ),
    (
        MemoryEventOp::Search,
        "event_read_ops_search_emits_candidates_with_content",
    ),
    (
        MemoryEventOp::PerMessageRecall,
        "event_read_ops_per_message_recall_emits_candidates_with_content",
    ),
    (
        MemoryEventOp::ListStats,
        "event_read_ops_list_and_stats_emit_list_stats",
    ),
    (
        MemoryEventOp::WriteAdd,
        "write_add_emits_event_through_injected_append_port",
    ),
    (
        MemoryEventOp::Update,
        "event_write_ops_update_emits_before_and_after_content",
    ),
    (
        MemoryEventOp::Delete,
        "event_write_ops_delete_emits_full_entry_in_before",
    ),
    (
        MemoryEventOp::Pin,
        "event_write_ops_pin_emits_before_and_after_for_pin_and_unpin",
    ),
    (
        MemoryEventOp::MarkOutdated,
        "event_write_ops_mark_outdated_emits_before_and_after_content",
    ),
    (
        MemoryEventOp::ArchiveRestore,
        "event_write_ops_archive_and_restore_emit_distinct_stages",
    ),
    (
        MemoryEventOp::Compact,
        "event_write_ops_compact_emits_affected_full_text",
    ),
    (
        MemoryEventOp::SupersedeSynthesis,
        "event_write_ops_synthesis_write_emits_full_entry_via_apply_reflection",
    ),
    (
        MemoryEventOp::ReflectionTriggered,
        "event_reflection_ops_complete_emits_trigger_apply_cost",
    ),
    (
        MemoryEventOp::ReflectionApplied,
        "event_reflection_ops_complete_emits_trigger_apply_cost",
    ),
    (
        MemoryEventOp::ReflectionCost,
        "event_reflection_ops_complete_emits_trigger_apply_cost",
    ),
    (
        MemoryEventOp::OpenLoad,
        "event_lifecycle_open_load_covers_both_layers_with_count_summary",
    ),
    (
        MemoryEventOp::CommitCas,
        "event_lifecycle_commit_cas_emits_revision_and_layer",
    ),
    (
        MemoryEventOp::AssemblyFingerprint,
        "event_lifecycle_assembly_fingerprint_emits_on_open",
    ),
    (
        MemoryEventOp::EvictionWatermark,
        "event_lifecycle_eviction_watermark_emits_candidate_entries",
    ),
];

#[test]
fn event_coverage_locks_all_nineteen_ops() {
    assert_eq!(COVERED_OPS.len(), 19, "映射表必须正好 19 项");
    let mut seen = std::collections::HashSet::new();
    for (op, test_name) in COVERED_OPS {
        assert!(!test_name.is_empty());
        assert!(seen.insert(*op), "op {op:?} 重复出现在 COVERED_OPS");
    }
    let all = [
        MemoryEventOp::RetrieveForInject,
        MemoryEventOp::Search,
        MemoryEventOp::PerMessageRecall,
        MemoryEventOp::ListStats,
        MemoryEventOp::WriteAdd,
        MemoryEventOp::Update,
        MemoryEventOp::Delete,
        MemoryEventOp::Pin,
        MemoryEventOp::MarkOutdated,
        MemoryEventOp::ArchiveRestore,
        MemoryEventOp::Compact,
        MemoryEventOp::SupersedeSynthesis,
        MemoryEventOp::ReflectionTriggered,
        MemoryEventOp::ReflectionApplied,
        MemoryEventOp::ReflectionCost,
        MemoryEventOp::OpenLoad,
        MemoryEventOp::CommitCas,
        MemoryEventOp::AssemblyFingerprint,
        MemoryEventOp::EvictionWatermark,
    ];
    assert_eq!(all.len(), 19);
    for op in all {
        assert!(seen.contains(&op), "COVERED_OPS 缺少 {op:?}");
    }
}

#[test]
fn event_coverage_referenced_tests_exist_in_sources() {
    let sources = [
        include_str!("service_tests.rs"),
        include_str!("application_tests.rs"),
        include_str!("application/recall_tests.rs"),
    ]
    .concat();
    for (op, test_name) in COVERED_OPS {
        assert!(
            sources.contains(test_name),
            "覆盖 {op:?} 的测试 `{test_name}` 未在源中找到"
        );
    }
}
