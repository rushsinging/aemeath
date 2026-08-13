use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use crate::adapters::compact_summary::{compact_messages_with_llm, CompactGenerator};
use crate::domain::session::{
    AcceptedInputProjection, ActiveCompactMarker, CanonicalSession, CommittedStep,
    FinalizedOutcomeProjection, SessionCommitPlan, SnapshotState,
};
use crate::domain::{
    AcceptedInputAppend, AcceptedInputError, AcceptedInputReceipt, AppendReceipt, CompactOutcome,
    CompactRequest, CompactSkipReason, ContextAppend, ContextAppendError, ContextPortError,
    ManualCompactRequest, SessionId, SessionRevision, ToolReceiptMutation,
    ToolReceiptMutationError, ToolReceiptMutationReceipt,
};
use crate::ports::{ContextPort, MainContextFactory, SessionRepository, SessionSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSaveIntent {
    CommitPartialHistory,
    ReplaceCompleteHistory,
}

#[async_trait]
pub trait CanonicalSessionWriter: Send + Sync {
    async fn commit(
        &self,
        session_id: &str,
        expected_revision: u64,
        plan: SessionCommitPlan,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait AcceptedInputWriter: Send + Sync {
    async fn save(
        &self,
        session_id: &str,
        revision: u64,
        run_id: &str,
        step_id: &str,
        input: &AcceptedInputProjection,
    ) -> Result<(), String>;

    async fn acknowledge_finalized_input(
        &self,
        _session_id: &str,
        _run_id: &str,
        _step_id: &str,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn delete_all(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }
}

pub struct NoOpAcceptedInputWriter;

#[async_trait]
impl AcceptedInputWriter for NoOpAcceptedInputWriter {
    async fn save(
        &self,
        _session_id: &str,
        _revision: u64,
        _run_id: &str,
        _step_id: &str,
        _input: &AcceptedInputProjection,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[async_trait]
pub trait ToolReceiptWriter: Send + Sync {
    async fn save(
        &self,
        session_id: &str,
        revision: u64,
        receipt: &crate::domain::ToolCallReceipt,
    ) -> Result<(), String>;
}

pub struct AtomicBlobAcceptedInputWriter {
    blob: Arc<dyn storage::api::AtomicBlobPort>,
}

impl AtomicBlobAcceptedInputWriter {
    pub fn new(blob: Arc<dyn storage::api::AtomicBlobPort>) -> Self {
        Self { blob }
    }
}

#[async_trait]
impl AcceptedInputWriter for AtomicBlobAcceptedInputWriter {
    async fn save(
        &self,
        session_id: &str,
        revision: u64,
        run_id: &str,
        step_id: &str,
        input: &AcceptedInputProjection,
    ) -> Result<(), String> {
        crate::adapters::accepted_input_ledger::AtomicBlobAcceptedInputLedger::new(
            Arc::clone(&self.blob),
            session_id,
        )?
        .save(revision, run_id, step_id, input)
        .await
    }

    async fn acknowledge_finalized_input(
        &self,
        session_id: &str,
        run_id: &str,
        step_id: &str,
    ) -> Result<(), String> {
        crate::adapters::accepted_input_ledger::AtomicBlobAcceptedInputLedger::new(
            Arc::clone(&self.blob),
            session_id,
        )?
        .acknowledge_finalized_input(run_id, step_id)
        .await
    }

    async fn delete_all(&self, session_id: &str) -> Result<(), String> {
        crate::adapters::accepted_input_ledger::AtomicBlobAcceptedInputLedger::new(
            Arc::clone(&self.blob),
            session_id,
        )?
        .delete()
        .await
    }
}

pub struct AtomicBlobToolReceiptWriter {
    blob: Arc<dyn storage::api::AtomicBlobPort>,
}

impl AtomicBlobToolReceiptWriter {
    pub fn new(blob: Arc<dyn storage::api::AtomicBlobPort>) -> Self {
        Self { blob }
    }
}

#[async_trait]
impl ToolReceiptWriter for AtomicBlobToolReceiptWriter {
    async fn save(
        &self,
        session_id: &str,
        revision: u64,
        receipt: &crate::domain::ToolCallReceipt,
    ) -> Result<(), String> {
        crate::adapters::tool_receipt_ledger::AtomicBlobToolReceiptLedger::new(
            Arc::clone(&self.blob),
            session_id,
        )?
        .save(revision, receipt)
        .await
    }
}

pub struct NoOpToolReceiptWriter;

#[async_trait]
impl ToolReceiptWriter for NoOpToolReceiptWriter {
    async fn save(
        &self,
        _session_id: &str,
        _revision: u64,
        _receipt: &crate::domain::ToolCallReceipt,
    ) -> Result<(), String> {
        Ok(())
    }
}

pub struct AtomicBlobCanonicalSessionWriter {
    blob: Arc<dyn storage::api::AtomicBlobPort>,
}

impl AtomicBlobCanonicalSessionWriter {
    pub fn new(blob: Arc<dyn storage::api::AtomicBlobPort>) -> Self {
        Self { blob }
    }

    pub async fn save_tool_receipt(
        &self,
        session_id: &str,
        revision: u64,
        receipt: &crate::domain::ToolCallReceipt,
    ) -> Result<(), String> {
        crate::adapters::tool_receipt_ledger::AtomicBlobToolReceiptLedger::new(
            Arc::clone(&self.blob),
            session_id,
        )?
        .save(revision, receipt)
        .await
    }
}

#[async_trait]
impl ToolReceiptWriter for AtomicBlobCanonicalSessionWriter {
    async fn save(
        &self,
        session_id: &str,
        revision: u64,
        receipt: &crate::domain::ToolCallReceipt,
    ) -> Result<(), String> {
        AtomicBlobCanonicalSessionWriter::save_tool_receipt(self, session_id, revision, receipt)
            .await
    }
}

pub struct NoOpCanonicalSessionWriter;

#[async_trait]
impl CanonicalSessionWriter for NoOpCanonicalSessionWriter {
    async fn commit(
        &self,
        _session_id: &str,
        _expected_revision: u64,
        _plan: SessionCommitPlan,
    ) -> Result<(), String> {
        Ok(())
    }
}

pub struct ProductionMainContextFactory {
    writer: Arc<dyn CanonicalSessionWriter>,
    accepted_input_writer: Arc<dyn AcceptedInputWriter>,
    tool_receipt_writer: Arc<dyn ToolReceiptWriter>,
    /// 可选注入的 Skill metadata catalog 与 Context-owned query factory。
    skill_catalog: Option<Arc<dyn tools::SkillCatalogPort>>,
    query_factory: Option<Arc<dyn crate::ports::SkillQueryFactory>>,
    /// 可选注入的 LLM 摘要生成器（#1486）；None 时 compact 走本地压缩。
    generator: Option<Arc<dyn CompactGenerator>>,
}

impl ProductionMainContextFactory {
    pub fn new(writer: Arc<dyn CanonicalSessionWriter>) -> Self {
        Self {
            writer,
            accepted_input_writer: Arc::new(NoOpAcceptedInputWriter),
            tool_receipt_writer: Arc::new(NoOpToolReceiptWriter),
            skill_catalog: None,
            query_factory: None,
            generator: None,
        }
    }

    pub fn with_accepted_input_writer(mut self, writer: Arc<dyn AcceptedInputWriter>) -> Self {
        self.accepted_input_writer = writer;
        self
    }

    pub fn with_tool_receipt_writer(mut self, writer: Arc<dyn ToolReceiptWriter>) -> Self {
        self.tool_receipt_writer = writer;
        self
    }

    pub fn with_skill_catalog(
        mut self,
        catalog: Arc<dyn tools::SkillCatalogPort>,
        query_factory: Arc<dyn crate::ports::SkillQueryFactory>,
    ) -> Self {
        self.skill_catalog = Some(catalog);
        self.query_factory = Some(query_factory);
        self
    }

    /// 注入 LLM 摘要生成器（#1486），compact 优先走 LLM 语义压缩。
    pub fn with_generator(mut self, generator: Arc<dyn CompactGenerator>) -> Self {
        self.generator = Some(generator);
        self
    }
}

impl MainContextFactory for ProductionMainContextFactory {
    fn build(
        &self,
        session: Arc<RwLock<Arc<CanonicalSession>>>,
        task_persist: Arc<dyn task::TaskPersist>,
        workspace_persist: Arc<dyn project::WorkspacePersist>,
        memory: Arc<RwLock<Arc<dyn memory::MemoryPort>>>,
        mutation_gate: Arc<tokio::sync::Mutex<()>>,
    ) -> Arc<dyn ContextPort> {
        let prompt: Arc<dyn crate::ports::ContextPromptSource> =
            match (&self.skill_catalog, &self.query_factory) {
                (Some(catalog), Some(factory)) => {
                    Arc::new(crate::adapters::SkillPromptSource::new(
                        Arc::clone(catalog),
                        Arc::clone(factory),
                    ))
                }
                _ => Arc::new(crate::adapters::BaselinePromptSource),
            };
        let mut repository = CanonicalSessionRepository::new(
            session,
            task_persist,
            workspace_persist,
            Arc::clone(&self.writer),
            mutation_gate,
        );
        repository = repository.with_accepted_input_writer(Arc::clone(&self.accepted_input_writer));
        if let Some(generator) = &self.generator {
            repository = repository.with_generator(Arc::clone(generator));
        }
        repository = repository.with_tool_receipt_writer(Arc::clone(&self.tool_receipt_writer));
        Arc::new(crate::application::ContextApplicationService::new(
            Arc::new(repository),
            prompt,
            Arc::new(crate::adapters::CommittedMemoryRetrieveAdapter::new(memory)),
        ))
    }
}

#[derive(Clone)]
struct CompactSource {
    revision: SessionRevision,
    messages: Vec<share::message::Message>,
    visible_steps: Vec<(
        crate::domain::session::RunStepCursor,
        Vec<share::message::Message>,
    )>,
    previous_summary: Option<String>,
}

struct GeneratedCompact {
    summary: String,
    recent_messages: Vec<share::message::Message>,
    quality: crate::domain::CompactSummaryQuality,
}

struct AutoCompactAttemptPermit {
    state: Arc<std::sync::Mutex<crate::domain::compact::AutoCompactState>>,
    failure_limit: u8,
    finished: bool,
}

impl AutoCompactAttemptPermit {
    fn finish(mut self, outcome: &Result<CompactOutcome, ContextPortError>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match outcome {
            Ok(CompactOutcome::Committed(_)) => state.record_success(),
            Ok(CompactOutcome::Skipped(_)) => {}
            Err(_) => state.record_failure(self.failure_limit),
        }
        self.finished = true;
    }
}

impl Drop for AutoCompactAttemptPermit {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .record_failure(self.failure_limit);
    }
}

pub struct CanonicalSessionRepository {
    session: Arc<RwLock<Arc<CanonicalSession>>>,
    task_persist: Arc<dyn task::TaskPersist>,
    workspace_persist: Arc<dyn project::WorkspacePersist>,
    writer: Arc<dyn CanonicalSessionWriter>,
    accepted_input_writer: Arc<dyn AcceptedInputWriter>,
    tool_receipt_writer: Arc<dyn ToolReceiptWriter>,
    mutation_gate: Arc<tokio::sync::Mutex<()>>,
    /// 可选注入的 LLM 摘要生成器（#1486）。Some 时 compact 走 LLM 语义压缩，
    /// 失败自动 fallback 本地；None 时直接走本地文本压缩。
    generator: Option<Arc<dyn CompactGenerator>>,
    auto_compact_state: Arc<std::sync::Mutex<crate::domain::compact::AutoCompactState>>,
}

impl CanonicalSessionRepository {
    pub fn new(
        session: Arc<RwLock<Arc<CanonicalSession>>>,
        task_persist: Arc<dyn task::TaskPersist>,
        workspace_persist: Arc<dyn project::WorkspacePersist>,
        writer: Arc<dyn CanonicalSessionWriter>,
        mutation_gate: Arc<tokio::sync::Mutex<()>>,
    ) -> Self {
        Self {
            session,
            task_persist,
            workspace_persist,
            accepted_input_writer: Arc::new(NoOpAcceptedInputWriter),
            tool_receipt_writer: Arc::new(NoOpToolReceiptWriter),
            writer,
            mutation_gate,
            generator: None,
            auto_compact_state: Arc::new(std::sync::Mutex::new(
                crate::domain::compact::AutoCompactState::default(),
            )),
        }
    }

    pub fn with_accepted_input_writer(mut self, writer: Arc<dyn AcceptedInputWriter>) -> Self {
        self.accepted_input_writer = writer;
        self
    }

    pub fn with_tool_receipt_writer(mut self, writer: Arc<dyn ToolReceiptWriter>) -> Self {
        self.tool_receipt_writer = writer;
        self
    }

    pub fn with_generator(mut self, generator: Arc<dyn CompactGenerator>) -> Self {
        self.generator = Some(generator);
        self
    }

    async fn acknowledge_finalized_input_ledger(
        &self,
        session_id: &str,
        run_id: &str,
        step_id: &str,
    ) {
        if let Err(error) = self
            .accepted_input_writer
            .acknowledge_finalized_input(session_id, run_id, step_id)
            .await
        {
            log::warn!(
                target: crate::LOG_TARGET,
                "accepted input ledger cleanup deferred session_id={} run_id={} step_id={} error={}",
                session_id,
                run_id,
                step_id,
                error
            );
        }
    }

    /// 压缩可见消息：
    /// - 注入 generator 时走 LLM 语义压缩（`compact_messages_with_llm`，
    ///   内部失败自动 fallback 本地，分块并发 + 汇总收敛）；
    /// - 未注入时走本地文本压缩 `compact_messages`，并手动补齐
    ///   previous_summary（`build_summary_text` 已保证不全文累加）。
    ///
    /// `progress` 为压缩进度回调（#1500）：Preparing/Summarizing chunk
    /// 计数/Finalizing 阶段实时上报；`None` 表示调用方不关心进度。
    ///
    /// 返回 `(summary, recent_messages)`；消息太少无法压缩时返回 `None`。
    async fn compact_visible_messages(
        &self,
        messages: &[share::message::Message],
        previous_summary: Option<&str>,
        context_size: usize,
        progress: Option<std::sync::Arc<dyn crate::domain::CompactProgressFn>>,
        task_snapshot: Option<&crate::domain::compact::CompactTaskSnapshot>,
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> Option<crate::adapters::compact_summary::CompactResult> {
        match &self.generator {
            Some(generator) => {
                let result = compact_messages_with_llm(
                    messages,
                    previous_summary,
                    context_size,
                    Some(generator.as_ref()),
                    progress.as_deref(),
                    task_snapshot,
                    cancellation,
                )
                .await;
                result.inspect(|compacted| {
                    log::info!(
                        target: crate::LOG_TARGET,
                        "[compact] LLM 路径生成完成：summary={} chars recent={} quality={:?}",
                        compacted.summary.len(),
                        compacted.recent_messages.len(),
                        compacted.quality,
                    );
                })
            }
            None => {
                let mut compacted = crate::adapters::compact_summary::compact_messages(messages)?;
                let window = crate::adapters::compact_summary::compact_window(messages.len())?;
                let early = &messages[..window.split_point]; // allow unsafe_text_op: Vec slice
                compacted.summary =
                    crate::adapters::compact_summary::build_summary_text(early, previous_summary);
                log::info!(
                    target: crate::LOG_TARGET,
                    "[compact] 本地路径生成完成：summary={} chars recent={}",
                    compacted.summary.len(),
                    compacted.recent_messages.len(),
                );
                Some(compacted)
            }
        }
    }

    /// 将 typed Task snapshot 确定性渲染为非权威 companion。
    fn append_task_snapshot_companion(
        summary: &str,
        task_snapshot: Option<&crate::domain::compact::CompactTaskSnapshot>,
    ) -> String {
        let (checkpoint, _) = crate::domain::compact::split_checkpoint_and_task_state(summary);
        match task_snapshot {
            Some(snapshot) if !snapshot.items().is_empty() => format!(
                "{checkpoint}\n\n## Current Task State\n{}",
                snapshot.render_companion()
            ),
            _ => checkpoint.to_string(),
        }
    }

    async fn commit_automatic_compaction(
        &self,
        request: &CompactRequest,
    ) -> Result<CompactOutcome, ContextPortError> {
        let source = self
            .freeze_compact_source(&request.source.session_id, Some(request.source_revision))
            .await?;
        let generated = match self
            .generate_compact(
                &source,
                request.source.context_size,
                request.progress.clone(),
                request.task_snapshot.as_ref(),
                &request.cancellation,
            )
            .await
        {
            Ok(Some(generated)) => generated,
            Ok(None) => {
                return Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection));
            }
            Err(reason) => return Ok(CompactOutcome::Skipped(reason)),
        };
        self.commit_generated_compact(&request.source.session_id, &source, generated)
            .await
    }

    fn begin_auto_compact_attempt(&self, failure_limit: u8) -> Option<AutoCompactAttemptPermit> {
        let state = self
            .auto_compact_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.should_attempt() {
            return None;
        }
        drop(state);
        Some(AutoCompactAttemptPermit {
            state: Arc::clone(&self.auto_compact_state),
            failure_limit: failure_limit.max(1),
            finished: false,
        })
    }

    async fn freeze_compact_source(
        &self,
        session_id: &SessionId,
        expected_revision: Option<SessionRevision>,
    ) -> Result<CompactSource, ContextPortError> {
        let _mutation = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| ContextPortError::SessionRepository(error.to_string()))?
            .clone();
        if current.id != session_id.as_str() {
            return Err(ContextPortError::SessionNotFound(session_id.clone()));
        }
        let revision = SessionRevision::new(current.revision);
        if let Some(expected_revision) = expected_revision {
            if expected_revision != revision {
                return Err(Self::compact_revision_conflict(expected_revision, revision));
            }
        }
        let visible_steps = current.flattened_steps_from_marker();
        let messages = visible_steps
            .iter()
            .flat_map(|(_, messages)| messages.iter().cloned())
            .collect();
        Ok(CompactSource {
            revision,
            messages,
            visible_steps,
            previous_summary: current
                .compact
                .as_ref()
                .map(|marker| marker.summary.clone()),
        })
    }

    async fn generate_compact(
        &self,
        source: &CompactSource,
        context_size: usize,
        progress: Option<std::sync::Arc<dyn crate::domain::CompactProgressFn>>,
        task_snapshot: Option<&crate::domain::compact::CompactTaskSnapshot>,
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> Result<Option<GeneratedCompact>, crate::domain::CompactSkipReason> {
        let Some(compacted) = self
            .compact_visible_messages(
                &source.messages,
                source.previous_summary.as_deref(),
                context_size,
                progress,
                task_snapshot,
                cancellation,
            )
            .await
        else {
            return if cancellation.is_cancelled() {
                Err(crate::domain::CompactSkipReason::Cancelled)
            } else {
                Ok(None)
            };
        };
        let reconciled_summary =
            crate::domain::compact::ContinuationCheckpoint::parse(&compacted.summary)
                .and_then(|checkpoint| {
                    crate::domain::compact::reconcile_checkpoint_with_task_snapshot(
                        checkpoint,
                        task_snapshot,
                    )
                })
                .map(|checkpoint| checkpoint.render())
                .unwrap_or_else(|_| compacted.summary.clone());
        Ok(Some(GeneratedCompact {
            summary: Self::append_task_snapshot_companion(&reconciled_summary, task_snapshot),
            recent_messages: compacted.recent_messages,
            quality: compacted.quality,
        }))
    }

    async fn commit_generated_compact(
        &self,
        session_id: &SessionId,
        source: &CompactSource,
        generated: GeneratedCompact,
    ) -> Result<CompactOutcome, ContextPortError> {
        let _mutation = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| ContextPortError::SessionRepository(error.to_string()))?
            .clone();
        if current.id != session_id.as_str() {
            return Err(ContextPortError::SessionNotFound(session_id.clone()));
        }
        let actual_revision = SessionRevision::new(current.revision);
        if actual_revision != source.revision {
            return Err(Self::compact_revision_conflict(
                source.revision,
                actual_revision,
            ));
        }
        let keep_messages = generated.recent_messages.len();
        let mut retained = 0usize;
        let mut start_at = None;
        for (cursor, step_messages) in source.visible_steps.iter().rev() {
            retained += step_messages.len();
            start_at = Some(cursor.clone());
            if retained >= keep_messages {
                break;
            }
        }
        let mut candidate = (*current).clone();
        candidate.compact = Some(ActiveCompactMarker {
            summary: generated.summary.clone(),
            start_at,
            source_revision: source.revision.get(),
        });
        candidate.revision += 1;
        candidate.updated_at = crate::domain::session::now_iso();
        self.persist_candidate(
            &current,
            &candidate,
            SessionSaveIntent::CommitPartialHistory,
        )
        .await
        .map_err(ContextPortError::Compact)?;
        self.publish_generation(&current, candidate)
            .map_err(ContextPortError::SessionRepository)?;
        Ok(CompactOutcome::Committed(crate::domain::CompactResult {
            summary: generated.summary,
            recent_messages: generated.recent_messages,
            source_revision: source.revision,
            quality: generated.quality,
        }))
    }

    fn compact_revision_conflict(
        expected: SessionRevision,
        actual: SessionRevision,
    ) -> ContextPortError {
        ContextPortError::Compact(format!(
            "Session revision 冲突：期望 {expected:?}，实际 {actual:?}"
        ))
    }

    fn receipt(append: &ContextAppend, revision: SessionRevision) -> AppendReceipt {
        AppendReceipt {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: revision,
            fingerprint: append.fingerprint.clone(),
        }
    }

    fn accepted_receipt(
        append: &AcceptedInputAppend,
        revision: SessionRevision,
    ) -> AcceptedInputReceipt {
        AcceptedInputReceipt {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: revision,
            fingerprint: append.fingerprint.clone(),
        }
    }

    fn build_commit_plan(
        before: &CanonicalSession,
        after: &CanonicalSession,
        intent: SessionSaveIntent,
    ) -> Result<SessionCommitPlan, String> {
        match intent {
            SessionSaveIntent::CommitPartialHistory => {
                let manifest = crate::domain::session::SessionGenerationManifest::new(
                    before.id.clone(),
                    before.revision,
                    before
                        .run_slices
                        .iter()
                        .flat_map(|slice| {
                            slice
                                .steps
                                .iter()
                                .map(|step| crate::domain::session::RunStepCursor {
                                    run_id: slice.run_id.clone(),
                                    step_id: step.step_id.clone(),
                                })
                        })
                        .collect(),
                )
                .map_err(|error| error.to_string())?;
                SessionCommitPlan::between_preserving_unloaded_steps(before, after, &manifest)
            }
            SessionSaveIntent::ReplaceCompleteHistory => SessionCommitPlan::between(before, after),
        }
        .map_err(|error| error.to_string())
    }

    async fn persist_candidate(
        &self,
        before: &CanonicalSession,
        after: &CanonicalSession,
        intent: SessionSaveIntent,
    ) -> Result<(), String> {
        let plan = Self::build_commit_plan(before, after, intent)?;
        self.writer.commit(&after.id, before.revision, plan).await
    }

    fn publish_generation(
        &self,
        current: &Arc<CanonicalSession>,
        candidate: CanonicalSession,
    ) -> Result<(), String> {
        let mut committed = self.session.write().map_err(|error| error.to_string())?;
        #[cfg(any(test, feature = "dev"))]
        crate::adapters::session_lifecycle::record_generation_transition(current, &candidate);
        #[cfg(not(any(test, feature = "dev")))]
        let _ = current;
        *committed = Arc::new(candidate);
        Ok(())
    }
}

#[async_trait]
impl SessionRepository for CanonicalSessionRepository {
    async fn snapshot(&self, session_id: &SessionId) -> Result<SessionSnapshot, String> {
        let session = self.session.read().map_err(|error| error.to_string())?;
        if session.id != session_id.as_str() {
            return Err(format!("Session 不存在：{session_id}"));
        }
        let structured_history = session.visible_history();
        let messages = crate::domain::ContextMessages::from_committed_steps(
            structured_history
                .iter()
                .flat_map(|slice| slice.steps.iter())
                .flat_map(|step| {
                    step.accepted_input
                        .iter()
                        .map(|input| input.messages.as_arc())
                        .chain(step.outcome.iter().map(|outcome| outcome.messages.as_arc()))
                })
                .collect(),
            Vec::new(),
        );
        Ok(SessionSnapshot {
            revision: SessionRevision::new(session.revision),
            messages,
            structured_history: Some(structured_history),
            active_summary: session.active_summary().map(str::to_string),
        })
    }

    async fn append_accepted_input(
        &self,
        append: &AcceptedInputAppend,
    ) -> Result<AcceptedInputReceipt, AcceptedInputError> {
        let _mutation = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| AcceptedInputError::Storage(error.to_string()))?
            .clone();
        if current.id != append.session_id.as_str() {
            return Err(AcceptedInputError::SessionNotFound(
                append.session_id.clone(),
            ));
        }
        if let Some(input) = current.accepted_input(append.run_id.as_ref(), append.step_id.as_str())
        {
            if input.fingerprint == append.fingerprint.as_str() {
                return Ok(Self::accepted_receipt(
                    append,
                    SessionRevision::new(input.committed_revision),
                ));
            }
            return Err(AcceptedInputError::ContentConflict {
                run_id: append.run_id.clone(),
                step_id: append.step_id.clone(),
            });
        }
        let mut candidate = (*current).clone();
        candidate.updated_at = crate::domain::session::now_iso();
        candidate.tasks = SnapshotState::Captured(self.task_persist.collect_snapshot());
        candidate.workspace = SnapshotState::Captured(self.workspace_persist.snapshot());
        candidate.append_accepted_input(
            append.run_id.as_ref(),
            append.step_id.as_str(),
            AcceptedInputProjection::new(
                append.messages.clone(),
                append.fingerprint.as_str(),
                candidate.revision,
            ),
        );
        let input = candidate
            .accepted_input(append.run_id.as_ref(), append.step_id.as_str())
            .expect("accepted input must exist")
            .clone();
        self.accepted_input_writer
            .save(
                &current.id,
                candidate.revision,
                append.run_id.as_ref(),
                append.step_id.as_str(),
                &input,
            )
            .await
            .map_err(AcceptedInputError::Storage)?;
        let revision = SessionRevision::new(candidate.revision);
        self.publish_generation(&current, candidate)
            .map_err(AcceptedInputError::Storage)?;
        Ok(Self::accepted_receipt(append, revision))
    }

    async fn advance_tool_receipt(
        &self,
        mutation: ToolReceiptMutation,
    ) -> Result<ToolReceiptMutationReceipt, ToolReceiptMutationError> {
        let _mutation_guard = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| ToolReceiptMutationError::Storage(error.to_string()))?
            .clone();
        if current.id != mutation.identity.session_id.as_str() {
            return Err(ToolReceiptMutationError::SessionNotFound(
                mutation.identity.session_id.clone(),
            ));
        }
        if let Some(receipt) = current.tool_receipt(&mutation) {
            let advanced = receipt.clone().advance(mutation.clone())?;
            if !advanced.changed {
                return Ok(advanced);
            }
        }
        let mut candidate = (*current).clone();
        let changed = candidate.advance_tool_receipt(mutation.clone())?;
        if !changed {
            let receipt = candidate
                .tool_receipt(&mutation)
                .expect("unchanged receipt must exist")
                .clone();
            return Ok(ToolReceiptMutationReceipt {
                receipt,
                changed: false,
            });
        }
        candidate.updated_at = crate::domain::session::now_iso();
        candidate.tasks = SnapshotState::Captured(self.task_persist.collect_snapshot());
        candidate.workspace = SnapshotState::Captured(self.workspace_persist.snapshot());
        let receipt = candidate
            .tool_receipt(&mutation)
            .expect("advanced receipt must exist")
            .clone();
        let revision = current.revision;
        self.tool_receipt_writer
            .save(&current.id, revision, &receipt)
            .await
            .map_err(ToolReceiptMutationError::Storage)?;
        self.publish_generation(&current, candidate)
            .map_err(ToolReceiptMutationError::Storage)?;
        return Ok(ToolReceiptMutationReceipt {
            receipt,
            changed: true,
        });
    }

    async fn step_receipts(
        &self,
        session_id: &SessionId,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
    ) -> Result<Vec<crate::domain::StepReceipt>, ToolReceiptMutationError> {
        let current = self
            .session
            .read()
            .map_err(|error| ToolReceiptMutationError::Storage(error.to_string()))?
            .clone();
        if current.id != session_id.as_str() {
            return Err(ToolReceiptMutationError::SessionNotFound(
                session_id.clone(),
            ));
        }
        Ok(current.step_receipts(run_id.as_ref(), step_id.as_str()))
    }

    async fn compare_and_record_skill_load(
        &self,
        mutation: tools::SkillLoadMutation,
    ) -> Result<tools::SkillLoadDecision, tools::SkillLoadStateError> {
        let _mutation_guard = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| tools::SkillLoadStateError::Storage(error.to_string()))?
            .clone();
        if current.id != mutation.session_id() {
            return Err(tools::SkillLoadStateError::SessionNotFound(
                mutation.session_id().to_string(),
            ));
        }
        let mut candidate = (*current).clone();
        let decision = candidate.compare_and_record_skill(
            mutation.scope(),
            mutation.skill_name(),
            mutation.revision(),
        );
        if decision == tools::SkillLoadDecision::AlreadyLoaded {
            return Ok(decision);
        }
        candidate.revision += 1;
        candidate.updated_at = crate::domain::session::now_iso();
        candidate.tasks = SnapshotState::Captured(self.task_persist.collect_snapshot());
        candidate.workspace = SnapshotState::Captured(self.workspace_persist.snapshot());
        self.persist_candidate(
            &current,
            &candidate,
            SessionSaveIntent::CommitPartialHistory,
        )
        .await
        .map_err(tools::SkillLoadStateError::Storage)?;
        self.publish_generation(&current, candidate)
            .map_err(tools::SkillLoadStateError::Storage)?;
        Ok(decision)
    }

    async fn append_finalized(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError> {
        let _mutation = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| ContextAppendError::Storage(error.to_string()))?
            .clone();
        if current.id != append.session_id.as_str() {
            return Err(ContextAppendError::SessionNotFound(
                append.session_id.clone(),
            ));
        }
        if let Some(committed) = current
            .committed_steps
            .find(append.run_id.as_ref(), append.step_id.as_str())
        {
            if committed.fingerprint == append.fingerprint.as_str() {
                let revision = SessionRevision::new(committed.committed_revision);
                self.acknowledge_finalized_input_ledger(
                    &current.id,
                    append.run_id.as_ref(),
                    append.step_id.as_str(),
                )
                .await;
                return Ok(Self::receipt(append, revision));
            }
            return Err(ContextAppendError::ContentConflict {
                run_id: append.run_id.clone(),
                step_id: append.step_id.clone(),
            });
        }
        let actual = SessionRevision::new(current.revision);
        if actual != append.expected_revision {
            let receipt_only_advances = current
                .run_slices
                .iter()
                .find(|slice| slice.run_id == append.run_id.as_ref())
                .and_then(|slice| {
                    slice
                        .steps
                        .iter()
                        .find(|step| step.step_id == append.step_id.as_str())
                })
                .is_some_and(|step| step.outcome.is_none());
            if !receipt_only_advances {
                return Err(ContextAppendError::RevisionConflict {
                    expected: append.expected_revision,
                    actual,
                });
            }
        }

        let mut candidate = (*current).clone();
        let mut append = append.clone();
        if append.receipts.is_empty() {
            append.receipts =
                current.step_receipts(append.run_id.as_ref(), append.step_id.as_str());
        }
        candidate.revision += 1;
        candidate.updated_at = crate::domain::session::now_iso();
        candidate.tasks = SnapshotState::Captured(self.task_persist.collect_snapshot());
        candidate.workspace = SnapshotState::Captured(self.workspace_persist.snapshot());
        candidate.append_finalized_outcome(
            append.run_id.as_ref(),
            append.step_id.as_str(),
            FinalizedOutcomeProjection {
                finalize_cause: append.finalize_cause,
                duration_ms: append.duration_ms,
                messages: append.messages.clone().into(),
                receipts: append.receipts.clone(),
                api_input_tokens: append.api_input_tokens,
                fingerprint: append.fingerprint.as_str().to_string(),
                committed_revision: candidate.revision,
            },
        );
        candidate.committed_steps = candidate.committed_steps.append(CommittedStep {
            run_id: append.run_id.to_string(),
            step_id: append.step_id.as_str().to_string(),
            fingerprint: append.fingerprint.as_str().to_string(),
            committed_revision: candidate.revision,
        });

        self.persist_candidate(
            &current,
            &candidate,
            SessionSaveIntent::CommitPartialHistory,
        )
        .await
        .map_err(ContextAppendError::Storage)?;
        let revision = SessionRevision::new(candidate.revision);
        self.publish_generation(&current, candidate)
            .map_err(ContextAppendError::Storage)?;
        self.acknowledge_finalized_input_ledger(
            &current.id,
            append.run_id.as_ref(),
            append.step_id.as_str(),
        )
        .await;
        Ok(Self::receipt(&append, revision))
    }

    async fn commit_compaction(
        &self,
        request: &CompactRequest,
    ) -> Result<CompactOutcome, ContextPortError> {
        let Some(attempt) = self.begin_auto_compact_attempt(
            request.source.config_snapshot.auto_compact_failure_limit(),
        ) else {
            return Ok(CompactOutcome::Skipped(
                CompactSkipReason::CircuitBreakerOpen,
            ));
        };
        let result = self.commit_automatic_compaction(request).await;
        attempt.finish(&result);
        result
    }

    async fn commit_manual_compaction(
        &self,
        request: &ManualCompactRequest,
    ) -> Result<CompactOutcome, ContextPortError> {
        let source = self
            .freeze_compact_source(&request.session_id, None)
            .await?;
        if source.messages.len() <= 4 {
            return Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection));
        }
        let generated = match self
            .generate_compact(
                &source,
                request.context_size,
                request.progress.clone(),
                request.task_snapshot.as_ref(),
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
        {
            Ok(Some(generated)) => generated,
            Ok(None) => {
                return Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection));
            }
            Err(reason) => return Ok(CompactOutcome::Skipped(reason)),
        };
        self.commit_generated_compact(&request.session_id, &source, generated)
            .await
    }

    async fn clear(&self, session_id: &SessionId) -> Result<(), ContextPortError> {
        let _mutation = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| ContextPortError::SessionRepository(error.to_string()))?
            .clone();
        if current.id != session_id.as_str() {
            return Err(ContextPortError::SessionNotFound(session_id.clone()));
        }
        let mut candidate = (*current).clone();
        candidate.chats.clear();
        candidate.compact = None;
        candidate.run_slices = candidate.run_slices.cleared();
        candidate.committed_steps = candidate.committed_steps.cleared();
        candidate.skill_load_records.clear();
        candidate.revision += 1;
        candidate.updated_at = crate::domain::session::now_iso();
        candidate.tasks = SnapshotState::Captured(self.task_persist.collect_snapshot());
        candidate.workspace = SnapshotState::Captured(self.workspace_persist.snapshot());
        self.persist_candidate(
            &current,
            &candidate,
            SessionSaveIntent::ReplaceCompleteHistory,
        )
        .await
        .map_err(ContextPortError::SessionRepository)?;
        self.publish_generation(&current, candidate)
            .map_err(ContextPortError::SessionRepository)?;
        self.accepted_input_writer
            .delete_all(&current.id)
            .await
            .map_err(ContextPortError::SessionRepository)?;
        Ok(())
    }
}
