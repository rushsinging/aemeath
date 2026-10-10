use super::*;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex as StdMutex},
};

/// A per-layer script of committed loads and commit receipts. Each layer is
/// an independent generation, so its loads, commits, and call counts are
/// tracked separately.
#[derive(Default)]
pub(super) struct LayerScript {
    pub(super) loads: VecDeque<Result<CommittedMemoryDataset<u64>, MemoryError>>,
    pub(super) commits: VecDeque<Result<MemoryCommitReceipt<u64>, MemoryError>>,
    pub(super) load_calls: usize,
    pub(super) commit_calls: usize,
}

pub(super) struct Script {
    pub(super) global: LayerScript,
    pub(super) project: LayerScript,
}

impl Script {
    fn layer(&mut self, layer: MemoryLayer) -> &mut LayerScript {
        match layer {
            MemoryLayer::Global => &mut self.global,
            MemoryLayer::Project => &mut self.project,
        }
    }
}

#[derive(Clone)]
pub(super) struct ScriptedStore {
    pub(super) script: Arc<StdMutex<Script>>,
}

impl ScriptedStore {
    pub(super) fn new(global: LayerScript, project: LayerScript) -> Self {
        Self {
            script: Arc::new(StdMutex::new(Script { global, project })),
        }
    }

    pub(super) fn calls(&self, layer: MemoryLayer) -> (usize, usize) {
        let mut script = self.script.lock().unwrap();
        let layer = script.layer(layer);
        (layer.load_calls, layer.commit_calls)
    }
}

#[async_trait]
impl MemoryDatasetStore for ScriptedStore {
    type Revision = u64;

    async fn load_committed(
        &self,
        layer: MemoryLayer,
    ) -> Result<CommittedMemoryDataset<Self::Revision>, MemoryError> {
        let mut script = self.script.lock().unwrap();
        let layer = script.layer(layer);
        layer.load_calls += 1;
        layer.loads.pop_front().expect("unexpected load")
    }

    async fn commit(
        &self,
        layer: MemoryLayer,
        _expected: &Self::Revision,
        _dataset: &MemoryDataset,
    ) -> Result<MemoryCommitReceipt<Self::Revision>, MemoryError> {
        let mut script = self.script.lock().unwrap();
        let layer = script.layer(layer);
        layer.commit_calls += 1;
        layer.commits.pop_front().expect("unexpected commit")
    }
}

fn layer_script(
    loads: Vec<Result<CommittedMemoryDataset<u64>, MemoryError>>,
    commits: Vec<Result<MemoryCommitReceipt<u64>, MemoryError>>,
) -> LayerScript {
    LayerScript {
        loads: loads.into(),
        commits: commits.into(),
        load_calls: 0,
        commit_calls: 0,
    }
}

fn entry(layer: MemoryLayer, content: &str) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        10,
        layer,
        MemoryCategory::Fact,
        content,
        MemorySource::User,
    )
    .unwrap()
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

fn receipt(revision: u64, visibility: MemoryCommitVisibility) -> MemoryCommitReceipt<u64> {
    MemoryCommitReceipt::new(revision, visibility)
}

fn storage(kind: MemoryStorageErrorKind) -> MemoryError {
    MemoryError::Storage { kind }
}

fn small_policy() -> MemoryPolicy {
    MemoryPolicy {
        max_entries: 1,
        similarity_threshold: 0.8,
    }
}

#[tokio::test]
async fn commit_error_keeps_old_committed_state() {
    let old = entry(MemoryLayer::Project, "old");
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 2], vec![]),
        layer_script(
            vec![Ok(committed(1, MemoryLayer::Project, vec![old.clone()])); 2],
            vec![Err(storage(MemoryStorageErrorKind::Io))],
        ),
    );
    let service = MemoryService::open(store, MemoryPolicy::default())
        .await
        .unwrap();

    assert!(service
        .write(entry(MemoryLayer::Project, "candidate"))
        .await
        .is_err());
    assert_eq!(service.list(None).await, vec![old]);
}

#[tokio::test]
async fn recovery_pending_receipt_publishes_candidate() {
    let candidate = entry(MemoryLayer::Project, "committed");
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 2], vec![]),
        layer_script(
            vec![
                Ok(empty_layer(1, MemoryLayer::Project)),
                Ok(committed(2, MemoryLayer::Project, vec![candidate.clone()])),
            ],
            vec![Ok(receipt(2, MemoryCommitVisibility::RecoveryPending))],
        ),
    );
    let service = MemoryService::open(store, MemoryPolicy::default())
        .await
        .unwrap();

    service.write(candidate.clone()).await.unwrap();
    assert_eq!(service.list(None).await, vec![candidate]);
}

