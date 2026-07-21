use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::domain::session::{AcceptedInputProjection, FinalizedOutcomeProjection};
use crate::domain::{
    AcceptedInputAppend, AcceptedInputError, AcceptedInputReceipt, AppendReceipt, CompactOutcome,
    CompactRequest, CompactSkipReason, ContextAppend, ContextAppendError, ContextMessage,
    ContextPortError, SessionId, SessionRevision,
};
use crate::ports::{SessionRepository, SessionSnapshot};

#[derive(Default)]
struct SessionState {
    revision: u64,
    messages: Vec<ContextMessage>,
    active_summary: Option<String>,
    accepted_steps: HashMap<(String, String), AcceptedInputProjection>,
    committed_steps: HashMap<(String, String), FinalizedOutcomeProjection>,
}

/// #870 的确定性内存 backing；durable Envelope/AtomicBlob 由 #869/#880 替换。
#[derive(Default)]
pub struct InMemorySessionRepository {
    sessions: Mutex<HashMap<String, SessionState>>,
}

impl InMemorySessionRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed(
        &self,
        session_id: &SessionId,
        revision: SessionRevision,
        messages: Vec<ContextMessage>,
        active_summary: Option<String>,
    ) {
        self.sessions
            .lock()
            .expect("session mutex poisoned")
            .insert(
                session_id.as_str().to_string(),
                SessionState {
                    revision: revision.get(),
                    messages,
                    active_summary,
                    accepted_steps: HashMap::new(),
                    committed_steps: HashMap::new(),
                },
            );
    }

    fn receipt(append: &ContextAppend, committed_revision: SessionRevision) -> AppendReceipt {
        AppendReceipt {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision,
            fingerprint: append.fingerprint.clone(),
        }
    }

    fn accepted_receipt(
        append: &AcceptedInputAppend,
        committed_revision: SessionRevision,
    ) -> AcceptedInputReceipt {
        AcceptedInputReceipt {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision,
            fingerprint: append.fingerprint.clone(),
        }
    }
}

#[async_trait]
impl SessionRepository for InMemorySessionRepository {
    async fn snapshot(&self, session_id: &SessionId) -> Result<SessionSnapshot, String> {
        let sessions = self.sessions.lock().map_err(|error| error.to_string())?;
        let state = sessions
            .get(session_id.as_str())
            .ok_or_else(|| format!("Session 不存在：{session_id}"))?;
        Ok(SessionSnapshot {
            revision: SessionRevision::new(state.revision),
            messages: state.messages.clone(),
            active_summary: state.active_summary.clone(),
        })
    }

    async fn append_accepted_input(
        &self,
        append: &AcceptedInputAppend,
    ) -> Result<AcceptedInputReceipt, AcceptedInputError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|error| AcceptedInputError::Storage(error.to_string()))?;
        let state = sessions
            .get_mut(append.session_id.as_str())
            .ok_or_else(|| AcceptedInputError::SessionNotFound(append.session_id.clone()))?;
        let key = (
            append.run_id.to_string(),
            append.step_id.as_str().to_string(),
        );
        if let Some(input) = state.accepted_steps.get(&key) {
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
        state.messages.extend(append.messages.clone());
        state.revision += 1;
        let committed_revision = SessionRevision::new(state.revision);
        state.accepted_steps.insert(
            key,
            AcceptedInputProjection::new(
                append.messages.clone(),
                append.fingerprint.as_str(),
                committed_revision.get(),
            ),
        );
        Ok(Self::accepted_receipt(append, committed_revision))
    }

    async fn append_finalized(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|error| ContextAppendError::Storage(error.to_string()))?;
        let state = sessions
            .get_mut(append.session_id.as_str())
            .ok_or_else(|| ContextAppendError::SessionNotFound(append.session_id.clone()))?;
        let key = (
            append.run_id.to_string(),
            append.step_id.as_str().to_string(),
        );
        if let Some(outcome) = state.committed_steps.get(&key) {
            if outcome.fingerprint == append.fingerprint.as_str() {
                return Ok(Self::receipt(
                    append,
                    SessionRevision::new(outcome.committed_revision),
                ));
            }
            return Err(ContextAppendError::ContentConflict {
                run_id: append.run_id.clone(),
                step_id: append.step_id.clone(),
            });
        }
        let actual = SessionRevision::new(state.revision);
        if actual != append.expected_revision {
            return Err(ContextAppendError::RevisionConflict {
                expected: append.expected_revision,
                actual,
            });
        }
        state.messages.extend(append.messages.clone());
        state.revision += 1;
        let committed_revision = SessionRevision::new(state.revision);
        state.committed_steps.insert(
            key,
            FinalizedOutcomeProjection {
                finalize_cause: append.finalize_cause,
                messages: append.messages.clone(),
                receipts: append.receipts.clone(),
                api_input_tokens: append.api_input_tokens,
                fingerprint: append.fingerprint.as_str().to_string(),
                committed_revision: committed_revision.get(),
            },
        );
        Ok(Self::receipt(append, committed_revision))
    }

    async fn commit_compaction(
        &self,
        _request: &CompactRequest,
    ) -> Result<CompactOutcome, ContextPortError> {
        Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection))
    }

    async fn commit_manual_compaction(
        &self,
        _request: &crate::domain::ManualCompactRequest,
    ) -> Result<CompactOutcome, ContextPortError> {
        Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection))
    }

    async fn clear(&self, session_id: &SessionId) -> Result<(), ContextPortError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|error| ContextPortError::SessionRepository(error.to_string()))?;
        let state = sessions
            .get_mut(session_id.as_str())
            .ok_or_else(|| ContextPortError::SessionNotFound(session_id.clone()))?;
        state.messages.clear();
        state.active_summary = None;
        state.accepted_steps.clear();
        state.committed_steps.clear();
        state.revision += 1;
        Ok(())
    }
}
