use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::{
    AcceptedInputAppend, AcceptedInputError, AcceptedInputReceipt, AppendReceipt, CompactOutcome,
    CompactRequest, CompactionDecision, ContextAppend, ContextAppendError, ContextPortError,
    ContextRequest, ContextWindow, ManualCompactRequest, SessionId, SystemBlock,
};
use crate::ports::{ContextMemorySource, ContextPort, ContextPromptSource, SessionRepository};

pub struct ContextApplicationService {
    session: Arc<dyn SessionRepository>,
    prompt: Arc<dyn ContextPromptSource>,
    memory: Arc<dyn ContextMemorySource>,
}

impl ContextApplicationService {
    pub fn new(
        session: Arc<dyn SessionRepository>,
        prompt: Arc<dyn ContextPromptSource>,
        memory: Arc<dyn ContextMemorySource>,
    ) -> Self {
        Self {
            session,
            prompt,
            memory,
        }
    }

    async fn build_candidate(
        &self,
        request: &ContextRequest,
    ) -> Result<ContextWindow, ContextPortError> {
        let snapshot = self
            .session
            .snapshot(&request.session_id)
            .await
            .map_err(ContextPortError::SessionRepository)?;
        let mut messages = snapshot.messages;
        messages.extend(request.pending_messages.clone());

        let prompt = self
            .prompt
            .materialize(request)
            .await
            .map_err(ContextPortError::PromptMaterialization)?;
        let memory = self
            .memory
            .materialize(request)
            .await
            .map_err(ContextPortError::MemoryMaterialization)?;

        let mut blocks = prompt.cacheable;
        blocks.extend(memory.blocks);
        if let Some(summary) = snapshot.active_summary {
            blocks.push(SystemBlock {
                kind: "active_summary".into(),
                content: summary,
                cacheable: true,
                cache_break: false,
            });
        }
        if let Some(last_cacheable) = blocks.last_mut() {
            last_cacheable.cache_break = true;
        }
        blocks.extend(prompt.uncached);
        if let Some(reminder) = &request.task_reminder.text {
            blocks.push(SystemBlock {
                kind: "task_reminder".into(),
                content: reminder.clone(),
                cacheable: false,
                cache_break: false,
            });
        }

        let token_estimation =
            crate::domain::context_decision::token_budget(request, &messages, &blocks);
        let decision = crate::domain::context_decision::calculate(request, &messages, &blocks);
        Ok(ContextWindow {
            backing_revision: snapshot.revision,
            system_blocks: blocks,
            messages,
            tool_schemas: request.tool_schemas.clone(),
            token_estimation,
            compaction_decision: decision,
        })
    }
}

#[async_trait]
impl ContextPort for ContextApplicationService {
    async fn build_window(
        &self,
        request: &ContextRequest,
    ) -> Result<ContextWindow, ContextPortError> {
        self.build_candidate(request).await
    }

    async fn needs_compaction(
        &self,
        request: &ContextRequest,
    ) -> Result<CompactionDecision, ContextPortError> {
        Ok(self.build_candidate(request).await?.compaction_decision)
    }

    async fn compact(&self, request: &CompactRequest) -> Result<CompactOutcome, ContextPortError> {
        self.session.commit_compaction(request).await
    }

    async fn manual_compact(
        &self,
        request: &ManualCompactRequest,
    ) -> Result<CompactOutcome, ContextPortError> {
        self.session.commit_manual_compaction(request).await
    }

    async fn clear_session(&self, session_id: &SessionId) -> Result<(), ContextPortError> {
        self.session.clear(session_id).await
    }

    async fn append_accepted_input(
        &self,
        append: &AcceptedInputAppend,
    ) -> Result<AcceptedInputReceipt, AcceptedInputError> {
        self.session.append_accepted_input(append).await
    }

    async fn append_and_persist(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError> {
        self.session.append_finalized(append).await
    }
}
