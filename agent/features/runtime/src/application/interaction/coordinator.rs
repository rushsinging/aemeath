//! #1248 Task 4: Interaction coordinator — bridges [`InteractionPort`] and domain [`Run`].
//!
//! The coordinator owns the fixed ordering of an interaction lifecycle:
//!
//! 1. Call [`InteractionPort::register`] to get a oneshot receiver
//! 2. Call [`Run::begin_interaction`] to transition the domain state machine
//!    (with compensation: if begin fails, the port registration is cancelled)
//! 3. Wait for the receiver to resolve (or cancellation)
//! 4. Validate the reply body matches the request body via shared [`validate_reply`]
//! 5. Call [`Run::complete_interaction`] to transition back to the working state
//! 6. Return the [`InteractionContinuation`] or an error
//!
//! Cancel/disconnect handling: [`cancel_and_drain`] drains the port for the run,
//! removes the pending interaction from the domain Run, and transitions the Run
//! to `Cancelling` via [`Run::request_cancellation`].

use sdk::{
    InteractionCancelReason, InteractionReply, InteractionReplyError, InteractionRequest,
    InteractionRequestId, RunId,
};

use crate::application::interaction::port::{
    validate_reply, InteractionCompletion, InteractionPort, InteractionPortError,
    InteractionRequestMetadata,
};
use crate::application::loop_engine::{
    ApprovalRequiredCall, InteractionWorkOutcome, LoopEngineError,
};
pub(crate) use crate::application::run::execution_state::ActiveInteractionAlreadyRegistered;
use crate::application::run::execution_state::RunExecutionState;
use crate::application::tool::agent::ToolExecution;
use crate::domain::agent_run::ToolCallStatus;
use crate::domain::agent_run::{InteractionContinuation, Run, RunTransitionError};

// ── Coordinator result ──

/// Run-scoped dependencies required to complete one interaction.
///
/// Role adapters only construct this narrow context. The coordinator owns all
/// completion decisions, approved tool execution, and result materialization.
pub struct InteractionCompletionContext<'a> {
    tool_context: tools::ToolExecutionContext,
    tool_execution: &'a dyn tools::ToolExecutionPort,
    materializer: &'a crate::application::tool::result_materialization::ToolResultMaterializer,
    session_id: &'a str,
}

impl<'a> InteractionCompletionContext<'a> {
    pub fn new(
        tool_context: tools::ToolExecutionContext,
        tool_execution: &'a dyn tools::ToolExecutionPort,
        materializer: &'a crate::application::tool::result_materialization::ToolResultMaterializer,
        session_id: &'a str,
    ) -> Self {
        Self {
            tool_context,
            tool_execution,
            materializer,
            session_id,
        }
    }
}

pub trait InteractionCompletionContextProvider {
    fn interaction_completion_context(
        &self,
        step_cancel: tokio_util::sync::CancellationToken,
    ) -> InteractionCompletionContext<'_>;
}

// ── Coordinator result ──

/// Outcome of a coordinated interaction round-trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinationError {
    /// The interaction port is unavailable.
    Unavailable,
    /// The scoped interaction adapter rejected a different Run identity.
    RunIdMismatch,
    #[cfg(test)]
    /// The interaction waiter was dropped before resolution.
    WaiterDropped,
    /// The interaction was already registered (duplicate ID).
    AlreadyRegistered,
    /// Reply validation failed — body type mismatch or answer count mismatch.
    InvalidReply(InteractionReplyError),
    /// The domain Run rejected the interaction request.
    RunError(RunTransitionError),
}

impl From<InteractionPortError> for CoordinationError {
    fn from(e: InteractionPortError) -> Self {
        match e {
            InteractionPortError::Unavailable => CoordinationError::Unavailable,
            InteractionPortError::WrongRun => CoordinationError::RunIdMismatch,
            InteractionPortError::AlreadyRegistered => CoordinationError::AlreadyRegistered,
        }
    }
}

// ── Coordinator ──

/// Stateless coordinator that bridges an [`InteractionPort`] and a domain [`Run`].
///
/// #1248 Task 4: Single coordinator handles all four body types
/// (`UserQuestions`, `ToolApproval`, `PlanApproval`, `HardPause`)
/// via exhaustive matching.
#[derive(Debug, Default)]
pub struct InteractionCoordinator;

