use crate::*;
use async_trait::async_trait;
use std::{
    str::FromStr,
    sync::{Arc, RwLock},
};
use storage::api as storage_api;

const ACTIVE_MEMBER: &str = "active";
const ARCHIVE_MEMBER: &str = "archive";
/// Members are canonicalized into name order by Storage; "active" sorts before
/// "archive".
const MEMORY_MEMBER_NAMES: [&str; 2] = [ACTIVE_MEMBER, ARCHIVE_MEMBER];
/// Fixed, project-independent segment for the shared global layer generation.
const GLOBAL_DATASET_SEGMENT: &str = "global";

/// Memory-owned translation from the per-layer persistence contract to Storage
/// atomic datasets. Each `MemoryLayer` maps to its own dataset key with exactly
/// two members (`active`, `archive`) and therefore its own CAS revision.
///
/// The global layer uses a fixed, project-independent key, so every project
/// shares one global generation. The project layer uses the `ProjectMemoryKey`
/// dataset key, so distinct projects are fully isolated from one another.
///
/// Legacy-file discovery and migration are intentionally outside this adapter;
/// legacy classification remains follow-up work.
pub struct AtomicDatasetMemoryStore {
    storage: Arc<dyn storage_api::AtomicDatasetPort>,
    global: storage_api::DatasetKey,
    project: storage_api::DatasetKey,
}

impl AtomicDatasetMemoryStore {
    pub fn new(
        storage: Arc<dyn storage_api::AtomicDatasetPort>,
        project: ProjectMemoryKey,
    ) -> Self {
        let global = dataset_key(GLOBAL_DATASET_SEGMENT);
        let project = dataset_key(project.as_str());
        Self {
            storage,
            global,
            project,
        }
    }

    fn dataset_key_for(&self, layer: MemoryLayer) -> &storage_api::DatasetKey {
        match layer {
            MemoryLayer::Global => &self.global,
            MemoryLayer::Project => &self.project,
        }
    }

    async fn load_for_open(
        &self,
        layer: MemoryLayer,
    ) -> Result<CommittedMemoryDataset<storage_api::DatasetRevision>, MemoryOpenerError> {
        let dataset_key = self.dataset_key_for(layer);
        let manifest = self
            .storage
            .read_manifest(dataset_key)
            .await
            .map_err(map_storage_open_error)?;
        let revision = manifest.revision().clone();
        if manifest.members().is_empty() {
            return Ok(CommittedMemoryDataset {
                dataset: MemoryDataset::empty(layer),
                revision,
            });
        }
        let expected = expected_member_names();
        if manifest.members() != expected.as_slice() {
            return Err(MemoryOpenerError::CorruptTransaction);
        }
        let read = self
            .storage
            .read_consistent(dataset_key, &expected)
            .await
            .map_err(map_storage_open_error)?;
        let storage_api::DatasetReadOutcome::Found(read) = read else {
            return Err(MemoryOpenerError::CorruptTransaction);
        };
        if read.revision() != &revision {
            return Err(MemoryOpenerError::CorruptTransaction);
        }
        let bytes = |name: &str| {
            read.members()
                .iter()
                .find(|member| member.name().as_str() == name)
                .map(storage_api::DatasetMember::bytes)
                .ok_or(MemoryOpenerError::CorruptTransaction)
        };
        let dataset =
            crate::codec::decode_dataset(layer, bytes(ACTIVE_MEMBER)?, bytes(ARCHIVE_MEMBER)?)
                .map_err(map_codec_open_error)?;
        Ok(CommittedMemoryDataset { dataset, revision })
    }
}

fn map_codec_open_error(error: MemoryOpenError) -> MemoryOpenerError {
    match error {
        MemoryOpenError::UnsupportedSchema { version } => {
            MemoryOpenerError::UnsupportedSchema { version }
        }
        _ => MemoryOpenerError::CorruptDataset,
    }
}

