//! Event-strategy trait and concrete implementations for Main and Sub adapters.
//!
//! The [`EventStrategy`] trait abstracts domain-event projection: the Main
//! adapter projects to [`RuntimeStreamEvent`] and handles finish/reflection,
//! while the Sub adapter extracts terminal state and reports progress.
//!
//! Each adapter holds a concrete strategy and delegates [`emit`] through it.
//! Because the two strategies have fundamentally different behaviour
//! (sink-based projection vs progress reporting), the trait exists for
//! interface consistency, not for dynamic dispatch.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;

use crate::application::loop_engine::LoopEngineError;
use crate::application::main_loop::looping::finalize::finish_completed_loop;
use crate::application::main_loop::looping::{
    ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext,
};
use crate::application::subagent::runner::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
use crate::domain::agent_run::RunDomainEvent;
use share::message::Message;
use tools::AgentRunTerminal;

/// Extract terminal state from a domain event. Shared between Main and Sub.
///
/// Returns `Some(AgentRunTerminal)` for terminal events (Completed, Failed,
/// Cancelled, Terminated) and `None` for all other events.
pub(crate) fn terminal_from_domain_event(event: &RunDomainEvent) -> Option<AgentRunTerminal> {
    match event {
        RunDomainEvent::Completed { result, .. } => Some(AgentRunTerminal::Completed {
            result: result.clone(),
        }),
        RunDomainEvent::Failed { error, .. } => Some(AgentRunTerminal::Failed {
            error: error.clone(),
        }),
        RunDomainEvent::Cancelled { .. } | RunDomainEvent::Terminated { .. } => {
            Some(AgentRunTerminal::Cancelled)
        }
        RunDomainEvent::Transitioned { .. }
        | RunDomainEvent::Started { .. }
        | RunDomainEvent::StepStarted { .. }
        | RunDomainEvent::StepCompleted { .. }
        | RunDomainEvent::StepCancellationRequested { .. }
        | RunDomainEvent::StepFinalizationStarted { .. }
        | RunDomainEvent::StepCancelled { .. }
        | RunDomainEvent::DrainingInput { .. }
        | RunDomainEvent::TerminationRequested { .. }
        | RunDomainEvent::CancellationRequested { .. }
        | RunDomainEvent::AwaitingUser { .. }
        | RunDomainEvent::Resumed { .. }
        | RunDomainEvent::StuckDetected { .. } => None,
    }
}

/// Common interface for event-projection strategies.
///
/// Each adapter constructs its concrete strategy and delegates its
/// [`RunLoopPort::emit`] implementation through it. Because the two
/// strategies have fundamentally different output channels
/// (sink-based vs progress-based), the trait exists for interface
/// consistency, not for dynamic dispatch.
#[async_trait]
pub(crate) trait EventStrategy {
    /// Project domain events into adapter-specific output.
    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError>;
}

// ── Main adapter strategy ──────────────────────────────────────────────

/// Event strategy for the **Main** adapter.
///
/// Projects domain events to [`RuntimeStreamEvent`] via the event sink,
/// handles completion finalization (log, DoneWithDuration, task archival),
/// and sends cancellation/error events with message snapshots.
pub(crate) struct MainEventStrategy<'a, S>
where
    S: ChatEventSink,
{
    pub sink: &'a S,
    pub session_id: &'a str,
    pub turn_context: &'a RuntimeTurnContext,
    pub task_access: &'a Arc<dyn task::TaskAccess>,
    pub model: &'a str,
    pub started_at: Instant,
    pub turn_count: usize,
    /// Snapshot of messages at emit time (cloned by the caller).
    pub messages_snapshot: Vec<Message>,
}

