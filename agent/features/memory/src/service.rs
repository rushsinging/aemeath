use crate::*;
use async_trait::async_trait;
use std::sync::RwLock;
use tokio::sync::Mutex;

/// One layer's committed dataset together with the Storage revision it was read
/// or committed at. Each layer advances its own revision independently.
#[derive(Clone)]
struct LayerState<R> {
    dataset: MemoryDataset,
    revision: R,
}

#[derive(Clone)]
struct CommittedState<R> {
    global: LayerState<R>,
    project: LayerState<R>,
}

/// Durable Memory application service. Queries only inspect committed memory;
/// every mutation is serialized through one async gate and publishes only
/// after Storage reports a committed receipt. Each layer owns an independent
/// revision, so a mutation only commits the layer it actually changed.
pub struct MemoryService<S: MemoryDatasetStore> {
    store: S,
    policy: MemoryPolicy,
    state: RwLock<CommittedState<S::Revision>>,
    mutation_gate: Mutex<()>,
}

impl<S: MemoryDatasetStore> MemoryService<S> {
    pub async fn open(store: S, policy: MemoryPolicy) -> Result<Self, MemoryError> {
        validate_policy(policy)?;
        let global = load_layer(&store, MemoryLayer::Global).await?;
        let project = load_layer(&store, MemoryLayer::Project).await?;
        Ok(Self {
            store,
            policy,
            state: RwLock::new(CommittedState { global, project }),
            mutation_gate: Mutex::new(()),
        })
    }

