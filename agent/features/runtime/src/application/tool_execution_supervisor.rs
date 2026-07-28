use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use context::domain::{
    CleanupConfirmation as ReceiptCleanupConfirmation, ToolCallIdentity, ToolReceiptMutation,
    ToolTerminalReceipt,
};
use tokio_util::sync::CancellationToken;
use tools::{
    CancellationDeclaration, CancellationSignal, CleanupConfirmation, ToolCatalogSnapshot,
    ToolExecutionOutcome as PublishedToolOutcome, ToolExecutionPort, ToolInvocation,
};

use crate::application::context_coordination::ContextCoordinator;

struct SupervisorCancellation(CancellationToken);

#[async_trait]
impl CancellationSignal for SupervisorCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }

    async fn cancelled(&self) {
        self.0.cancelled().await;
    }

    fn child_signal(&self) -> Arc<dyn CancellationSignal> {
        Arc::new(Self(self.0.child_token()))
    }
}

const DEFAULT_GRACE: Duration = Duration::from_millis(250);

#[derive(Clone)]
pub(crate) struct ToolExecutionSupervisor {
    execution: Arc<dyn ToolExecutionPort>,
    catalog: ToolCatalogSnapshot,
    context: ContextCoordinator,
    grace: Duration,
}

pub(crate) struct SupervisedToolCall {
    pub identity: ToolCallIdentity,
    pub invocation: ToolInvocation,
    pub input_preview: String,
    pub run_deadline: Option<SystemTime>,
    pub cancellation: CancellationToken,
}