impl InteractionCoordinator {
    /// Begin a synchronous interaction round-trip (steps 1–2).
    ///
    /// **Atomicity guarantee**: registers on the port FIRST.  If registration
    /// fails, the Run is never touched.  If registration succeeds but
    /// [`Run::begin_interaction`] fails, the port registration is cancelled
    /// to prevent the Run from being left in `AwaitingUser` without a pending
    /// port registration.
    pub fn begin(
        run: &mut Run,
        port: &dyn InteractionPort,
        request_id: InteractionRequestId,
        run_id: RunId,
        body: sdk::InteractionRequestBody,
        continuation: InteractionContinuation,
    ) -> Result<
        (
            InteractionRequestId,
            tokio::sync::oneshot::Receiver<InteractionCompletion>,
        ),
        CoordinationError,
    > {
        // Step 1: Create request
        let request = InteractionRequest {
            id: request_id.clone(),
            run_id,
            tool_call_id: match &continuation {
                InteractionContinuation::CompleteToolCall(tool_call_id) => {
                    Some(tool_call_id.to_string())
                }
                _ => None,
            },
            body,
        };

        // Step 2: Register on the port FIRST (no domain state change yet).
        let receiver = port.register(request).map_err(CoordinationError::from)?;

        // Step 3: Begin interaction on the domain Run.
        // If this fails, compensate by cancelling the port registration.
        run.begin_interaction(request_id.clone(), continuation)
            .map_err(|e| {
                port.cancel(&request_id, InteractionCancelReason::RunCancelled);
                CoordinationError::RunError(e)
            })?;

        Ok((request_id, receiver))
    }

    #[cfg(test)]
    pub async fn wait(
        receiver: tokio::sync::oneshot::Receiver<InteractionCompletion>,
    ) -> Result<InteractionCompletion, CoordinationError> {
        receiver.await.map_err(|_| CoordinationError::WaiterDropped)
    }

    pub fn store_mailbox_receiver(
        execution: &mut RunExecutionState,
        metadata: InteractionRequestMetadata,
        receiver: tokio::sync::oneshot::Receiver<InteractionCompletion>,
    ) -> Result<(), ActiveInteractionAlreadyRegistered> {
        execution.store_interaction_receiver(metadata, receiver)
    }

    pub(crate) async fn complete_tool_interaction(
        context: &InteractionCompletionContext<'_>,
        execution: &mut RunExecutionState,
        metadata: &InteractionRequestMetadata,
        completion: &InteractionCompletion,
    ) -> Result<InteractionWorkOutcome, LoopEngineError> {
        let work = execution.take_pending_interaction_work();
        let current = work.as_ref().and_then(|work| work.current.clone());
        let remaining_queue = work.map(|work| work.queue).unwrap_or_default();
        let (call_id, tool_execution, status, continuation) =
            match (&metadata.continuation, completion) {
                (
                    InteractionContinuation::CompleteToolCall(id),
                    InteractionCompletion::Replied(InteractionReply::UserQuestions(answers)),
                ) => {
                    let (provider_id, tool_name) = suspended_identity(current.as_ref());
                    let text = answers
                        .iter()
                        .enumerate()
                        .map(|(index, answer)| format!("Q{}: {}", index + 1, answer.0))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let outcome = tools::ToolOutcome {
                        text,
                        data: serde_json::json!({"status": "ok", "answers": answers}),
                        is_error: false,
                        images: Vec::new(),
                    };
                    (
                        id.clone(),
                        Some(ToolExecution::from_parts(
                            id.clone(),
                            provider_id,
                            tool_name,
                            outcome,
                        )),
                        ToolCallStatus::Success,
                        true,
                    )
                }
                (
                    InteractionContinuation::CompleteToolCall(id),
                    InteractionCompletion::Cancelled(_),
                ) => {
                    let (provider_id, tool_name) = suspended_identity(current.as_ref());
                    (
                        id.clone(),
                        Some(ToolExecution::from_parts(
                            id.clone(),
                            provider_id,
                            tool_name,
                            tools::ToolOutcome::error("user cancelled interaction"),
                        )),
                        ToolCallStatus::Cancelled,
                        false,
                    )
                }
                (
                    InteractionContinuation::ContinueToolApproval(id),
                    InteractionCompletion::Replied(InteractionReply::ToolApproval(
                        sdk::ApprovalDecision::Approve,
                    )),
                ) => {
                    let approval = current.as_ref().and_then(|item| item.approval_call.clone());
                    let execution = execute_approved_call(context, id, approval).await;
                    let status = if execution.outcome.is_error {
                        ToolCallStatus::Error
                    } else {
                        ToolCallStatus::Success
                    };
                    (id.clone(), Some(execution), status, true)
                }
                (
                    InteractionContinuation::ContinueToolApproval(id),
                    InteractionCompletion::Replied(InteractionReply::ToolApproval(
                        sdk::ApprovalDecision::Deny { .. },
                    ))
                    | InteractionCompletion::Cancelled(_),
                ) => (id.clone(), None, ToolCallStatus::Cancelled, false),
                _ => {
                    return Err(LoopEngineError::Adapter(
                        "interaction completion 与 continuation 不匹配".to_string(),
                    ));
                }
            };
        if let Some(result) = tool_execution {
            let message = crate::application::loop_engine::shared::materialize_tool_results(
                context.materializer,
                vec![result],
                context.session_id,
            )
            .await;
            execution.append_message(message.clone());
            execution.record_step_message(message);
        }
        Ok(InteractionWorkOutcome::Completed {
            call_id,
            status,
            remaining_queue,
            schedule_tool_results: continuation,
        })
    }

