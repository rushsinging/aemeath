use crate::adapters::{InMemoryMemory, MemoryPolicy};
use crate::application::*;
use crate::{domain::*, ports::*};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

const NORMAL_RESPONSE: &str = include_str!("../tests/fixtures/reflection/synthesis_normal.json");
const NOOP_RESPONSE: &str = include_str!("../tests/fixtures/reflection/synthesis_noop.json");
const INVALID_REFERENCE_RESPONSE: &str =
    include_str!("../tests/fixtures/reflection/synthesis_invalid_reference.json");
const CAPACITY_BREACH_RESPONSE: &str =
    include_str!("../tests/fixtures/reflection/synthesis_capacity_breach.json");
const SUPERSEDE_CYCLE_RESPONSE: &str =
    include_str!("../tests/fixtures/reflection/synthesis_supersede_cycle.json");

#[derive(Default)]
struct GoldenHistory {
    records: Mutex<Vec<ReflectionRecord>>,
}

#[async_trait]
impl ReflectionHistoryQuery for GoldenHistory {
    async fn list(&self, limit: usize) -> Result<Vec<ReflectionSafeSummary>, MemoryError> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .iter()
            .rev()
            .take(limit)
            .map(ReflectionRecord::safe_summary)
            .collect())
    }
}

#[async_trait]
impl ReflectionHistoryStore for GoldenHistory {
    async fn append(&self, record: &ReflectionRecord) -> Result<(), crate::domain::MemoryError> {
        self.records.lock().unwrap().push(record.clone());
        Ok(())
    }
    async fn upsert(&self, record: &ReflectionRecord) -> Result<(), crate::domain::MemoryError> {
        let mut records = self.records.lock().unwrap();
        if let Some(existing) = records.iter_mut().find(|item| item.id == record.id) {
            *existing = record.clone();
        } else {
            records.push(record.clone());
        }
        Ok(())
    }
}

/// 事件透传包装：让 golden 断言可读取事件流终态。
struct MemoryWithEvents {
    inner: InMemoryMemory,
    events: Arc<dyn MemoryEventAppendPort>,
    recorder: Arc<RecordingEventAppend>,
}

#[async_trait]
impl MemoryPort for MemoryWithEvents {
    async fn retrieve_for_inject(&self, query: &MemoryQuery) -> MemorySearchResult {
        self.inner.retrieve_for_inject(query).await
    }
    async fn search(&self, query: &MemorySearchQuery) -> MemorySearchResult {
        self.inner.search(query).await
    }
    async fn write(&self, entry: MemoryEntry) -> Result<WriteResult, MemoryError> {
        self.inner.write(entry).await
    }
    async fn update(&self, id: &MemoryId, content: &str) -> Result<bool, MemoryError> {
        self.inner.update(id, content).await
    }
    async fn delete(&self, id: &MemoryId) -> Result<bool, MemoryError> {
        self.inner.delete(id).await
    }
    async fn pin(&self, id: &MemoryId, pinned: bool) -> Result<bool, MemoryError> {
        self.inner.pin(id, pinned).await
    }
    async fn mark_outdated(&self, id: &MemoryId) -> Result<bool, MemoryError> {
        self.inner.mark_outdated(id).await
    }
    async fn apply_reflection(
        &self,
        output: &ReflectionOutput,
    ) -> Result<ReflectionApplyResult, MemoryError> {
        self.inner.apply_reflection(output).await
    }
    async fn archive(&self, ids: &[MemoryId]) -> Result<bool, MemoryError> {
        self.inner.archive(ids).await
    }
    async fn restore(&self, id: &MemoryId) -> Result<RestoreResult, MemoryError> {
        self.inner.restore(id).await
    }
    async fn compact(&self) -> Result<CompactResult, MemoryError> {
        self.inner.compact().await
    }
    async fn list(&self, layer: Option<MemoryLayer>) -> Vec<MemoryEntry> {
        self.inner.list(layer).await
    }
    async fn stats(&self) -> MemoryStats {
        self.inner.stats().await
    }
    fn event_append_port(&self) -> Option<Arc<dyn MemoryEventAppendPort>> {
        Some(Arc::clone(&self.events))
    }
}

fn golden_entry(content: &str) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        100,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        content,
        MemorySource::User,
    )
    .expect("golden entry")
}

async fn golden_memory(entries: &[&str]) -> MemoryWithEvents {
    let inner = InMemoryMemory::new(MemoryPolicy {
        max_entries: 200,
        similarity_threshold: 0.8,
        target_active_entries: 150,
        default_ttl_secs: 0,
    })
    .expect("golden policy");
    for content in entries {
        inner
            .write(golden_entry(content))
            .await
            .expect("golden write");
    }
    let recorder = Arc::new(RecordingEventAppend::default());
    let events: Arc<dyn MemoryEventAppendPort> = Arc::clone(&recorder) as _;
    MemoryWithEvents {
        inner,
        events,
        recorder,
    }
}

fn golden_identity() -> ReflectionExecutionIdentity {
    ReflectionExecutionIdentity {
        id: "golden-reflection".to_string(),
        timestamp: 42,
        trigger: ReflectionTrigger::Manual,
        coverage_end: None,
    }
}

