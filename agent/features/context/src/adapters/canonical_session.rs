use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use crate::adapters::compact_summary::{compact_messages_with_llm, CompactGenerator};
use crate::domain::session::{
    AcceptedInputProjection, ActiveCompactMarker, CanonicalSession, CommittedStep,
    FinalizedOutcomeProjection, SnapshotState,
};
use crate::domain::{
    AcceptedInputAppend, AcceptedInputError, AcceptedInputReceipt, AppendReceipt, CompactOutcome,
    CompactRequest, CompactSkipReason, ContextAppend, ContextAppendError, ContextPortError,
    ManualCompactRequest, SessionId, SessionRevision, ToolReceiptMutation,
    ToolReceiptMutationError, ToolReceiptMutationReceipt,
};
use crate::ports::{ContextPort, MainContextFactory, SessionRepository, SessionSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionWriteScope {
    PreserveUnloadedHistory,
    ReplaceCompleteHistory,
}

#[async_trait]
pub trait CanonicalSessionWriter: Send + Sync {
    async fn save(
        &self,
        before: &CanonicalSession,
        after: &CanonicalSession,
        scope: SessionWriteScope,
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

#[async_trait]
impl CanonicalSessionWriter for AtomicBlobCanonicalSessionWriter {
    async fn save(
        &self,
        _before: &CanonicalSession,
        session: &CanonicalSession,
        _scope: SessionWriteScope,
    ) -> Result<(), String> {
        use crate::ports::SessionSnapshotStore;

        let store =
            crate::adapters::AtomicBlobSessionStore::new(Arc::clone(&self.blob), &session.id)
                .map_err(|error| error.to_string())?;
        let bytes = crate::domain::session::SessionCodec::encode(session)
            .map_err(|error| error.to_string())?;
        store
            .write(&bytes)
            .await
            .map_err(|error| error.to_string())?;
        crate::adapters::tool_receipt_ledger::AtomicBlobToolReceiptLedger::new(
            Arc::clone(&self.blob),
            &session.id,
        )?
        .delete()
        .await
    }
}

pub struct NoOpCanonicalSessionWriter;

#[async_trait]
impl CanonicalSessionWriter for NoOpCanonicalSessionWriter {
    async fn save(
        &self,
        _before: &CanonicalSession,
        _after: &CanonicalSession,
        _scope: SessionWriteScope,
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
    /// 返回 `(summary, recent_messages)`；消息太少无法压缩时返回 `None`。
    async fn compact_visible_messages(
        &self,
        messages: &[share::message::Message],
        previous_summary: Option<&str>,
        context_size: usize,
    ) -> Option<(String, Vec<share::message::Message>)> {
        match &self.generator {
            Some(generator) => {
                let result = compact_messages_with_llm(
                    messages,
                    previous_summary,
                    context_size,
                    Some(generator.as_ref()),
                    None,
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await;
                result.map(|compacted| {
                    log::info!(
                        target: crate::LOG_TARGET,
                        "[compact] LLM 路径提交：summary={} chars recent={}",
                        compacted.summary.len(),
                        compacted.recent_messages.len(),
                    );
                    (compacted.summary, compacted.recent_messages)
                })
            }
            None => {
                let compacted = crate::adapters::compact_summary::compact_messages(messages)?;
                let window = crate::adapters::compact_summary::compact_window(messages.len())?;
                let early = &messages[..window.split_point];
                let summary =
                    crate::adapters::compact_summary::build_summary_text(early, previous_summary);
                log::info!(
                    target: crate::LOG_TARGET,
                    "[compact] 本地路径提交：summary={} chars recent={}",
                    summary.len(),
                    compacted.recent_messages.len(),
                );
                Some((summary, compacted.recent_messages))
            }
        }
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
        let messages = crate::domain::ContextMessages::from_committed_steps(
            session
                .visible_message_steps()
                .into_iter()
                .map(|messages| messages.as_arc())
                .collect(),
            Vec::new(),
        );
        Ok(SessionSnapshot {
            revision: SessionRevision::new(session.revision),
            messages,
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
        self.writer
            .save(
                &current,
                &candidate,
                SessionWriteScope::PreserveUnloadedHistory,
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

        self.writer
            .save(
                &current,
                &candidate,
                SessionWriteScope::PreserveUnloadedHistory,
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
        let _mutation = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| ContextPortError::SessionRepository(error.to_string()))?
            .clone();
        if current.id != request.source.session_id.as_str() {
            return Err(ContextPortError::SessionNotFound(
                request.source.session_id.clone(),
            ));
        }
        let source_revision = request.source_revision;
        let actual_revision = SessionRevision::new(current.revision);
        if source_revision != actual_revision {
            return Err(ContextPortError::Compact(format!(
                "Session revision 冲突：期望 {source_revision:?}，实际 {actual_revision:?}"
            )));
        }
        let visible_steps = current.flattened_steps_from_marker();
        let messages: Vec<_> = visible_steps
            .iter()
            .flat_map(|(_, messages)| messages.iter().cloned())
            .collect();
        let previous_summary = current
            .compact
            .as_ref()
            .map(|marker| marker.summary.as_str());
        let Some((summary, recent_messages)) = self
            .compact_visible_messages(&messages, previous_summary, request.source.context_size)
            .await
        else {
            return Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection));
        };
        let mut candidate = (*current).clone();
        let source_revision = SessionRevision::new(candidate.revision);
        let keep_messages = recent_messages.len();
        let mut retained = 0usize;
        let mut start_at = None;
        for (cursor, step_messages) in visible_steps.iter().rev() {
            retained += step_messages.len();
            start_at = Some(cursor.clone());
            if retained >= keep_messages {
                break;
            }
        }
        candidate.compact = Some(ActiveCompactMarker {
            summary: summary.clone(),
            start_at,
            source_revision: source_revision.get(),
        });
        candidate.revision += 1;
        candidate.updated_at = crate::domain::session::now_iso();
        self.writer
            .save(
                &current,
                &candidate,
                SessionWriteScope::PreserveUnloadedHistory,
            )
            .await
            .map_err(ContextPortError::Compact)?;
        self.publish_generation(&current, candidate)
            .map_err(ContextPortError::SessionRepository)?;
        Ok(CompactOutcome::Committed(crate::domain::CompactResult {
            summary,
            recent_messages,
            source_revision,
        }))
    }
    async fn commit_manual_compaction(
        &self,
        request: &ManualCompactRequest,
    ) -> Result<CompactOutcome, ContextPortError> {
        let _mutation = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| ContextPortError::SessionRepository(error.to_string()))?
            .clone();
        if current.id != request.session_id.as_str() {
            return Err(ContextPortError::SessionNotFound(
                request.session_id.clone(),
            ));
        }
        let visible_steps = current.flattened_steps_from_marker();
        let messages: Vec<_> = visible_steps
            .iter()
            .flat_map(|(_, messages)| messages.iter().cloned())
            .collect();
        if messages.len() <= 4 {
            return Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection));
        }
        let previous_summary = current
            .compact
            .as_ref()
            .map(|marker| marker.summary.as_str());
        let Some((summary, recent_messages)) = self
            .compact_visible_messages(&messages, previous_summary, request.context_size)
            .await
        else {
            return Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection));
        };
        let mut candidate = (*current).clone();
        let source_revision = SessionRevision::new(candidate.revision);
        let keep_messages = recent_messages.len();
        let mut retained = 0usize;
        let mut start_at = None;
        for (cursor, step_messages) in visible_steps.iter().rev() {
            retained += step_messages.len();
            start_at = Some(cursor.clone());
            if retained >= keep_messages {
                break;
            }
        }
        candidate.compact = Some(ActiveCompactMarker {
            summary: summary.clone(),
            start_at,
            source_revision: source_revision.get(),
        });
        candidate.revision += 1;
        candidate.updated_at = crate::domain::session::now_iso();
        self.writer
            .save(
                &current,
                &candidate,
                SessionWriteScope::PreserveUnloadedHistory,
            )
            .await
            .map_err(ContextPortError::Compact)?;
        self.publish_generation(&current, candidate)
            .map_err(ContextPortError::SessionRepository)?;
        Ok(CompactOutcome::Committed(crate::domain::CompactResult {
            summary,
            recent_messages,
            source_revision,
        }))
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
        self.writer
            .save(
                &current,
                &candidate,
                SessionWriteScope::ReplaceCompleteHistory,
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

#[async_trait]
impl CanonicalSessionWriter for crate::application::SessionPersistenceService {
    async fn save(
        &self,
        _before: &CanonicalSession,
        session: &CanonicalSession,
        _scope: SessionWriteScope,
    ) -> Result<(), String> {
        crate::application::SessionPersistenceService::save(self, session)
            .await
            .map_err(|error| error.to_string())
    }
}