impl ToolExecutionSupervisor {
    pub(crate) fn new(
        execution: Arc<dyn ToolExecutionPort>,
        catalog: ToolCatalogSnapshot,
        context: ContextCoordinator,
    ) -> Self {
        Self {
            execution,
            catalog,
            context,
            grace: DEFAULT_GRACE,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_grace(mut self, grace: Duration) -> Self {
        self.grace = grace;
        self
    }

    pub(crate) async fn execute(
        &self,
        call: SupervisedToolCall,
    ) -> Result<PublishedToolOutcome, ToolExecutionSupervisorError> {
        let descriptor = self
            .catalog
            .find(&call.invocation.tool_name)
            .ok_or_else(|| {
                ToolExecutionSupervisorError::ToolUnavailable(call.invocation.tool_name.to_string())
            })?;
        self.context
            .advance_tool_receipt(ToolReceiptMutation::pending(
                call.identity.clone(),
                call.input_preview,
            ))
            .await?;
        log::debug!(
            target: crate::LOG_TARGET,
            "tool dispatch accepted: run_id={} step_id={} call_id={} tool={} index={}",
            call.identity.run_id,
            call.identity.step_id,
            call.identity.runtime_call_id,
            call.identity.tool_name,
            call.identity.call_index,
        );
        self.context
            .advance_tool_receipt(ToolReceiptMutation::running(call.identity.clone()))
            .await?;
        log::debug!(
            target: crate::LOG_TARGET,
            "tool execution started: run_id={} step_id={} call_id={} tool={} timeout_secs={} cancellation={:?}",
            call.identity.run_id,
            call.identity.step_id,
            call.identity.runtime_call_id,
            call.identity.tool_name,
            descriptor.timeout_secs,
            descriptor.cancellation,
        );
        let started = std::time::Instant::now();

        let effective_deadline = earliest_deadline(
            call.invocation.execution_scope.deadline(),
            call.run_deadline,
            Some(SystemTime::now() + Duration::from_secs(descriptor.timeout_secs)),
        );
        let child = call.cancellation.child_token();
        let signal: Arc<dyn CancellationSignal> = Arc::new(SupervisorCancellation(child.clone()));
        let future = self.execution.execute(call.invocation, signal.as_ref());
        tokio::pin!(future);

        let outcome = match effective_deadline {
            Some(deadline) => {
                let wait = deadline
                    .duration_since(SystemTime::now())
                    .unwrap_or_default();
                tokio::select! {
                    result = &mut future => result,
                    _ = call.cancellation.cancelled() => {
                        child.cancel();
                        cancellation_outcome(descriptor.cancellation, &mut future, self.grace, false).await
                    }
                    _ = tokio::time::sleep(wait) => {
                        child.cancel();
                        cancellation_outcome(descriptor.cancellation, &mut future, self.grace, true).await
                    }
                }
            }
            None => tokio::select! {
                result = &mut future => result,
                _ = call.cancellation.cancelled() => {
                    child.cancel();
                    cancellation_outcome(descriptor.cancellation, &mut future, self.grace, false).await
                }
            },
        };

        let terminal = terminal_receipt(&outcome);
        let unconfirmed = matches!(outcome, PublishedToolOutcome::CancellationUnconfirmed(_));
        if unconfirmed {
            log::warn!(
                target: crate::LOG_TARGET,
                "tool cleanup unconfirmed: run_id={} step_id={} call_id={} tool={} elapsed_ms={} outcome={:?}",
                call.identity.run_id,
                call.identity.step_id,
                call.identity.runtime_call_id,
                call.identity.tool_name,
                started.elapsed().as_millis(),
                terminal.outcome,
            );
        } else {
            log::debug!(
                target: crate::LOG_TARGET,
                "tool execution terminal: run_id={} step_id={} call_id={} tool={} elapsed_ms={} outcome={:?}",
                call.identity.run_id,
                call.identity.step_id,
                call.identity.runtime_call_id,
                call.identity.tool_name,
                started.elapsed().as_millis(),
                terminal.outcome,
            );
        }
        self.context
            .advance_tool_receipt(ToolReceiptMutation::terminal(call.identity, terminal))
            .await?;
        Ok(outcome)
    }
}

async fn cancellation_outcome<F>(
    declaration: CancellationDeclaration,
    future: &mut std::pin::Pin<&mut F>,
    grace: Duration,
    timed_out: bool,
) -> PublishedToolOutcome
where
    F: std::future::Future<Output = PublishedToolOutcome>,
{
    if declaration == CancellationDeclaration::Cooperative
        && tokio::time::timeout(grace, future).await.is_ok()
    {
        return if timed_out {
            PublishedToolOutcome::timed_out(
                "tool reached effective deadline",
                CleanupConfirmation::Confirmed,
            )
        } else {
            PublishedToolOutcome::cancelled("tool cancelled by caller")
        };
    }
    PublishedToolOutcome::cancellation_unconfirmed(
        if timed_out {
            "tool reached effective deadline; cleanup unconfirmed"
        } else {
            "tool cancellation requested; cleanup unconfirmed"
        },
        vec!["tool may still have observable side effects".to_string()],
        Vec::new(),
    )
}

fn terminal_receipt(outcome: &PublishedToolOutcome) -> ToolTerminalReceipt {
    match outcome {
        PublishedToolOutcome::TimedOut(details) => ToolTerminalReceipt::new(
            context::domain::ToolOutcomeKind::TimedOut,
            details.safe_reason.clone(),
            receipt_cleanup(details.cleanup),
        ),
        PublishedToolOutcome::CancellationUnconfirmed(details) => {
            details.possible_side_effects.iter().fold(
                ToolTerminalReceipt::new(
                    context::domain::ToolOutcomeKind::CancellationUnconfirmed,
                    details.safe_reason.clone(),
                    receipt_cleanup(details.cleanup),
                ),
                |receipt, effect| receipt.with_possible_side_effect(effect.clone()),
            )
        }
        PublishedToolOutcome::Cancelled(cancelled) => ToolTerminalReceipt::new(
            context::domain::ToolOutcomeKind::Cancelled,
            cancelled.reason.clone(),
            ReceiptCleanupConfirmation::Confirmed,
        ),
        PublishedToolOutcome::Success(_) => ToolTerminalReceipt::new(
            context::domain::ToolOutcomeKind::Success,
            "tool completed",
            ReceiptCleanupConfirmation::NotApplicable,
        ),
        PublishedToolOutcome::Failure(failure) => ToolTerminalReceipt::new(
            context::domain::ToolOutcomeKind::Failure,
            failure.safe_message.clone(),
            ReceiptCleanupConfirmation::NotApplicable,
        ),
        PublishedToolOutcome::Suspended(_) => ToolTerminalReceipt::new(
            context::domain::ToolOutcomeKind::Suspended,
            "tool suspended",
            ReceiptCleanupConfirmation::NotApplicable,
        ),
    }
}

fn receipt_cleanup(cleanup: CleanupConfirmation) -> ReceiptCleanupConfirmation {
    match cleanup {
        CleanupConfirmation::Confirmed => ReceiptCleanupConfirmation::Confirmed,
        CleanupConfirmation::Unconfirmed => ReceiptCleanupConfirmation::Unconfirmed,
        CleanupConfirmation::NotApplicable => ReceiptCleanupConfirmation::NotApplicable,
    }
}

pub(crate) fn earliest_deadline(
    scope: Option<SystemTime>,
    run: Option<SystemTime>,
    descriptor: Option<SystemTime>,
) -> Option<SystemTime> {
    [scope, run, descriptor].into_iter().flatten().min()
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ToolExecutionSupervisorError {
    #[error("Tool 不在当前 Catalog：{0}")]
    ToolUnavailable(String),
    #[error(transparent)]
    Receipt(#[from] context::domain::ToolReceiptMutationError),
}
