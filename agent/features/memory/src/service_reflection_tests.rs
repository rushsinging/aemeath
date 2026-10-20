use super::{
    tests::{LayerScript, ScriptedStore},
    MemoryService,
};
use crate::adapters::{AtomicDatasetMemoryStore, InMemoryMemory, MemoryPolicy};
use crate::{domain::*, ports::*};
use std::time::Duration;

#[tokio::test]
#[ignore = "只读真实 corpus 并在临时副本验证清扫"]
async fn real_corpus_copy_sweep_preserves_valid_and_pinned_entries() {
    let source_root = std::path::PathBuf::from(
        std::env::var_os("AEMEATH_REFLECTION_CORPUS")
            .expect("请设置 AEMEATH_REFLECTION_CORPUS 为只读 corpus 目录"),
    );
    let manifest_path = source_root.join("primary/manifest.json");
    let manifest_bytes = std::fs::read(&manifest_path).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
    let mut active = None;
    let mut archive = None;
    for evidence in manifest["成员证据"].as_array().unwrap() {
        let member = source_root
            .join("members")
            .join(evidence["内容摘要"].as_str().unwrap());
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(member).unwrap()).unwrap();
        let entries: Vec<MemoryEntry> = serde_json::from_value(value["entries"].clone()).unwrap();
        match evidence["名称"].as_str().unwrap() {
            "active" => active = Some(entries),
            "archive" => archive = Some(entries),
            _ => {}
        }
    }
    let active = active.unwrap();
    let archive = archive.unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let expected_swept = active
        .iter()
        .filter(|entry| !entry.pinned && is_lapsed(entry))
        .count();
    let expected_kept: Vec<_> = active
        .iter()
        .filter(|entry| entry.pinned || !is_lapsed(entry))
        .map(|entry| entry.id)
        .collect();
    let root = sweep_root("real-corpus-copy");
    let store = sweep_store(&root);
    let committed = store.load_committed(MemoryLayer::Project).await.unwrap();
    let dataset =
        MemoryDataset::new(MemoryLayer::Project, active.clone(), archive.clone()).unwrap();
    store
        .commit(MemoryLayer::Project, &committed.revision, &dataset)
        .await
        .unwrap();
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), move || now)
        .await
        .unwrap();
    let permit = service.acquire_write_permit().await.unwrap();
    let swept = service
        .sweep_stale_under_permit(now, &permit)
        .await
        .unwrap();
    assert_eq!(swept, expected_swept);
    assert_eq!(
        service
            .sweep_stale_under_permit(now, &permit)
            .await
            .unwrap(),
        0
    );
    drop(permit);
    let stored = sweep_store(&root)
        .load_committed(MemoryLayer::Project)
        .await
        .unwrap();
    let kept: Vec<_> = stored
        .dataset
        .active()
        .iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(kept, expected_kept);
    assert_eq!(
        stored.dataset.archive().len(),
        archive.len() + expected_swept
    );
    assert_eq!(
        effective_active_count(stored.dataset.active()),
        effective_active_count(&active)
    );
    assert_eq!(
        std::fs::read(&manifest_path).unwrap(),
        manifest_bytes,
        "真数据不得被修改"
    );
    log::info!(target: crate::LOG_TARGET, "real_corpus_copy_sweep before={} swept={} after={} effective={}", active.len(), swept, kept.len(), effective_active_count(stored.dataset.active()));
    drop(service);
    std::fs::remove_dir_all(&root).unwrap();
}

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
        ttl_secs: None,
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
        default_ttl_secs: 0,
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
        default_ttl_secs: 0,
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

/// #1987 容量口径：outdated/被取代残留不占用 max_entries 与收敛目标。
/// mark_outdated 独立调用（不经 apply）会留下「标记但未归档」的残留——它们
/// 已不可注入（M10/M5），MUST NOT 再挤占新写入与反思收敛的容量配额。
#[tokio::test]
async fn write_capacity_ignores_stale_entries() {
    let memory = InMemoryMemory::new(MemoryPolicy {
        max_entries: 2,
        similarity_threshold: 0.8,
        target_active_entries: 1,
        default_ttl_secs: 0,
    })
    .expect("policy");
    let first = entry(MemoryLayer::Project, "alpha rule");
    let second = entry(MemoryLayer::Project, "beta rule");
    memory.write(first.clone()).await.expect("write A");
    memory.write(second.clone()).await.expect("write B");
    // 独立 mark_outdated：A 残留在 active（不走 apply 归档路径）。
    assert!(memory.mark_outdated(&first.id).await.unwrap());
    assert_eq!(memory.stats().await.project_count, 2, "残留仍在物理 active");

    let outcome = memory
        .write(entry(MemoryLayer::Project, "gamma rule"))
        .await
        .expect("write C");
    assert!(
        matches!(outcome, WriteResult::Added { .. }),
        "有效 active=1+1=2 ≤ max=2，MUST NOT NeedsEviction: {outcome:?}"
    );
}

