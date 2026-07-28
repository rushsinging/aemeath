use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::{
    AcceptedInputAppend, AcceptedInputError, AcceptedInputReceipt, AppendReceipt, CompactOutcome,
    CompactRequest, CompactionDecision, ContextAppend, ContextAppendError, ContextPortError,
    ContextRequest, ContextWindow, InvocationReminder, ManualCompactRequest, SessionId,
    SystemBlock,
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
        #[cfg(test)]
        let build_started = std::time::Instant::now();
        #[cfg(test)]
        let snapshot_started = std::time::Instant::now();
        let snapshot = self
            .session
            .snapshot(&request.session_id)
            .await
            .map_err(ContextPortError::SessionRepository)?;
        #[cfg(test)]
        {
            let (snapshot_committed_steps, snapshot_shared_messages) =
                snapshot.messages.shared_backing_metrics();
            crate::application::performance::record_snapshot(
                snapshot.revision.get(),
                snapshot.messages.len(),
                snapshot_committed_steps,
                snapshot_shared_messages,
                snapshot_started.elapsed(),
            );
        }
        #[cfg(test)]
        let messages_started = std::time::Instant::now();
        let messages = snapshot
            .messages
            .with_pending(request.pending_messages.clone());
        #[cfg(test)]
        let messages_assembly_duration = messages_started.elapsed();

        #[cfg(test)]
        let prompt_started = std::time::Instant::now();
        let prompt = self
            .prompt
            .materialize(request)
            .await
            .map_err(ContextPortError::PromptMaterialization)?;
        #[cfg(test)]
        crate::application::performance::record_prompt(prompt_started.elapsed());
        #[cfg(test)]
        let memory_started = std::time::Instant::now();
        let memory = self
            .memory
            .materialize(request)
            .await
            .map_err(ContextPortError::MemoryMaterialization)?;
        #[cfg(test)]
        crate::application::performance::record_memory(memory_started.elapsed());

        #[cfg(test)]
        let blocks_started = std::time::Instant::now();
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
        let invocation_reminder =
            InvocationReminder::from_task_snapshot(&request.task_reminder, &request.language);

        #[cfg(test)]
        {
            let (tool_result_blocks, tool_result_content_bytes) =
                context_message_tool_result_metrics(&messages);
            crate::application::performance::record_assembly(
                crate::application::performance::AssemblyMetrics {
                    pending_messages: request.pending_messages.len(),
                    final_messages: messages.len(),
                    system_blocks: blocks.len(),
                    tool_result_blocks,
                    tool_result_content_bytes,
                },
                messages_assembly_duration.saturating_add(blocks_started.elapsed()),
            );
        }
        #[cfg(test)]
        let decision_started = std::time::Instant::now();
        let token_estimation = crate::domain::context_decision::token_budget(
            request,
            &messages,
            &blocks,
            invocation_reminder.as_ref().map(InvocationReminder::as_str),
        );
        let decision = crate::domain::context_decision::calculate(
            request,
            &messages,
            &blocks,
            invocation_reminder.as_ref().map(InvocationReminder::as_str),
        );
        #[cfg(test)]
        crate::application::performance::record_decision(
            token_estimation.total_tokens,
            request.last_api_total_tokens,
            decision.decision_token_count,
            decision.reason,
            decision_started.elapsed(),
        );
        let window = ContextWindow {
            backing_revision: snapshot.revision,
            system_blocks: blocks,
            messages,
            invocation_reminder,
            tool_schemas: request.tool_schemas.clone(),
            token_estimation,
            compaction_decision: decision,
        };
        #[cfg(test)]
        crate::application::performance::record_build(build_started.elapsed());
        Ok(window)
    }
}

#[cfg(test)]
fn context_message_tool_result_metrics(messages: &crate::domain::ContextMessages) -> (usize, u64) {
    messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            share::message::ContentBlock::ToolResult { content, .. } => Some(content),
            _ => None,
        })
        .fold((0usize, 0u64), |(count, bytes), content| {
            (
                count.saturating_add(1),
                bytes.saturating_add(u64::try_from(content.to_string().len()).unwrap_or(u64::MAX)),
            )
        })
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
