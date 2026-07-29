use context::domain::ToolCallIdentity;
use share::message::{ContentBlock, Message};
use std::sync::Arc;
use tools::{
    ToolCatalogSnapshot, ToolExecutionContext, ToolExecutionOutcome, ToolExecutionPort,
    ToolInvocation, ToolOutcome,
};

use crate::application::context::coordination::ContextCoordinator;
use crate::application::tool::execution_supervisor::{SupervisedToolCall, ToolExecutionSupervisor};

#[derive(Debug, Clone)]
pub struct ToolExecution {
    pub call_id: sdk::ids::ToolCallId,
    pub provider_id: String,
    pub tool_name: String,
    pub outcome: ToolOutcome,
}

impl ToolExecution {
    pub fn new(call: &ToolCall, outcome: ToolOutcome) -> Self {
        Self {
            call_id: call.id.clone(),
            provider_id: call.provider_id.clone(),
            tool_name: call.name.clone(),
            outcome,
        }
    }

    pub fn from_parts(
        call_id: sdk::ids::ToolCallId,
        provider_id: String,
        tool_name: String,
        outcome: ToolOutcome,
    ) -> Self {
        Self {
            call_id,
            provider_id,
            tool_name,
            outcome,
        }
    }
}

pub struct Agent {
    pub catalog: ToolCatalogSnapshot,
    pub execution: Arc<dyn ToolExecutionPort>,
    pub ctx: ToolExecutionContext,
    pub max_tool_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub workspace_persist: Arc<dyn project::WorkspacePersist>,
    pub(crate) context: ContextCoordinator,
    pub(crate) session_id: context::domain::SessionId,
    pub runtime_cancellation: tokio_util::sync::CancellationToken,
}

pub use crate::domain::agent_run::ToolCall;

#[cfg(test)]
fn tool_call_cancelled_message(name: &str) -> String {
    format!("tool.call execution cancelled: tool={name}")
}

#[cfg(test)]
async fn call_tool_with_timeout(
    tool: Arc<dyn tools::Tool>,
    name: &str,
    mut input: serde_json::Value,
    ctx: &ToolExecutionContext,
) -> Result<tools::ToolResult, String> {
    if ctx.cancellation().is_cancelled() {
        return Err(tool_call_cancelled_message(name));
    }
    tools::strip_runtime_meta(&mut input);
    if let Err(mismatch) = tools::validate_tool_input(name, &tool.input_schema(), &input) {
        let message = tools::format_tool_input_error(&mismatch);
        return Ok(tools::ToolResult {
            text: message.clone(),
            data: serde_json::json!({ "status": "error", "message": message }),
            is_error: true,
            error_kind: Some(tools::ToolErrorKind::InvalidInput),
            images: Vec::new(),
        });
    }
    let timeout = tool.timeout_secs();
    let cancellation = ctx.cancellation();
    tokio::select! {
        _ = cancellation.cancelled() => Err(tool_call_cancelled_message(name)),
        result = tokio::time::timeout(std::time::Duration::from_secs(timeout), tool.call(input, ctx)) => {
            result.map_err(|_| format!("tool.call execution timed out: tool={name}, timeout_secs={timeout}"))
        }
    }
}

impl Agent {
    #[cfg(test)]
    pub(crate) fn for_test(
        factory: &tools::composition::TestCatalogExecutionFactory,
        ctx: ToolExecutionContext,
        max_tool_concurrency: usize,
    ) -> Self {
        let ports = factory.build(ctx.clone());
        Self {
            catalog: ports.catalog(),
            execution: ports.execution(),
            workspace_persist: crate::application::run::workspace_test_support::workspace_persist(
                &ctx,
            ),
            context: ContextCoordinator::new(context::adapters::isolated_context("test-session")),
            session_id: context::domain::SessionId::new("test-session"),
            ctx,
            max_tool_concurrency,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            runtime_cancellation: tokio_util::sync::CancellationToken::new(),
        }
    }

