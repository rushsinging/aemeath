use super::constants::{
    ACTIVE_MEMBER, ARCHIVE_MEMBER, GLOBAL_DATASET_SEGMENT, LEGACY_ARCHIVE_SUFFIX, LEGACY_FILE_EXT,
    LEGACY_GLOBAL_STEM, MEMORY_MEMBER_NAMES,
};
use crate::domain::constants::MIN_SYNTHESIS_EVIDENCE;
use crate::domain::*;
use crate::ports::*;
use crate::service::MemoryService;
use crate::write_lock::{MemoryWritePermit, OsWriteLockHandle, WriteGate};
use async_trait::async_trait;
use std::{
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, RwLock},
};
use storage as storage_api;

#[cfg(test)]
#[path = "adapters_tests.rs"]
mod tests;

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
pub(crate) struct AtomicDatasetMemoryStore {
    storage: Arc<dyn storage_api::AtomicDatasetPort>,
    global: storage_api::DatasetKeyData,
    project: storage_api::DatasetKeyData,
    /// agents_dir：corpus `write.lock` 根；None 时仅进程内门闩（测试 Scripted/旧构造）。
    lock_root: Option<storage_api::SafeStorageRoot>,
}

impl AtomicDatasetMemoryStore {
    /// 无 OS 文件锁根的便捷构造（进程内门闩）；仅测试使用，生产走 `with_lock_root`。
    #[cfg(test)]
    pub fn new(
        storage: Arc<dyn storage_api::AtomicDatasetPort>,
        project: ProjectMemoryKey,
    ) -> Self {
        Self::with_lock_root(storage, project, None)
    }

    pub fn with_lock_root(
        storage: Arc<dyn storage_api::AtomicDatasetPort>,
        project: ProjectMemoryKey,
        lock_root: Option<storage_api::SafeStorageRoot>,
    ) -> Self {
        let global = dataset_key(GLOBAL_DATASET_SEGMENT);
        let project = dataset_key(project.as_str());
        Self {
            storage,
            global,
            project,
            lock_root,
        }
    }

    fn dataset_key_for(&self, layer: MemoryLayer) -> &storage_api::DatasetKeyData {
        match layer {
            MemoryLayer::Global => &self.global,
            MemoryLayer::Project => &self.project,
        }
    }