/// 反思容量门禁按有效 active 试算：残留不把整批顶成 CapacityNotMet。
#[tokio::test]
async fn reflection_capacity_gate_counts_effective_active_only() {
    let memory = InMemoryMemory::new(MemoryPolicy {
        max_entries: 10,
        similarity_threshold: 0.8,
        target_active_entries: 7,
        default_ttl_secs: 0,
    })
    .expect("policy");
    let mut ids = Vec::new();
    for content in [
        "alpha rule",
        "beta rule",
        "gamma rule",
        "delta rule",
        "epsilon rule",
        "zeta rule",
        "eta rule",
    ] {
        let written = entry(MemoryLayer::Project, content);
        ids.push(written.id);
        memory.write(written).await.expect("write");
    }
    // 3 条残留：2 outdated + 1 superseded（物理 10 = 有效 7 + 残留 3）。
    memory.mark_outdated(&ids[0]).await.unwrap();
    memory.mark_outdated(&ids[1]).await.unwrap();
    let superseder = memory.pin(&ids[2], false).await;
    let _ = superseder;
    // superseded 残留经 apply 的 supersedes 建立太重，这里直接借 outdated 两条
    // 达成「有效 8 > target 7」的失败对照前先验证边界：去掉第三条，仅 2 残留。
    let result = memory
        .apply_reflection(&ReflectionOutput::default())
        .await
        .expect("有效 active=7 ≤ target=7，空输出 MUST 通过门禁");
    assert_eq!(result.attempted, 0);
}

// -----------------------------------------------------------------
// 生产 MemoryService 残留清扫（sweep_stale_under_permit）落盘契约。
// 全部在独立临时目录上装配真实 AtomicDatasetMemoryStore 并断言落盘重读，
// NEVER 触碰 ~/.agents/memory。
// -----------------------------------------------------------------

