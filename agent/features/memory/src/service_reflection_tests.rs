use super::{
    tests::{LayerScript, ScriptedStore},
    MemoryService,
};
use crate::adapters::{InMemoryMemory, MemoryPolicy};
use crate::{domain::*, ports::*};

fn layer_script(
    loads: Vec<Result<CommittedMemoryDataset<u64>, MemoryError>>,
    commits: Vec<Result<MemoryCommitReceipt<u64>, MemoryError>>,
) -> LayerScript {
    LayerScript {
        loads: loads.into(),
        commits: commits.into(),
        ..LayerScript::default()
    }
}

fn empty_layer(revision: u64, layer: MemoryLayer) -> CommittedMemoryDataset<u64> {
    CommittedMemoryDataset {
        dataset: MemoryDataset::empty(layer),
        revision,
    }
}

fn committed(
    revision: u64,
    layer: MemoryLayer,
    entries: Vec<MemoryEntry>,
) -> CommittedMemoryDataset<u64> {
    CommittedMemoryDataset {
        dataset: MemoryDataset::new(layer, entries, vec![]).unwrap(),
        revision,
    }
}

fn receipt(revision: u64) -> MemoryCommitReceipt<u64> {
    MemoryCommitReceipt::new(revision, MemoryCommitVisibility::Visible)
}

fn storage_error() -> MemoryError {
    MemoryError::Storage {
        kind: crate::domain::MemoryStorageErrorKind::Io,
    }
}

fn entry(layer: MemoryLayer, content: &str) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        100,
        layer,
        MemoryCategory::Fact,
        content,
        MemorySource::User,
    )
    .unwrap()
}

fn suggestion(layer: MemoryLayer, content: &str) -> MemorySuggestion {
    MemorySuggestion {
        layer,
        category: MemoryCategory::Fact,
        content: content.to_string(),
        tags: vec!["reflection".to_string()],
        reason: "test".to_string(),
        supersedes: vec![],
        synthesizes: Vec::new(),
    }
}

#[tokio::test]
async fn reflection_partial_apply_reports_committed_suggestion_before_outdated_write_failure() {
    let existing = entry(MemoryLayer::Project, "obsolete fact");
    let existing_id = existing.id;
    // list() 现读磁盘：suggestion 已提交、outdated 标记提交失败，
    // 磁盘保持 suggestion 提交后的状态（existing 未被标 outdated）。
    let current = entry(MemoryLayer::Project, "current fact");
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 2], vec![]),
        layer_script(
            vec![
                Ok(committed(1, MemoryLayer::Project, vec![existing.clone()])),
                Ok(committed(
                    2,
                    MemoryLayer::Project,
                    vec![existing.clone(), current],
                )),
            ],
            vec![Ok(receipt(2)), Err(storage_error())],
        ),
    );
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), || 200)
        .await
        .unwrap();

    let error = service
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion(MemoryLayer::Project, "current fact")],
            outdated_memories: vec![existing_id.to_string()],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        MemoryError::PartialApply {
            result_attempted: 2,
            result_completed: 1,
            suggestions_added: 1,
            outdated_marked: 0,
            superseded: 0,
        }
    ));
    let entries = service.list(Some(MemoryLayer::Project)).await;
    assert!(entries.iter().any(|entry| entry.content == "current fact"));
    assert!(entries
        .iter()
        .any(|entry| entry.id == existing_id && !entry.outdated));
}

#[tokio::test]
async fn retrieve_for_inject_reads_committed_memory_without_write() {
    let stored = entry(MemoryLayer::Project, "read only fact");
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 2], vec![]),
        layer_script(
            vec![Ok(committed(1, MemoryLayer::Project, vec![stored.clone()])); 2],
            vec![],
        ),
    );
    let observer = store.clone();
    let service = MemoryService::open(store, MemoryPolicy::default())
        .await
        .unwrap();

    let result = service
        .retrieve_for_inject(&crate::ports::MemoryQuery {
            limit: 1,
            layer: Some(MemoryLayer::Project),
            category: None,
            now: 200,
        })
        .await;

    assert_eq!(
        result.mode,
        crate::ports::MemoryRetrievalMode::InjectionPriority
    );
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].entry, stored);
    // open 1 次 + retrieve_for_inject 现读 1 次；读路径零提交。
    assert_eq!(
        observer.calls(MemoryLayer::Project),
        (2, 0),
        "injection retrieval must not write the committed project layer"
    );
}

