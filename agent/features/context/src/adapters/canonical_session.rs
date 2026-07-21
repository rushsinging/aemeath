use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use crate::domain::session::{
    AcceptedInputProjection, ActiveCompactMarker, CanonicalSession, CommittedStep,
    FinalizedOutcomeProjection, SnapshotState,
};
use crate::domain::{
    AcceptedInputAppend, AcceptedInputError, AcceptedInputReceipt, AppendReceipt, CompactOutcome,
    CompactRequest, CompactSkipReason, ContextAppend, ContextAppendError, ContextPortError,
    ManualCompactRequest, SessionId, SessionRevision,
};
use crate::ports::{ContextPort, MainContextFactory, SessionRepository, SessionSnapshot};

#[async_trait]
pub trait CanonicalSessionWriter: Send + Sync {
    async fn save(&self, session: &CanonicalSession) -> Result<(), String>;
}

pub struct AtomicBlobCanonicalSessionWriter {
    blob: Arc<dyn storage::api::AtomicBlobPort>,
}

impl AtomicBlobCanonicalSessionWriter {
    pub fn new(blob: Arc<dyn storage::api::AtomicBlobPort>) -> Self {
        Self { blob }
    }
}

#[async_trait]
impl CanonicalSessionWriter for AtomicBlobCanonicalSessionWriter {
    async fn save(&self, session: &CanonicalSession) -> Result<(), String> {
        use crate::ports::SessionSnapshotStore;

        let store =
            crate::adapters::AtomicBlobSessionStore::new(Arc::clone(&self.blob), &session.id)
                .map_err(|error| error.to_string())?;
        let bytes = crate::domain::session::SessionCodec::encode(session)
            .map_err(|error| error.to_string())?;
        store.write(&bytes).await.map_err(|error| error.to_string())
    }
}

pub struct NoOpCanonicalSessionWriter;

#[async_trait]
impl CanonicalSessionWriter for NoOpCanonicalSessionWriter {
    async fn save(&self, _session: &CanonicalSession) -> Result<(), String> {
        Ok(())
    }
}

pub struct ProductionMainContextFactory {
    writer: Arc<dyn CanonicalSessionWriter>,
    /// 可选注入的 Skill supplier port 与 Context-owned query factory。
    /// 注入后 `build` 组装 `SkillPromptSource`；否则退回 `BaselinePromptSource`。
    /// Composition 后续注入真实 `FilesystemSkillAdapter` + `WorkspaceSkillQueryFactory`。
    skill_supplier: Option<Arc<dyn tools::SkillMaterializationPort>>,
    query_factory: Option<Arc<dyn crate::ports::SkillQueryFactory>>,
}

impl ProductionMainContextFactory {
    pub fn new(writer: Arc<dyn CanonicalSessionWriter>) -> Self {
        Self {
            writer,
            skill_supplier: None,
            query_factory: None,
        }
    }