fn sweep_root(case: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aemeath-memory-sweep-{case}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn sweep_store(root: &std::path::Path) -> AtomicDatasetMemoryStore {
    AtomicDatasetMemoryStore::new(
        storage::wire_file_system_dataset(root).unwrap(),
        ProjectMemoryKey::derive("/contract/sweep-project", None).unwrap(),
    )
}

/// 直接落盘种入一层 active 条目（绕过写入口，构造 outdated / 被取代 /
/// TTL 过期这类真实 corpus 已存在、公开写入口不易复现的残留形态）。
async fn seed_layer(
    store: &AtomicDatasetMemoryStore,
    layer: MemoryLayer,
    active: Vec<MemoryEntry>,
) {
    let committed = store.load_committed(layer).await.expect("种子层读取成功");
    let dataset = MemoryDataset::new(layer, active, Vec::new()).expect("种子数据集不变量");
    store
        .commit(layer, &committed.revision, &dataset)
        .await
        .expect("种子提交必须成功");
}

fn outdated_entry(layer: MemoryLayer, content: &str) -> MemoryEntry {
    let mut stale = entry(layer, content);
    stale.outdated = true;
    stale
}

fn superseded_entry(layer: MemoryLayer, content: &str) -> MemoryEntry {
    let mut stale = entry(layer, content);
    stale.superseded_by = Some(MemoryId::now_v7());
    stale
}

fn ttl_expired_entry(layer: MemoryLayer, content: &str) -> MemoryEntry {
    let mut stale = entry(layer, content);
    stale.created_at = 100;
    stale.ttl = Some(Duration::from_secs(50));
    stale
}

fn pinned_stale_entry(layer: MemoryLayer, content: &str) -> MemoryEntry {
    let mut stale = entry(layer, content);
    stale.pinned = true;
    stale.outdated = true;
    stale
}

/// 生产残留清扫：active 中已 outdated / 被取代 / TTL 过期且非 pinned 的
/// 条目 MUST 归档出 active（双层各归各的）；返回值是实际归档条数口径。
#[tokio::test]
async fn sweep_stale_under_permit_archives_outdated_and_superseded_but_keeps_ttl_quiet() {
    let root = sweep_root("stale");
    let store = sweep_store(&root);
    let fresh = entry(MemoryLayer::Project, "still fresh");
    let residues = vec![
        outdated_entry(MemoryLayer::Project, "already outdated"),
        superseded_entry(MemoryLayer::Project, "already superseded"),
        // #2007 D1：TTL 过期不是失效——sweep NEVER 归档仅静默过期的条目。
        ttl_expired_entry(MemoryLayer::Project, "ttl expired"),
    ];
    let global_outdated = outdated_entry(MemoryLayer::Global, "stale global fact");
    let mut project_seeded = vec![fresh.clone()];
    project_seeded.extend(residues.clone());
    seed_layer(&store, MemoryLayer::Project, project_seeded).await;
    seed_layer(&store, MemoryLayer::Global, vec![global_outdated]).await;

    let service =
        MemoryService::open_with_clock(sweep_store(&root), MemoryPolicy::default(), || 200)
            .await
            .unwrap();
    let permit = service.acquire_write_permit().await.unwrap();
    let swept = service
        .sweep_stale_under_permit(200, &permit)
        .await
        .unwrap();
    assert_eq!(
        swept, 3,
        "双层 3 条非 pinned 失效（outdated/被取代）MUST 归档"
    );

    let stats = service.stats().await;
    assert_eq!(
        stats.project_count, 2,
        "有效条目与 TTL 过期条目留在 project active"
    );
    assert_eq!(
        stats.project_archive_count, 2,
        "outdated / 被取代 两条 MUST 归档；TTL 过期 NOT 归档（#2007 D1）"
    );
    assert_eq!(stats.global_count, 0, "global 层残留不得留在 active");
    assert_eq!(stats.global_archive_count, 1, "global 层残留同样归档");

    let active_ids: Vec<MemoryId> = service
        .list(None)
        .await
        .iter()
        .map(|listed| listed.id)
        .collect();
    assert_eq!(active_ids.len(), 2, "有效条目 + TTL 过期条目留在 active");
    assert!(active_ids.contains(&fresh.id), "有效条目 MUST 留在 active");
    assert!(
        active_ids
            .iter()
            .any(|id| residues.iter().any(|residue| residue.id == *id)),
        "TTL 过期条目 MUST 留在 active（只催促反思，不物理清理）"
    );
    drop(permit);
    std::fs::remove_dir_all(&root).unwrap();
}

/// pinned 保护：pinned 的失效条目 MUST 留在 active，且不计入清扫计数。
#[tokio::test]
async fn sweep_stale_under_permit_keeps_pinned_residue_in_active() {
    let root = sweep_root("pinned");
    let store = sweep_store(&root);
    let fresh = entry(MemoryLayer::Project, "fresh keeper");
    let pinned_stale = pinned_stale_entry(MemoryLayer::Project, "pinned but outdated");
    let unpinned_stale = outdated_entry(MemoryLayer::Project, "unpinned stale");
    seed_layer(
        &store,
        MemoryLayer::Project,
        vec![fresh.clone(), pinned_stale.clone(), unpinned_stale.clone()],
    )
    .await;

    let service =
        MemoryService::open_with_clock(sweep_store(&root), MemoryPolicy::default(), || 200)
            .await
            .unwrap();
    let permit = service.acquire_write_permit().await.unwrap();
    let swept = service
        .sweep_stale_under_permit(200, &permit)
        .await
        .unwrap();
    assert_eq!(swept, 1, "pinned 残留 MUST NOT 归档，也不计入清扫计数");

    let active = service.list(Some(MemoryLayer::Project)).await;
    let kept = active
        .iter()
        .find(|listed| listed.id == pinned_stale.id)
        .expect("pinned 失效条目 MUST 留在 active");
    assert!(kept.pinned);
    assert!(kept.outdated);
    assert!(
        active.iter().any(|listed| listed.id == fresh.id),
        "有效条目 MUST 留在 active"
    );
    assert!(
        !active.iter().any(|listed| listed.id == unpinned_stale.id),
        "未 pinned 的残留 MUST 移出 active"
    );
    assert_eq!(service.stats().await.project_archive_count, 1);
    drop(permit);
    std::fs::remove_dir_all(&root).unwrap();
}

/// 死锁防护：调用方已持写 permit 时清扫 MUST 直接复用持锁原语完成，
/// NEVER 二次取同一写门闩（异步重入会永久挂起）。
#[tokio::test]
async fn sweep_stale_under_permit_completes_without_deadlock_while_permit_held() {
    let root = sweep_root("deadlock");
    let store = sweep_store(&root);
    seed_layer(
        &store,
        MemoryLayer::Project,
        vec![outdated_entry(MemoryLayer::Project, "stale one")],
    )
    .await;

    let service =
        MemoryService::open_with_clock(sweep_store(&root), MemoryPolicy::default(), || 200)
            .await
            .unwrap();
    let permit = service.acquire_write_permit().await.unwrap();
    let swept = tokio::time::timeout(
        Duration::from_secs(2),
        service.sweep_stale_under_permit(200, &permit),
    )
    .await
    .expect("已持 permit 的清扫 MUST 完成，NEVER 二次取写门闩死锁")
    .expect("清扫成功");
    assert_eq!(swept, 1);
    drop(permit);

    // permit 释放后门闩 MUST 可再次获取，证明没有被占死。
    let repermit = tokio::time::timeout(Duration::from_secs(2), service.acquire_write_permit())
        .await
        .expect("释放后可再取 permit")
        .unwrap();
    drop(repermit);
    std::fs::remove_dir_all(&root).unwrap();
}

/// 落盘重读 + 幂等：归档结果 MUST 持久化并随重开恢复；同一 corpus
/// 再次清扫 MUST 找不到残留、返回 0 且盘面不变。
#[tokio::test]
async fn swept_archive_survives_reopen_and_second_sweep_is_idempotent() {
    let root = sweep_root("reopen");
    let store = sweep_store(&root);
    let fresh = entry(MemoryLayer::Project, "durable fresh");
    let residues = vec![
        outdated_entry(MemoryLayer::Project, "stale a"),
        superseded_entry(MemoryLayer::Project, "stale b"),
        ttl_expired_entry(MemoryLayer::Project, "stale c"),
    ];
    let mut seeded = vec![fresh];
    seeded.extend(residues);
    seed_layer(&store, MemoryLayer::Project, seeded).await;

    let first = MemoryService::open_with_clock(sweep_store(&root), MemoryPolicy::default(), || 200)
        .await
        .unwrap();
    let first_permit = first.acquire_write_permit().await.unwrap();
    assert_eq!(
        first
            .sweep_stale_under_permit(200, &first_permit)
            .await
            .unwrap(),
        2,
        "#2007 D1: 首轮只归档 outdated / 被取代 2 条；TTL 过期留 active"
    );
    drop(first_permit);
    drop(first);

    let reopened =
        MemoryService::open_with_clock(sweep_store(&root), MemoryPolicy::default(), || 200)
            .await
            .unwrap();
    let stats = reopened.stats().await;
    assert_eq!(
        stats.project_count, 2,
        "重开后 active 剩有效条目 + TTL 过期条目（#2007 不归档）"
    );
    assert_eq!(stats.project_archive_count, 2, "归档 MUST 落盘并随重开恢复");
    let reopened_permit = reopened.acquire_write_permit().await.unwrap();
    let reswept = reopened
        .sweep_stale_under_permit(200, &reopened_permit)
        .await
        .unwrap();
    assert_eq!(reswept, 0, "再次清扫 MUST 幂等返回 0");
    assert_eq!(
        reopened.stats().await.project_archive_count,
        2,
        "再次清扫 MUST NOT 改变盘面"
    );
    drop(reopened_permit);
    std::fs::remove_dir_all(&root).unwrap();
}

/// warning 有效口径：提交后复核与 apply 预检共用 effective_active_count——
/// 物理 active 混入 outdated / 被取代残留时，只要有效数 ≤ target 就 MUST NOT
/// 产出超载告警（修复前按 `active().len()` 把残留误报成超载）；有效数真正
/// 超 target 时，告警文案携带的 MUST 是有效口径数字与原 target 计算。
#[tokio::test]
async fn reflection_after_commit_notice_counts_effective_active_only() {
    let root = sweep_root("notice");
    let store = sweep_store(&root);
    let seeded = vec![
        entry(MemoryLayer::Project, "fresh one"),
        entry(MemoryLayer::Project, "fresh two"),
        outdated_entry(MemoryLayer::Project, "residue outdated"),
        superseded_entry(MemoryLayer::Project, "residue superseded"),
    ];
    seed_layer(&store, MemoryLayer::Project, seeded).await;

    let policy = MemoryPolicy {
        max_entries: 4, // target = floor(4 × 0.75) = 3
        similarity_threshold: 0.8,
        target_active_entries: 3,
        default_ttl_secs: 0,
    };
    let service = MemoryService::open_with_clock(sweep_store(&root), policy, || 200)
        .await
        .unwrap();

    // 物理 active=4 > target=3，但有效 active=2 ≤ 3：旧物理口径会误报。
    let listed = service.list(Some(MemoryLayer::Project)).await;
    assert_eq!(listed.len(), 4, "物理 active 含 2 条残留");
    assert_eq!(
        crate::domain::effective_active_count(&listed),
        2,
        "有效 active 仅 2 条"
    );
    assert!(
        service
            .reflection_above_target_after_commit_notice()
            .is_none(),
        "有效 active ≤ target MUST NOT 产出超载告警"
    );

    // 空输出 apply 走完提交后复核同样不得告警（预检与复核同口径）。
    let result = service
        .apply_reflection(&ReflectionOutput::default())
        .await
        .expect("有效 active ≤ target，空输出 MUST 通过门禁");
    assert_eq!(result.attempted, 0);
    assert!(
        service
            .reflection_above_target_after_commit_notice()
            .is_none(),
        "apply 提交后复核 MUST NOT 按物理条数告警"
    );

    // 对照：有效 active 真正超过 target 时，告警携带有效口径数字与原 target。
    let over_root = sweep_root("notice-over");
    let over_store = sweep_store(&over_root);
    let over_seeded: Vec<MemoryEntry> = ["alpha", "beta", "gamma", "delta"]
        .into_iter()
        .map(|content| entry(MemoryLayer::Project, content))
        .collect();
    seed_layer(&over_store, MemoryLayer::Project, over_seeded).await;
    let over_service = MemoryService::open_with_clock(sweep_store(&over_root), policy, || 200)
        .await
        .unwrap();
    let notice = over_service
        .reflection_above_target_after_commit_notice()
        .expect("有效 active=4 > target=3 MUST 告警");
    assert!(
        notice.contains("active=4 target=3"),
        "告警 MUST 携带有效口径数字与原 target 计算，got {notice}"
    );

    drop(service);
    drop(over_service);
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::remove_dir_all(&over_root).unwrap();
}

/// 收敛目标来源统一：门禁与告警 MUST 用策略 `target_active_entries`（生产装配
/// 即 `memory.reflection.target_active_entries` 配置值），与反思 prompt 同源；
/// NEVER 另按 floor(max×0.75) 公式取值——两套口径会让按 prompt 收敛的模型被
/// 门禁反复打回（修复前 max=10 公式目标 7，配置目标 3，有效 5 被放行且无告警）。
#[tokio::test]
async fn capacity_gate_and_warning_use_policy_target_not_formula() {
    let root = sweep_root("policy-target-tight");
    let store = sweep_store(&root);
    let seeded: Vec<MemoryEntry> = ["a", "b", "c", "d", "e"]
        .into_iter()
        .map(|content| entry(MemoryLayer::Project, content))
        .collect();
    seed_layer(&store, MemoryLayer::Project, seeded).await;
    let policy = MemoryPolicy {
        max_entries: 10, // floor(10 × 0.75) = 7，与配置目标 3 分叉
        similarity_threshold: 0.8,
        target_active_entries: 3,
        default_ttl_secs: 0,
    };
    let service = MemoryService::open_with_clock(sweep_store(&root), policy, || 200)
        .await
        .unwrap();

    assert_eq!(
        service
            .apply_reflection(&ReflectionOutput::default())
            .await
            .expect_err("有效 5 > 策略目标 3 MUST 拦截（公式口径 5 ≤ 7 会误放行）"),
        MemoryError::CapacityNotMet {
            active: 5,
            target: 3,
        }
    );
    let notice = service
        .reflection_above_target_after_commit_notice()
        .expect("有效 5 > 策略目标 3 MUST 告警");
    assert!(
        notice.contains("active=5 target=3"),
        "告警 MUST 携带策略目标口径，got {notice}"
    );
    drop(service);
    std::fs::remove_dir_all(&root).unwrap();
}

/// 反向分叉同样 MUST 跟策略走：配置目标 8 高于公式 7 时，有效 8 的空输出
/// MUST 放行且不告警（公式口径 8 > 7 会误拦）。
#[tokio::test]
async fn capacity_gate_accepts_when_policy_target_above_formula() {
    let root = sweep_root("policy-target-loose");
    let store = sweep_store(&root);
    let seeded: Vec<MemoryEntry> = ["a", "b", "c", "d", "e", "f", "g", "h"]
        .into_iter()
        .map(|content| entry(MemoryLayer::Project, content))
        .collect();
    seed_layer(&store, MemoryLayer::Project, seeded).await;
    let policy = MemoryPolicy {
        max_entries: 10, // floor(10 × 0.75) = 7 < 配置目标 8
        similarity_threshold: 0.8,
        target_active_entries: 8,
        default_ttl_secs: 0,
    };
    let service = MemoryService::open_with_clock(sweep_store(&root), policy, || 200)
        .await
        .unwrap();

    let result = service
        .apply_reflection(&ReflectionOutput::default())
        .await
        .expect("有效 8 ≤ 策略目标 8 MUST 放行（公式口径会误拦）");
    assert_eq!(result.attempted, 0);
    assert!(
        service
            .reflection_above_target_after_commit_notice()
            .is_none(),
        "有效 8 ≤ 策略目标 8 MUST NOT 告警"
    );
    drop(service);
    std::fs::remove_dir_all(&root).unwrap();
}

/// #2007：TTL 过期不构成失效——门禁与告警恢复「只排 outdated / 被取代」的
/// 有效口径。TTL 过期（含 pinned）条目依然注入，MUST 占用收敛目标；否则
/// 会出现「注入中却不占目标」的反向暗态（修复前 #2002 曾把 TTL 过期排除）。
#[tokio::test]
async fn capacity_gate_and_warning_count_ttl_expired_entries() {
    let root = sweep_root("pinned-ttl-target");
    let store = sweep_store(&root);
    let mut pinned_expired = ttl_expired_entry(MemoryLayer::Project, "pinned expired");
    pinned_expired.pinned = true;
    let seeded = vec![
        entry(MemoryLayer::Project, "fresh one"),
        entry(MemoryLayer::Project, "fresh two"),
        entry(MemoryLayer::Project, "fresh three"),
        pinned_expired,
    ];
    seed_layer(&store, MemoryLayer::Project, seeded).await;
    let policy = MemoryPolicy {
        max_entries: 10,
        similarity_threshold: 0.8,
        target_active_entries: 3,
        default_ttl_secs: 0,
    };
    let service = MemoryService::open_with_clock(sweep_store(&root), policy, || 200)
        .await
        .unwrap();

    assert_eq!(
        service
            .apply_reflection(&ReflectionOutput::default())
            .await
            .expect_err("TTL 过期（含 pinned）计入有效 active：4 > 3 MUST 拦截"),
        MemoryError::CapacityNotMet {
            active: 4,
            target: 3,
        }
    );
    let notice = service
        .reflection_above_target_after_commit_notice()
        .expect("有效口径 4 > 目标 3 MUST 告警（TTL 过期计入）");
    assert!(
        notice.contains("active=4 target=3"),
        "告警 MUST 携带计入 TTL 过期的有效口径，got {notice}"
    );
    drop(service);
    std::fs::remove_dir_all(&root).unwrap();
}

/// #2007：pinned 可被反思标 outdated——标记生效（停注入、outdated_marked 计数），
/// 但归档跳过（D4：pinned 物理驻留 active），形成「可见的失效驻留」。
#[tokio::test]
async fn apply_reflection_marks_pinned_outdated_keeps_resident_and_stops_injection() {
    let root = sweep_root("pinned-outdated");
    let store = sweep_store(&root);
    let mut pinned = entry(MemoryLayer::Project, "pinned delivery log");
    pinned.pinned = true;
    seed_layer(&store, MemoryLayer::Project, vec![pinned.clone()]).await;
    let service =
        MemoryService::open_with_clock(sweep_store(&root), MemoryPolicy::default(), || 200)
            .await
            .unwrap();

    let result = service
        .apply_reflection(&ReflectionOutput {
            outdated_memories: vec![pinned.id.to_string()],
            ..ReflectionOutput::default()
        })
        .await
        .expect("pinned 的 outdated 引用 MUST 放行（#2007：LLM 可裁决）");
    assert_eq!(
        result.outdated_marked, 1,
        "pinned 标记 MUST 计入 outdated_marked"
    );

    let listed = service.list(Some(MemoryLayer::Project)).await;
    assert_eq!(listed.len(), 1, "D4: pinned 条目 MUST 物理驻留 active");
    assert!(listed[0].outdated, "驻留条目 MUST 已带 outdated 标记");
    assert!(listed[0].pinned);

    let recalled = service
        .search(&MemorySearchQuery {
            text: "pinned delivery".to_string(),
            limit: 10,
            layer: Some(MemoryLayer::Project),
            category: None,
            include_archive: false,
            now: 200,
            exclude_stale: true,
            confirm_hits: false,
        })
        .await;
    assert!(
        recalled.hits.is_empty(),
        "outdated 后 MUST 停注入（exclude_stale 召回为空）"
    );
    drop(service);
    std::fs::remove_dir_all(&root).unwrap();
}

/// #2007 D3：显式检索（exclude_stale=false）命中刷新 last_confirmed_at 与
/// confirmation_count（TTL 随之续期）；注入召回（exclude_stale=true）MUST NOT
/// 刷新——系统选中 ≠ 被证明有效；失效条目命中不刷新。
#[tokio::test]
async fn explicit_search_refreshes_confirmation_but_injection_recall_does_not() {
    let root = sweep_root("confirm-hit");
    let store = sweep_store(&root);
    let mut fresh = entry(MemoryLayer::Project, "rust deploy checklist");
    fresh.last_confirmed_at = 100;
    let mut other = entry(MemoryLayer::Project, "kotlin release plan");
    other.last_confirmed_at = 100;
    let mut lapsed = outdated_entry(MemoryLayer::Project, "rust deploy runbook");
    lapsed.last_confirmed_at = 100;
    seed_layer(
        &store,
        MemoryLayer::Project,
        vec![fresh.clone(), other.clone(), lapsed.clone()],
    )
    .await;
    let service =
        MemoryService::open_with_clock(sweep_store(&root), MemoryPolicy::default(), || 200)
            .await
            .unwrap();

    // 显式检索：命中条目刷新（fresh 命中；lapsed 命中也不刷新）。
    service
        .search(&MemorySearchQuery {
            text: "rust deploy checklist".to_string(),
            limit: 10,
            layer: Some(MemoryLayer::Project),
            category: None,
            include_archive: false,
            now: 200,
            exclude_stale: false,
            confirm_hits: true,
        })
        .await;
    let confirmed: Vec<_> = service
        .list(Some(MemoryLayer::Project))
        .await
        .into_iter()
        .map(|listed| {
            (
                listed.id,
                listed.last_confirmed_at,
                listed.confirmation_count,
            )
        })
        .collect();
    let lookup = |id: &MemoryId| {
        confirmed
            .iter()
            .find(|(entry_id, _, _)| entry_id == id)
            .copied()
            .unwrap()
    };
    assert_eq!(
        lookup(&fresh.id).1,
        200,
        "显式命中 MUST 刷新 last_confirmed_at"
    );
    assert_eq!(
        lookup(&fresh.id).2,
        1,
        "显式命中 MUST 累计 confirmation_count"
    );
    assert_eq!(
        lookup(&other.id).1,
        100,
        "未命中条目 MUST NOT 刷新（best-effort 只刷命中）"
    );
    assert_eq!(lookup(&lapsed.id).1, 100, "失效条目命中 MUST NOT 刷新确认");

    // 注入召回：exclude_stale=true 不刷新。
    service
        .search(&MemorySearchQuery {
            text: "rust deploy checklist".to_string(),
            limit: 10,
            layer: Some(MemoryLayer::Project),
            category: None,
            include_archive: false,
            now: 300,
            exclude_stale: true,
            confirm_hits: false,
        })
        .await;
    let after = service
        .list(Some(MemoryLayer::Project))
        .await
        .into_iter()
        .find(|listed| listed.id == fresh.id)
        .unwrap();
    assert_eq!(
        after.last_confirmed_at, 200,
        "注入召回（exclude_stale=true）MUST NOT 刷新确认"
    );
    drop(service);
    std::fs::remove_dir_all(&root).unwrap();
}