impl<'a, S> MainEventStrategy<'a, S>
where
    S: ChatEventSink,
{
    fn outcome(&self, status: AgentRunStatus) -> AgentRunOutcome {
        AgentRunOutcome {
            status,
            turns: self.turn_count,
            duration: self.started_at.elapsed(),
            role: None,
            model: self.model.to_string(),
        }
    }

    async fn project_done(&self, status: AgentRunStatus) {
        let outcome = self.outcome(status);
        log_agent_outcome(&outcome, self.session_id);
        finish_completed_loop(&outcome, self.sink, self.turn_context, &**self.task_access).await;
    }

    async fn send_cancelled(&self) {
        self.sink
            .send_event(RuntimeStreamEvent::Cancelled {
                context: self.turn_context.clone(),
            })
            .await;
    }
}

#[async_trait]
impl<S> EventStrategy for MainEventStrategy<'_, S>
where
    S: ChatEventSink + Send + Sync,
{
    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
        for event in events {
            match event {
                RunDomainEvent::Completed { .. } => {
                    self.project_done(AgentRunStatus::Completed).await;
                }
                RunDomainEvent::Failed { error, .. } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::ApiError {
                            messages: self.messages_snapshot.clone(),
                            error: error.clone(),
                        })
                        .await;
                    self.project_done(AgentRunStatus::ApiError(error)).await;
                }
                RunDomainEvent::Cancelled { run_id, .. } => {
                    self.send_cancelled().await;
                    self.sink
                        .send_event(RuntimeStreamEvent::RunCancelled { run_id })
                        .await;
                }
                RunDomainEvent::Terminated { run_id, .. } => {
                    self.send_cancelled().await;
                    self.sink
                        .send_event(RuntimeStreamEvent::RunCancelled { run_id })
                        .await;
                }
                RunDomainEvent::CancellationRequested { run_id, .. } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::RunCancelling { run_id })
                        .await;
                }
                RunDomainEvent::Started {
                    run_id,
                    parent_run_id,
                } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::RunStarted {
                            run_id,
                            parent_run_id,
                        })
                        .await;
                }
                RunDomainEvent::StuckDetected { reason, .. } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::SystemMessage(format!(
                            "[StuckGuard: {reason}]"
                        )))
                        .await;
                }
                RunDomainEvent::Transitioned { .. }
                | RunDomainEvent::AwaitingUser { .. }
                | RunDomainEvent::Resumed { .. }
                | RunDomainEvent::StepStarted { .. }
                | RunDomainEvent::StepCompleted { .. }
                | RunDomainEvent::StepCancellationRequested { .. }
                | RunDomainEvent::StepFinalizationStarted { .. }
                | RunDomainEvent::StepCancelled { .. }
                | RunDomainEvent::DrainingInput { .. }
                | RunDomainEvent::TerminationRequested { .. } => {
                    self.sink.send_domain_event(event).await;
                }
            }
        }
        Ok(())
    }
}

// ── Sub adapter strategy ───────────────────────────────────────────────

/// Event strategy for the **Sub** adapter.
///
/// Extracts terminal state from domain events via
/// [`terminal_from_domain_event`] and reports progress text. Stores the
/// terminal result for the caller to consume after the loop ends.
pub(crate) struct SubEventStrategy<'a> {
    pub progress: &'a (dyn Fn(Option<usize>, &str) + Send + Sync),
    pub terminal: &'a mut Option<AgentRunTerminal>,
    pub turn_count: usize,
}

#[async_trait]
impl EventStrategy for SubEventStrategy<'_> {
    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
        for event in events {
            if let Some(terminal) = terminal_from_domain_event(&event) {
                match &terminal {
                    AgentRunTerminal::Completed { .. } => {
                        (self.progress)(Some(self.turn_count), "Agent completed");
                    }
                    AgentRunTerminal::Failed { error } => {
                        (self.progress)(Some(self.turn_count), &format!("Agent error: {error}"));
                    }
                    AgentRunTerminal::Cancelled => {
                        (self.progress)(Some(self.turn_count), "Agent cancelled by user");
                    }
                }
                *self.terminal = Some(terminal);
            }
        }
        Ok(())
    }
}
