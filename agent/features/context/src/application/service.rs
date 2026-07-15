use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::{
    AppendReceipt, CompactOutcome, CompactRequest, CompactionDecision, ContextAppend,
    ContextAppendError, ContextPortError, ContextRequest, ContextWindow, SystemBlock,
};
use crate::ports::{
    ContextPort, MemoryMaterializer, PromptMaterializer, SessionBacking, WindowProjector,
};

pub struct ContextApplicationService {
    session: Arc<dyn SessionBacking>,
    projector: Arc<dyn WindowProjector>,
    prompt: Arc<dyn PromptMaterializer>,
    memory: Arc<dyn MemoryMaterializer>,
}

impl ContextApplicationService {
    pub fn new(
        session: Arc<dyn SessionBacking>,
        projector: Arc<dyn WindowProjector>,
        prompt: Arc<dyn PromptMaterializer>,
        memory: Arc<dyn MemoryMaterializer>,
    ) -> Self {
        Self {
            session,
            projector,
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
            .map_err(ContextPortError::SessionBacking)?;
        let mut messages = snapshot.messages;
        messages.extend(request.pending_messages.clone());
        let projection = self.projector.project(messages);

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
            });
        }
        blocks.push(SystemBlock {
            kind: "cache_breakpoint".into(),
            content: String::new(),
            cacheable: true,
        });
        blocks.extend(prompt.uncached);
        if let Some(reminder) = &request.task_reminder.text {
            blocks.push(SystemBlock {
                kind: "task_reminder".into(),
                content: reminder.clone(),
                cacheable: false,
            });
        }

        let token_estimation =
            crate::domain::context_decision::token_budget(request, &projection.messages, &blocks);
        let decision =
            crate::domain::context_decision::calculate(request, &projection.messages, &blocks);
        Ok(ContextWindow {
            system_blocks: blocks,
            messages: projection.messages,
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
        self.session.compact(request).await
    }

    async fn append_and_persist(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError> {
        self.session.append(append).await
    }
}