#[tokio::test]
async fn apply_reflection_skips_invalid_outdated_reference_without_failing_batch() {
    // 复现修复前的 apply 全批失败：outdated_memories 携带非法引用
    // （真实事故形态：模型把标签 slug 或行格式当作 memory id）。
    // 合法的新建议 MUST 照常写入，非法引用 MUST 跳过并记录，NEVER 整批丢弃。
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global))], vec![]),
        layer_script(
            vec![Ok(committed(1, MemoryLayer::Project, vec![]))],
            vec![Ok(receipt(2))],
        ),
    );
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), || 200)
        .await
        .unwrap();

    let result = service
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion(MemoryLayer::Project, "有效的新记忆")],
            outdated_memories: vec!["some-tag-slug".to_string()],
            ..ReflectionOutput::default()
        })
        .await
        .expect("非法引用 MUST 跳过而不是整批失败");

    assert_eq!(result.suggestions_added, 1, "合法建议 MUST 写入");
    assert_eq!(result.outdated_marked, 0, "非法引用 MUST 跳过");
    // 跳过的引用不计入 attempted（没有实际尝试的操作）。
    assert_eq!(result.attempted, 1);
    assert_eq!(result.completed, 1);
}

#[tokio::test]
async fn apply_reflection_missing_outdated_target_is_skipped_without_failure() {
    // 引用在解析时有效、apply 时目标已不存在（反射运行期间被删除/变更）：
    // 跳过并记录，NEVER 失败整批。
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global))], vec![]),
        layer_script(vec![Ok(committed(1, MemoryLayer::Project, vec![]))], vec![]),
    );
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), || 200)
        .await
        .unwrap();

    let missing = MemoryId::now_v7();
    let result = service
        .apply_reflection(&ReflectionOutput {
            outdated_memories: vec![missing.to_string()],
            ..ReflectionOutput::default()
        })
        .await
        .expect("目标缺失 MUST 跳过而不是失败");

    assert_eq!(result.outdated_marked, 0);
    assert_eq!(result.attempted, 1);
    assert_eq!(result.completed, 1);
}

#[tokio::test]
async fn apply_reflection_rejects_when_above_target_without_writing() {
    // target=2、hard_cap=10：active=3 且未建议淘汰 → CapacityNotMet，盘面不变，NEVER 机械硬删。
    let memory = InMemoryMemory::new(MemoryPolicy {
        max_entries: 10,
        similarity_threshold: 0.8,
        target_active_entries: 2,
    })
    .expect("policy");
    for content in ["keep-1", "keep-2", "keep-3"] {
        memory
            .write(entry(MemoryLayer::Project, content))
            .await
            .expect("write");
    }
    assert_eq!(memory.stats().await.project_count, 3);

    let error = memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion(MemoryLayer::Project, "extra")],
            ..ReflectionOutput::default()
        })
        .await
        .expect_err("超 target 且未收敛 MUST CapacityNotMet");
    assert_eq!(
        error,
        MemoryError::CapacityNotMet {
            active: 4,
            target: 2,
        }
    );
    assert_eq!(
        memory.stats().await.project_count,
        3,
        "CapacityNotMet 时整批不写"
    );
}

#[tokio::test]
async fn apply_reflection_commits_when_outdated_archives_down_to_target() {
    // outdated_memories 在 reflect apply 中 MUST 归档出 active，才能把物理条数压到 ≤target。
    let memory = InMemoryMemory::new(MemoryPolicy {
        max_entries: 10,
        similarity_threshold: 0.8,
        target_active_entries: 2,
    })
    .expect("policy");
    let mut ids = Vec::new();
    for content in ["keep-1", "drop-a", "drop-b"] {
        let written = entry(MemoryLayer::Project, content);
        ids.push(written.id);
        memory.write(written).await.expect("write");
    }
    let result = memory
        .apply_reflection(&ReflectionOutput {
            outdated_memories: vec![ids[1].to_string(), ids[2].to_string()],
            ..ReflectionOutput::default()
        })
        .await
        .expect("收敛到 target 应成功");
    assert_eq!(result.outdated_marked, 2);
    let stats = memory.stats().await;
    assert_eq!(stats.project_count, 1);
    assert_eq!(stats.project_archive_count, 2);
}

#[test]
fn reflect_target_active_entries_is_three_quarters_of_max() {
    assert_eq!(reflect_target_active_entries(200), 150);
    assert_eq!(reflect_target_active_entries(10), 7);
    assert_eq!(reflect_target_active_entries(0), 0);
}