fn map_storage_open_error(error: storage_api::StorageError) -> MemoryOpenerError {
    match map_storage_error(&error) {
        MemoryStorageErrorKind::PermissionDenied => MemoryOpenerError::PermissionDenied,
        MemoryStorageErrorKind::CorruptTransaction => MemoryOpenerError::CorruptTransaction,
        _ => MemoryOpenerError::Io,
    }
}

fn map_memory_open_error(error: MemoryError) -> MemoryOpenerError {
    match error {
        MemoryError::Storage {
            kind: MemoryStorageErrorKind::PermissionDenied,
        } => MemoryOpenerError::PermissionDenied,
        MemoryError::Storage {
            kind: MemoryStorageErrorKind::CorruptTransaction,
        } => MemoryOpenerError::CorruptTransaction,
        MemoryError::Storage {
            kind: MemoryStorageErrorKind::Io | MemoryStorageErrorKind::DiskFull,
        } => MemoryOpenerError::Io,
        _ => MemoryOpenerError::LegacyMigrationFailed,
    }
}

fn dataset_key(segment: &str) -> storage_api::DatasetKey {
    let segment = storage_api::SafePathSegment::from_str(segment)
        .expect("Memory dataset segment is always a safe Storage path segment");
    storage_api::DatasetKey::new(storage_api::StorageNamespace::Memory, vec![segment])
        .expect("Memory dataset segment always forms a valid dataset key")
}

/// Anti-corruption mapping: Storage's published failures do not cross the
/// Memory boundary.
pub fn map_storage_error(error: &storage_api::StorageError) -> MemoryStorageErrorKind {
    match error.kind() {
        storage_api::StorageErrorKind::PermissionDenied => MemoryStorageErrorKind::PermissionDenied,
        storage_api::StorageErrorKind::ConcurrentWrite => MemoryStorageErrorKind::ConcurrentWrite,
        storage_api::StorageErrorKind::CorruptTransaction(_) => {
            MemoryStorageErrorKind::CorruptTransaction
        }
        storage_api::StorageErrorKind::InvalidKey => MemoryStorageErrorKind::Serialization,
        storage_api::StorageErrorKind::Io
        | storage_api::StorageErrorKind::UnsupportedDurability => MemoryStorageErrorKind::Io,
    }
}

fn storage_error(error: storage_api::StorageError) -> MemoryError {
    MemoryError::Storage {
        kind: map_storage_error(&error),
    }
}

fn invalid_dataset(kind: MemoryStorageErrorKind) -> MemoryError {
    MemoryError::Storage { kind }
}

fn member_name(value: &str) -> storage_api::SafePathSegment {
    storage_api::SafePathSegment::from_str(value).expect("fixed Memory member name is safe")
}

fn expected_member_names() -> Vec<storage_api::SafePathSegment> {
    MEMORY_MEMBER_NAMES.into_iter().map(member_name).collect()
}

fn encode_members(
    layer: MemoryLayer,
    dataset: &MemoryDataset,
) -> Result<Vec<storage_api::DatasetMember>, MemoryError> {
    if dataset.layer() != layer {
        return Err(invalid_dataset(MemoryStorageErrorKind::Serialization));
    }
    let (active, archive) = crate::codec::encode_dataset(dataset)?;
    Ok(vec![
        storage_api::DatasetMember::new(member_name(ACTIVE_MEMBER), active),
        storage_api::DatasetMember::new(member_name(ARCHIVE_MEMBER), archive),
    ])
}

#[async_trait]
impl MemoryDatasetStore for AtomicDatasetMemoryStore {
    type Revision = storage_api::DatasetRevision;