    /// 注入 Skill supplier port 与 Context-owned query factory，使 `build`
    /// 组装 `SkillPromptSource`（Issue #912）。
    pub fn with_skill_supplier(
        mut self,
        supplier: Arc<dyn tools::SkillMaterializationPort>,
        query_factory: Arc<dyn crate::ports::SkillQueryFactory>,
    ) -> Self {
        self.skill_supplier = Some(supplier);
        self.query_factory = Some(query_factory);
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
            match (&self.skill_supplier, &self.query_factory) {
                (Some(supplier), Some(factory)) => {
                    Arc::new(crate::adapters::SkillPromptSource::new(
                        Arc::clone(supplier),
                        Arc::clone(factory),
                    ))
                }
                _ => Arc::new(crate::adapters::BaselinePromptSource),
            };
        Arc::new(crate::application::ContextApplicationService::new(
            Arc::new(CanonicalSessionRepository::new(
                session,
                task_persist,
                workspace_persist,
                Arc::clone(&self.writer),
                mutation_gate,
            )),
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
    mutation_gate: Arc<tokio::sync::Mutex<()>>,
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
            writer,
            mutation_gate,
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
}

#[async_trait]
impl SessionRepository for CanonicalSessionRepository {
    async fn snapshot(&self, session_id: &SessionId) -> Result<SessionSnapshot, String> {
        let session = self.session.read().map_err(|error| error.to_string())?;
        if session.id != session_id.as_str() {
            return Err(format!("Session 不存在：{session_id}"));
        }
        let messages = session.structured_messages();
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
        candidate.revision += 1;
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
        self.writer
            .save(&candidate)
            .await
            .map_err(AcceptedInputError::Storage)?;
        let revision = SessionRevision::new(candidate.revision);
        *self
            .session
            .write()
            .map_err(|error| AcceptedInputError::Storage(error.to_string()))? = Arc::new(candidate);
        Ok(Self::accepted_receipt(append, revision))
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
        if let Some(committed) = current.committed_steps.iter().find(|committed| {
            committed.run_id == append.run_id.to_string()
                && committed.step_id == append.step_id.as_str()
        }) {
            if committed.fingerprint == append.fingerprint.as_str() {
                return Ok(Self::receipt(
                    append,
                    SessionRevision::new(committed.committed_revision),
                ));
            }
            return Err(ContextAppendError::ContentConflict {
                run_id: append.run_id.clone(),
                step_id: append.step_id.clone(),
            });
        }
        let actual = SessionRevision::new(current.revision);
        if actual != append.expected_revision {
            return Err(ContextAppendError::RevisionConflict {
                expected: append.expected_revision,
                actual,
            });
        }

        let mut candidate = (*current).clone();
        candidate.revision += 1;
        candidate.updated_at = crate::domain::session::now_iso();
        candidate.tasks = SnapshotState::Captured(self.task_persist.collect_snapshot());
        candidate.workspace = SnapshotState::Captured(self.workspace_persist.snapshot());
        candidate.append_finalized_outcome(
            append.run_id.as_ref(),
            append.step_id.as_str(),
            FinalizedOutcomeProjection {
                finalize_cause: append.finalize_cause,
                messages: append.messages.clone(),
                receipts: append.receipts.clone(),
                api_input_tokens: append.api_input_tokens,
                fingerprint: append.fingerprint.as_str().to_string(),
                committed_revision: candidate.revision,
            },
        );
        candidate.committed_steps.push(CommittedStep {
            run_id: append.run_id.to_string(),
            step_id: append.step_id.as_str().to_string(),
            fingerprint: append.fingerprint.as_str().to_string(),
            committed_revision: candidate.revision,
        });

        self.writer
            .save(&candidate)
            .await
            .map_err(ContextAppendError::Storage)?;
        let revision = SessionRevision::new(candidate.revision);
        *self
            .session
            .write()
            .map_err(|error| ContextAppendError::Storage(error.to_string()))? = Arc::new(candidate);
        Ok(Self::receipt(append, revision))
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
        let Some(compacted) = crate::adapters::compact_summary::compact_messages(
            &messages,
            request.source.system_prompt.as_str(),
            request.source.context_size,
        ) else {
            return Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection));
        };
        let mut candidate = (*current).clone();
        let source_revision = SessionRevision::new(candidate.revision);
        let keep_messages = compacted.recent_messages.len();
        let mut retained = 0usize;
        let mut start_at = None;
        for (cursor, step_messages) in visible_steps.iter().rev() {
            retained += step_messages.len();
            start_at = Some(cursor.clone());
            if retained >= keep_messages {
                break;
            }
        }
        let summary = crate::adapters::compact_summary::build_summary_text(
            &messages[..messages.len().saturating_sub(retained)],
            candidate
                .compact
                .as_ref()
                .map(|marker| marker.summary.as_str()),
        );
        candidate.compact = Some(ActiveCompactMarker {
            summary: summary.clone(),
            start_at,
            source_revision: source_revision.get(),
        });
        candidate.revision += 1;
        candidate.updated_at = crate::domain::session::now_iso();
        self.writer
            .save(&candidate)
            .await
            .map_err(ContextPortError::Compact)?;
        *self
            .session
            .write()
            .map_err(|error| ContextPortError::SessionRepository(error.to_string()))? =
            Arc::new(candidate);
        Ok(CompactOutcome::Committed(crate::domain::CompactResult {
            summary,
            recent_messages: compacted.recent_messages,
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
        let context_size = request.context_size.max(1);
        let Some(compacted) = crate::adapters::compact_summary::compact_messages(
            &messages,
            request.system_prompt.as_str(),
            context_size,
        ) else {
            return Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection));
        };
        let mut candidate = (*current).clone();
        let source_revision = SessionRevision::new(candidate.revision);
        let keep_messages = compacted.recent_messages.len();
        let mut retained = 0usize;
        let mut start_at = None;
        for (cursor, step_messages) in visible_steps.iter().rev() {
            retained += step_messages.len();
            start_at = Some(cursor.clone());
            if retained >= keep_messages {
                break;
            }
        }
        let summary = crate::adapters::compact_summary::build_summary_text(
            &messages[..messages.len().saturating_sub(retained)],
            candidate
                .compact
                .as_ref()
                .map(|marker| marker.summary.as_str()),
        );
        candidate.compact = Some(ActiveCompactMarker {
            summary: summary.clone(),
            start_at,
            source_revision: source_revision.get(),
        });
        candidate.revision += 1;
        candidate.updated_at = crate::domain::session::now_iso();
        self.writer
            .save(&candidate)
            .await
            .map_err(ContextPortError::Compact)?;
        *self
            .session
            .write()
            .map_err(|error| ContextPortError::SessionRepository(error.to_string()))? =
            Arc::new(candidate);
        Ok(CompactOutcome::Committed(crate::domain::CompactResult {
            summary,
            recent_messages: compacted.recent_messages,
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
        candidate.run_slices.clear();
        candidate.committed_steps.clear();
        candidate.revision += 1;
        candidate.updated_at = crate::domain::session::now_iso();
        candidate.tasks = SnapshotState::Captured(self.task_persist.collect_snapshot());
        candidate.workspace = SnapshotState::Captured(self.workspace_persist.snapshot());
        self.writer
            .save(&candidate)
            .await
            .map_err(ContextPortError::SessionRepository)?;
        *self
            .session
            .write()
            .map_err(|error| ContextPortError::SessionRepository(error.to_string()))? =
            Arc::new(candidate);
        Ok(())
    }
}

#[async_trait]
impl CanonicalSessionWriter for crate::application::SessionPersistenceService {
    async fn save(&self, session: &CanonicalSession) -> Result<(), String> {
        crate::application::SessionPersistenceService::save(self, session)
            .await
            .map_err(|error| error.to_string())
    }
}