    /// Serializes and commits a change scoped to exactly one layer. Only the
    /// changed layer is committed, and a single stale-CAS conflict refreshes and
    /// recomputes this layer once before publishing a committed receipt.
    async fn mutate_layer<T, F>(&self, layer: MemoryLayer, operation: F) -> Result<T, MemoryError>
    where
        F: Fn(&mut MemoryDataset) -> Result<(T, bool), MemoryError>,
    {
        let _permit = self.mutation_gate.lock().await;
        for attempt in 0..=1 {
            let mut candidate = self.layer_state(layer);
            let (output, changed) = operation(&mut candidate.dataset)?;
            if !changed {
                return Ok(output);
            }
            match self
                .store
                .commit(layer, &candidate.revision, &candidate.dataset)
                .await
            {
                Ok(receipt) => {
                    // Visible and RecoveryPending are both committed receipts.
                    candidate.revision = receipt.into_revision();
                    self.set_layer_state(layer, candidate);
                    return Ok(output);
                }
                Err(error) if is_concurrent_write(&error) && attempt == 0 => {
                    let refreshed = load_layer(&self.store, layer).await?;
                    self.set_layer_state(layer, refreshed);
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("the mutation retry loop has exactly two attempts")
    }

    /// Applies an entry-targeted mutation to whichever layer currently holds the
    /// entry. The entry lives in exactly one layer, so at most one layer is
    /// committed; a non-matching layer is a no-op and never commits.
    async fn mutate_owning_layer<F>(&self, operation: F) -> Result<bool, MemoryError>
    where
        F: Fn(&mut MemoryDataset) -> bool,
    {
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let changed = self
                .mutate_layer(layer, |dataset| {
                    let changed = operation(dataset);
                    Ok((changed, changed))
                })
                .await?;
            if changed {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn layer_state(&self, layer: MemoryLayer) -> LayerState<S::Revision> {
        let state = self.state.read().expect("memory state lock poisoned");
        match layer {
            MemoryLayer::Global => state.global.clone(),
            MemoryLayer::Project => state.project.clone(),
        }
    }

    fn set_layer_state(&self, layer: MemoryLayer, new: LayerState<S::Revision>) {
        let mut state = self.state.write().expect("memory state lock poisoned");
        match layer {
            MemoryLayer::Global => state.global = new,
            MemoryLayer::Project => state.project = new,
        }
    }

    fn snapshot(&self) -> (MemoryDataset, MemoryDataset) {
        let state = self.state.read().expect("memory state lock poisoned");
        (state.global.dataset.clone(), state.project.dataset.clone())
    }

    /// Compacts a single layer as one observable mutation, archiving entries
    /// that exceed the policy budget and reporting that layer's totals.
    async fn compact_layer(&self, layer: MemoryLayer) -> Result<CompactResult, MemoryError> {
        let policy = self.policy;
        self.mutate_layer(layer, move |dataset| {
            let excess = dataset.active().len().saturating_sub(policy.max_entries);
            let now = dataset
                .active()
                .iter()
                .map(|entry| entry.accessed_at)
                .max()
                .unwrap_or(0);
            let ids = eviction_candidates(dataset.active(), excess, now)
                .into_iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>();
            let mut moved = Vec::new();
            dataset.active_mut().retain(|entry| {
                if ids.contains(&entry.id) {
                    moved.push(entry.clone());
                    false
                } else {
                    true
                }
            });
            let archived = moved.len();
            dataset.archive_mut().extend(moved);
            Ok((
                CompactResult {
                    archived,
                    remaining: dataset.active().len(),
                },
                archived > 0,
            ))
        })
        .await
    }
}

#[async_trait]
impl<S: MemoryDatasetStore> MemoryPort for MemoryService<S> {
    fn retrieve_for_inject(&self, query: &MemoryQuery) -> MemorySearchResult {
        let (global, project) = self.snapshot();
        let mut entries = global
            .active()
            .iter()
            .chain(project.active())
            .filter(|entry| matches_filters(entry, query.layer, query.category))
            .filter(|entry| is_injection_eligible(entry, query.now))
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| std::cmp::Reverse(injection_score(entry, query.now)));
        entries.truncate(query.limit);
        MemorySearchResult {
            mode: MemoryRetrievalMode::InjectionPriority,
            hits: entries
                .into_iter()
                .map(|entry| MemorySearchHit {
                    entry,
                    location: MemoryLocation::Active,
                    outdated: false,
                    ttl_expired: false,
                    relevance: None,
                })
                .collect(),
        }
    }

    fn search(&self, query: &MemorySearchQuery) -> MemorySearchResult {
        let (global, project) = self.snapshot();
        let active = global
            .active()
            .iter()
            .chain(project.active())
            .map(|entry| (entry, MemoryLocation::Active));
        let archive = global
            .archive()
            .iter()
            .chain(project.archive())
            .map(|entry| (entry, MemoryLocation::Archive));
        let mut hits = if query.include_archive {
            active.chain(archive).collect::<Vec<_>>()
        } else {
            active.collect::<Vec<_>>()
        }
        .into_iter()
        .filter(|(entry, _)| matches_filters(entry, query.layer, query.category))
        .filter_map(|(entry, location)| {
            relevance(entry, &query.text).map(|score| MemorySearchHit {
                entry: entry.clone(),
                location,
                outdated: entry.outdated,
                ttl_expired: entry.is_ttl_expired(query.now),
                relevance: Some(score),
            })
        })
        .collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .relevance
                .partial_cmp(&left.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    search_tie_break_score(&right.entry, query.now)
                        .cmp(&search_tie_break_score(&left.entry, query.now))
                })
                .then_with(|| left.entry.id.cmp(&right.entry.id))
        });
        hits.truncate(query.limit);
        MemorySearchResult {
            mode: MemoryRetrievalMode::ExplicitSearch,
            hits,
        }
    }

    async fn write(&self, entry: MemoryEntry) -> Result<WriteResult, MemoryError> {
        validate_content(&entry.content)?;
        let policy = self.policy;
        self.mutate_layer(entry.layer, move |dataset| {
            if dataset
                .active()
                .iter()
                .chain(dataset.archive())
                .any(|stored| stored.id == entry.id)
            {
                return Err(MemoryError::InvalidEntry {
                    message: "记忆 ID 必须唯一".to_string(),
                });
            }
            if let Some(existing) = dataset.active_mut().iter_mut().find(|stored| {
                jaccard_similarity(&stored.content, &entry.content) >= policy.similarity_threshold
            }) {
                let mut tags = entry.tags.clone();
                existing.tags.append(&mut tags);
                existing.tags.sort();
                existing.tags.dedup();
                existing.accessed_at = entry.created_at;
                existing.access_count = existing.access_count.saturating_add(1);
                return Ok((
                    WriteResult::Merged {
                        existing_id: existing.id,
                    },
                    true,
                ));
            }
            if dataset.active().len() >= policy.max_entries {
                return Ok((
                    WriteResult::NeedsEviction {
                        candidates: eviction_candidates(dataset.active(), 3, entry.created_at),
                    },
                    false,
                ));
            }
            let id = entry.id;
            dataset.active_mut().push(entry.clone());
            Ok((WriteResult::Added { id }, true))
        })
        .await
    }

    async fn update(&self, id: &MemoryId, content: &str) -> Result<bool, MemoryError> {
        validate_content(content)?;
        let id = *id;
        let content = content.to_string();
        self.mutate_owning_layer(move |dataset| {
            mutate_active(dataset, &id, |entry| entry.content.clone_from(&content))
        })
        .await
    }

    async fn delete(&self, id: &MemoryId) -> Result<bool, MemoryError> {
        let id = *id;
        self.mutate_owning_layer(move |dataset| {
            let before = dataset.active().len();
            dataset.active_mut().retain(|entry| entry.id != id);
            before != dataset.active().len()
        })
        .await
    }

    async fn pin(&self, id: &MemoryId, pinned: bool) -> Result<bool, MemoryError> {
        let id = *id;
        self.mutate_owning_layer(move |dataset| {
            mutate_active(dataset, &id, |entry| entry.pinned = pinned)
        })
        .await
    }

    async fn mark_outdated(&self, id: &MemoryId) -> Result<bool, MemoryError> {
        let id = *id;
        self.mutate_owning_layer(move |dataset| {
            mutate_active(dataset, &id, |entry| entry.outdated = true)
        })
        .await
    }

    async fn apply_reflection(
        &self,
        _output: &ReflectionOutput,
    ) -> Result<ReflectionApplyResult, MemoryError> {
        Ok(ReflectionApplyResult::default())
    }

    async fn archive(&self, ids: &[MemoryId]) -> Result<(), MemoryError> {
        // The ids may span both layers; archive each layer as its own observable
        // mutation so a stale-CAS conflict is scoped to a single layer.
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let ids = ids.to_vec();
            self.mutate_layer(layer, move |dataset| {
                let mut moved = Vec::new();
                dataset.active_mut().retain(|entry| {
                    if ids.contains(&entry.id) && !entry.pinned {
                        moved.push(entry.clone());
                        false
                    } else {
                        true
                    }
                });
                let changed = !moved.is_empty();
                dataset.archive_mut().extend(moved);
                Ok(((), changed))
            })
            .await?;
        }
        Ok(())
    }