    async fn load_committed(
        &self,
        layer: MemoryLayer,
    ) -> Result<CommittedMemoryDataset<Self::Revision>, MemoryError> {
        let dataset_key = self.dataset_key_for(layer);
        let manifest = self
            .storage
            .read_manifest(dataset_key)
            .await
            .map_err(storage_error)?;
        let revision = manifest.revision().clone();

        if manifest.members().is_empty() {
            return Ok(CommittedMemoryDataset {
                dataset: MemoryDataset::empty(layer),
                revision,
            });
        }

        let expected = expected_member_names();
        if manifest.members() != expected.as_slice() {
            return Err(invalid_dataset(MemoryStorageErrorKind::CorruptTransaction));
        }
        let read = self
            .storage
            .read_consistent(dataset_key, &expected)
            .await
            .map_err(storage_error)?;
        let storage_api::DatasetReadOutcome::Found(read) = read else {
            return Err(invalid_dataset(MemoryStorageErrorKind::CorruptTransaction));
        };
        if read.revision() != &revision {
            return Err(invalid_dataset(MemoryStorageErrorKind::ConcurrentWrite));
        }

        let bytes = |name: &str| {
            read.members()
                .iter()
                .find(|member| member.name().as_str() == name)
                .map(storage_api::DatasetMember::bytes)
                .ok_or_else(|| invalid_dataset(MemoryStorageErrorKind::CorruptTransaction))
        };
        let dataset =
            crate::codec::decode_dataset(layer, bytes(ACTIVE_MEMBER)?, bytes(ARCHIVE_MEMBER)?)
                .map_err(|_| invalid_dataset(MemoryStorageErrorKind::Serialization))?;

        Ok(CommittedMemoryDataset { dataset, revision })
    }

    async fn commit(
        &self,
        layer: MemoryLayer,
        expected: &Self::Revision,
        dataset: &MemoryDataset,
    ) -> Result<MemoryCommitReceipt<Self::Revision>, MemoryError> {
        let members = encode_members(layer, dataset)?;
        let receipt = self
            .storage
            .commit_atomic(
                self.dataset_key_for(layer),
                expected,
                &members,
                storage_api::WriteOptions::new(storage_api::Durability::ProcessCrashSafe),
            )
            .await
            .map_err(storage_error)?;
        let visibility = match receipt.visibility() {
            storage_api::DatasetCommitVisibility::Visible => MemoryCommitVisibility::Visible,
            storage_api::DatasetCommitVisibility::RecoveryPending => {
                MemoryCommitVisibility::RecoveryPending
            }
        };
        Ok(MemoryCommitReceipt::new(
            receipt.revision().clone(),
            visibility,
        ))
    }
}

pub struct ProjectMemoryOpener {
    store: AtomicDatasetMemoryStore,
    legacy: Arc<dyn LegacyMemorySource>,
}

impl ProjectMemoryOpener {
    pub fn new(store: AtomicDatasetMemoryStore, legacy: Arc<dyn LegacyMemorySource>) -> Self {
        Self { store, legacy }
    }

    pub async fn open(
        self,
        policy: MemoryPolicy,
    ) -> Result<MemoryService<AtomicDatasetMemoryStore>, MemoryOpenerError> {
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let committed = self.store.load_for_open(layer).await?;
            let legacy = self
                .legacy
                .probe(layer)
                .await
                .map_err(|error| match error {
                    LegacyMemorySourceError::PermissionDenied => {
                        MemoryOpenerError::PermissionDenied
                    }
                    LegacyMemorySourceError::Io => MemoryOpenerError::Io,
                })?;
            if !legacy.is_present() {
                continue;
            }
            let new_is_empty =
                committed.dataset.active().is_empty() && committed.dataset.archive().is_empty();
            if !new_is_empty {
                return Err(MemoryOpenerError::LegacyKeyConflict);
            }
            let active = match &legacy.active {
                LegacyMemoryMember::Missing => None,
                LegacyMemoryMember::Present(bytes) => Some(bytes.as_slice()),
            };
            let archive = match &legacy.archive {
                LegacyMemoryMember::Missing => None,
                LegacyMemoryMember::Present(bytes) => Some(bytes.as_slice()),
            };
            let dataset = crate::codec::decode_legacy_dataset(layer, active, archive)
                .map_err(map_codec_open_error)?;
            self.store
                .commit(layer, &committed.revision, &dataset)
                .await
                .map_err(map_memory_open_error)?;
        }
        MemoryService::open(self.store, policy)
            .await
            .map_err(map_memory_open_error)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryPolicy {
    pub max_entries: usize,
    pub similarity_threshold: f64,
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        Self {
            max_entries: 100,
            similarity_threshold: 0.8,
        }
    }
}

