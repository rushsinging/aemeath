use async_trait::async_trait;

use crate::application::context::coordination::ContextCoordinator;
use crate::application::loop_engine::{LoopEngineError, StepCommit};
use crate::application::run::context::RuntimeContext;
use crate::application::run::execution_state::RunExecutionState;

#[async_trait]
pub(crate) trait AcceptedInputObserver: Send {
    async fn on_accepted_input(
        &mut self,
        _execution: &mut RunExecutionState,
    ) -> Result<(), LoopEngineError> {
        Ok(())
    }
}

pub(crate) struct NoopAcceptedInputObserver;

#[async_trait]
impl AcceptedInputObserver for NoopAcceptedInputObserver {}

pub(crate) struct StepPersistenceCoordinator {
    context: ContextCoordinator,
    usage: crate::application::run::context::RunUsageTracker,
}

impl StepPersistenceCoordinator {
    pub(crate) fn from_context(runtime_context: &RuntimeContext) -> Self {
        Self {
            context: ContextCoordinator::new(runtime_context.context().clone()),
            usage: runtime_context.usage(),
        }
    }

    pub(crate) async fn accept_step_input<O>(
        &self,
        execution: &mut RunExecutionState,
        step_id: &sdk::RunStepId,
        observer: &mut O,
    ) -> Result<(), LoopEngineError>
    where
        O: AcceptedInputObserver,
    {
        let accepted = execution.accepted_input_snapshot();
        if accepted.is_empty() {
            return Ok(());
        }
        let request = execution
            .context_request()
            .ok_or_else(|| LoopEngineError::Adapter("ContextRequestData 尚未冻结".to_string()))?;
        debug_assert_eq!(&request.step_id, step_id);
        self.context
            .append_accepted_input(request, accepted)
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        observer.on_accepted_input(execution).await
    }

    pub(crate) async fn load_step_receipts(
        &self,
        request: &crate::ports::ContextRequestData,
    ) -> Result<Vec<crate::ports::RunStepReceiptData>, LoopEngineError> {
        self.context
            .step_receipts(request)
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))
    }

    pub(crate) async fn persist_step_commit(
        &self,
        commit: &StepCommit,
    ) -> Result<(), LoopEngineError> {
        let (Some(request), Some(expected_revision)) =
            (commit.request.as_ref(), commit.expected_revision)
        else {
            return Ok(());
        };
        let receipts = if commit.receipts.is_empty() {
            self.context
                .step_receipts(request)
                .await
                .map_err(|error| LoopEngineError::Adapter(error.to_string()))?
        } else {
            commit.receipts.clone()
        };
        // CAS 基准取会话当前修订号：窗口快照里的 revision 早于本 step 自身的
        // accepted-input 与工具回执推进，直接使用会把正常收口误判成并发冲突
        //（取消收口被拒后 Run 会卡死在 FinalizingStep）。
        let expected_revision = resolve_commit_revision(
            expected_revision,
            self.context.current_revision(&request.session_id).await,
            &request.session_id,
            &commit.step_id,
        );
        self.context
            .append_finalized(
                request,
                commit.step_id.clone(),
                expected_revision,
                commit.cause,
                commit.duration_ms,
                commit.messages.clone(),
                receipts,
                self.usage.get(),
            )
            .await
            .map(|_| ())
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))
    }
}

/// 收口提交的 CAS 基准：优先用会话当前修订号（本 step 自身的 accepted-input
/// 与工具回执推进已计入），读不到时回退到窗口快照里的 revision。
fn resolve_commit_revision(
    snapshot: crate::ports::SessionRevision,
    current: Result<Option<crate::ports::SessionRevision>, crate::ports::ContextPortError>,
    session_id: &crate::ports::SessionId,
    step_id: &sdk::RunStepId,
) -> crate::ports::SessionRevision {
    match current {
        Ok(Some(current)) => current,
        Ok(None) => snapshot,
        Err(error) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "step commit revision refresh failed: session={session_id} step={step_id} error={error}",
            );
            snapshot
        }
    }
}

#[cfg(test)]
#[path = "step_persistence_tests.rs"]
mod tests;