    async fn compact(&self) -> Result<CompactResult, MemoryError> {
        // Compact spans both layers, but each layer is committed as its own
        // observable mutation. A single layer failing surfaces the real error;
        // no partial commit is hidden behind one aggregate result.
        let mut archived = 0;
        let mut remaining = 0;
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let CompactResult {
                archived: layer_archived,
                remaining: layer_remaining,
            } = self.compact_layer(layer).await?;
            archived += layer_archived;
            remaining += layer_remaining;
        }
        Ok(CompactResult {
            archived,
            remaining,
        })
    }

    fn list(&self, layer: Option<MemoryLayer>) -> Vec<MemoryEntry> {
        let (global, project) = self.snapshot();
        global
            .active()
            .iter()
            .chain(project.active())
            .filter(|entry| layer.is_none_or(|layer| entry.layer == layer))
            .cloned()
            .collect()
    }

    fn stats(&self) -> MemoryStats {
        let (global, project) = self.snapshot();
        MemoryStats {
            global_count: global.active().len(),
            global_archive_count: global.archive().len(),
            project_count: project.active().len(),
            project_archive_count: project.archive().len(),
        }
    }
}

fn validate_policy(policy: MemoryPolicy) -> Result<(), MemoryError> {
    if policy.max_entries == 0 || !(0.0..=1.0).contains(&policy.similarity_threshold) {
        return Err(MemoryError::InvalidEntry {
            message: "无效的记忆策略".to_string(),
        });
    }
    Ok(())
}