#[tokio::test]
async fn concurrent_write_reloads_and_recomputes_once() {
    let external = entry(MemoryLayer::Project, "external");
    let local = entry(MemoryLayer::Project, "local");
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 2], vec![]),
        layer_script(
            vec![
                Ok(empty_layer(1, MemoryLayer::Project)),
                Ok(committed(2, MemoryLayer::Project, vec![external.clone()])),
                Ok(committed(
                    3,
                    MemoryLayer::Project,
                    vec![external.clone(), local.clone()],
                )),
            ],
            vec![
                Err(storage(MemoryStorageErrorKind::ConcurrentWrite)),
                Ok(receipt(3, MemoryCommitVisibility::Visible)),
            ],
        ),
    );
    let observer = store.clone();
    let service = MemoryService::open(store, MemoryPolicy::default())
        .await
        .unwrap();

    service.write(local.clone()).await.unwrap();
    // Only the project layer refreshed and recomputed exactly once; the
    // global layer was only loaded at open and never committed.
    assert_eq!(observer.calls(MemoryLayer::Project), (2, 2));
    assert_eq!(observer.calls(MemoryLayer::Global), (1, 0));
    assert_eq!(service.list(None).await, vec![external, local]);
    // open + write 的 CAS 重读 + list 现读；读路径不提交。
    assert_eq!(observer.calls(MemoryLayer::Project), (3, 2));
    assert_eq!(observer.calls(MemoryLayer::Global), (2, 0));
}

#[tokio::test]
async fn second_concurrent_write_is_typed_and_not_retried_again() {
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 2], vec![]),
        layer_script(
            vec![
                Ok(empty_layer(1, MemoryLayer::Project)),
                Ok(empty_layer(2, MemoryLayer::Project)),
                Ok(empty_layer(2, MemoryLayer::Project)),
            ],
            vec![
                Err(storage(MemoryStorageErrorKind::ConcurrentWrite)),
                Err(storage(MemoryStorageErrorKind::ConcurrentWrite)),
            ],
        ),
    );
    let observer = store.clone();
    let service = MemoryService::open(store, MemoryPolicy::default())
        .await
        .unwrap();

    let error = service
        .write(entry(MemoryLayer::Project, "local"))
        .await
        .unwrap_err();
    assert!(is_concurrent_write(&error));
    assert_eq!(observer.calls(MemoryLayer::Project), (2, 2));
    assert!(service.list(None).await.is_empty());
}

#[tokio::test]
async fn write_commits_only_the_targeted_layer() {
    // A global write commits the global generation and never touches the
    // project generation; the empty project commit script would panic if
    // the service tried to commit it.
    let global_fact = entry(MemoryLayer::Global, "global fact");
    let store = ScriptedStore::new(
        layer_script(
            vec![
                Ok(empty_layer(1, MemoryLayer::Global)),
                Ok(committed(2, MemoryLayer::Global, vec![global_fact.clone()])),
            ],
            vec![Ok(receipt(2, MemoryCommitVisibility::Visible))],
        ),
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Project)); 2], vec![]),
    );
    let observer = store.clone();
    let service = MemoryService::open(store, MemoryPolicy::default())
        .await
        .unwrap();

    service.write(global_fact.clone()).await.unwrap();
    assert_eq!(observer.calls(MemoryLayer::Global), (1, 1));
    assert_eq!(observer.calls(MemoryLayer::Project), (1, 0));
    assert_eq!(service.list(None).await, vec![global_fact]);
}

#[tokio::test]
async fn compact_commits_each_layer_as_its_own_mutation() {
    let store = ScriptedStore::new(
        layer_script(
            vec![Ok(committed(
                1,
                MemoryLayer::Global,
                vec![
                    entry(MemoryLayer::Global, "g1"),
                    entry(MemoryLayer::Global, "g2"),
                ],
            ))],
            vec![Ok(receipt(2, MemoryCommitVisibility::Visible))],
        ),
        layer_script(
            vec![Ok(committed(
                1,
                MemoryLayer::Project,
                vec![
                    entry(MemoryLayer::Project, "p1"),
                    entry(MemoryLayer::Project, "p2"),
                ],
            ))],
            vec![Ok(receipt(2, MemoryCommitVisibility::Visible))],
        ),
    );
    let observer = store.clone();
    let service = MemoryService::open(store, small_policy()).await.unwrap();

    let result = service.compact().await.unwrap();
    assert_eq!(result.archived, 2);
    assert_eq!(result.remaining, 2);
    // Each layer committed exactly one compaction generation of its own.
    assert_eq!(observer.calls(MemoryLayer::Global), (1, 1));
    assert_eq!(observer.calls(MemoryLayer::Project), (1, 1));
}