    async fn load_for_open(
        &self,
        layer: MemoryLayer,
    ) -> Result<CommittedMemoryDataset<storage_api::DatasetRevisionData>, MemoryOpenerError> {
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
        let storage_api::DatasetReadOutcomeData::Found(read) = read else {
            return Err(MemoryOpenerError::CorruptTransaction);
        };
        if read.revision() != &revision {
            return Err(MemoryOpenerError::CorruptTransaction);
        }
        let bytes = |name: &str| {
            read.members()
                .iter()
                .find(|member| member.name().as_str() == name)
                .map(storage_api::DatasetMemberData::bytes)
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

fn lock_corpus_via_root(
    root: &storage_api::SafeStorageRoot,
    segment: &str,
) -> Result<OsWriteLockHandle, MemoryError> {
    use fs2::FileExt;
    use std::str::FromStr;

    let memory =
        storage_api::SafePathSegmentData::from_str("memory").map_err(|_| MemoryError::Storage {
            kind: MemoryStorageErrorKind::Io,
        })?;
    let segment =
        storage_api::SafePathSegmentData::from_str(segment).map_err(|_| MemoryError::Storage {
            kind: MemoryStorageErrorKind::Io,
        })?;
    let dir = root
        .ensure_dir(&[memory, segment])
        .map_err(|_| MemoryError::Storage {
            kind: MemoryStorageErrorKind::Io,
        })?;
    let name = storage_api::SafePathSegmentData::from_str(crate::constants::WRITE_LOCK_FILE)
        .map_err(|_| MemoryError::Storage {
            kind: MemoryStorageErrorKind::Io,
        })?;
    let file = dir
        .create_or_open(
            &name,
            storage_api::SafeOpenOptions {
                // create 需要 write/append；锁文件用 append 仅为拿到可写 fd。
                read: true,
                append: true,
            },
        )
        .map_err(|_| MemoryError::Storage {
            kind: MemoryStorageErrorKind::Io,
        })?;
    file.lock_exclusive().map_err(|_| MemoryError::Storage {
        kind: MemoryStorageErrorKind::Io,
    })?;
    Ok(OsWriteLockHandle::from_file(file))
}

fn dataset_key(segment: &str) -> storage_api::DatasetKeyData {
    let segment = storage_api::SafePathSegmentData::from_str(segment)
        .expect("Memory dataset segment is always a safe Storage path segment");
    storage_api::DatasetKeyData::new(storage_api::StorageNamespaceData::Memory, vec![segment])
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

fn member_name(value: &str) -> storage_api::SafePathSegmentData {
    storage_api::SafePathSegmentData::from_str(value).expect("fixed Memory member name is safe")
}

fn expected_member_names() -> Vec<storage_api::SafePathSegmentData> {
    MEMORY_MEMBER_NAMES.into_iter().map(member_name).collect()
}

fn encode_members(
    layer: MemoryLayer,
    dataset: &MemoryDataset,
) -> Result<Vec<storage_api::DatasetMemberData>, MemoryError> {
    if dataset.layer() != layer {
        return Err(invalid_dataset(MemoryStorageErrorKind::Serialization));
    }
    let (active, archive) = crate::codec::encode_dataset(dataset)?;
    Ok(vec![
        storage_api::DatasetMemberData::new(member_name(ACTIVE_MEMBER), active),
        storage_api::DatasetMemberData::new(member_name(ARCHIVE_MEMBER), archive),
    ])
}

#[async_trait]
impl MemoryDatasetStore for AtomicDatasetMemoryStore {
    type Revision = storage_api::DatasetRevisionData;

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
        let storage_api::DatasetReadOutcomeData::Found(read) = read else {
            return Err(invalid_dataset(MemoryStorageErrorKind::CorruptTransaction));
        };
        if read.revision() != &revision {
            return Err(invalid_dataset(MemoryStorageErrorKind::ConcurrentWrite));
        }

        let bytes = |name: &str| {
            read.members()
                .iter()
                .find(|member| member.name().as_str() == name)
                .map(storage_api::DatasetMemberData::bytes)
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
                storage_api::WriteOptionsData::new(storage_api::DurabilityData::ProcessCrashSafe),
            )
            .await
            .map_err(storage_error)?;
        let visibility = match receipt.visibility() {
            storage_api::DatasetCommitVisibilityData::Visible => MemoryCommitVisibility::Visible,
            storage_api::DatasetCommitVisibilityData::RecoveryPending => {
                MemoryCommitVisibility::RecoveryPending
            }
        };
        Ok(MemoryCommitReceipt::new(
            receipt.revision().clone(),
            visibility,
        ))
    }

    async fn acquire_os_write_locks(&self) -> Result<Vec<OsWriteLockHandle>, MemoryError> {
        let Some(root) = &self.lock_root else {
            return Ok(Vec::new());
        };
        let mut locks = Vec::with_capacity(2);
        // 锁序固定：global → project，避免跨 session ABBA。
        for key in [&self.global, &self.project] {
            let segment = key
                .segments()
                .first()
                .map(|segment| segment.as_str())
                .ok_or(MemoryError::Storage {
                    kind: MemoryStorageErrorKind::Io,
                })?;
            locks.push(lock_corpus_via_root(root, segment)?);
        }
        Ok(locks)
    }
}

pub(crate) struct ProjectMemoryOpener {
    store: AtomicDatasetMemoryStore,
    legacy: Arc<dyn LegacyMemorySource>,
    /// System One 评分端口（None = 重排关闭）。
    scorer: Option<Arc<dyn systemone::ScoringPort>>,
    /// 事件追加端口（None = `NoopEventAppend`，仅测试/降级路径）。
    events: Option<Arc<dyn MemoryEventAppendPort>>,
}

impl ProjectMemoryOpener {
    pub fn new(store: AtomicDatasetMemoryStore, legacy: Arc<dyn LegacyMemorySource>) -> Self {
        Self {
            store,
            legacy,
            scorer: None,
            events: None,
        }
    }

    /// 装配评分端口（composition 在场景开关开启时调用）。
    pub fn with_scorer(mut self, scorer: Option<Arc<dyn systemone::ScoringPort>>) -> Self {
        self.scorer = scorer;
        self
    }

    /// 装配事件追加端口（`DatasetMemoryOpener` 注入真 event store）。
    pub fn with_events(mut self, events: Arc<dyn MemoryEventAppendPort>) -> Self {
        self.events = Some(events);
        self
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
        // 事件端口缺省（None）走既有 Noop 默认双入口，语义与接线前完全一致；
        // 注入真 event store（生产路径）时走全参入口（system clock + scorer + events）。
        match (self.scorer, self.events) {
            (scorer, Some(events)) => {
                MemoryService::open_with_clock_and_scorer_and_events(
                    self.store,
                    policy,
                    crate::service::system_time_seconds,
                    scorer,
                    events,
                )
                .await
            }
            (None, None) => MemoryService::open(self.store, policy).await,
            (Some(scorer), None) => {
                MemoryService::open_with_scorer(self.store, policy, Some(scorer)).await
            }
        }
        .map_err(map_memory_open_error)
    }
}

/// Concrete [`MemoryOpener`] backed by atomic-dataset Storage. Cloning shares
/// the same Storage port and legacy source factory, so each clone can
/// independently open Memory for any project identity.
///
/// The opener holds a [`LegacyMemorySourceFactory`] rather than a single
/// [`LegacyMemorySource`] because legacy file positions are project-specific
/// (the project layer's legacy file stem is derived from the caller's cwd).
/// For each `open_memory` call the factory creates a fresh source pre-bound
/// to the correct project positions; `probe(layer)` then only needs the
/// layer argument.
///
/// This reuses [`ProjectMemoryOpener`] for the actual eager-open and legacy
/// migration logic; the seam only adds the project-key-scoped store
/// construction and the `MemoryConfig` → [`MemoryPolicy`] mapping so that
/// callers obtain `Arc<dyn MemoryPort>` without depending on concrete Memory
/// types.
#[derive(Clone)]
pub(crate) struct DatasetMemoryOpener {
    storage: Arc<dyn storage_api::AtomicDatasetPort>,
    legacy_factory: Arc<dyn LegacyMemorySourceFactory>,
    /// System One 评分端口（None = 重排关闭）。
    scorer: Option<Arc<dyn systemone::ScoringPort>>,
    /// 事件 jsonl 根（`memory/{project}/events/…` 相对此根；与 dataset 根
    /// 同为 agents_dir）。retention 天数在 `open_memory` 时取 candidate config。
    event_root: storage_api::SafeStorageRoot,
}

impl DatasetMemoryOpener {
    pub(crate) fn new(
        storage: Arc<dyn storage_api::AtomicDatasetPort>,
        legacy_factory: Arc<dyn LegacyMemorySourceFactory>,
        event_root: storage_api::SafeStorageRoot,
    ) -> Self {
        Self {
            storage,
            legacy_factory,
            scorer: None,
            event_root,
        }
    }

    pub(crate) fn with_scorer(mut self, scorer: Option<Arc<dyn systemone::ScoringPort>>) -> Self {
        self.scorer = scorer;
        self
    }
}

#[async_trait]
impl MemoryOpener for DatasetMemoryOpener {
    async fn open_memory(
        &self,
        key: &ProjectMemoryKey,
        config: &share::config::MemoryConfig,
    ) -> Result<Arc<dyn MemoryPort>, MemoryOpenerError> {
        let store = AtomicDatasetMemoryStore::with_lock_root(
            Arc::clone(&self.storage),
            key.clone(),
            Some(self.event_root.clone()),
        );
        let policy = MemoryPolicy {
            max_entries: config.max_entries,
            similarity_threshold: config.similarity_threshold,
            target_active_entries: config.reflection.target_active_entries,
        };
        let legacy = self.legacy_factory.create_for(key);
        // 生产路径注入真 event store：retention 取 candidate config（`0` = 禁用 GC），
        // 构造期已 fail-open 触发一次 GC。
        let events = crate::wire_memory_event_store(
            self.event_root.clone(),
            key.clone(),
            config.event_retention_days,
        );
        let service = ProjectMemoryOpener::new(store, legacy)
            .with_scorer(self.scorer.clone())
            .with_events(events)
            .open(policy)
            .await?;
        Ok(Arc::new(service))
    }

    fn boxed_clone(&self) -> Box<dyn MemoryOpener> {
        Box::new(self.clone())
    }
}

/// Production [`LegacyMemorySource`] backed by the predecessor memory file
/// layout (`~/.agents/memory/`).
///
/// A single instance is **pre-bound** at construction to the four file
/// positions (global active/archive, project active/archive) for exactly one
/// project. [`LegacyMemorySource::probe`] then only needs the `layer` argument
/// to select which pair to read — it never sees a filesystem path. Files are
/// plain JSON arrays of entries; a missing file maps to [`LegacyMemoryMember::Missing`].
struct FileLegacyMemorySource {
    global_active: PathBuf,
    global_archive: PathBuf,
    project_active: PathBuf,
    project_archive: PathBuf,
}

impl FileLegacyMemorySource {
    fn new(base_dir: &Path, key: &ProjectMemoryKey) -> Self {
        let project_stem = key.legacy_project_name();
        Self {
            global_active: base_dir.join(format!("{LEGACY_GLOBAL_STEM}{LEGACY_FILE_EXT}")),
            global_archive: base_dir.join(format!(
                "{LEGACY_GLOBAL_STEM}{LEGACY_ARCHIVE_SUFFIX}{LEGACY_FILE_EXT}"
            )),
            project_active: base_dir.join(format!("{project_stem}{LEGACY_FILE_EXT}")),
            project_archive: base_dir.join(format!(
                "{project_stem}{LEGACY_ARCHIVE_SUFFIX}{LEGACY_FILE_EXT}"
            )),
        }
    }

    fn paths_for(&self, layer: MemoryLayer) -> (&Path, &Path) {
        match layer {
            MemoryLayer::Global => (&self.global_active, &self.global_archive),
            MemoryLayer::Project => (&self.project_active, &self.project_archive),
        }
    }
}

#[async_trait]
impl LegacyMemorySource for FileLegacyMemorySource {
    async fn probe(
        &self,
        layer: MemoryLayer,
    ) -> Result<LegacyMemoryLayer, LegacyMemorySourceError> {
        let (active_path, archive_path) = self.paths_for(layer);
        let active = read_legacy_member(active_path)?;
        let archive = read_legacy_member(archive_path)?;
        Ok(LegacyMemoryLayer { active, archive })
    }
}

fn read_legacy_member(path: &Path) -> Result<LegacyMemoryMember, LegacyMemorySourceError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(LegacyMemoryMember::Present(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(LegacyMemoryMember::Missing)
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(LegacyMemorySourceError::PermissionDenied)
        }
        Err(_) => Err(LegacyMemorySourceError::Io),
    }
}

/// Production [`LegacyMemorySourceFactory`]. Reads old-format JSON files from
/// the legacy memory directory layout.
///
/// The factory holds only the legacy base directory. When [`create_for`] is
/// called with a [`ProjectMemoryKey`], it resolves the project-specific file
/// stems (from the key's cwd-derived name) and returns a [`FileLegacyMemorySource`]
/// pre-bound to those positions. No path ever crosses the port boundary — the
/// created source exposes only bytes through `probe`.
///
/// [`create_for`]: LegacyMemorySourceFactory::create_for
#[derive(Clone)]
pub(crate) struct FileLegacyMemorySourceFactory {
    base_dir: PathBuf,
}

impl FileLegacyMemorySourceFactory {
    pub(crate) fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }
}

impl LegacyMemorySourceFactory for FileLegacyMemorySourceFactory {
    fn create_for(&self, key: &ProjectMemoryKey) -> Arc<dyn LegacyMemorySource> {
        Arc::new(FileLegacyMemorySource::new(&self.base_dir, key))
    }