    /// Complete a successfully replied interaction (steps 5–6).
    ///
    /// Validates the reply body matches the request body via the shared
    /// [`validate_reply`] function, then calls [`Run::complete_interaction`]
    /// to transition back to the working state.
    pub fn complete_reply(
        run: &mut Run,
        request_id: &InteractionRequestId,
        body: &sdk::InteractionRequestBody,
        reply: &InteractionReply,
    ) -> Result<InteractionContinuation, CoordinationError> {
        // Step 5: Validate reply matches request body (shared fn)
        validate_reply(body, reply).map_err(CoordinationError::InvalidReply)?;
        // Step 6: Complete interaction on domain Run
        run.complete_interaction(request_id)
            .map_err(CoordinationError::RunError)
    }

    /// Cancel a pending interaction (does NOT change run status).
    ///
    /// Removes the pending interaction from the domain [`Run`] and returns
    /// the continuation.  The caller is responsible for deciding whether
    /// to terminate the run.
    ///
    /// Prefer [`cancel_and_drain`] for complete disconnect/cancel handling
    /// that also drains the port and transitions the Run to a terminal state.
    pub fn cancel(
        run: &mut Run,
        request_id: &InteractionRequestId,
    ) -> Result<InteractionContinuation, CoordinationError> {
        run.cancel_interaction(request_id)
            .map_err(CoordinationError::RunError)
    }

    /// Complete cancel/disconnect: drain the port for this run, cancel the
    /// pending interaction on the domain Run, and transition the Run to
    /// `Cancelling` via [`Run::request_cancellation`].
    ///
    /// #1248 Task 4: This is the recommended path for handling parent
    /// disconnect, user cancellation, or any scenario where the coordinator
    /// must leave the Run in a legal terminal state without hanging.
    ///
    /// # Returns
    ///
    /// - `Ok(())` when the Run is successfully transitioned to `Cancelling`.
    /// - `Err(CoordinationError::RunError(AlreadyTerminal))` if the Run was
    ///   already in a terminal state (no-op — still safe).
    pub fn cancel_and_drain(
        run: &mut Run,
        execution: &mut RunExecutionState,
        port: &dyn InteractionPort,
        run_id: &RunId,
        reason: InteractionCancelReason,
    ) -> Result<(), CoordinationError> {
        port.drain_run(run_id, reason);
        execution.take_active_interaction();
        execution.take_pending_interaction_work();

        // Transition the Run to Cancelling.  This clears pending_interaction
        // and sets the state to Cancelling.
        match run.request_cancellation() {
            crate::domain::agent_run::RunCancellationRequest::Accepted => Ok(()),
            crate::domain::agent_run::RunCancellationRequest::AlreadyCancelling => Ok(()),
            crate::domain::agent_run::RunCancellationRequest::AlreadyTerminal => Ok(()),
        }
    }
}

fn suspended_identity(
    current: Option<&crate::application::loop_engine::PendingInteractionItem>,
) -> (String, String) {
    current
        .and_then(|item| item.suspended_call.as_ref())
        .map(|suspended| {
            (
                suspended.call.provider_id.clone(),
                suspended.call.name.clone(),
            )
        })
        .unwrap_or_else(|| (String::new(), "AskUserQuestion".to_string()))
}

async fn execute_approved_call(
    context: &InteractionCompletionContext<'_>,
    id: &sdk::ToolCallId,
    approval: Option<ApprovalRequiredCall>,
) -> ToolExecution {
    let Some(approval) = approval else {
        return ToolExecution::from_parts(
            id.clone(),
            String::new(),
            "ToolApproval".to_string(),
            tools::ToolOutcome::error("tool approval call data not found"),
        );
    };
    let call = approval.call;
    let mut input = call.input.clone();
    tools::strip_runtime_meta(&mut input);
    let invocation = tools::ToolInvocation::new(
        call.name.as_str(),
        input,
        context.tool_context.scope().clone(),
    )
    .with_authorization(approval.authorization);
    let tool_context = context
        .tool_context
        .with_authorization(approval.authorization);
    let domain = context
        .tool_execution
        .execute(invocation, &tool_context)
        .await;
    ToolExecution::from_parts(
        id.clone(),
        call.provider_id,
        call.name,
        crate::application::tool::agent::legacy_outcome(domain),
    )
}

// ── Tests ──

#[cfg(test)]
#[path = "coordinator_tests.rs"]
mod tests;