#[tokio::test]
async fn compact_layer_failure_returns_real_error_without_hiding_partial_commit() {
    let g1 = entry(MemoryLayer::Global, "g1");
    let g2 = entry(MemoryLayer::Global, "g2");
    let p1 = entry(MemoryLayer::Project, "p1");
    let p2 = entry(MemoryLayer::Project, "p2");
    let store = ScriptedStore::new(
        layer_script(
            vec![
                Ok(committed(
                    1,
                    MemoryLayer::Global,
                    vec![g1.clone(), g2.clone()],
                )),
                // stats() 现读磁盘：全局层已提交压缩代（一条 active + 一条 archive）。
                Ok(CommittedMemoryDataset {
                    dataset: MemoryDataset::new(MemoryLayer::Global, vec![g1], vec![g2]).unwrap(),
                    revision: 2,
                }),
            ],
            vec![Ok(receipt(2, MemoryCommitVisibility::Visible))],
        ),
        layer_script(
            vec![
                Ok(committed(
                    1,
                    MemoryLayer::Project,
                    vec![p1.clone(), p2.clone()],
                )),
                // 项目层提交失败，磁盘保持原提交态。
                Ok(committed(1, MemoryLayer::Project, vec![p1, p2])),
            ],
            vec![Err(storage(MemoryStorageErrorKind::Io))],
        ),
    );
    let observer = store.clone();
    let service = MemoryService::open(store, small_policy()).await.unwrap();

    let error = service.compact().await.unwrap_err();
    assert_eq!(error, storage(MemoryStorageErrorKind::Io));
    assert_eq!(observer.calls(MemoryLayer::Global), (1, 1));
    assert_eq!(observer.calls(MemoryLayer::Project), (1, 1));
    // The global layer's compaction is published as its own observable
    // mutation; the failed project layer keeps its prior committed state.
    let stats = service.stats().await;
    assert_eq!(stats.global_count, 1);
    assert_eq!(stats.global_archive_count, 1);
    assert_eq!(stats.project_count, 2);
    assert_eq!(stats.project_archive_count, 0);
}

#[tokio::test]
async fn explicit_search_ranks_non_contiguous_multi_term_matches() {
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 2], vec![]),
        layer_script(
            vec![
                Ok(committed(
                    1,
                    MemoryLayer::Project,
                    vec![
                        entry(MemoryLayer::Project, "rust ownership memory safety"),
                        entry(MemoryLayer::Project, "rust ownership"),
                        entry(MemoryLayer::Project, "python memory safety"),
                    ],
                ));
                2
            ],
            vec![],
        ),
    );
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), || 4_242)
        .await
        .unwrap();

    let result = service
        .search(&MemorySearchQuery {
            text: "rust safety".to_string(),
            limit: 10,
            layer: Some(MemoryLayer::Project),
            category: None,
            include_archive: false,
            now: 4_242,
        })
        .await;

    let contents = result
        .hits
        .iter()
        .map(|hit| hit.entry.content.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        contents,
        vec![
            "rust ownership memory safety",
            "rust ownership",
            "python memory safety"
        ]
    );
    assert!(
        result.hits[0].relevance > result.hits[1].relevance,
        "matching both query terms must outrank matching one"
    );
}

#[tokio::test]
async fn explicit_search_matches_chinese_subphrases_and_mixed_code_terms() {
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 3], vec![]),
        layer_script(
            vec![
                Ok(committed(
                    1,
                    MemoryLayer::Project,
                    vec![
                        entry(MemoryLayer::Project, "用户偏好使用中文回复"),
                        entry(MemoryLayer::Project, "MemoryPort 支持中文检索"),
                        entry(MemoryLayer::Project, "用户偏好启用英文日志"),
                    ],
                ));
                3
            ],
            vec![],
        ),
    );
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), || 4_242)
        .await
        .unwrap();

    let chinese = service
        .search(&MemorySearchQuery {
            text: "中文回复".to_string(),
            limit: 10,
            layer: Some(MemoryLayer::Project),
            category: None,
            include_archive: false,
            now: 4_242,
        })
        .await;
    assert_eq!(chinese.hits.len(), 2);
    assert_eq!(chinese.hits[0].entry.content, "用户偏好使用中文回复");
    assert_eq!(chinese.hits[1].entry.content, "MemoryPort 支持中文检索");
    assert!(chinese.hits[0].relevance > chinese.hits[1].relevance);

    let mixed = service
        .search(&MemorySearchQuery {
            text: "MemoryPort 中文".to_string(),
            limit: 10,
            layer: Some(MemoryLayer::Project),
            category: None,
            include_archive: false,
            now: 4_242,
        })
        .await;
    assert_eq!(mixed.hits[0].entry.content, "MemoryPort 支持中文检索");
    assert!(
        mixed.hits[0].relevance > mixed.hits[1].relevance,
        "matching the Latin identifier and Chinese bigram must rank first"
    );
}

