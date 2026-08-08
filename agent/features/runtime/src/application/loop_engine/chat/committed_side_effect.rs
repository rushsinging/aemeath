use async_trait::async_trait;

use crate::application::activity::ActivityCoordinator;
use crate::application::loop_engine::chat::{ChatEventSink, RuntimeStreamEvent};
use crate::application::tool::agent::{ToolCall, ToolExecution};
use hook::{HookInvocation, HookPort, TaskInput};
use std::path::Path;
use std::sync::Arc;

#[async_trait]
pub(crate) trait CommittedSideEffectHandler: Send + Sync {
    fn accepts(&self, execution: &ToolExecution) -> bool;

    async fn observe(
        &self,
        call: &ToolCall,
        execution: &ToolExecution,
        step_id: &sdk::RunStepId,
        cancel: &tokio_util::sync::CancellationToken,
    );
}

#[derive(Clone, Default)]
pub(crate) struct CommittedSideEffectDispatcher {
    handlers: Vec<Arc<dyn CommittedSideEffectHandler>>,
}

impl CommittedSideEffectDispatcher {
    pub(crate) fn new(handlers: Vec<Arc<dyn CommittedSideEffectHandler>>) -> Self {
        Self { handlers }
    }

    pub(crate) async fn observe(
        &self,
        call: &ToolCall,
        execution: &ToolExecution,
        step_id: &sdk::RunStepId,
        cancel: &tokio_util::sync::CancellationToken,
    ) {
        for handler in &self.handlers {
            if handler.accepts(execution) {
                handler.observe(call, execution, step_id, cancel).await;
            }
        }
    }
}

pub(crate) struct TaskCommittedSideEffectHandler {
    access: Arc<dyn task::TaskAccess>,
    sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
    session_id: String,
    hooks: Arc<dyn HookPort>,
    activities: Arc<ActivityCoordinator>,
    workspace_root: std::path::PathBuf,
}

impl TaskCommittedSideEffectHandler {
    pub(crate) fn new(
        access: Arc<dyn task::TaskAccess>,
        sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
        session_id: String,
        hooks: Arc<dyn HookPort>,
        activities: Arc<ActivityCoordinator>,
        workspace_root: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            access,
            sink,
            session_id,
            hooks,
            activities,
            workspace_root: workspace_root.into(),
        }
    }
}

#[async_trait]
impl CommittedSideEffectHandler for TaskCommittedSideEffectHandler {
    fn accepts(&self, execution: &ToolExecution) -> bool {
        execution.outcome.task_change.is_some()
    }

    async fn observe(
        &self,
        call: &ToolCall,
        execution: &ToolExecution,
        step_id: &sdk::RunStepId,
        cancel: &tokio_util::sync::CancellationToken,
    ) {
        let Some(change) = execution.outcome.task_change.as_ref() else {
            return;
        };
        let state = super::task_snapshot::build_task_state_view(
            self.access.as_ref(),
            self.session_id.clone(),
        );
        if state.revision != change.revision().get() {
            log::warn!(
                target: crate::LOG_TARGET,
                "忽略 revision 不一致的 Task committed observation: committed={} observed={}",
                change.revision().get(),
                state.revision,
            );
            return;
        }
        self.sink
            .send_event(RuntimeStreamEvent::TaskStateChanged {
                state: Box::new(state),
            })
            .await;
        for fact in change.facts() {
            let invocation = match fact {
                tools::TaskChangeFact::Created { .. } => HookInvocation::TaskCreated(TaskInput {
                    tool_input: call.input.clone(),
                    tool_output: execution.outcome.text.clone(),
                }),
                tools::TaskChangeFact::Completed { .. } => {
                    HookInvocation::TaskCompleted(TaskInput {
                        tool_input: call.input.clone(),
                        tool_output: execution.outcome.text.clone(),
                    })
                }
            };
            let _ = super::hook_ui::dispatch_hook(
                &self.hooks,
                self.activities.as_ref(),
                step_id,
                invocation,
                Path::new(&self.workspace_root),
                cancel,
            )
            .await;
        }
    }
}

pub(crate) fn task_dispatcher(
    runtime_context: &crate::application::run::context::RuntimeContext,
    session_id: impl Into<String>,
    workspace_root: impl Into<std::path::PathBuf>,
) -> CommittedSideEffectDispatcher {
    CommittedSideEffectDispatcher::new(vec![Arc::new(TaskCommittedSideEffectHandler::new(
        runtime_context.task(),
        runtime_context.event_sink(),
        session_id.into(),
        runtime_context.hooks(),
        runtime_context.activities().clone(),
        workspace_root,
    ))])
}

#[cfg(test)]
#[path = "committed_side_effect_tests.rs"]
mod tests;
