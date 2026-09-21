//! Run 领域事件的窄输出 observer。
//!
//! 共享 Engine 只发出 [`RuntimeLifecycleEvent`]。外层根据真实输出目标选择 observer：
//! chat stream observer 负责 TUI/SDK 事件与独立 Run 收尾展示；progress observer
//! 负责派生 Run 的进度与 terminal capture。名称表达输出职责，不表达 Main/Sub 角色。

use async_trait::async_trait;

use crate::application::loop_engine::chat::finalize::MainRunFinalizationObserver;
use crate::application::loop_engine::chat::{
    ChatEventSink as _, RuntimeRunContext, RuntimeStreamEvent,
};
use crate::application::loop_engine::run_finalization::{
    RunFinalizationCoordinator, RunFinalizationStatus,
};
use crate::application::loop_engine::LoopEngineError;
use crate::domain::agent_run::RuntimeLifecycleEvent;
use tools::AgentRunTerminal;

/// Extract terminal state from a domain event. Shared between Main and Sub.
///
/// Returns `Some(AgentRunTerminal)` for terminal events (Completed, Failed,
/// Terminated) and `None` for all other events.
pub(crate) fn terminal_from_domain_event(
    event: &RuntimeLifecycleEvent,
) -> Option<AgentRunTerminal> {
    match event {
        RuntimeLifecycleEvent::Completed { result, .. } => Some(AgentRunTerminal::Completed {
            result: result.clone(),
        }),
        RuntimeLifecycleEvent::Failed { error, .. } => Some(AgentRunTerminal::Failed {
            error: error.clone(),
        }),
        RuntimeLifecycleEvent::Terminated { .. } => Some(AgentRunTerminal::Cancelled),
        RuntimeLifecycleEvent::Transitioned { .. }
        | RuntimeLifecycleEvent::Started { .. }
        | RuntimeLifecycleEvent::StepStarted { .. }
        | RuntimeLifecycleEvent::StepCompleted { .. }
        | RuntimeLifecycleEvent::StepCancellationRequested { .. }
        | RuntimeLifecycleEvent::StepFinalizationStarted { .. }
        | RuntimeLifecycleEvent::StepCancelled { .. }
        | RuntimeLifecycleEvent::DrainingInput { .. }
        | RuntimeLifecycleEvent::TerminationRequested { .. }
        | RuntimeLifecycleEvent::AwaitingUser { .. }
        | RuntimeLifecycleEvent::Resumed { .. }
        | RuntimeLifecycleEvent::StuckDetected { .. } => None,
    }
}

/// Common interface for domain event observers.
#[async_trait]
pub(crate) trait RunEventObserver {
    async fn emit(&mut self, events: Vec<RuntimeLifecycleEvent>) -> Result<(), LoopEngineError>;
}

/// Publishes Run domain events to the chat stream and finalizes terminal UI state.
pub(crate) struct ChatStreamEventObserver<'a> {
    pub sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
    pub session_id: &'a str,
    pub turn_context: &'a RuntimeRunContext,
    pub task_access: &'a std::sync::Arc<dyn task::TaskAccess>,
    pub model: &'a str,
    pub started_at: std::time::Instant,
    pub step_count: usize,
    /// Snapshot of messages at emit time (cloned by the caller).
    pub messages_snapshot: Vec<share::message::Message>,
}

impl ChatStreamEventObserver<'_> {
    async fn project_done(&self, status: RunFinalizationStatus) {
        RunFinalizationCoordinator::new(
            None,
            self.model,
            MainRunFinalizationObserver {
                sink: self.sink.clone(),
                context: self.turn_context,
                access: &**self.task_access,
                session_id: self.session_id,
            },
        )
        .finalize_terminal(
            status,
            self.step_count,
            self.started_at.elapsed(),
            &AgentRunTerminal::Completed {
                result: String::new(),
            },
        )
        .await;
    }

    async fn send_cancelled(&self) {
        self.sink
            .send_event(RuntimeStreamEvent::Cancelled {
                context: self.turn_context.clone(),
                duration: self.started_at.elapsed(),
            })
            .await;
    }
}

#[async_trait]
impl RunEventObserver for ChatStreamEventObserver<'_> {
    async fn emit(&mut self, events: Vec<RuntimeLifecycleEvent>) -> Result<(), LoopEngineError> {
        for event in events {
            match event {
                RuntimeLifecycleEvent::Completed {
                    user_cancelled_step,
                    ..
                } => {
                    if user_cancelled_step {
                        self.send_cancelled().await;
                    } else {
                        self.project_done(RunFinalizationStatus::Completed).await;
                    }
                }
                RuntimeLifecycleEvent::Failed { error, .. } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::ApiError {
                            messages: self.messages_snapshot.clone(),
                            error: error.clone(),
                        })
                        .await;
                    self.project_done(RunFinalizationStatus::ApiError(error))
                        .await;
                }
                RuntimeLifecycleEvent::Terminated { .. } => {
                    self.send_cancelled().await;
                }
                RuntimeLifecycleEvent::Started {
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
                RuntimeLifecycleEvent::StuckDetected { reason, .. } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::SystemMessage(format!(
                            "[StuckGuard: {reason}]"
                        )))
                        .await;
                }
                RuntimeLifecycleEvent::Transitioned { .. }
                | RuntimeLifecycleEvent::AwaitingUser { .. }
                | RuntimeLifecycleEvent::Resumed { .. }
                | RuntimeLifecycleEvent::StepStarted { .. }
                | RuntimeLifecycleEvent::StepCompleted { .. }
                | RuntimeLifecycleEvent::StepCancellationRequested { .. }
                | RuntimeLifecycleEvent::StepFinalizationStarted { .. }
                | RuntimeLifecycleEvent::StepCancelled { .. }
                | RuntimeLifecycleEvent::DrainingInput { .. }
                | RuntimeLifecycleEvent::TerminationRequested { .. } => {
                    self.sink.send_lifecycle_event(event).await;
                }
            }
        }
        Ok(())
    }
}

/// Reports derived-run progress and captures terminal state for the caller.
pub(crate) struct ProgressTerminalObserver<'a> {
    pub progress: &'a (dyn Fn(Option<usize>, &str) + Send + Sync),
    pub terminal: &'a mut Option<AgentRunTerminal>,
    pub step_count: usize,
}

#[async_trait]
impl RunEventObserver for ProgressTerminalObserver<'_> {
    async fn emit(&mut self, events: Vec<RuntimeLifecycleEvent>) -> Result<(), LoopEngineError> {
        for event in events {
            if let Some(terminal) = terminal_from_domain_event(&event) {
                match &terminal {
                    AgentRunTerminal::Completed { .. } => {
                        (self.progress)(Some(self.step_count), "Agent completed");
                    }
                    AgentRunTerminal::Failed { error } => {
                        (self.progress)(Some(self.step_count), &format!("Agent error: {error}"));
                    }
                    AgentRunTerminal::Cancelled => {
                        (self.progress)(Some(self.step_count), "Agent cancelled by user");
                    }
                }
                *self.terminal = Some(terminal);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "event_strategy_tests.rs"]
mod tests;