#[tokio::test]
async fn explicit_search_chinese_bigram_ranking_is_deterministic() {
    let first_id = MemoryId::new("00000000-0000-7000-8000-000000000001").unwrap();
    let second_id = MemoryId::new("00000000-0000-7000-8000-000000000002").unwrap();
    let mut first = entry(MemoryLayer::Project, "始终使用中文回答");
    first.id = first_id;
    let mut second = entry(MemoryLayer::Project, "始终使用中文回答");
    second.id = second_id;
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 3], vec![]),
        layer_script(
            vec![Ok(committed(1, MemoryLayer::Project, vec![second, first])); 3],
            vec![],
        ),
    );
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), || 4_242)
        .await
        .unwrap();
    let query = MemorySearchQuery {
        text: "中文回答".to_string(),
        limit: 10,
        layer: Some(MemoryLayer::Project),
        category: None,
        include_archive: false,
        now: 4_242,
    };

    let first_result = service.search(&query).await;
    let second_result = service.search(&query).await;
    let first_ids = first_result
        .hits
        .iter()
        .map(|hit| hit.entry.id)
        .collect::<Vec<_>>();
    let second_ids = second_result
        .hits
        .iter()
        .map(|hit| hit.entry.id)
        .collect::<Vec<_>>();

    assert_eq!(first_ids, vec![first_id, second_id]);
    assert_eq!(first_ids, second_ids);
    assert_eq!(
        first_result.hits[0].relevance,
        second_result.hits[0].relevance
    );
}

#[tokio::test]
async fn explicit_search_is_deterministic_and_empty_query_returns_no_hits() {
    let first_id = MemoryId::new("00000000-0000-7000-8000-000000000001").unwrap();
    let second_id = MemoryId::new("00000000-0000-7000-8000-000000000002").unwrap();
    let mut first = entry(MemoryLayer::Project, "stable lexical match");
    first.id = first_id;
    let mut second = entry(MemoryLayer::Project, "stable lexical match");
    second.id = second_id;
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 4], vec![]),
        layer_script(
            vec![Ok(committed(1, MemoryLayer::Project, vec![second, first])); 4],
            vec![],
        ),
    );
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), || 4_242)
        .await
        .unwrap();
    let query = MemorySearchQuery {
        text: "stable lexical".to_string(),
        limit: 10,
        layer: Some(MemoryLayer::Project),
        category: None,
        include_archive: false,
        now: 4_242,
    };

    let first_result = service.search(&query).await;
    let second_result = service.search(&query).await;
    let first_ids = first_result
        .hits
        .iter()
        .map(|hit| hit.entry.id)
        .collect::<Vec<_>>();
    let second_ids = second_result
        .hits
        .iter()
        .map(|hit| hit.entry.id)
        .collect::<Vec<_>>();
    assert_eq!(first_ids, second_ids);
    assert_eq!(first_ids, vec![first_id, second_id]);

    let empty = service
        .search(&MemorySearchQuery {
            text: "  ".to_string(),
            ..query
        })
        .await;
    assert!(empty.hits.is_empty());
}

#[tokio::test]
async fn explicit_search_filters_by_tag_category_and_layer() {
    let matching = MemoryEntry::new(
        MemoryId::new("01890f3c-7c00-7000-8000-000000000010").unwrap(),
        4_000,
        MemoryLayer::Project,
        MemoryCategory::Pattern,
        "workspace validation",
        MemorySource::User,
    )
    .map(|mut entry| {
        entry.tags = vec!["clippy".to_string()];
        entry
    })
    .unwrap();
    let filtered_by_category = MemoryEntry::new(
        MemoryId::new("01890f3c-7c00-7000-8000-000000000011").unwrap(),
        4_000,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        "clippy fact",
        MemorySource::User,
    )
    .unwrap();
    let filtered_by_layer = MemoryEntry::new(
        MemoryId::new("01890f3c-7c00-7000-8000-000000000012").unwrap(),
        4_000,
        MemoryLayer::Global,
        MemoryCategory::Pattern,
        "clippy global",
        MemorySource::User,
    )
    .unwrap();
    let store = ScriptedStore::new(
        layer_script(
            vec![Ok(committed(1, MemoryLayer::Global, vec![filtered_by_layer],)); 2],
            vec![],
        ),
        layer_script(
            vec![
                Ok(committed(
                    1,
                    MemoryLayer::Project,
                    vec![matching, filtered_by_category],
                ));
                2
            ],
            vec![],
        ),
    );
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), || 4_242)
        .await
        .unwrap();

    let result = service
        .search(&MemorySearchQuery {
            text: "clippy".to_string(),
            limit: 10,
            layer: Some(MemoryLayer::Project),
            category: Some(MemoryCategory::Pattern),
            include_archive: false,
            now: 4_242,
        })
        .await;

    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].entry.tags, vec!["clippy"]);
    assert_eq!(result.hits[0].entry.layer, MemoryLayer::Project);
    assert_eq!(result.hits[0].entry.category, MemoryCategory::Pattern);
}