async fn run_golden(
    corpus: &[&str],
    response: &str,
    _stale_swept: u32,
) -> (
    crate::application::ReflectionExecutionResult,
    ReflectionRecord,
    MemoryWithEvents,
    Arc<RecordingEventAppend>,
) {
    let memory = golden_memory(corpus).await;
    let port: &dyn MemoryPort = &memory;
    let entries = port.list(None).await;
    let references = crate::domain::ReflectionReferenceTable::from_entries(&entries);
    let history = GoldenHistory::default();
    let result = ReflectionWorkflow::complete(
        &history,
        port,
        &golden_identity(),
        response,
        &references,
        "en",
        true,
        ReflectionTokenUsage {
            input_tokens: 10,
            output_tokens: 5,
        },
        9,
        None,
    )
    .await
    .expect("golden complete");
    let record = history
        .records
        .lock()
        .unwrap()
        .last()
        .cloned()
        .expect("record persisted");
    let events = memory.recorder.clone();
    (result, record, memory, events)
}

#[tokio::test]
async fn golden_synthesis_merges_and_marks_outdated_with_capacity_fields() {
    let (result, record, memory, events) = run_golden(
        &["部署目标在旧集群 A（K1）", "旧服务端口清单（K2）"],
        NORMAL_RESPONSE,
        0,
    )
    .await;

    assert_eq!(result.error_category, None);
    let apply = record.apply_result.as_ref().expect("apply result");
    assert_eq!(apply.suggestions_added, 1, "归纳建议写入");
    assert_eq!(apply.outdated_marked, 1, "M2 过期标记");
    assert_eq!(apply.superseded, 1, "M1 取代关系建立");
    assert_eq!(apply.active_after, Some(1), "合并后有效 active = 1");
    assert_eq!(apply.capacity_target, Some(150));
    assert_eq!(record.stale_swept, Some(0));
    // superseded 的 M1 物理留在 active（关系不迁移），有效计数由 active_after 表达。
    assert_eq!(memory.inner.stats().await.project_count, 2);
    // 事件链终态：cost 事件携带容量注脚（active_after/target）。
    let appended = events.events();
    let cost_with_capacity = appended
        .iter()
        .filter(|event| event.op == crate::domain::event::MemoryEventOp::ReflectionCost)
        .any(|event| {
            matches!(
                &event.change,
                crate::domain::event::EventChange::Reflection { summary, .. }
                    if summary.contains("active_after=1") && summary.contains("target=150")
            )
        });
    assert!(cost_with_capacity, "cost 事件 MUST 携带容量注脚");
}

#[tokio::test]
async fn golden_noop_reports_zero_changes_without_failure() {
    let (_result, record, memory, _events) =
        run_golden(&["稳定事实，无需归纳"], NOOP_RESPONSE, 0).await;

    assert_eq!(record.status, crate::domain::ReflectionStatus::Succeeded);
    let apply = record.apply_result.as_ref().expect("apply result");
    assert_eq!(apply.attempted, 0);
    assert_eq!(apply.suggestions_added, 0);
    assert_eq!(memory.inner.stats().await.project_count, 1, "盘面不变");
}

#[tokio::test]
async fn golden_invalid_reference_skips_only_bad_token() {
    let (_result, record, memory, _events) = run_golden(
        &["会被合法过期标记的条目（M2）"],
        INVALID_REFERENCE_RESPONSE,
        0,
    )
    .await;

    let apply = record.apply_result.as_ref().expect("apply result");
    assert_eq!(apply.suggestions_added, 1, "合法建议照常写入");
    assert_eq!(apply.outdated_marked, 1, "合法 M2 过期生效");
    // 非法 slug 不计入 attempted。
    assert_eq!(apply.attempted, 2);
    assert_eq!(memory.inner.stats().await.project_count, 1);
}

#[tokio::test]
async fn golden_capacity_breach_fails_whole_batch_without_writing() {
    let memory = InMemoryMemory::new(MemoryPolicy {
        max_entries: 10,
        similarity_threshold: 0.8,
        target_active_entries: 1,
        default_ttl_secs: 0,
    })
    .expect("golden policy");
    for content in ["占位事实一", "占位事实二"] {
        memory.write(golden_entry(content)).await.unwrap();
    }
    let port: &dyn MemoryPort = &memory;
    let references = crate::domain::ReflectionReferenceTable::from_entries(&port.list(None).await);
    let history = GoldenHistory::default();
    let outcome = ReflectionWorkflow::complete(
        &history,
        port,
        &golden_identity(),
        CAPACITY_BREACH_RESPONSE,
        &references,
        "en",
        true,
        ReflectionTokenUsage {
            input_tokens: 3,
            output_tokens: 2,
        },
        7,
        None,
    )
    .await;

    let result = outcome.expect("CapacityNotMet 记录为 record 终态而非 Err");
    assert_eq!(
        result.error_category,
        Some(crate::domain::ReflectionErrorCategory::CapacityNotMet)
    );
    let record = history.records.lock().unwrap().last().cloned().unwrap();
    assert_eq!(record.status, crate::domain::ReflectionStatus::Failed);
    assert!(record.apply_result.is_none(), "整批不写，无 apply 结果");
    assert_eq!(memory.stats().await.project_count, 2, "盘面保持不变");
}

#[tokio::test]
async fn golden_supersede_missing_reference_rejected_but_suggestion_lands() {
    // supersede 引用不存在的行号（M9 越界）：该关系拒写不计数，建议本体
    // 照常落盘；attempted-completed 保留诚实差值（M9 环检测同理由
    // service 层单测覆盖，此处验证 workflow 全链路对坏引用的韧性）。
    let (_result, record, memory, _events) =
        run_golden(&["事实一（M1）"], SUPERSEDE_CYCLE_RESPONSE, 0).await;

    let apply = record.apply_result.as_ref().expect("apply result");
    assert_eq!(apply.suggestions_added, 1, "建议本体写入");
    assert_eq!(
        apply.superseded, 1,
        "坏引用（M2 越界）跳过，好引用（M1）照常建立"
    );
    assert_eq!(memory.inner.stats().await.project_count, 2);
}
