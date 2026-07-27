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

use crate::application::interaction::{
    validate_reply, InteractionCompletion, InteractionPort, InteractionPortError,
};
use crate::domain::agent_run::{InteractionContinuation, Run, RunTransitionError};

// ── Coordinator result ──

/// Outcome of a coordinated interaction round-trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinationError {
    /// The interaction port is unavailable.
    Unavailable,
    /// The scoped interaction adapter rejected a different Run identity.
    RunIdMismatch,
    /// The interaction was already registered (duplicate ID).
    AlreadyRegistered,
    /// The interaction waiter was dropped before resolution.
    WaiterDropped,
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
    /// Create a new coordinator.
    pub fn new() -> Self {
        Self
    }

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

    /// Wait for the receiver to resolve (step 4).
    ///
    /// Returns either `Replied(reply)` or `Cancelled(reason)`.
    pub async fn wait(
        receiver: tokio::sync::oneshot::Receiver<InteractionCompletion>,
    ) -> Result<InteractionCompletion, CoordinationError> {
        receiver.await.map_err(|_| CoordinationError::WaiterDropped)
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
        port: &dyn InteractionPort,
        run_id: &RunId,
        reason: InteractionCancelReason,
    ) -> Result<(), CoordinationError> {
        // Drain the port for this run — cancels all pending waiters.
        port.drain_run(run_id, reason);

        // Transition the Run to Cancelling.  This clears pending_interaction
        // and sets the state to Cancelling.
        match run.request_cancellation() {
            crate::domain::agent_run::RunCancellationRequest::Accepted => Ok(()),
            crate::domain::agent_run::RunCancellationRequest::AlreadyCancelling => Ok(()),
            crate::domain::agent_run::RunCancellationRequest::AlreadyTerminal => Ok(()),
        }
    }
}

// ── Tests ──

#[cfg(test)]
#[path = "interaction_coordinator_tests.rs"]
mod tests;