#[tokio::test]
async fn explicit_search_includes_archive_status_without_mutation() {
    let active = entry(MemoryLayer::Project, "archive query active");
    let mut archived = entry(MemoryLayer::Project, "archive query historical");
    archived.outdated = true;
    archived.ttl = Some(std::time::Duration::from_secs(1));
    let dataset =
        MemoryDataset::new(MemoryLayer::Project, vec![active], vec![archived.clone()]).unwrap();
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 2], vec![]),
        layer_script(
            vec![
                Ok(CommittedMemoryDataset {
                    dataset,
                    revision: 1,
                });
                2
            ],
            vec![],
        ),
    );
    let observer = store.clone();
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), || 4_242)
        .await
        .unwrap();

    let result = service
        .search(&MemorySearchQuery {
            text: "archive query".to_string(),
            limit: 10,
            layer: Some(MemoryLayer::Project),
            category: None,
            include_archive: true,
            now: 4_242,
        })
        .await;

    assert_eq!(result.hits.len(), 2);
    let archived_hit = result
        .hits
        .iter()
        .find(|hit| hit.location == MemoryLocation::Archive)
        .unwrap();
    assert!(archived_hit.outdated);
    assert!(archived_hit.ttl_expired);
    assert_eq!(archived_hit.entry.id, archived.id);
    // open 1 次 + search 现读 1 次；搜索不提交。
    assert_eq!(observer.calls(MemoryLayer::Project), (2, 0));
}

fn reflection_output(layer: MemoryLayer, content: &str) -> ReflectionOutput {
    ReflectionOutput {
        suggested_memories: vec![MemorySuggestion {
            layer,
            category: MemoryCategory::Fact,
            content: content.to_string(),
            tags: vec!["reflected".to_string()],
            reason: "test".to_string(),
            supersedes: vec![],
            synthesizes: Vec::new(),
        }],
        ..ReflectionOutput::default()
    }
}

#[tokio::test]
async fn reflection_commit_failure_propagates_and_keeps_committed_state() {
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 2], vec![]),
        layer_script(
            vec![Ok(empty_layer(1, MemoryLayer::Project)); 2],
            vec![Err(storage(MemoryStorageErrorKind::Io))],
        ),
    );
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), || 4242)
        .await
        .unwrap();

    let error = service
        .apply_reflection(&reflection_output(MemoryLayer::Project, "new reflection"))
        .await
        .unwrap_err();

    assert_eq!(error, storage(MemoryStorageErrorKind::Io));
    assert!(service.list(None).await.is_empty());
}

#[tokio::test]
async fn reflection_recomputes_once_after_cas_conflict() {
    let external = entry(MemoryLayer::Project, "external fact");
    // list() 现读磁盘：CAS 冲突后的提交代 = external + 本次反思新增条目。
    let reflected = {
        let mut entry = MemoryEntry::new(
            reflection_memory_id(4242).unwrap(),
            4242,
            MemoryLayer::Project,
            MemoryCategory::Fact,
            "new reflection",
            MemorySource::Llm,
        )
        .unwrap();
        entry.tags = vec!["reflected".to_string()];
        entry
    };
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 2], vec![]),
        layer_script(
            vec![
                Ok(empty_layer(1, MemoryLayer::Project)),
                Ok(committed(2, MemoryLayer::Project, vec![external.clone()])),
                Ok(committed(
                    3,
                    MemoryLayer::Project,
                    vec![external.clone(), reflected],
                )),
            ],
            vec![
                Err(storage(MemoryStorageErrorKind::ConcurrentWrite)),
                Ok(receipt(3, MemoryCommitVisibility::Visible)),
            ],
        ),
    );
    let observer = store.clone();
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), || 4242)
        .await
        .unwrap();

    let result = service
        .apply_reflection(&reflection_output(MemoryLayer::Project, "new reflection"))
        .await
        .unwrap();

    assert_eq!(result.suggestions_added, 1);
    assert_eq!(observer.calls(MemoryLayer::Project), (2, 2));
    let entries = service.list(Some(MemoryLayer::Project)).await;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].created_at, 4242);
    assert!(share::ids::is_typed_id(&entries[1].id.to_string(), "mem"));
}

#[tokio::test]
async fn queries_read_latest_from_store_per_call() {
    // #1886 新契约：每个读方法每次调用都现读磁盘（open 后外部提交立即可见），
    // 但查询永不提交（commits 恒 0）。
    let initial = entry(MemoryLayer::Project, "searchable");
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 5], vec![]),
        layer_script(
            vec![Ok(committed(1, MemoryLayer::Project, vec![initial])); 5],
            vec![],
        ),
    );
    let observer = store.clone();
    let service = MemoryService::open(store, MemoryPolicy::default())
        .await
        .unwrap();

    service
        .retrieve_for_inject(&MemoryQuery {
            limit: 10,
            layer: None,
            category: None,
            now: 10,
        })
        .await;
    service
        .search(&MemorySearchQuery {
            text: "searchable".to_string(),
            limit: 10,
            layer: None,
            category: None,
            include_archive: true,
            now: 10,
        })
        .await;
    service.list(None).await;
    service.stats().await;
    // open 1 次 + 四读各 1 次；零提交。
    assert_eq!(observer.calls(MemoryLayer::Global), (5, 0));
    assert_eq!(observer.calls(MemoryLayer::Project), (5, 0));
}

