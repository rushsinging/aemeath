use share::message::{ContentBlock, Message};
use std::sync::Arc;
use tools::{
    ToolCatalogSnapshot, ToolExecutionContext, ToolExecutionOutcome, ToolExecutionPort,
    ToolInvocation, ToolOutcome,
};

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
            workspace_persist: crate::application::testing::workspace_persist(&ctx),
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

    fn is_concurrent(&self, call: &ToolCall) -> bool {
        self.catalog
            .find(&tools::ToolName::new(&call.name))
            .is_some_and(|d| d.is_concurrency_safe())
    }

    async fn execute_call(
        &self,
        call: &ToolCall,
        ctx: &ToolExecutionContext,
        authorization: tools::AuthorizationContext,
    ) -> ToolExecution {
        if ctx.cancellation().is_cancelled() {
            return ToolExecution::new(
                call,
                ToolOutcome::error("tool execution cancelled by user"),
            );
        }
        let Some(descriptor) = self.catalog.find(&tools::ToolName::new(&call.name)) else {
            return ToolExecution::new(
                call,
                ToolOutcome::error(format!("unknown tool: {}", call.name)),
            );
        };
        let mut input = call.input.clone();
        tools::strip_runtime_meta(&mut input);
        let invocation = ToolInvocation::new(call.name.as_str(), input, ctx.scope().clone())
            .with_authorization(authorization);
        let cancellation = ctx.cancellation();
        let domain = tokio::select! {
            _ = cancellation.cancelled() => ToolExecutionOutcome::cancelled("tool execution cancelled by user"),
            result = tokio::time::timeout(
                std::time::Duration::from_secs(descriptor.timeout_secs),
                self.execution.execute(invocation, cancellation.as_ref()),
            ) => match result {
                Ok(outcome) => outcome,
                Err(_) => ToolExecutionOutcome::failure(
                    tools::ToolErrorKind::Internal,
                    format!("tool.call execution timed out: tool={}, timeout_secs={}", call.name, descriptor.timeout_secs),
                ),
            }
        };
        ToolExecution::new(call, legacy_outcome(domain))
    }

    pub async fn execute_tools(&self, calls: &[ToolCall]) -> Vec<ToolExecution> {
        let prepared = calls
            .iter()
            .cloned()
            .map(
                |call| crate::application::tool_coordination::PreparedToolCall {
                    call,
                    authorization: tools::AuthorizationContext::STANDARD,
                },
            )
            .collect::<Vec<_>>();
        self.execute_prepared_tools(&prepared).await
    }

    pub(crate) async fn execute_prepared_tools(
        &self,
        calls: &[crate::application::tool_coordination::PreparedToolCall],
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
                        self.execute_call(call, &self.ctx, authorization).await,
                    )
                } else {
                    let _serial = sequential.lock().await;
                    (
                        position,
                        self.execute_call(call, &self.ctx, authorization).await,
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
    ) -> ToolExecution {
        self.execute_call(call, ctx, ctx.authorization()).await
    }

    pub async fn execute_tools_filtered(&self, calls: &[&ToolCall]) -> Vec<ToolExecution> {
        let owned = calls.iter().map(|call| (*call).clone()).collect::<Vec<_>>();
        self.execute_tools(&owned).await
    }
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
        ToolExecutionOutcome::Suspended(_) => {
            ToolOutcome::error("tool execution suspended at an unsupported ordinary-execution seam")
        }
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