    pub fn extract_tool_calls_with_ids<F>(message: &Message, mut id_for: F) -> Vec<ToolCall>
    where
        F: FnMut(&str) -> sdk::ids::ToolCallId,
    {
        message
            .content
            .iter()
            .enumerate()
            .filter_map(|(index, block)| match block {
                ContentBlock::ToolUse { id, name, input } => Some(ToolCall {
                    id: id_for(id),
                    provider_id: id.clone(),
                    name: name.clone(),
                    index,
                    input: input.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    pub fn extract_tool_calls(message: &Message) -> Vec<ToolCall> {
        Self::extract_tool_calls_with_ids(message, sdk::ids::ToolCallId::from_legacy_or_new)
    }

    #[cfg(test)]
    fn is_concurrent(&self, call: &ToolCall) -> bool {
        self.catalog
            .find(&tools::ToolName::new(&call.name))
            .is_some_and(|d| d.is_concurrency_safe())
    }

    #[cfg(test)]
    async fn execute_call(
        &self,
        call: &ToolCall,
        ctx: &ToolExecutionContext,
        authorization: tools::AuthorizationContext,
        step_id: &sdk::RunStepId,
    ) -> ToolExecution {
        let authorized_ctx = ctx.with_authorization(authorization);
        self.execute_one_with_ctx(call, &authorized_ctx, step_id)
            .await
    }

    #[cfg(test)]
    async fn execute_tools(&self, calls: &[ToolCall]) -> Vec<ToolExecution> {
        let prepared = calls
            .iter()
            .cloned()
            .map(
                |call| crate::application::tool::coordination::PreparedToolCall {
                    call,
                    authorization: tools::AuthorizationContext::STANDARD,
                },
            )
            .collect::<Vec<_>>();
        self.execute_prepared_tools(&prepared, &sdk::RunStepId::new_v7())
            .await
    }

    #[cfg(test)]
    async fn execute_prepared_tools(
        &self,
        calls: &[crate::application::tool::coordination::PreparedToolCall],
        step_id: &sdk::RunStepId,
    ) -> Vec<ToolExecution> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.max_tool_concurrency));
        let sequential = Arc::new(tokio::sync::Mutex::new(()));
        let futures = calls.iter().enumerate().map(|(position, prepared)| {
            let call = &prepared.call;
            let authorization = prepared.authorization;
            let semaphore = semaphore.clone();
            let sequential = sequential.clone();
            async move {
                if self.is_concurrent(call) {
                    let _permit = semaphore.acquire().await.expect("tool semaphore closed");
                    (
                        position,
                        self.execute_call(call, &self.ctx, authorization, step_id)
                            .await,
                    )
                } else {
                    let _serial = sequential.lock().await;
                    (
                        position,
                        self.execute_call(call, &self.ctx, authorization, step_id)
                            .await,
                    )
                }
            }
        });
        let mut results = futures::future::join_all(futures).await;
        results.sort_by_key(|(position, _)| *position);
        results.into_iter().map(|(_, result)| result).collect()
    }

    pub async fn execute_one_with_ctx(
        &self,
        call: &ToolCall,
        ctx: &ToolExecutionContext,
        step_id: &sdk::RunStepId,
    ) -> ToolExecution {
        ToolExecution::new(
            call,
            legacy_outcome(self.execute_one_outcome_with_ctx(call, ctx, step_id).await),
        )
    }

    pub(crate) async fn execute_one_outcome_with_ctx(
        &self,
        call: &ToolCall,
        ctx: &ToolExecutionContext,
        step_id: &sdk::RunStepId,
    ) -> ToolExecutionOutcome {
        let authorization = ctx.authorization();
        let mut input = call.input.clone();
        tools::strip_runtime_meta(&mut input);
        let invocation = ToolInvocation::new(call.name.as_str(), input, ctx.scope().clone())
            .with_authorization(authorization);
        let supervisor = ToolExecutionSupervisor::new(
            Arc::clone(&self.execution),
            self.catalog.clone(),
            self.context.clone(),
        );
        supervisor
            .execute(SupervisedToolCall {
                identity: ToolCallIdentity {
                    session_id: self.session_id.clone(),
                    run_id: sdk::RunId::from_legacy_or_new(ctx.scope().run_id()),
                    step_id: step_id.clone(),
                    runtime_call_id: call.id.to_string(),
                    provider_call_id: Some(call.provider_id.clone()),
                    tool_name: call.name.clone(),
                    call_index: call.index,
                    agent: call.name == "Agent",
                },
                invocation,
                context: ctx.clone(),
                input_preview: safe_input_preview(&call.input),
                run_deadline: ctx.scope().deadline(),
                cancellation: ctx.cancellation(),
            })
            .await
            .unwrap_or_else(|error| {
                ToolExecutionOutcome::failure(tools::ToolErrorKind::Internal, error.to_string())
            })
    }
}

fn safe_input_preview(input: &serde_json::Value) -> String {
    let rendered = serde_json::to_string(input).unwrap_or_else(|_| "<unserializable>".to_string());
    rendered.chars().take(500).collect()
}

pub(crate) fn legacy_outcome(outcome: ToolExecutionOutcome) -> ToolOutcome {
    match outcome {
        ToolExecutionOutcome::Success(success) => ToolOutcome::new(
            success
                .content
                .into_iter()
                .map(|block| block.text)
                .collect::<Vec<_>>()
                .join("\n"),
            success.data.unwrap_or(serde_json::Value::Null),
            Vec::new(),
        ),
        ToolExecutionOutcome::Failure(failure) => ToolOutcome {
            text: failure.safe_message,
            data: failure.data.unwrap_or(serde_json::Value::Null),
            is_error: true,
            images: Vec::new(),
        },
        ToolExecutionOutcome::Cancelled(cancelled) => ToolOutcome::error(cancelled.reason),
        ToolExecutionOutcome::TimedOut(details)
        | ToolExecutionOutcome::CancellationUnconfirmed(details) => {
            ToolOutcome::error(details.safe_reason)
        }
        ToolExecutionOutcome::Suspended(_) => {
            ToolOutcome::error("tool execution suspended at an unsupported ordinary-execution seam")
        }
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