// -----------------------------------------------------------------
// open_with_clock logging contract.
//
// A thread-local capturing logger records only memory-target records so
// tests can assert the enter / success-exit / failure-exit markers that
// the *real* entry point emits — without touching production code and
// without leaking memory content into any log line.
// -----------------------------------------------------------------

thread_local! {
    static CAPTURED_MEMORY_LOGS: std::cell::RefCell<Vec<(log::Level, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct CapturingLogger;

impl log::Log for CapturingLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if record.target() == crate::LOG_TARGET {
            CAPTURED_MEMORY_LOGS.with(|cell| {
                cell.borrow_mut()
                    .push((record.level(), format!("{}", record.args())));
            });
        }
    }

    fn flush(&self) {}
}

/// Installs the capturing logger exactly once per test process. Safe to
/// call from every test: `log::set_logger` only succeeds once, later
/// calls are no-ops via `Once`. Capture storage is thread-local, so
/// tests running on separate OS threads never observe each other's
/// captured records.
fn install_capturing_logger() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        log::set_boxed_logger(Box::new(CapturingLogger))
            .expect("capturing logger must install exactly once per process");
        log::set_max_level(log::LevelFilter::Trace);
    });
}

fn drain_captured_memory_logs() -> Vec<(log::Level, String)> {
    CAPTURED_MEMORY_LOGS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

#[tokio::test]
async fn open_with_clock_logs_enter_and_success_exit_without_memory_content() {
    install_capturing_logger();
    drain_captured_memory_logs();

    let secret = "DO-NOT-LOG-THIS-MEMORY-CONTENT";
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global))], vec![]),
        layer_script(
            vec![Ok(committed(
                1,
                MemoryLayer::Project,
                vec![entry(MemoryLayer::Project, secret)],
            ))],
            vec![],
        ),
    );

    MemoryService::open_with_clock(store, MemoryPolicy::default(), || 0)
        .await
        .expect("open must succeed with a valid policy and store");

    let logs = drain_captured_memory_logs();
    let payloads: Vec<&str> = logs.iter().map(|(_, msg)| msg.as_str()).collect();
    assert!(
        payloads.iter().any(|msg| msg.contains("enter")),
        "open_with_clock must log an enter record, got {payloads:?}"
    );
    assert!(
        payloads.iter().any(|msg| msg.contains("ok")),
        "open_with_clock must log a success-exit record, got {payloads:?}"
    );
    // The success path loaded a project layer carrying sensitive content;
    // none of the emitted records may echo it back.
    assert!(
        !payloads.iter().any(|msg| msg.contains(secret)),
        "open_with_clock logs must not contain memory content, got {payloads:?}"
    );
}

#[tokio::test]
async fn open_with_clock_logs_enter_and_failure_exit_without_memory_content() {
    install_capturing_logger();
    drain_captured_memory_logs();

    let secret = "DO-NOT-LOG-THIS-MEMORY-CONTENT";
    let store = ScriptedStore::new(
        layer_script(
            vec![Ok(committed(
                1,
                MemoryLayer::Global,
                vec![entry(MemoryLayer::Global, secret)],
            ))],
            vec![],
        ),
        layer_script(vec![Err(storage(MemoryStorageErrorKind::Io))], vec![]),
    );

    let error = match MemoryService::open_with_clock(store, MemoryPolicy::default(), || 0).await {
        Err(error) => error,
        Ok(_) => panic!("open must fail when the project layer load errors"),
    };
    assert_eq!(error, storage(MemoryStorageErrorKind::Io));

    let logs = drain_captured_memory_logs();
    let payloads: Vec<&str> = logs.iter().map(|(_, msg)| msg.as_str()).collect();
    assert!(
        payloads.iter().any(|msg| msg.contains("enter")),
        "open_with_clock must log an enter record, got {payloads:?}"
    );
    assert!(
        payloads.iter().any(|msg| msg.contains("error")),
        "open_with_clock must log a failure-exit record, got {payloads:?}"
    );
    // The global layer carrying sensitive content *was* loaded before the
    // project load failed; the failure-exit record must not echo it.
    assert!(
        !payloads.iter().any(|msg| msg.contains(secret)),
        "open_with_clock logs must not contain memory content, got {payloads:?}"
    );
}

// --- System One 重排（RERANK）------------------------------------------------

use async_trait::async_trait as rerank_async_trait;

/// 固定应答的评分桩：`preferred` 序号的候选给 0.9，其余平分 0.1。
struct StubScoringPort {
    outcome: Result<usize, systemone::UnavailableKind>,
}