/// Loads one layer's committed generation and enforces that Storage returned a
/// dataset for the requested layer.
async fn load_layer<S: MemoryDatasetStore>(
    store: &S,
    layer: MemoryLayer,
) -> Result<LayerState<S::Revision>, MemoryError> {
    let loaded = store.load_committed(layer).await?;
    if loaded.dataset.layer() != layer {
        return Err(MemoryError::Storage {
            kind: MemoryStorageErrorKind::Serialization,
        });
    }
    Ok(LayerState {
        dataset: loaded.dataset,
        revision: loaded.revision,
    })
}

fn is_concurrent_write(error: &MemoryError) -> bool {
    matches!(
        error,
        MemoryError::Storage {
            kind: MemoryStorageErrorKind::ConcurrentWrite
        }
    )
}

fn mutate_active(
    dataset: &mut MemoryDataset,
    id: &MemoryId,
    mutation: impl FnOnce(&mut MemoryEntry),
) -> bool {
    if let Some(entry) = dataset
        .active_mut()
        .iter_mut()
        .find(|entry| &entry.id == id)
    {
        mutation(entry);
        true
    } else {
        false
    }
}

fn validate_content(content: &str) -> Result<(), MemoryError> {
    if content.trim().is_empty() {
        return Err(MemoryError::InvalidEntry {
            message: "记忆内容不能为空".to_string(),
        });
    }
    Ok(())
}

fn matches_filters(
    entry: &MemoryEntry,
    layer: Option<MemoryLayer>,
    category: Option<MemoryCategory>,
) -> bool {
    layer.is_none_or(|layer| entry.layer == layer)
        && category.is_none_or(|category| entry.category == category)
}

