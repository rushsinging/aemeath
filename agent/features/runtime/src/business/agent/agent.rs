use crate::LOG_TARGET;
use share::message::{ContentBlock, Message};
use share::tool::{ImageData, ToolResult};
use tools::api::{Tool, ToolExecutionContext, ToolRegistry};

/// (runtime_id, provider_id, output_text, json_content, is_error, images)
pub type ToolResultTuple = (
    sdk::ids::ToolCallId,
    String,
    String,
    serde_json::Value,
    bool,
    Vec<ImageData>,
);

pub struct Agent<'a> {
    pub registry: &'a ToolRegistry,
    pub ctx: ToolExecutionContext,
}

pub struct ToolCall {
    pub id: sdk::ids::ToolCallId,
    pub provider_id: String,
    pub name: String,
    pub index: usize,
    pub input: serde_json::Value,
}

fn tool_call_timeout_message(name: &str, timeout: u64, elapsed: std::time::Duration) -> String {
    format!(
        "tool.call execution timed out: tool={name}, timeout_secs={timeout}, elapsed_ms={}",
        elapsed.as_millis()
    )
}

fn tool_call_cancelled_message(name: &str) -> String {
    format!("tool.call execution cancelled: tool={name}")
}

async fn call_tool_with_timeout(
    tool: std::sync::Arc<dyn Tool<Result = share::tool::ToolResult>>,
    name: &str,
    input: serde_json::Value,
    ctx: &ToolExecutionContext,
) -> Result<ToolResult, String> {
    if ctx.cancel.is_cancelled() {
        return Err(tool_call_cancelled_message(name));
    }

    let timeout = tool.timeout_secs();
    let started = std::time::Instant::now();
    tokio::select! {
        _ = ctx.cancel.cancelled() => {
            let message = tool_call_cancelled_message(name);
            log::info!(target: LOG_TARGET, "{message}");
            Err(message)
        }
        result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            tool.call(input, ctx),
        ) => {
            match result {
                Ok(result) => {
                    log::debug!(target: LOG_TARGET,
                        "tool.call execution finished: tool={}, timeout_secs={}, elapsed_ms={}",
                        name,
                        timeout,
                        started.elapsed().as_millis()
                    );
                    Ok(result)
                }
                Err(_) => {
                    let elapsed = started.elapsed();
                    let message = tool_call_timeout_message(name, timeout, elapsed);
                    log::warn!(target: LOG_TARGET, "{message}");
                    Err(message)
                }
            }
        }
    }
}

