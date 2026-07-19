use std::sync::{Arc, RwLock as StdRwLock};

use async_trait::async_trait;
use config::{
    ConfigChangeSet, ConfigError, ConfigPersistOutcome, ConfigQuery, ConfigQueryError,
    ConfigReader, ConfigSubscription, ConfigUpdate, ConfigUpdateError, ConfigWriter,
    PreparedConfigUpdate, PreparedProjectConfig, ProjectConfigLocation, ProjectConfigLocationError,
    ProjectConfigParticipant,
};
use memory::{MemoryOpenError, MemoryOpener, MemoryOpenerError, MemoryPort, ProjectMemoryKey};
use project::{PreparedWorkspaceRestore, WorkspacePersist, WorkspaceRead, WorkspaceRestoreError};
use share::config::domain::snapshot::ConfigSnapshot;
use share::session_types::ProjectIdentity;
use task::{PreparedTaskRestore, TaskPersist, TaskSnapshot, TaskSnapshotValidationError};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock as TokioRwLock};

use crate::domain::session::{CanonicalSession, SessionRestore, SnapshotState};

// ─── SessionSwitchGate (existing) ────────────────────────────────────

/// Context-owned Main Session 切换门禁。
///
/// Main Run、Tool、Reflection 与派生 Sub 持 owned shared permit；resume 与
/// project-scoped Config update 持 owned exclusive permit。owned permit 可以安全
/// 移交给 spawned holder/critical section，调用方无需也不得原地升级。
#[derive(Debug, Clone)]
pub struct SessionSwitchGate {
    inner: Arc<TokioRwLock<()>>,
}

impl Default for SessionSwitchGate {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionSwitchGate {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TokioRwLock::new(())),
        }
    }

    pub async fn acquire_shared(&self) -> Result<OwnedSessionSharedPermit, SessionSwitchClosed> {
        Ok(OwnedSessionSharedPermit(
            self.inner.clone().read_owned().await,
        ))
    }

    pub fn try_acquire_shared(&self) -> Result<OwnedSessionSharedPermit, SessionSwitchInProgress> {
        self.inner
            .clone()
            .try_read_owned()
            .map(OwnedSessionSharedPermit)
            .map_err(|_| SessionSwitchInProgress)
    }

    pub async fn acquire_owned_exclusive(
        &self,
    ) -> Result<OwnedSessionExclusivePermit, SessionSwitchClosed> {
        Ok(OwnedSessionExclusivePermit(
            self.inner.clone().write_owned().await,
        ))
    }
}

