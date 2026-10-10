use super::*;

#[test]
fn envelope_preserves_mode_metadata_and_relevance() {
    let entry = MemoryEntry::new(
        MemoryId::now_v7(),
        10,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        "legacy fact",
        MemorySource::User,
    )
    .unwrap();
    let result = MemorySearchResult {
        mode: MemoryRetrievalMode::ExplicitSearch,
        hits: vec![MemorySearchHit {
            entry,
            location: MemoryLocation::Archive,
            outdated: true,
            ttl_expired: true,
            superseded_by: None,
            relevance: Some(0.75),
        }],
    };
    assert_eq!(result.mode, MemoryRetrievalMode::ExplicitSearch);
    assert_eq!(result.hits[0].location, MemoryLocation::Archive);
    assert_eq!(result.hits[0].relevance, Some(0.75));
}

// ---------- MemoryEventAppendPort ----------

use crate::domain::event::*;
use crate::noop::NoopEventAppend;

/// 构造一条带指定 id 的样例事件；正文字段故意填入“内容”，用于验证
/// [`EventAppendError`] 的展示文案绝不携带记忆正文。
fn append_sample_event(event_id: &str) -> MemoryEvent {
    MemoryEvent::new(
        event_id,
        1_760_000_000_123,
        MemoryEventOp::WriteAdd,
        EventOutcome::Succeeded,
        "corr-append-01",
        EventActor::Service,
        EventChange::Write {
            before: vec![],
            after: vec![],
        },
        EventContext {
            query: None,
            trigger_summary: Some("write 命令提交成功".to_string()),
            coverage_range: None,
        },
        ConfigFingerprint {
            scoring_enabled: true,
            reflection_model: None,
            reflection_model_revision: None,
            similarity_threshold: None,
            inject_token_budget: None,
            event_retention_days: Some(30),
        },
    )
}

#[tokio::test]
async fn event_append_noop_succeeds_without_side_effects() {
    let noop_port: Arc<dyn MemoryEventAppendPort> = Arc::new(NoopEventAppend);
    let event = append_sample_event("evt-noop");
    assert_eq!(noop_port.append(&event).await, Ok(()));
    // 空实现无状态：重复 append 依旧成功，不记录任何内容。
    assert_eq!(noop_port.append(&event).await, Ok(()));
}

#[tokio::test]
async fn event_append_recording_pushes_a_clone_of_each_event() {
    let recorder = RecordingEventAppend::default();
    let port: Arc<dyn MemoryEventAppendPort> = Arc::new(recorder.clone());
    let mut first = append_sample_event("evt-first");
    let second = append_sample_event("evt-second");
    port.append(&first)
        .await
        .expect("recording append never fails");
    port.append(&second)
        .await
        .expect("recording append never fails");
    // 录制的是事件克隆：调用方事后改动原事件，不影响已录制的记录。
    let expected = vec![first.clone(), second.clone()];
    first.correlation_id = "mutated-after-append".to_string();
    assert_eq!(recorder.events(), expected);
}

#[test]
fn event_append_error_display_never_carries_memory_body() {
    // 固定文案断言：错误变体没有任何 payload 字段；若有人开始往错误里
    // 携带记忆正文，这些精确相等断言会先失败。
    assert_eq!(
        EventAppendError::Rejected.to_string(),
        "event append rejected by sink"
    );
    assert_eq!(
        EventAppendError::Unavailable.to_string(),
        "event sink is unavailable"
    );
    assert_eq!(
        EventAppendError::Serialization.to_string(),
        "event serialization failed"
    );
    assert_eq!(
        EventAppendError::Io.to_string(),
        "I/O failure while appending event"
    );
}