#[rerank_async_trait]
impl systemone::ScoringPort for StubScoringPort {
    async fn answer(
        &self,
        _state: &systemone::ScoringState,
        questions: &[systemone::ScoringQuestion],
    ) -> Result<Vec<systemone::ScoringAnswer>, systemone::ScoringUnavailable> {
        let criterion_count = match &questions[0] {
            systemone::ScoringQuestion::Choice { criteria, .. } => criteria.len(),
            _ => panic!("重排应使用 Choice 题型"),
        };
        let preferred = self
            .outcome
            .map_err(|kind| systemone::ScoringUnavailable::new(kind, "stub 故障"))?;
        let probabilities: Vec<(String, f64)> = (0..criterion_count)
            .map(|index| {
                let probability = if index == preferred {
                    0.9
                } else {
                    0.1 / (criterion_count.saturating_sub(1).max(1)) as f64
                };
                (index.to_string(), probability)
            })
            .collect();
        Ok(vec![systemone::ScoringAnswer::choice(
            preferred.to_string(),
            probabilities,
            0.9,
            systemone::CalibrationLevel::Raw,
        )
        .expect("答案构造")])
    }
}

fn rerank_service(
    entries: Vec<MemoryEntry>,
    scorer: Option<std::sync::Arc<dyn systemone::ScoringPort>>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = MemoryService<ScriptedStore>> + Send>> {
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 2], vec![]),
        layer_script(
            vec![Ok(committed(1, MemoryLayer::Project, entries)); 2],
            vec![],
        ),
    );
    Box::pin(async move {
        MemoryService::open_with_clock_and_scorer(store, MemoryPolicy::default(), || 4_242, scorer)
            .await
            .unwrap()
    })
}

fn rerank_query(text: &str, limit: usize) -> MemorySearchQuery {
    MemorySearchQuery {
        text: text.to_string(),
        limit,
        layer: Some(MemoryLayer::Project),
        category: None,
        include_archive: false,
        now: 4_242,
    }
}

#[tokio::test]
async fn search_with_scorer_reranks_top_candidates() {
    let service = rerank_service(
        vec![
            entry(MemoryLayer::Project, "rust ownership memory safety guide"),
            entry(MemoryLayer::Project, "rust ownership basics"),
            entry(MemoryLayer::Project, "python memory safety notes"),
        ],
        Some(std::sync::Arc::new(StubScoringPort { outcome: Ok(2) })),
    )
    .await;

    let result = service
        .search(&rerank_query("rust memory safety", 10))
        .await;

    // 词法序为 [guide(三词全中), python(两词), basics(一词)]；stub 偏好序号 2 = basics。
    assert_eq!(
        result.hits.first().map(|hit| hit.entry.content.as_str()),
        Some("rust ownership basics"),
        "词法第三名应被评分提升到第一，实际 {:?}",
        result
            .hits
            .iter()
            .map(|hit| hit.entry.content.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        result.hits.last().map(|hit| hit.entry.content.as_str()),
        Some("python memory safety notes"),
        "词法第二名被压到最后（等概率保持相对序）"
    );
}

#[tokio::test]
async fn search_with_failing_scorer_falls_back_to_lexical_order() {
    let entries = vec![
        entry(MemoryLayer::Project, "rust ownership memory safety guide"),
        entry(MemoryLayer::Project, "python memory safety notes"),
    ];
    let scorer: std::sync::Arc<dyn systemone::ScoringPort> = std::sync::Arc::new(StubScoringPort {
        outcome: Err(systemone::UnavailableKind::Timeout),
    });
    let failing = rerank_service(entries.clone(), Some(scorer)).await;
    let plain = rerank_service(entries, None).await;

    let with_failure = failing
        .search(&rerank_query("rust memory safety", 10))
        .await;
    let without_scorer = plain.search(&rerank_query("rust memory safety", 10)).await;

    let failure_ids: Vec<_> = with_failure.hits.iter().map(|hit| &hit.entry.id).collect();
    let plain_ids: Vec<_> = without_scorer
        .hits
        .iter()
        .map(|hit| &hit.entry.id)
        .collect();
    assert_eq!(
        failure_ids, plain_ids,
        "评分失败必须静默回退词法序（与无评分一致）"
    );
}

#[tokio::test]
async fn search_recall_expansion_lets_rerank_promote_beyond_limit() {
    // limit=1 时：无评分 → 词法 top1；有评分 → 召回扩大后重排可提升词法非 top1 的候选。
    let entries = vec![
        entry(MemoryLayer::Project, "rust ownership memory safety guide"),
        entry(MemoryLayer::Project, "python memory safety notes"),
    ];
    let scorer: std::sync::Arc<dyn systemone::ScoringPort> =
        std::sync::Arc::new(StubScoringPort { outcome: Ok(1) });
    let scored = rerank_service(entries.clone(), Some(scorer)).await;
    let plain = rerank_service(entries, None).await;

    let scored_result = scored.search(&rerank_query("rust memory safety", 1)).await;
    let plain_result = plain.search(&rerank_query("rust memory safety", 1)).await;

    assert_eq!(scored_result.hits.len(), 1);
    assert_eq!(plain_result.hits.len(), 1);
    assert_eq!(
        scored_result.hits[0].entry.content, "python memory safety notes",
        "召回扩大 + 重排应提升词法第二名"
    );
    assert_eq!(
        plain_result.hits[0].entry.content, "rust ownership memory safety guide",
        "无评分时应保持词法 top1"
    );
    assert_ne!(
        scored_result.hits[0].entry.id, plain_result.hits[0].entry.id,
        "重排应改变 top1"
    );
}

// ── #1886：读路径必须现读磁盘，外部（跨进程）提交后立即可见 ──────────────

#[tokio::test]
async fn read_methods_reflect_dataset_committed_after_open() {
    // open 时两层为空；之后外部进程提交了一条 project 记忆。
    // 四个读方法各自现读磁盘，都必须看到该记录。
    let external = entry(MemoryLayer::Project, "external fact");
    let project_load = || Ok(committed(2, MemoryLayer::Project, vec![external.clone()]));
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 5], vec![]),
        layer_script(vec![project_load(); 5], vec![]),
    );
    let observer = store.clone();
    let service = MemoryService::open(store, MemoryPolicy::default())
        .await
        .unwrap();

    let searched = service
        .search(&MemorySearchQuery {
            text: "external".to_string(),
            limit: 10,
            layer: None,
            category: None,
            include_archive: false,
            now: 100,
        })
        .await;
    assert_eq!(
        searched
            .hits
            .iter()
            .map(|hit| hit.entry.content.clone())
            .collect::<Vec<_>>(),
        vec!["external fact".to_string()],
        "search 必须看到 open 后外部提交的记录"
    );

    let listed = service.list(None).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].content, "external fact");

    let stats = service.stats().await;
    assert_eq!(stats.project_count, 1);
    assert_eq!(stats.global_count, 0);

    let injected = service
        .retrieve_for_inject(&MemoryQuery {
            limit: 10,
            layer: None,
            category: None,
            now: 100,
        })
        .await;
    assert!(injected
        .hits
        .iter()
        .any(|hit| hit.entry.content == "external fact"));

    // open 1 次 + 四读各 1 次。
    assert_eq!(observer.calls(MemoryLayer::Project), (5, 0));
    assert_eq!(observer.calls(MemoryLayer::Global), (5, 0));
}