#[derive(Default)]
struct MemoryState {
    active: Vec<MemoryEntry>,
    archive: Vec<MemoryEntry>,
    revision: u64,
}

pub struct InMemoryMemory {
    policy: MemoryPolicy,
    state: RwLock<MemoryState>,
    clock: crate::service::MemoryClock,
}

impl InMemoryMemory {
    pub fn new(policy: MemoryPolicy) -> Result<Self, MemoryError> {
        Self::new_with_clock(policy, crate::service::system_time_seconds)
    }

    pub fn new_with_clock(
        policy: MemoryPolicy,
        clock: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Result<Self, MemoryError> {
        if policy.max_entries == 0 {
            return Err(MemoryError::InvalidEntry {
                message: "max_entries 必须大于 0".to_string(),
            });
        }
        if !(0.0..=1.0).contains(&policy.similarity_threshold) {
            return Err(MemoryError::InvalidEntry {
                message: "similarity_threshold 必须在 0 到 1 之间".to_string(),
            });
        }
        Ok(Self {
            policy,
            state: RwLock::new(MemoryState::default()),
            clock: Arc::new(clock),
        })
    }

    pub fn revision(&self) -> u64 {
        self.state
            .read()
            .expect("memory state lock poisoned")
            .revision
    }
}

#[async_trait]
impl MemoryPort for InMemoryMemory {
    fn retrieve_for_inject(&self, query: &MemoryQuery) -> MemorySearchResult {
        let state = self.state.read().expect("memory state lock poisoned");
        let mut entries = state
            .active
            .iter()
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
        let state = self.state.read().expect("memory state lock poisoned");
        let active = state
            .active
            .iter()
            .map(|entry| (entry, MemoryLocation::Active));
        let archive = state
            .archive
            .iter()
            .map(|entry| (entry, MemoryLocation::Archive));
        let entries: Box<dyn Iterator<Item = (&MemoryEntry, MemoryLocation)>> =
            if query.include_archive {
                Box::new(active.chain(archive))
            } else {
                Box::new(active)
            };
        let mut hits = entries
            .filter(|(entry, _)| matches_filters(entry, query.layer, query.category))
            .filter_map(|(entry, location)| {
                relevance(entry, &query.text).map(|relevance| MemorySearchHit {
                    entry: entry.clone(),
                    location,
                    outdated: entry.outdated,
                    ttl_expired: entry.is_ttl_expired(query.now),
                    relevance: Some(relevance),
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

    async fn write(&self, mut entry: MemoryEntry) -> Result<WriteResult, MemoryError> {
        validate_content(&entry.content)?;
        let mut state = self.state.write().expect("memory state lock poisoned");
        if state.active.iter().any(|stored| stored.id == entry.id)
            || state.archive.iter().any(|stored| stored.id == entry.id)
        {
            return Err(MemoryError::InvalidEntry {
                message: "记忆 ID 必须唯一".to_string(),
            });
        }
        if let Some(existing) = state.active.iter_mut().find(|stored| {
            stored.layer == entry.layer
                && jaccard_similarity(&stored.content, &entry.content)
                    >= self.policy.similarity_threshold
        }) {
            existing.tags.append(&mut entry.tags);
            existing.tags.sort();
            existing.tags.dedup();
            existing.accessed_at = entry.created_at;
            existing.access_count = existing.access_count.saturating_add(1);
            let existing_id = existing.id;
            state.revision = state.revision.saturating_add(1);
            return Ok(WriteResult::Merged { existing_id });
        }
        let layer_entries = state
            .active
            .iter()
            .filter(|stored| stored.layer == entry.layer)
            .cloned()
            .collect::<Vec<_>>();
        if layer_entries.len() >= self.policy.max_entries {
            return Ok(WriteResult::NeedsEviction {
                candidates: eviction_candidates(&layer_entries, 3, entry.created_at),
            });
        }
        let id = entry.id;
        state.active.push(entry);
        state.revision = state.revision.saturating_add(1);
        Ok(WriteResult::Added { id })
    }

    async fn update(&self, id: &MemoryId, content: &str) -> Result<bool, MemoryError> {
        validate_content(content)?;
        self.mutate_entry(id, |entry| entry.content = content.to_string())
    }

    async fn delete(&self, id: &MemoryId) -> Result<bool, MemoryError> {
        let mut state = self.state.write().expect("memory state lock poisoned");
        let before = state.active.len();
        state.active.retain(|entry| &entry.id != id);
        if state.active.len() == before {
            return Ok(false);
        }
        state.revision = state.revision.saturating_add(1);
        Ok(true)
    }

    async fn pin(&self, id: &MemoryId, pinned: bool) -> Result<bool, MemoryError> {
        self.mutate_entry(id, |entry| entry.pinned = pinned)
    }

    async fn mark_outdated(&self, id: &MemoryId) -> Result<bool, MemoryError> {
        self.mutate_entry(id, |entry| entry.outdated = true)
    }

    async fn apply_reflection(
        &self,
        output: &ReflectionOutput,
    ) -> Result<ReflectionApplyResult, MemoryError> {
        let mut result = ReflectionApplyResult::default();
        for suggestion in &output.suggested_memories {
            let now = (self.clock)();
            let id = reflection_memory_id(now)?;
            let mut entry = MemoryEntry::new(
                id,
                now,
                suggestion.layer,
                suggestion.category,
                suggestion.content.clone(),
                MemorySource::Llm,
            )?;
            entry.tags = suggestion.tags.clone();

            let mut state = self.state.write().expect("memory state lock poisoned");
            apply_reflection_entry(&mut state, entry, self.policy)?;
            state.revision = state.revision.saturating_add(1);
            result.suggestions_added += 1;
        }

        for raw_id in &output.outdated_memories {
            let id = MemoryId::new(raw_id)?;
            let mut state = self.state.write().expect("memory state lock poisoned");
            if let Some(entry) = state.active.iter_mut().find(|entry| entry.id == id) {
                entry.outdated = true;
                state.revision = state.revision.saturating_add(1);
                result.outdated_marked += 1;
            }
        }
        Ok(result)
    }

    async fn archive(&self, ids: &[MemoryId]) -> Result<(), MemoryError> {
        let mut state = self.state.write().expect("memory state lock poisoned");
        let mut moved = Vec::new();
        state.active.retain(|entry| {
            if ids.contains(&entry.id) && !entry.pinned {
                moved.push(entry.clone());
                false
            } else {
                true
            }
        });
        if !moved.is_empty() {
            state.archive.extend(moved);
            state.revision = state.revision.saturating_add(1);
        }
        Ok(())
    }

    async fn compact(&self) -> Result<CompactResult, MemoryError> {
        let mut state = self.state.write().expect("memory state lock poisoned");
        let mut candidates = Vec::new();
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let layer_entries = state
                .active
                .iter()
                .filter(|entry| entry.layer == layer)
                .cloned()
                .collect::<Vec<_>>();
            let excess = layer_entries.len().saturating_sub(self.policy.max_entries);
            candidates.extend(eviction_candidates(
                &layer_entries,
                excess,
                layer_entries
                    .iter()
                    .map(|entry| entry.accessed_at)
                    .max()
                    .unwrap_or(0),
            ));
        }
        let ids = candidates.iter().map(|entry| entry.id).collect::<Vec<_>>();
        let mut moved = Vec::new();
        state.active.retain(|entry| {
            if ids.contains(&entry.id) {
                moved.push(entry.clone());
                false
            } else {
                true
            }
        });
        let archived = moved.len();
        if archived > 0 {
            state.archive.extend(moved);
            state.revision = state.revision.saturating_add(1);
        }
        Ok(CompactResult {
            archived,
            remaining: state.active.len(),
        })
    }

    fn list(&self, layer: Option<MemoryLayer>) -> Vec<MemoryEntry> {
        self.state
            .read()
            .expect("memory state lock poisoned")
            .active
            .iter()
            .filter(|entry| layer.is_none_or(|layer| entry.layer == layer))
            .cloned()
            .collect()
    }

    fn stats(&self) -> MemoryStats {
        let state = self.state.read().expect("memory state lock poisoned");
        MemoryStats {
            global_count: count_layer(&state.active, MemoryLayer::Global),
            global_archive_count: count_layer(&state.archive, MemoryLayer::Global),
            project_count: count_layer(&state.active, MemoryLayer::Project),
            project_archive_count: count_layer(&state.archive, MemoryLayer::Project),
        }
    }
}

impl InMemoryMemory {
    fn mutate_entry(
        &self,
        id: &MemoryId,
        mutation: impl FnOnce(&mut MemoryEntry),
    ) -> Result<bool, MemoryError> {
        let mut state = self.state.write().expect("memory state lock poisoned");
        let Some(entry) = state.active.iter_mut().find(|entry| &entry.id == id) else {
            return Ok(false);
        };
        mutation(entry);
        state.revision = state.revision.saturating_add(1);
        Ok(true)
    }
}

fn reflection_memory_id(now: u64) -> Result<MemoryId, MemoryError> {
    let timestamp = uuid::Timestamp::from_unix_time(now, 0, 0, 0);
    MemoryId::new(uuid::Uuid::new_v7(timestamp).to_string())
}

fn reflection_capacity_error() -> MemoryError {
    MemoryError::InvalidEntry {
        message: "Reflection 淘汰非 pinned 候选后重试一次仍超过记忆容量".to_string(),
    }
}

fn apply_reflection_entry(
    state: &mut MemoryState,
    mut entry: MemoryEntry,
    policy: MemoryPolicy,
) -> Result<(), MemoryError> {
    validate_content(&entry.content)?;
    if state.active.iter().any(|stored| stored.id == entry.id)
        || state.archive.iter().any(|stored| stored.id == entry.id)
    {
        return Err(MemoryError::InvalidEntry {
            message: "记忆 ID 必须唯一".to_string(),
        });
    }
    if let Some(existing) = state.active.iter_mut().find(|stored| {
        stored.layer == entry.layer
            && jaccard_similarity(&stored.content, &entry.content) >= policy.similarity_threshold
    }) {
        existing.tags.append(&mut entry.tags);
        existing.tags.sort();
        existing.tags.dedup();
        existing.accessed_at = entry.created_at;
        existing.access_count = existing.access_count.saturating_add(1);
        return Ok(());
    }
    let layer_entries = state
        .active
        .iter()
        .filter(|stored| stored.layer == entry.layer)
        .cloned()
        .collect::<Vec<_>>();
    if layer_entries.len() >= policy.max_entries {
        let candidates = eviction_candidates(&layer_entries, 3, entry.created_at);
        let ids = candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>();
        let mut moved = Vec::new();
        state.active.retain(|stored| {
            if ids.contains(&stored.id) && !stored.pinned {
                moved.push(stored.clone());
                false
            } else {
                true
            }
        });
        state.archive.extend(moved);
        let remaining = state
            .active
            .iter()
            .filter(|stored| stored.layer == entry.layer)
            .count();
        if remaining >= policy.max_entries {
            return Err(reflection_capacity_error());
        }
    }
    state.active.push(entry);
    Ok(())
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

fn count_layer(entries: &[MemoryEntry], layer: MemoryLayer) -> usize {
    entries.iter().filter(|entry| entry.layer == layer).count()
}