    fn boxed_clone(&self) -> Box<dyn LegacyMemorySourceFactory> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryPolicy {
    pub max_entries: usize,
    pub similarity_threshold: f64,
    /// Reflect 收敛目标 active 条数；与 max_entries 差值作 add 缓冲。
    pub target_active_entries: usize,
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        let defaults = share::config::MemoryConfig::default();
        Self {
            max_entries: defaults.max_entries,
            similarity_threshold: defaults.similarity_threshold,
            target_active_entries: defaults.reflection.target_active_entries,
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
    write_gate: Arc<WriteGate>,
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
            write_gate: WriteGate::new(),
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
    async fn acquire_write_permit(&self) -> Result<MemoryWritePermit, MemoryError> {
        Ok(self.write_gate.acquire(Vec::new()).await)
    }

    async fn retrieve_for_inject(&self, query: &MemoryQuery) -> MemorySearchResult {
        let state = self.state.read().expect("memory state lock poisoned");
        let mut entries = state
            .active
            .iter()
            .filter(|entry| matches_filters(entry, query.layer, query.category))
            .filter(|entry| is_injection_eligible(entry, query.now))
            .cloned()
            .collect::<Vec<_>>();
        order_for_injection(&mut entries, query.now);
        entries.truncate(query.limit);
        MemorySearchResult {
            mode: MemoryRetrievalMode::InjectionPriority,
            hits: entries
                .into_iter()
                .map(|entry| MemorySearchHit {
                    superseded_by: entry.superseded_by,
                    entry,
                    location: MemoryLocation::Active,
                    outdated: false,
                    ttl_expired: false,
                    relevance: None,
                })
                .collect(),
        }
    }

    async fn search(&self, query: &MemorySearchQuery) -> MemorySearchResult {
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
        let candidates = entries
            .filter(|(entry, _)| matches_filters(entry, query.layer, query.category))
            .filter(|(entry, _)| !query.exclude_stale || is_injection_eligible(entry, query.now));
        let hits = crate::domain::lexical_search::rank_explicit_search(candidates, query);
        MemorySearchResult {
            mode: MemoryRetrievalMode::ExplicitSearch,
            hits,
        }
    }

    async fn write(&self, mut entry: MemoryEntry) -> Result<WriteResult, MemoryError> {
        let _permit = self.acquire_write_permit().await?;
        validate_content(&entry.content)?;
        let mut state = self.state.write().expect("memory state lock poisoned");
        if state.active.iter().any(|stored| stored.id == entry.id)
            || state.archive.iter().any(|stored| stored.id == entry.id)
        {
            return Err(MemoryError::InvalidEntry {
                message: "记忆 ID 必须唯一".to_string(),
            });
        }
        // M11: every evidence pointer must resolve from day one.
        if entry.evidence.iter().any(|source| {
            !state
                .active
                .iter()
                .chain(state.archive.iter())
                .any(|stored| &stored.id == source)
        }) {
            return Err(MemoryError::InvalidEntry {
                message: "证据指针指向不存在的记忆".to_string(),
            });
        }
        let dedup_hit = state
            .active
            .iter_mut()
            .find(|stored| {
                stored.layer == entry.layer
                    && jaccard_similarity(&stored.content, &entry.content)
                        >= self.policy.similarity_threshold
            })
            .map(|existing| {
                existing.tags.append(&mut entry.tags);
                existing.tags.sort();
                existing.tags.dedup();
                existing.last_confirmed_at = entry.created_at;
                existing.confirmation_count = existing.confirmation_count.saturating_add(1);
                existing.id
            });
        if let Some(existing_id) = dedup_hit {
            // #1775: archive the incoming entry instead of dropping it, and
            // let the survivor keep a pointer to it.
            let incoming_id = entry.id;
            state.archive.push(entry);
            if let Some(existing) = state
                .active
                .iter_mut()
                .find(|stored| stored.id == existing_id)
            {
                existing.evidence.push(incoming_id);
            }
            state.revision = state.revision.saturating_add(1);
            return Ok(WriteResult::Merged { existing_id });
        }
        let layer_entries = state
            .active
            .iter()
            .filter(|stored| stored.layer == entry.layer)
            .cloned()
            .collect::<Vec<_>>();
        if crate::domain::effective_active_count(&layer_entries) >= self.policy.max_entries {
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
        let _permit = self.acquire_write_permit().await?;
        validate_content(content)?;
        self.mutate_entry(id, |entry| entry.content = content.to_string())
    }

    async fn delete(&self, id: &MemoryId) -> Result<bool, MemoryError> {
        let _permit = self.acquire_write_permit().await?;
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
        let _permit = self.acquire_write_permit().await?;
        self.mutate_entry(id, |entry| entry.pinned = pinned)
    }

    async fn mark_outdated(&self, id: &MemoryId) -> Result<bool, MemoryError> {
        let _permit = self.acquire_write_permit().await?;
        self.mutate_entry(id, |entry| entry.outdated = true)
    }

    async fn apply_reflection(
        &self,
        output: &ReflectionOutput,
    ) -> Result<ReflectionApplyResult, MemoryError> {
        let permit = self.acquire_write_permit().await?;
        self.apply_reflection_under_permit(output, &permit).await
    }

    async fn apply_reflection_under_permit(
        &self,
        output: &ReflectionOutput,
        _permit: &MemoryWritePermit,
    ) -> Result<ReflectionApplyResult, MemoryError> {
        // 非法引用跳过；整批在候选态试算，project active ≤ target 才提交，否则 CapacityNotMet 整批不写。
        let mut outdated_ids = Vec::with_capacity(output.outdated_memories.len());
        for raw_id in &output.outdated_memories {
            match MemoryId::new(raw_id) {
                Ok(id) => outdated_ids.push(id),
                Err(_) => log::warn!(
                    target: crate::LOG_TARGET,
                    "memory_reflection_reference_skipped field=outdated token={} len={}",
                    crate::domain::truncate_reference_token(raw_id, 40),
                    raw_id.chars().count(),
                ),
            }
        }

        let mut result = ReflectionApplyResult {
            attempted: output.suggested_memories.len() + outdated_ids.len(),
            ..ReflectionApplyResult::default()
        };

        let mut state = self.state.write().expect("memory state lock poisoned");
        let mut candidate = MemoryState {
            active: state.active.clone(),
            archive: state.archive.clone(),
            revision: state.revision,
        };

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
            if !apply_synthesis(&mut entry, &suggestion.synthesizes)
                && !suggestion.synthesizes.is_empty()
            {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "memory_reflection_synthesis_demoted evidence_count={} min={}",
                    suggestion.synthesizes.len(),
                    MIN_SYNTHESIS_EVIDENCE,
                );
            }

            let surviving = apply_reflection_entry(&mut candidate, entry, self.policy)?;
            candidate.revision = candidate.revision.saturating_add(1);
            result.suggestions_added += 1;
            result.completed += 1;
            if let Some(surviving) = surviving {
                result.attempted += suggestion.supersedes.len();
                let established = establish_supersede_relations(
                    &mut candidate,
                    surviving,
                    &suggestion.supersedes,
                );
                result.completed += established;
                result.superseded += established;
            }
        }

        for id in &outdated_ids {
            // Reflect outdated：标 outdated 并移出 active（归档），否则物理条数无法收敛到 target。
            // pinned 与 archive 工具一致：不可归档，跳过并计入 attempted/completed。
            let mut moved = None;
            candidate.active.retain(|entry| {
                if &entry.id == id && !entry.pinned {
                    let mut archived = entry.clone();
                    archived.outdated = true;
                    moved = Some(archived);
                    false
                } else {
                    true
                }
            });
            if let Some(archived) = moved {
                candidate.archive.push(archived);
                candidate.revision = candidate.revision.saturating_add(1);
                result.outdated_marked += 1;
            }
            result.completed += 1;
        }

        // 与生产 MemoryService 门禁同口径：有效且未过期（pinned TTL 过期残留
        // 不占收敛目标），target 用策略值。
        let gate_now = (self.clock)();
        let project_active = crate::domain::effective_active_count_at(&candidate.active, gate_now);
        let target = self.policy.target_active_entries;
        if project_active > target {
            return Err(MemoryError::CapacityNotMet {
                active: project_active,
                target,
            });
        }

        *state = candidate;
        Ok(result)
    }
    async fn archive(&self, ids: &[MemoryId]) -> Result<bool, MemoryError> {
        let _permit = self.acquire_write_permit().await?;
        self.archive_locked(ids)
    }

    async fn sweep_stale_under_permit(
        &self,
        now: u64,
        _permit: &MemoryWritePermit,
    ) -> Result<usize, MemoryError> {
        let stale: Vec<MemoryId> = {
            let state = self.state.read().expect("memory state lock poisoned");
            state
                .active
                .iter()
                .filter(|entry| !entry.pinned)
                .filter(|entry| !crate::domain::is_reflection_input_eligible(entry, now))
                .map(|entry| entry.id)
                .collect()
        };
        if stale.is_empty() {
            return Ok(0);
        }
        let swept = stale.len();
        self.archive_locked(&stale)?;
        Ok(swept)
    }

    async fn restore(&self, id: &MemoryId) -> Result<RestoreResult, MemoryError> {
        let _permit = self.acquire_write_permit().await?;
        let mut state = self.state.write().expect("memory state lock poisoned");
        let Some(archived) = state.archive.iter().find(|entry| &entry.id == id).cloned() else {
            return Ok(RestoreResult::NotFound);
        };
        let layer_entries = state
            .active
            .iter()
            .filter(|entry| entry.layer == archived.layer)
            .cloned()
            .collect::<Vec<_>>();
        if crate::domain::effective_active_count(&layer_entries) >= self.policy.max_entries {
            return Ok(RestoreResult::NeedsEviction {
                candidates: eviction_candidates(&layer_entries, 3, archived.last_confirmed_at),
            });
        }
        state.archive.retain(|entry| &entry.id != id);
        state.active.push(archived);
        state.revision = state.revision.saturating_add(1);
        Ok(RestoreResult::Restored { id: *id })
    }

    async fn compact(&self) -> Result<CompactResult, MemoryError> {
        let _permit = self.acquire_write_permit().await?;
        let mut state = self.state.write().expect("memory state lock poisoned");
        let mut candidates = Vec::new();
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let layer_entries = state
                .active
                .iter()
                .filter(|entry| entry.layer == layer)
                .cloned()
                .collect::<Vec<_>>();
            let excess = crate::domain::effective_active_count(&layer_entries)
                .saturating_sub(self.policy.max_entries);
            candidates.extend(eviction_candidates(
                &layer_entries,
                excess,
                layer_entries
                    .iter()
                    .map(|entry| entry.last_confirmed_at)
                    .max()
                    .unwrap_or(0),
            ));
        }
        let ids = candidates
            .iter()
            .map(|candidate| candidate.entry.id)
            .collect::<Vec<_>>();
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

    async fn list(&self, layer: Option<MemoryLayer>) -> Vec<MemoryEntry> {
        self.state
            .read()
            .expect("memory state lock poisoned")
            .active
            .iter()
            .filter(|entry| layer.is_none_or(|layer| entry.layer == layer))
            .cloned()
            .collect()
    }

    async fn stats(&self) -> MemoryStats {
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
    fn archive_locked(&self, ids: &[MemoryId]) -> Result<bool, MemoryError> {
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
        let changed = !moved.is_empty();
        if changed {
            state.archive.extend(moved);
            state.revision = state.revision.saturating_add(1);
        }
        Ok(changed)
    }

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

fn reflection_memory_id(_now: u64) -> Result<MemoryId, MemoryError> {
    Ok(MemoryId::now_v7())
}

fn reflection_capacity_error() -> MemoryError {
    MemoryError::InvalidEntry {
        message: "Reflection 淘汰非 pinned 候选后重试一次仍超过记忆容量".to_string(),
    }
}

/// 应用一条反思建议。返回 `Some(id)` = 实际留存条目的 id（新增时为新 id，
/// 合并时为被并入的既有 id），`None` = 建议被丢弃、未产生留存条目。
fn apply_reflection_entry(
    state: &mut MemoryState,
    mut entry: MemoryEntry,
    policy: MemoryPolicy,
) -> Result<Option<MemoryId>, MemoryError> {
    validate_content(&entry.content)?;
    // M11: a synthesized entry may only cite memories this store already holds.
    if entry.evidence.iter().any(|source| {
        !state
            .active
            .iter()
            .chain(state.archive.iter())
            .any(|stored| &stored.id == source)
    }) {
        return Err(MemoryError::InvalidEntry {
            message: "证据指针指向不存在的记忆".to_string(),
        });
    }
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
        existing.last_confirmed_at = entry.created_at;
        existing.confirmation_count = existing.confirmation_count.saturating_add(1);
        // #1775: archive the incoming entry and let the survivor point at it.
        let incoming_id = entry.id;
        state.archive.push(entry);
        existing.evidence.push(incoming_id);
        return Ok(Some(existing.id));
    }
    let layer_entries = state
        .active
        .iter()
        .filter(|stored| stored.layer == entry.layer)
        .cloned()
        .collect::<Vec<_>>();
    if crate::domain::effective_active_count(&layer_entries) >= policy.max_entries {
        let candidates = eviction_candidates(&layer_entries, 3, entry.created_at);
        let ids = candidates
            .iter()
            .map(|candidate| candidate.entry.id)
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
    let id = entry.id;
    state.active.push(entry);
    Ok(Some(id))
}

/// 在既有条目上建立取代关系，返回成功建立的数量。环检测在写入前基于
/// 同一锁内的一致状态完成（M9），被拒的关系不写入也不计数。
fn establish_supersede_relations(
    state: &mut MemoryState,
    superseding: MemoryId,
    superseded: &[MemoryId],
) -> usize {
    let mut established = 0;
    for target in superseded {
        let cycle = {
            let chain = [state.active.as_slice(), state.archive.as_slice()];
            would_create_supersede_cycle(&superseding, target, supersede_chain_of(&chain))
        };
        if cycle {
            log::info!(
                target: crate::LOG_TARGET,
                "memory_supersede_rejected target={} superseding={} reason=cycle",
                target,
                superseding,
            );
            continue;
        }
        if assign_supersede(&mut state.active, target, superseding)
            || assign_supersede(&mut state.archive, target, superseding)
        {
            established += 1;
        } else {
            log::info!(
                target: crate::LOG_TARGET,
                "memory_supersede_rejected target={} superseding={} reason=missing_target",
                target,
                superseding,
            );
        }
    }
    established
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

fn count_layer(entries: &[MemoryEntry], layer: MemoryLayer) -> usize {
    entries.iter().filter(|entry| entry.layer == layer).count()
}