#[tokio::test]
async fn read_hits_concurrent_write_and_reloads_once() {
    // 读路径撞 ConcurrentWrite（读 manifest 与成员之间被外部提交换代）
    // 时必须立即重读一次；第二次落在稳定代上并返回新数据。
    let external = entry(MemoryLayer::Project, "external fact");
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 2], vec![]),
        layer_script(
            vec![
                Ok(empty_layer(1, MemoryLayer::Project)),
                Err(storage(MemoryStorageErrorKind::ConcurrentWrite)),
                Ok(committed(2, MemoryLayer::Project, vec![external.clone()])),
            ],
            vec![],
        ),
    );
    let observer = store.clone();
    let service = MemoryService::open(store, MemoryPolicy::default())
        .await
        .unwrap();

    let searched = service
        .search(&MemorySearchQuery {
            text: "external".to_string(),
            limit: 10,
            layer: None,
            category: None,
            include_archive: false,
            now: 100,
        })
        .await;
    assert!(searched
        .hits
        .iter()
        .any(|hit| hit.entry.content == "external fact"));
    // open 1 + 失败 1 + 重读 1。
    assert_eq!(observer.calls(MemoryLayer::Project), (3, 0));
}

#[tokio::test]
async fn read_falls_back_to_snapshot_when_store_keeps_failing() {
    // 读路径两次（初读 + 重读）都失败时回退内存快照，绝不向上抛错。
    let old = entry(MemoryLayer::Project, "old fact");
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global)); 5], vec![]),
        layer_script(
            vec![
                Ok(committed(1, MemoryLayer::Project, vec![old.clone()])),
                Err(storage(MemoryStorageErrorKind::Io)),
                Err(storage(MemoryStorageErrorKind::Io)),
                Err(storage(MemoryStorageErrorKind::Io)),
                Err(storage(MemoryStorageErrorKind::Io)),
            ],
            vec![],
        ),
    );
    let service = MemoryService::open(store, MemoryPolicy::default())
        .await
        .unwrap();

    let searched = service
        .search(&MemorySearchQuery {
            text: "old".to_string(),
            limit: 10,
            layer: None,
            category: None,
            include_archive: false,
            now: 100,
        })
        .await;
    assert!(searched
        .hits
        .iter()
        .any(|hit| hit.entry.content == "old fact"));
    let listed = service.list(None).await;
    assert_eq!(listed.len(), 1);
}