#[derive(Debug)]
pub struct OwnedSessionSharedPermit(#[allow(dead_code)] OwnedRwLockReadGuard<()>);

#[derive(Debug)]
pub struct OwnedSessionExclusivePermit(#[allow(dead_code)] OwnedRwLockWriteGuard<()>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Main Session 正在切换")]
pub struct SessionSwitchInProgress;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Main Session 切换门禁已关闭")]
pub struct SessionSwitchClosed;

// ─── SessionProjectionParticipant ────────────────────────────────────

/// Narrow participant protocol that lets Runtime / composition register a
/// leased projection backing to be updated **atomically** inside the exclusive
/// session-switch gate during [`MainSessionWiring::resume_prepared`].
///
/// **`prepare`** is a pure, infallible derivation from the committed
/// [`CanonicalSession`]. It **MUST NOT** perform I/O or await — it runs in the
/// fallible-prepare window only conceptually; since it cannot fail it serves
/// as the pure token construction step.
///
/// **`commit`** is synchronous, infallible, and **MUST NOT await**. It is
/// called inside the no-failure commit phase of `resume_prepared`, after
/// Project / Task / Session / Memory commits and before the Config watch
/// publish. This closes the CM5 observability window: the leased backing
/// (`current_chain` / `frozen_chats` / `active_summary`) is atomically updated
/// before the exclusive gate is released, so any shared observer that
/// acquires a permit after the gate sees the new session **and** the new
/// projection on its very first read.
///
/// If no participant is registered, `resume_prepared` behaves exactly as
/// before — the projection rebuild remains the caller's responsibility (the
/// migration-debt second backing).
pub trait SessionProjectionParticipant: Send + Sync {
    /// Pure, infallible derivation of the leased projection token from the
    /// committed canonical session.
    fn prepare(&self, session: &CanonicalSession) -> SessionRestore;

    /// Sync, infallible, no-await publication of the prepared projection into
    /// the leased backing.
    fn commit(&self, prepared: SessionRestore);
}

// ─── MainSessionError ────────────────────────────────────────────────

/// Structured error for Main Session resume / binding failures.
///
/// Each variant identifies a distinct phase of the cross-BC prepare/commit
/// pipeline so the caller can distinguish *why* a resume was rejected.
#[derive(Debug, thiserror::Error)]
pub enum MainSessionError {
    /// The envelope's `workspace` slot is `Missing` or `CapturedEmpty`. A typed
    /// workspace context is mandatory for resume — there is no safe default.
    #[error("workspace snapshot is missing or captured empty; a typed workspace context is required for resume")]
    WorkspaceMissing,

    /// `WorkspacePersist::prepare_restore` rejected the candidate.
    #[error("workspace restore prepare failed: {0}")]
    WorkspaceRestore(#[from] WorkspaceRestoreError),

    /// Deriving the canonical project-config location failed.
    #[error("invalid config location: {0:?}")]
    ConfigLocation(ProjectConfigLocationError),

    /// `ProjectConfigParticipant::prepare_for_project` failed.
    #[error("config prepare failed: {0:?}")]
    ConfigPrepare(ConfigError),

    /// Deriving the project memory key failed.
    #[error("memory key derivation failed: {0}")]
    MemoryKey(#[from] MemoryOpenError),

    /// `MemoryOpener::open_memory` failed.
    #[error("memory open failed: {0}")]
    MemoryOpen(#[from] MemoryOpenerError),

    /// Task-owned `TaskPersist::prepare_restore` rejected the task snapshot.
    #[error("task restore prepare failed: {0}")]
    TaskRestore(#[from] TaskSnapshotValidationError),

    /// The session switch gate was closed (inner lock dropped).
    #[error("session switch gate closed")]
    GateClosed,
}

// ─── BoundMainRun ────────────────────────────────────────────────────

/// A captured snapshot of the committed Main Session state, held alive by a
/// shared permit.
///
/// All three fields (`session`, `memory`, `config`) are captured atomically at
/// the moment [`MainSessionWiring::bind_main_run`] is called. The shared permit
/// prevents [`MainSessionWiring::resume_prepared`] from running until every
/// `BoundMainRun` is dropped, so the bound triple stays internally consistent
/// for the entire lifetime of the main run.
pub struct BoundMainRun {
    _permit: OwnedSessionSharedPermit,
    session: Arc<CanonicalSession>,
    memory: Arc<dyn MemoryPort>,
    config: ConfigSnapshot,
}

impl BoundMainRun {
    /// The committed canonical session backing the main run.
    pub fn session(&self) -> &CanonicalSession {
        &self.session
    }

    /// The committed memory port backing the main run.
    pub fn memory(&self) -> &dyn MemoryPort {
        &*self.memory
    }

    /// Clones the exact Memory Arc captured by this shared lease.
    pub fn memory_arc(&self) -> Arc<dyn MemoryPort> {
        Arc::clone(&self.memory)
    }

    /// The committed config snapshot backing the main run.
    pub fn config(&self) -> &ConfigSnapshot {
        &self.config
    }
}

// ─── MainSessionDependencies + wire_main_session ─────────────────────

/// Dependencies required to construct a [`MainSessionWiring`] via
/// [`wire_main_session`].
///
/// The composition root supplies all ports plus the production
/// [`MemoryOpener`]. Context takes ownership and eager-opens the initial
/// [`MemoryPort`] from the workspace [`ProjectIdentity`] and the committed
/// config `MemoryConfig`.
pub struct MainSessionDependencies {
    pub workspace: project::WorkspaceViews,
    pub task_persist: Arc<dyn task::TaskPersist>,
    pub config_reader: Arc<dyn ConfigReader>,
    pub config_participant: Arc<dyn ProjectConfigParticipant>,
    pub memory_opener: Box<dyn MemoryOpener>,
}

/// Constructs a [`MainSessionWiring`] suitable for Runtime bootstrap.
///
/// This is the single production entry-point the Composition root calls to
/// create the cross-BC coordinator. It:
///
/// 1. Creates the initial [`CanonicalSession`] from the live workspace
///    snapshot so that a fresh (non-resume) start has a valid workspace slot.
/// 2. Eager-opens the initial [`MemoryPort`] from the workspace
///    [`ProjectIdentity`] and the committed config `MemoryConfig` — the
///    workspace identity is the single source of truth for the memory key.
/// 3. Assembles the wiring via [`MainSessionWiringBuilder`].
///
/// The admission gate, session resume, and config query/writer façades are
/// fully wired with the real opener. There is no compatibility no-op opener
/// in production.
pub async fn wire_main_session(
    deps: MainSessionDependencies,
) -> Result<Arc<MainSessionWiring>, MainSessionError> {
    let workspace_read = deps.workspace.read();
    let workspace_persist = deps.workspace.persist();
    let ws_ctx = workspace_persist.snapshot();
    let now = crate::domain::session::now_iso();
    let initial_session = CanonicalSession {
        id: crate::domain::session::new_session_id(),
        chats: Vec::new(),
        created_at: now.clone(),
        updated_at: now,
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Captured(ws_ctx),
        revision: 0,
        committed_steps: Vec::new(),
    };

    // Bootstrap the initial config location from the verified workspace
    // identity. This runs prepare_for_project + commit_project so that the
    // active project location is set from the very start — ConfigWriter
    // updates can proceed without requiring a prior session resume.
    //
    // The workspace identity is the single source of truth for the config
    // location, matching the derivation used in resume_prepared.
    let identity = workspace_read.project_identity();
    let config_location = derive_config_location(&identity)?;
    let prepared_config = deps
        .config_participant
        .prepare_for_project(&config_location)
        .await
        .map_err(MainSessionError::ConfigPrepare)?;
    let memory_config = prepared_config.memory_config().clone();
    deps.config_participant
        .commit_project(prepared_config)
        .await;

    // Eager-open initial memory from the workspace ProjectIdentity +
    // the project-scoped config MemoryConfig (which now includes any durable
    // override).
    let memory_key =
        ProjectMemoryKey::derive(&identity.initial_cwd, identity.git_common_dir.as_deref())?;
    let initial_memory = deps
        .memory_opener
        .open_memory(&memory_key, &memory_config)
        .await?;

    let builder = MainSessionWiringBuilder {
        workspace_read,
        workspace_persist,
        task_persist: deps.task_persist,
        config_reader: deps.config_reader,
        config_participant: deps.config_participant,
        memory_opener: deps.memory_opener,
        initial_session,
        initial_memory,
    };
    Ok(Arc::new(MainSessionWiring::build(builder)))
}

// ─── MainSessionWiring ───────────────────────────────────────────────

/// Context-owned coordinator that binds Main Run to a consistent
/// Session/Memory/Config triple and atomically resumes from a prepared
/// [`CanonicalSession`].
///
/// All fields are private. The coordinator owns the *committed* canonical
/// session and memory behind shared holders so that [`Self::bind_main_run`] can
/// hand out cheap `Arc` clones. The [`SessionSwitchGate`] ensures mutual
/// exclusion: multiple `bind_main_run` calls can coexist (shared permit), but
/// `resume_prepared` takes an exclusive permit that blocks all bindings.
///
/// # Resume pipeline
///
/// [`Self::resume_prepared`] runs entirely inside the exclusive permit and
/// follows a strict prepare-then-commit order:
///
/// 1. **Project prepare** — `WorkspacePersist::prepare_restore` validates the
///    envelope's workspace context and returns the **canonical** project
///    identity. `Missing` / `CapturedEmpty` is rejected as
///    [`MainSessionError::WorkspaceMissing`].
/// 2. **Config prepare** — the canonical identity from step 1 (not the live
///    workspace identity) is used to derive the project-config location via
///    [`Self::build_config_location`]. This identity is the **single source of
///    truth** for Config and Memory.
/// 3. **Memory eager open** — `MemoryOpener::open_memory` eagerly opens both
///    layers for the canonical identity.
/// 4. **Task prepare** — `TaskPersist::prepare_restore` validates the task
///    snapshot. `Missing` / `CapturedEmpty` maps to `TaskSnapshot::empty()`,
///    clearing any stale live tasks.
///
/// **Cross-project resume is allowed.** The prepared workspace may belong to a
/// different project than the live workspace. The canonical identity returned
/// by `prepare_restore` drives Config and Memory — it is not compared against
/// the live identity.
///
/// If **any** prepare fails, nothing is committed and the old state is kept
/// unchanged. If all prepares succeed, commits proceed **without await**:
/// Project → Task → publish Session/Memory → Config commit (last, so the config
/// watch only fires after everything else is visible).
///
/// # Gate-aware Config façades
///
/// [`Self::config_query`] and [`Self::config_writer`] return gate-aware façades
/// implementing [`ConfigQuery`] and [`ConfigWriter`]. The Query captures
/// snapshot/subscription under a shared permit; the Writer acquires an exclusive
/// permit, eagerly opens candidate Memory, then hands everything to a spawned
/// critical section that persists the config update.
///
/// # No shared→exclusive upgrade
///
/// `bind_main_run` acquires a shared permit and `resume_prepared` acquires an
/// exclusive permit independently. The coordinator never upgrades a held shared
/// permit to exclusive; callers that need to resume must drop their shared
/// permit first.
pub struct MainSessionWiring {
    gate: SessionSwitchGate,

    // ── Project BC ──
    workspace_read: Arc<dyn WorkspaceRead>,
    workspace_persist: Arc<dyn WorkspacePersist>,

    // ── Task BC ──
    task_persist: Arc<dyn TaskPersist>,

    // ── Config BC ──
    config_reader: Arc<dyn ConfigReader>,
    config_participant: Arc<dyn ProjectConfigParticipant>,

    // ── Memory BC ──
    memory_opener: Box<dyn MemoryOpener>,

    // ── Committed state holders ──
    committed_session: Arc<StdRwLock<Arc<CanonicalSession>>>,
    committed_memory: Arc<StdRwLock<Arc<dyn MemoryPort>>>,

    // ── Optional projection participant ──
    //
    // Registered by Runtime/composition so that `resume_prepared` can
    // atomically update the leased projection (chain/frozen/summary) inside
    // the exclusive gate. `None` until the participant is registered.
    projection_participant: StdRwLock<Option<Arc<dyn SessionProjectionParticipant>>>,
}

impl std::fmt::Debug for MainSessionWiring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MainSessionWiring")
            .field("gate", &self.gate)
            .finish_non_exhaustive()
    }
}

/// Dependencies required to construct a [`MainSessionWiring`].
///
/// The composition root supplies all ports plus the initial committed session
/// and memory. The wiring takes ownership and never exposes mutable access to
/// the committed holders.
pub struct MainSessionWiringBuilder {
    pub workspace_read: Arc<dyn WorkspaceRead>,
    pub workspace_persist: Arc<dyn WorkspacePersist>,
    pub task_persist: Arc<dyn task::TaskPersist>,
    pub config_reader: Arc<dyn ConfigReader>,
    pub config_participant: Arc<dyn ProjectConfigParticipant>,
    pub memory_opener: Box<dyn MemoryOpener>,
    pub initial_session: CanonicalSession,
    pub initial_memory: Arc<dyn MemoryPort>,
}

impl MainSessionWiring {
    /// Assembles the coordinator from its cross-BC dependencies and the initial
    /// committed state.
    pub fn build(builder: MainSessionWiringBuilder) -> Self {
        Self {
            gate: SessionSwitchGate::new(),
            workspace_read: builder.workspace_read,
            workspace_persist: builder.workspace_persist,
            task_persist: builder.task_persist,
            config_reader: builder.config_reader,
            config_participant: builder.config_participant,
            memory_opener: builder.memory_opener,
            committed_session: Arc::new(StdRwLock::new(Arc::new(builder.initial_session))),
            committed_memory: Arc::new(StdRwLock::new(builder.initial_memory)),
            projection_participant: StdRwLock::new(None),
        }
    }

    // ── accessors ──

    /// Returns a cheap clone of the session switch gate.
    ///
    /// Callers that only need admission control (e.g. tool execution, derived
    /// sub-runs) can share the gate without going through the full coordinator.
    pub fn gate(&self) -> SessionSwitchGate {
        self.gate.clone()
    }

    /// Returns the currently committed canonical session.
    pub fn committed_session(&self) -> Arc<CanonicalSession> {
        Arc::clone(&self.committed_session.read().unwrap())
    }

    /// Returns the currently committed memory port.
    pub fn committed_memory(&self) -> Arc<dyn MemoryPort> {
        Arc::clone(&self.committed_memory.read().unwrap())
    }

    /// Returns the raw [`ConfigReader`] backing this wiring.
    ///
    /// Runtime bootstrap uses this for one-shot snapshot reads (model resolution,
    /// hooks, skills) that happen before the first `bind_main_run`. Day-to-day
    /// config queries should go through [`Self::config_query`] which is gate-aware.
    pub fn config_reader(&self) -> Arc<dyn ConfigReader> {
        Arc::clone(&self.config_reader)
    }

    /// Returns the current committed config snapshot (one-shot read).
    ///
    /// Equivalent to `self.config_reader().committed_snapshot()` but avoids
    /// an `Arc` clone.
    pub fn committed_config(&self) -> ConfigSnapshot {
        self.config_reader.committed_snapshot()
    }

    /// Registers a [`SessionProjectionParticipant`] so that future
    /// [`Self::resume_prepared`] calls atomically update the leased projection
    /// backing inside the exclusive gate.
    ///
    /// **Startup ordering**: call this **before** the first `resume_prepared`.
    /// At bootstrap the `RuntimeHandle` is not yet constructed, so the caller
    /// creates the projection backing (the three `Arc<Mutex<…>>` fields),
    /// wraps them in a participant, registers it here, then calls
    /// `resume_prepared`. After the gate is released the backing already
    /// contains the resumed chain/frozen/summary.
    ///
    /// If called more than once, the most recent participant replaces the
    /// previous one.
    pub fn register_projection_participant(
        &self,
        participant: Arc<dyn SessionProjectionParticipant>,
    ) {
        *self.projection_participant.write().unwrap() = Some(participant);
    }

    /// Runs an async closure under a **shared** session-switch permit.
    ///
    /// This is the minimal gate-aware read path for idle-time operations
    /// (e.g. `/session export`) that read session data from disk. The shared
    /// permit prevents [`Self::resume_prepared`] (exclusive) from running
    /// concurrently, ensuring the read is not racing with a session switch.
    pub async fn with_shared<F, R>(&self, f: F) -> Result<R, SessionSwitchClosed>
    where
        F: std::future::Future<Output = R>,
    {
        let _permit = self.gate.acquire_shared().await?;
        Ok(f.await)
    }

    // ── gate-aware Config façade factories ──

    /// Returns a gate-aware [`ConfigQuery`] façade backed by this wiring's
    /// [`ConfigReader`].
    ///
    /// `snapshot` / `subscribe` capture the current committed snapshot under a
    /// shared session-switch permit, ensuring the read is not racing with a
    /// resume. The returned watch receiver continues to receive updates after
    /// the permit is released.
    pub fn config_query(&self) -> Arc<dyn ConfigQuery> {
        Arc::new(GateAwareConfigQuery {
            gate: self.gate.clone(),
            config_reader: Arc::clone(&self.config_reader),
        })
    }

    /// Returns a gate-aware [`ConfigWriter`] façade backed by this wiring's
    /// [`ProjectConfigParticipant`], [`WorkspaceRead`] and [`MemoryOpener`].
    ///
    /// See [`GateAwareConfigWriter`] for the full semantics.
    pub fn config_writer(&self) -> Arc<dyn ConfigWriter> {
        Arc::new(GateAwareConfigWriter {
            gate: self.gate.clone(),
            config_participant: Arc::clone(&self.config_participant),
            workspace_read: Arc::clone(&self.workspace_read),
            memory_opener: self.memory_opener.clone(),
            committed_memory: Arc::clone(&self.committed_memory),
        })
    }

    // ── shared-permit binding ──

    /// Captures the committed Session/Memory/Config triple under a shared
    /// permit.
    ///
    /// The returned [`BoundMainRun`] holds the shared permit for its entire
    /// lifetime, preventing [`Self::resume_prepared`] from running. All three
    /// fields are read from the committed holders at the same instant, so they
    /// are guaranteed to be mutually consistent.
    pub async fn bind_main_run(&self) -> Result<BoundMainRun, SessionSwitchClosed> {
        let permit = self.gate.acquire_shared().await?;
        let session = self.committed_session();
        let memory = self.committed_memory();
        let config = self.config_reader.committed_snapshot();
        Ok(BoundMainRun {
            _permit: permit,
            session,
            memory,
            config,
        })
    }

    // ── exclusive-permit resume ──

    /// Atomically restores the full Main Session state from a prepared
    /// [`CanonicalSession`].
    ///
    /// See the [type-level docs](MainSessionWiring) for the full pipeline
    /// description. In short: prepare every participant first (Project → Config
    /// → Memory → Task), and only if all succeed, commit them synchronously.
    /// Any prepare failure leaves the old state untouched.
    ///
    /// **Cross-project resume is allowed.** The canonical identity returned by
    /// `prepare_restore` is the sole source of truth for Config and Memory.
    /// It is never compared against the live workspace identity.
    pub async fn resume_prepared(&self, session: CanonicalSession) -> Result<(), MainSessionError> {
        let _exclusive = self
            .gate
            .acquire_owned_exclusive()
            .await
            .map_err(|_| MainSessionError::GateClosed)?;

        // ── 1. Project prepare ──
        //
        // The workspace slot must carry a typed context. Missing/CapturedEmpty
        // is a hard error: there is no safe default workspace to restore.
        //
        // The canonical identity returned here is the **single source of truth**
        // for Config and Memory. It is NOT compared against the live workspace
        // identity — cross-project resume is a valid operation.
        let prepared_workspace: PreparedWorkspaceRestore = match &session.workspace {
            SnapshotState::Captured(dto) => self.workspace_persist.prepare_restore(dto)?,
            SnapshotState::Missing | SnapshotState::CapturedEmpty => {
                return Err(MainSessionError::WorkspaceMissing);
            }
        };

        // Use the prepared canonical identity for everything downstream.
        let prepared_identity = prepared_workspace.project_identity().clone();

        // ── 2. Config prepare ──
        //
        // Derive the project-scoped config location from the prepared identity.
        // The search root is the prepared identity's `initial_cwd`, NOT the live
        // workspace's `initial_cwd`. This ensures Config/Memory reflect the
        // project being resumed into, which may differ from the live project.
        let config_location = self.build_config_location(&prepared_identity)?;
        let prepared_config: PreparedProjectConfig = self
            .config_participant
            .prepare_for_project(&config_location)
            .await
            .map_err(MainSessionError::ConfigPrepare)?;

        // ── 3. Memory eager open ──
        //
        // Open both memory layers for the prepared identity using the config
        // just prepared.
        let memory_key = ProjectMemoryKey::derive(
            &prepared_identity.initial_cwd,
            prepared_identity.git_common_dir.as_deref(),
        )?;
        let new_memory: Arc<dyn MemoryPort> = self
            .memory_opener
            .open_memory(&memory_key, prepared_config.memory_config())
            .await?;

        // ── 4. Task prepare ──
        //
        // Missing/CapturedEmpty maps to TaskSnapshot::empty(), which clears any
        // stale live tasks on commit.
        let prepared_task: PreparedTaskRestore = match &session.tasks {
            SnapshotState::Captured(snapshot) => self.task_persist.prepare_restore(snapshot)?,
            SnapshotState::Missing | SnapshotState::CapturedEmpty => {
                self.task_persist.prepare_restore(&TaskSnapshot::empty())?
            }
        };

        // ════════════════════════════════════════════════════════════════
        // All prepares succeeded — commit phase (synchronous, no await).
        // ════════════════════════════════════════════════════════════════

        // Commit Project + Task first — both are infallible.
        self.workspace_persist.commit_restore(prepared_workspace);
        self.task_persist.commit_restore(prepared_task);

        // Publish the new committed Session + Memory. Other shared-permit
        // holders are blocked by the exclusive permit, so this write is
        // uncontended.
        let session_arc = Arc::new(session);
        *self.committed_session.write().unwrap() = Arc::clone(&session_arc);
        *self.committed_memory.write().unwrap() = Arc::clone(&new_memory);

        // ── 5. Session projection participant commit ──
        //
        // If a Runtime/composition-owned participant is registered,
        // synchronously update its leased projection (chain/frozen/summary)
        // inside the exclusive gate. This closes the CM5 observability
        // window: the leased backing is atomically updated before the gate
        // is released, so any shared observer that acquires a permit after
        // release sees the new session **and** the new projection on its
        // very first read.
        //
        // `prepare` is pure + infallible (pure derivation from
        // CanonicalSession). `commit` is sync + infallible + no-await.
        if let Some(participant) = self.projection_participant.read().unwrap().as_ref() {
            let prepared = participant.prepare(&session_arc);
            participant.commit(prepared);
        }

        // Config commit/watch last. commit_project updates the ConfigReader's
        // internal watch, so any future bind_main_run sees the new config.
        self.config_participant
            .commit_project(prepared_config)
            .await;

        // _exclusive is dropped here, releasing the exclusive permit.
        Ok(())
    }

    /// Derives the canonical project-config location from a prepared identity.
    ///
    /// The `search_root` is the identity's `initial_cwd`; the `stable_identity`
    /// is the `git_common_dir` for git projects or `initial_cwd` for non-git
    /// projects — matching the derivation used by [`ProjectMemoryKey`].
    fn build_config_location(
        &self,
        identity: &ProjectIdentity,
    ) -> Result<ProjectConfigLocation, MainSessionError> {
        derive_config_location(identity)
    }
}

/// Derives the canonical project-config location from a [`ProjectIdentity`].
///
/// The `search_root` is the identity's `initial_cwd`; the `stable_identity`
/// is the `git_common_dir` for git projects or `initial_cwd` for non-git
/// projects — matching the derivation used by [`ProjectMemoryKey`].
///
/// This is shared by [`wire_main_session`] (initial bootstrap) and
/// [`MainSessionWiring::resume_prepared`] (session resume) to ensure
/// identical location derivation in both paths.
pub(crate) fn derive_config_location(
    identity: &ProjectIdentity,
) -> Result<ProjectConfigLocation, MainSessionError> {
    let search_root = std::path::PathBuf::from(&identity.initial_cwd);
    let stable_identity: &[u8] = match identity.git_common_dir.as_deref() {
        Some(common) if !common.is_empty() => common.as_bytes(),
        _ => identity.initial_cwd.as_bytes(),
    };
    ProjectConfigLocation::try_from_project_identity(search_root, stable_identity)
        .map_err(MainSessionError::ConfigLocation)
}

// ─── GateAwareConfigQuery ────────────────────────────────────────────

/// Gate-aware [`ConfigQuery`] façade produced by [`MainSessionWiring::config_query`].
///
/// `snapshot` / `subscribe` acquire a **shared** session-switch permit before
/// reading, ensuring no resume is in progress when the snapshot is captured.
/// The permit is released immediately after capture; a returned
/// `watch::Receiver` continues to receive future updates without holding the
/// permit.
pub struct GateAwareConfigQuery {
    gate: SessionSwitchGate,
    config_reader: Arc<dyn ConfigReader>,
}

#[async_trait]
impl ConfigQuery for GateAwareConfigQuery {
    async fn snapshot(&self) -> Result<ConfigSnapshot, ConfigQueryError> {
        let _permit = self
            .gate
            .acquire_shared()
            .await
            .map_err(|_| ConfigQueryError::Unavailable)?;
        Ok(self.config_reader.committed_snapshot())
    }

    async fn subscribe(&self) -> Result<ConfigSubscription, ConfigQueryError> {
        let _permit = self
            .gate
            .acquire_shared()
            .await
            .map_err(|_| ConfigQueryError::Unavailable)?;
        let changes = self.config_reader.subscribe_committed();
        let initial = changes.borrow().clone();
        Ok(ConfigSubscription { initial, changes })
    }
}

// ─── GateAwareConfigWriter ───────────────────────────────────────────

/// Gate-aware [`ConfigWriter`] façade produced by [`MainSessionWiring::config_writer`].
///
/// The Writer pipeline:
///
/// 1. **Acquire exclusive permit** — blocks all shared bindings and resumes
///    until the entire update is settled.
/// 2. **Config prepare_update** — produces a [`PreparedConfigUpdate`] without
///    committing anything.
/// 3. **Eager open candidate Memory** — based on the current committed
///    workspace identity and the candidate `MemoryConfig` from the prepared
///    update.
/// 4. **Spawn critical section** — the exclusive permit + prepared update +
///    candidate Memory are moved into a `tokio::spawn` owned task that calls
///    `persist_update`:
///    - **NotCommitted** — old Memory and Config are kept untouched; the task
///      returns `Err(Persist(…))`.
///    - **Committed** — candidate Memory is installed into the committed holder,
///      then `commit_update` fires the config watch **last**. A
///      [`ConfigCommitWarning`] is logged but **not** converted to an error.
///
/// The `update()` future awaits the spawned JoinHandle. Once execution reaches
/// the durable handoff (`tokio::spawn`), cancelling or dropping the outer future
/// only stops waiting: the spawned task continues to completion and installs
/// Memory and Config after a committed outcome.
pub struct GateAwareConfigWriter {
    gate: SessionSwitchGate,
    config_participant: Arc<dyn ProjectConfigParticipant>,
    workspace_read: Arc<dyn WorkspaceRead>,
    memory_opener: Box<dyn MemoryOpener>,
    committed_memory: Arc<StdRwLock<Arc<dyn MemoryPort>>>,
}

#[async_trait]
impl ConfigWriter for GateAwareConfigWriter {
    async fn update(&self, command: ConfigUpdate) -> Result<ConfigChangeSet, ConfigUpdateError> {
        // 1. Acquire owned exclusive permit.
        let permit = self
            .gate
            .acquire_owned_exclusive()
            .await
            .map_err(|_| ConfigUpdateError::Invalid("session switch gate closed".into()))?;

        // 2. Config prepare_update (does not commit).
        let prepared: PreparedConfigUpdate =
            self.config_participant.prepare_update(command).await?;

        // 3. Derive memory key from the current committed workspace identity.
        let identity = self.workspace_read.project_identity();
        let candidate_memory_config = prepared.memory_config().clone();
        let memory_key =
            ProjectMemoryKey::derive(&identity.initial_cwd, identity.git_common_dir.as_deref())
                .map_err(|e| ConfigUpdateError::Invalid(format!("memory key derivation: {e}")))?;

        // 4. Eager open candidate memory.
        let candidate_memory: Arc<dyn MemoryPort> = self
            .memory_opener
            .open_memory(&memory_key, &candidate_memory_config)
            .await
            .map_err(|e| ConfigUpdateError::Invalid(format!("memory open: {e}")))?;

        // 5. Spawn owned critical section: persist_update + conditional commit.
        let config_participant = Arc::clone(&self.config_participant);
        let committed_memory = Arc::clone(&self.committed_memory);

        let handle = tokio::spawn(async move {
            // Hold the exclusive permit for the entire critical section.
            let _permit = permit;

            let outcome = config_participant.persist_update(prepared).await;
            match outcome {
                ConfigPersistOutcome::NotCommitted(err) => {
                    // Old Memory and Config are kept untouched.
                    Err(ConfigUpdateError::Persist(err))
                }
                ConfigPersistOutcome::Committed(ready) => {
                    // Warnings are informational — do NOT convert to error.
                    if let Some(warning) = ready.warning() {
                        log::warn!(
                            target: crate::LOG_TARGET,
                            "config commit warning: {:?}",
                            warning
                        );
                    }
                    // Install new Memory before committing config.
                    *committed_memory.write().unwrap() = candidate_memory;
                    // commit_update fires the watch last.
                    let change_set = config_participant.commit_update(*ready);
                    Ok(change_set)
                }
            }
            // _permit dropped here → exclusive lock released.
        });

        // Await the spawned task. After the spawn handoff, dropping this outer
        // future drops only the JoinHandle; the owned task keeps running.
        match handle.await {
            Ok(result) => result,
            Err(join_err) => Err(ConfigUpdateError::Invalid(format!(
                "background config task: {join_err}"
            ))),
        }
    }
}

// ─── Test support ────────────────────────────────────────────────────
//
// Test-only helpers. Production code must never use these — they bypass the
// real Memory opener and return an `InMemoryMemory` so tests don't need a
// filesystem. The module is gated behind the `dev` Cargo feature so it is
// absent from production builds. Consumers add `context` to `[dev-dependencies]`
// with `features = ["dev"]`.

#[cfg(any(test, feature = "dev"))]
pub mod test_support {
    use super::*;

    /// Test-only [`MemoryOpener`] that always returns a fresh
    /// [`InMemoryMemory`]. Production code must never use this.
    #[derive(Clone, Default)]
    pub struct InMemoryTestOpener;

    #[async_trait]
    impl MemoryOpener for InMemoryTestOpener {
        async fn open_memory(
            &self,
            _key: &ProjectMemoryKey,
            _config: &share::config::MemoryConfig,
        ) -> Result<Arc<dyn MemoryPort>, MemoryOpenerError> {
            Ok(Arc::new(
                memory::InMemoryMemory::new(memory::MemoryPolicy::default())
                    .expect("default MemoryPolicy is always valid"),
            ))
        }

        fn boxed_clone(&self) -> Box<dyn MemoryOpener> {
            Box::new(self.clone())
        }
    }

    /// Test helper: creates a [`MainSessionWiring`] with an
    /// [`InMemoryTestOpener`] so tests don't need a real Memory store.
    ///
    /// For test use only — production code must call [`wire_main_session`]
    /// with a real [`MemoryOpener`].
    pub async fn wire_in_memory(
        workspace: &project::WorkspaceViews,
        task_persist: Arc<dyn task::TaskPersist>,
        config_reader: Arc<dyn ConfigReader>,
        config_participant: Arc<dyn ProjectConfigParticipant>,
    ) -> Arc<MainSessionWiring> {
        wire_main_session(MainSessionDependencies {
            workspace: workspace.clone(),
            task_persist,
            config_reader,
            config_participant,
            memory_opener: Box::new(InMemoryTestOpener),
        })
        .await
        .expect("test wiring with InMemoryTestOpener should not fail")
    }
}
