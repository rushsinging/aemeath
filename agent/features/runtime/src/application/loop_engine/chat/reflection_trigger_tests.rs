use super::reflection::should_run_turn_reflection;

fn enabled_config(interval_runs: usize) -> share::config::MemoryConfig {
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_runs = interval_runs;
    config
}

#[test]
fn turn_reflection_requires_enabled_interval_finish_boundary() {
    let config = enabled_config(2);

    assert!(should_run_turn_reflection(&config, 2));
    assert!(!should_run_turn_reflection(&config, 1));

    let mut memory_disabled = config.clone();
    memory_disabled.enabled = false;
    assert!(!should_run_turn_reflection(&memory_disabled, 2));

    let mut reflection_disabled = config.clone();
    reflection_disabled.reflection.enabled = false;
    assert!(!should_run_turn_reflection(&reflection_disabled, 2));

    let zero_interval = enabled_config(0);
    assert!(!should_run_turn_reflection(&zero_interval, 2));
}

// ── 反思游标（#1827）：增量切片、游标读取、材料槽 ─────────────────

use super::reflection::{
    latest_coverage_cursor, slice_increment_since_cursor, IntervalReflectionMaterialSlot,
};

fn msg(text: &str) -> share::message::Message {
    share::message::Message::user(text)
}

#[test]
fn slice_increment_since_cursor_valid_slices_from_cursor() {
    let messages = vec![msg("m0"), msg("m1"), msg("m2"), msg("m3")];
    let sliced = slice_increment_since_cursor(&messages, Some(2)).expect("有效游标必须切片");
    assert_eq!(sliced.len(), 2);
    assert_eq!(sliced[0].text_content(), "m2");
    assert_eq!(sliced[1].text_content(), "m3");
}

#[test]
fn slice_increment_since_cursor_zero_is_full_history() {
    let messages = vec![msg("m0"), msg("m1")];
    let sliced = slice_increment_since_cursor(&messages, Some(0)).expect("游标 0 = 全量");
    assert_eq!(sliced.len(), 2);
}

#[test]
fn slice_increment_since_cursor_missing_or_stale_returns_none() {
    let messages = vec![msg("m0"), msg("m1")];
    assert!(
        slice_increment_since_cursor(&messages, None).is_none(),
        "无游标（首次/旧记录）→ None（调用方回退）"
    );
    assert!(
        slice_increment_since_cursor(&messages, Some(5)).is_none(),
        "游标超出当前历史长度（compact 截断后失效）→ None（调用方回退）"
    );
    let empty = slice_increment_since_cursor(&messages, Some(2))
        .expect("游标=末尾 → 空增量切片（无新内容）");
    assert!(empty.is_empty(), "空增量切片: {empty:?} 条");
}

#[tokio::test]
async fn latest_coverage_cursor_picks_newest_succeeded_with_cursor() {
    use memory::api::reflection::{ReflectionErrorCategory, ReflectionRecord, ReflectionTrigger};

    use memory::api::ReflectionHistoryStore;
    use std::sync::Arc;
    let store: Arc<dyn ReflectionHistoryStore> =
        Arc::new(crate::application::reflection::test_support::RecordingHistory::default());
    // 旧→新：无游标 Succeeded、带游标 Succeeded(10)、带游标 Failed（不得采用）、
    // 最新 Succeeded 带游标(20)
    let mut r1 = ReflectionRecord::running("r1", 1, ReflectionTrigger::Interval);
    r1.status = memory::api::reflection::ReflectionStatus::Succeeded;
    let mut r2 = ReflectionRecord::running("r2", 2, ReflectionTrigger::Interval);
    r2.status = memory::api::reflection::ReflectionStatus::Succeeded;
    r2.coverage_end = Some(10);
    let r3 = ReflectionRecord::failed(
        "r3",
        3,
        ReflectionTrigger::Manual,
        ReflectionErrorCategory::LlmCall,
        1,
    );
    let mut r4 = ReflectionRecord::running("r4", 4, ReflectionTrigger::Manual);
    r4.status = memory::api::reflection::ReflectionStatus::Succeeded;
    r4.coverage_end = Some(20);
    for record in [r1, r2, r3, r4] {
        store.upsert(&record).await.unwrap();
    }

    assert_eq!(latest_coverage_cursor(&store).await, Some(20));
}

#[tokio::test]
async fn latest_coverage_cursor_without_any_cursor_is_none() {
    use memory::api::reflection::{ReflectionRecord, ReflectionTrigger};

    use memory::api::ReflectionHistoryStore;
    use std::sync::Arc;
    let store: Arc<dyn ReflectionHistoryStore> =
        Arc::new(crate::application::reflection::test_support::RecordingHistory::default());
    let mut record = ReflectionRecord::running("r1", 1, ReflectionTrigger::Manual);
    record.status = memory::api::reflection::ReflectionStatus::Succeeded;
    store.upsert(&record).await.unwrap();

    assert_eq!(latest_coverage_cursor(&store).await, None);
}

#[test]
fn interval_material_slot_stage_and_take() {
    let slot = IntervalReflectionMaterialSlot::default();
    assert!(slot.take().is_none(), "空槽取出为 None");
    slot.stage(vec![msg("a"), msg("b")], 10);
    let material = slot.take().expect("装槽后必须取出");
    assert_eq!(material.messages.len(), 2);
    assert_eq!(material.history_len_at_stage, 10);
    assert!(slot.take().is_none(), "取出后槽必须清空（一次性消费）");
}