fn relevance(entry: &MemoryEntry, text: &str) -> Option<f64> {
    let text = text.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }
    let content = entry.content.to_lowercase();
    let category = format!("{:?}", entry.category).to_lowercase();
    let layer = format!("{:?}", entry.layer).to_lowercase();
    if content == text {
        Some(1.0)
    } else if content.contains(&text)
        || entry
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(&text))
        || category.contains(&text)
        || layer.contains(&text)
    {
        Some(0.5)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex as StdMutex},
    };

    /// A per-layer script of committed loads and commit receipts. Each layer is
    /// an independent generation, so its loads, commits, and call counts are
    /// tracked separately.
    #[derive(Default)]
    struct LayerScript {
        loads: VecDeque<Result<CommittedMemoryDataset<u64>, MemoryError>>,
        commits: VecDeque<Result<MemoryCommitReceipt<u64>, MemoryError>>,
        load_calls: usize,
        commit_calls: usize,
    }

    struct Script {
        global: LayerScript,
        project: LayerScript,
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
    struct ScriptedStore {
        script: Arc<StdMutex<Script>>,
    }

    impl ScriptedStore {
        fn new(global: LayerScript, project: LayerScript) -> Self {
            Self {
                script: Arc::new(StdMutex::new(Script { global, project })),
            }
        }

        fn calls(&self, layer: MemoryLayer) -> (usize, usize) {
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
            layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global))], vec![]),
            layer_script(
                vec![Ok(committed(1, MemoryLayer::Project, vec![old.clone()]))],
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
        assert_eq!(service.list(None), vec![old]);
    }

    #[tokio::test]
    async fn recovery_pending_receipt_publishes_candidate() {
        let store = ScriptedStore::new(
            layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global))], vec![]),
            layer_script(
                vec![Ok(empty_layer(1, MemoryLayer::Project))],
                vec![Ok(receipt(2, MemoryCommitVisibility::RecoveryPending))],
            ),
        );
        let service = MemoryService::open(store, MemoryPolicy::default())
            .await
            .unwrap();
        let candidate = entry(MemoryLayer::Project, "committed");

        service.write(candidate.clone()).await.unwrap();
        assert_eq!(service.list(None), vec![candidate]);
    }

    #[tokio::test]
    async fn concurrent_write_reloads_and_recomputes_once() {
        let external = entry(MemoryLayer::Project, "external");
        let store = ScriptedStore::new(
            layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global))], vec![]),
            layer_script(
                vec![
                    Ok(empty_layer(1, MemoryLayer::Project)),
                    Ok(committed(2, MemoryLayer::Project, vec![external.clone()])),
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
        let local = entry(MemoryLayer::Project, "local");

        service.write(local.clone()).await.unwrap();
        // Only the project layer refreshed and recomputed exactly once; the
        // global layer was only loaded at open and never committed.
        assert_eq!(observer.calls(MemoryLayer::Project), (2, 2));
        assert_eq!(observer.calls(MemoryLayer::Global), (1, 0));
        assert_eq!(service.list(None), vec![external, local]);
    }

    #[tokio::test]
    async fn second_concurrent_write_is_typed_and_not_retried_again() {
        let store = ScriptedStore::new(
            layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global))], vec![]),
            layer_script(
                vec![
                    Ok(empty_layer(1, MemoryLayer::Project)),
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
        assert!(service.list(None).is_empty());
    }

    #[tokio::test]
    async fn write_commits_only_the_targeted_layer() {
        // A global write commits the global generation and never touches the
        // project generation; the empty project commit script would panic if
        // the service tried to commit it.
        let store = ScriptedStore::new(
            layer_script(
                vec![Ok(empty_layer(1, MemoryLayer::Global))],
                vec![Ok(receipt(2, MemoryCommitVisibility::Visible))],
            ),
            layer_script(vec![Ok(empty_layer(1, MemoryLayer::Project))], vec![]),
        );
        let observer = store.clone();
        let service = MemoryService::open(store, MemoryPolicy::default())
            .await
            .unwrap();
        let global_fact = entry(MemoryLayer::Global, "global fact");

        service.write(global_fact.clone()).await.unwrap();
        assert_eq!(observer.calls(MemoryLayer::Global), (1, 1));
        assert_eq!(observer.calls(MemoryLayer::Project), (1, 0));
        assert_eq!(service.list(None), vec![global_fact]);
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
        let stats = service.stats();
        assert_eq!(stats.global_count, 1);
        assert_eq!(stats.global_archive_count, 1);
        assert_eq!(stats.project_count, 2);
        assert_eq!(stats.project_archive_count, 0);
    }

    #[tokio::test]
    async fn queries_never_call_store() {
        let initial = entry(MemoryLayer::Project, "searchable");
        let store = ScriptedStore::new(
            layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global))], vec![]),
            layer_script(
                vec![Ok(committed(1, MemoryLayer::Project, vec![initial]))],
                vec![],
            ),
        );
        let observer = store.clone();
        let service = MemoryService::open(store, MemoryPolicy::default())
            .await
            .unwrap();

        service.retrieve_for_inject(&MemoryQuery {
            limit: 10,
            layer: None,
            category: None,
            now: 10,
        });
        service.search(&MemorySearchQuery {
            text: "searchable".to_string(),
            limit: 10,
            layer: None,
            category: None,
            include_archive: true,
            now: 10,
        });
        service.list(None);
        service.stats();
        assert_eq!(observer.calls(MemoryLayer::Global), (1, 0));
        assert_eq!(observer.calls(MemoryLayer::Project), (1, 0));
    }
}