impl<'a> Agent<'a> {
    pub fn extract_tool_calls_with_ids<F>(
        message: &Message,
        mut runtime_id_for_provider: F,
    ) -> Vec<ToolCall>
    where
        F: FnMut(&str) -> sdk::ids::ToolCallId,
    {
        message
            .content
            .iter()
            .enumerate()
            .filter_map(|(index, block)| match block {
                ContentBlock::ToolUse { id, name, input } => Some(ToolCall {
                    id: runtime_id_for_provider(id),
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
        Self::extract_tool_calls_with_ids(message, |provider_id| {
            sdk::ids::ToolCallId::from_legacy_or_new(provider_id)
        })
    }

    pub async fn execute_tools(&self, tool_calls: &[ToolCall]) -> Vec<ToolResultTuple> {
        let mut concurrent_calls: Vec<&ToolCall> = Vec::new();
        let mut sequential_calls: Vec<&ToolCall> = Vec::new();

        for call in tool_calls {
            match self.registry.get(&call.name) {
                Some(tool) if tool.is_concurrency_safe() => concurrent_calls.push(call),
                _ => sequential_calls.push(call),
            }
        }

        // Pre-allocate result slots and track original positions
        // This ensures results are returned in original tool_calls order
        let total_len = tool_calls.len();
        let mut results: Vec<Option<ToolResultTuple>> = vec![None; total_len];
        let mut concurrent_positions: Vec<usize> = Vec::new();
        let mut sequential_positions: Vec<usize> = Vec::new();

        // Track positions for each call
        for (i, call) in tool_calls.iter().enumerate() {
            match self.registry.get(&call.name) {
                Some(tool) if tool.is_concurrency_safe() => {
                    concurrent_positions.push(i);
                }
                _ => {
                    sequential_positions.push(i);
                }
            }
        }

        // Execute concurrent-safe tools in parallel using join_all
        if !concurrent_calls.is_empty() {
            let semaphore =
                std::sync::Arc::new(tokio::sync::Semaphore::new(self.ctx.max_tool_concurrency));

            let futures: Vec<_> = concurrent_calls
                .iter()
                .zip(concurrent_positions.iter())
                .filter_map(|(call, &pos)| {
                    self.registry.get(&call.name).map(|tool| {
                        let input = call.input.clone();
                        let ctx = self.ctx.clone();
                        let id = call.id.clone();
                        let provider_id = call.provider_id.clone();
                        let name = call.name.clone();
                        let sem = semaphore.clone();

                        async move {
                            let _permit = sem.acquire().await.expect("semaphore closed");
                            match call_tool_with_timeout(tool, &name, input, &ctx).await {
                                Ok(result) => (
                                    pos,
                                    id,
                                    provider_id,
                                    result.output,
                                    result.content,
                                    result.is_error,
                                    result.images,
                                ),
                                Err(message) => (
                                    pos,
                                    id,
                                    provider_id,
                                    message.clone(),
                                    serde_json::json!({ "text": message }),
                                    true,
                                    Vec::new(),
                                ),
                            }
                        }
                    })
                })
                .collect();

            let concurrent_results = futures::future::join_all(futures).await;
            for (pos, id, provider_id, output, content, is_error, images) in concurrent_results {
                results[pos] = Some((id, provider_id, output, content, is_error, images));
            }
        }

        // Execute non-concurrent tools sequentially
        for (call, &pos) in sequential_calls.iter().zip(sequential_positions.iter()) {
            // Check for cancellation between sequential tool calls
            if self.ctx.cancel.is_cancelled() {
                results[pos] = Some((
                    call.id.clone(),
                    call.provider_id.clone(),
                    "Cancelled by user".to_string(),
                    serde_json::json!({ "text": "Cancelled by user" }),
                    true,
                    Vec::new(),
                ));
                continue;
            }
            if let Some(tool) = self.registry.get(&call.name) {
                match call_tool_with_timeout(tool, &call.name, call.input.clone(), &self.ctx).await
                {
                    Ok(result) => {
                        results[pos] = Some((
                            call.id.clone(),
                            call.provider_id.clone(),
                            result.output,
                            result.content,
                            result.is_error,
                            result.images,
                        ));
                    }
                    Err(message) => {
                        results[pos] = Some((
                            call.id.clone(),
                            call.provider_id.clone(),
                            message.clone(),
                            serde_json::json!({ "text": message }),
                            true,
                            Vec::new(),
                        ));
                    }
                }
            } else {
                results[pos] = Some((
                    call.id.clone(),
                    call.provider_id.clone(),
                    format!("unknown tool: {}", call.name),
                    serde_json::json!({ "text": format!("unknown tool: {}", call.name) }),
                    true,
                    Vec::new(),
                ));
            }
        }

        // All slots should be filled by either concurrent or sequential execution.
        // Use expect with a clear message instead of bare unwrap for debuggability.
        results
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                r.unwrap_or_else(|| {
                    panic!("agent::execute_tools: result slot {i} was not filled — this is a bug")
                })
            })
            .collect()
    }

    /// Execute a single tool call with a custom context (for streaming support).
    ///
    /// Mirrors the sequential path of `execute_tools` but allows the caller to
    /// inject a modified `ToolExecutionContext` (e.g., with `progress_tx` set
    /// for stdout streaming). Reuses `call_tool_with_timeout` for timeout/cancel.
    pub async fn execute_one_with_ctx(
        &self,
        call: &ToolCall,
        ctx: &ToolExecutionContext,
    ) -> ToolResultTuple {
        if ctx.cancel.is_cancelled() {
            return (
                call.id.clone(),
                call.provider_id.clone(),
                "Cancelled by user".to_string(),
                serde_json::json!({ "text": "Cancelled by user" }),
                true,
                Vec::new(),
            );
        }
        if let Some(tool) = self.registry.get(&call.name) {
            match call_tool_with_timeout(tool, &call.name, call.input.clone(), ctx).await {
                Ok(result) => (
                    call.id.clone(),
                    call.provider_id.clone(),
                    result.output,
                    result.content,
                    result.is_error,
                    result.images,
                ),
                Err(message) => (
                    call.id.clone(),
                    call.provider_id.clone(),
                    message.clone(),
                    serde_json::json!({ "text": message }),
                    true,
                    Vec::new(),
                ),
            }
        } else {
            (
                call.id.clone(),
                call.provider_id.clone(),
                format!("unknown tool: {}", call.name),
                serde_json::json!({ "text": format!("unknown tool: {}", call.name) }),
                true,
                Vec::new(),
            )
        }
    }

    /// Execute only the given tool calls (subset of all calls)
    pub async fn execute_tools_filtered(&self, tool_calls: &[&ToolCall]) -> Vec<ToolResultTuple> {
        let owned: Vec<ToolCall> = tool_calls
            .iter()
            .map(|c| ToolCall {
                id: c.id.clone(),
                provider_id: c.provider_id.clone(),
                name: c.name.clone(),
                index: c.index,
                input: c.input.clone(),
            })
            .collect();
        self.execute_tools(&owned).await
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
