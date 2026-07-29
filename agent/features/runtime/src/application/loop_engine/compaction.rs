use async_trait::async_trait;

use crate::application::context::coordination::{
    apply_automatic_compact_outcome, ContextCoordinator,
};
use crate::application::loop_engine::LoopEngineError;
use crate::application::run::context::{RunUsageTracker, RuntimeContext};
use crate::application::run::execution_state::RunExecutionState;
use crate::ports::CompactOutcome;

#[async_trait]
pub(crate) trait CompactionObserver: Send {
    async fn on_compacted(
        &mut self,
        _outcome: &CompactOutcome,
        _discarded_messages: &[share::message::Message],
    ) -> Result<(), LoopEngineError> {
        Ok(())
    }
}

pub(crate) struct NoopCompactionObserver;

#[async_trait]
impl CompactionObserver for NoopCompactionObserver {}

pub(crate) struct CompactionCoordinator {
    context: ContextCoordinator,
    usage: RunUsageTracker,
}

impl CompactionCoordinator {
    pub(crate) fn from_context(runtime_context: &RuntimeContext) -> Self {
        Self {
            context: ContextCoordinator::new(runtime_context.context()),
            usage: runtime_context.usage(),
        }
    }

    pub(crate) async fn needs_compaction(
        &self,
        execution: &mut RunExecutionState,
    ) -> Result<bool, LoopEngineError> {
        let request = execution
            .context_request()
            .ok_or_else(|| LoopEngineError::Adapter("ContextRequest 尚未冻结".to_string()))?;
        let window = self
            .context
            .build_window(request)
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        let needed = self
            .context
            .needs_compaction(request)
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        *execution.context_window_mut() = Some(window);
        Ok(needed)
    }

    pub(crate) async fn compact<O>(
        &self,
        execution: &mut RunExecutionState,
        observer: &mut O,
    ) -> Result<(), LoopEngineError>
    where
        O: CompactionObserver,
    {
        let (source_revision, discarded_messages) = execution
            .context_window()
            .map(|window| {
                (
                    window.backing_revision,
                    context::compact::messages_selected_for_precompact_memory(
                        &window.messages.to_vec(),
                    ),
                )
            })
            .ok_or_else(|| LoopEngineError::Adapter("ContextWindow 尚未构建".to_string()))?;
        let request = execution
            .context_request()
            .cloned()
            .ok_or_else(|| LoopEngineError::Adapter("ContextRequest 尚未冻结".to_string()))?;
        let outcome = self
            .context
            .compact(&request, source_revision)
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        apply_automatic_compact_outcome(&outcome, &self.usage, execution.context_window_mut());
        observer.on_compacted(&outcome, &discarded_messages).await
    }
}
