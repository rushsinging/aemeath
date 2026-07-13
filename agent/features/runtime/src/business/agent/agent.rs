use crate::LOG_TARGET;
use share::message::{ContentBlock, Message};
use share::tool::{ToolOutcome, ToolResult};
use tools::api::{Tool, ToolExecutionContext, ToolRegistry};

/// 一次工具调用的完整结果。取代历史的 6 元组 `ToolResultTuple` / `UiToolResult`，
/// 让管线全程按字段名访问而非位置取值。
#[derive(Debug, Clone)]
pub struct ToolExecution {
    pub call_id: sdk::ids::ToolCallId,
    pub provider_id: String,
    pub tool_name: String,
    pub outcome: ToolOutcome,
}

impl ToolExecution {
    /// 从 `&ToolCall` + outcome 构造（调用方仍持有 `call` 时用）。
    pub fn new(call: &ToolCall, outcome: ToolOutcome) -> Self {
        Self {
            call_id: call.id.clone(),
            provider_id: call.provider_id.clone(),
            tool_name: call.name.clone(),
            outcome,
        }
    }

    /// 从已拆出的 owned 字段构造（并发闭包中 `call` 已被借用拆解时用）。
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

pub struct Agent<'a> {
    pub registry: &'a ToolRegistry,
    pub ctx: ToolExecutionContext,
}

#[derive(Clone)]
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
    tool: std::sync::Arc<dyn Tool>,
    name: &str,
    mut input: serde_json::Value,
    ctx: &ToolExecutionContext,
) -> Result<ToolResult, String> {
    if ctx.cancel.is_cancelled() {
        return Err(tool_call_cancelled_message(name));
    }

    // 预校验 input 是否符合 schema（issue #430）：一次性收集全部参数错误
    // （缺失/多余/类型/enum），返回结构化中文消息，加速模型纠正参数污染。
    // 失败时不占用 Err 通道（保留给 timeout/cancel 等运行时故障）。
    //
    // 先剥离运行时元字段（phase 等，issue #491）：这些由系统提示要求 LLM
    // 注入，供 reasoning graph 使用，不属于任何工具业务 schema，必须在
    // 校验与派发前移除，否则严格 schema 工具会被误判为"多余字段"。
    super::input_validation::strip_runtime_meta(&mut input);
    if let Err(mismatch) =
        super::input_validation::validate_tool_input(name, &tool.input_schema(), &input)
    {
        let message = super::input_validation::format_tool_input_error(&mismatch);
        log::warn!(
            target: LOG_TARGET,
            "tool input validation failed: tool={name}, message={message}"
        );
        return Ok(ToolResult {
            text: message.clone(),
            data: serde_json::json!({ "status": "error", "message": message }),
            is_error: true,
            images: Vec::new(),
        });
    }

    let timeout = tool.timeout_secs();
    let started = std::time::Instant::now();
    tokio::select! {
        _ = ctx.cancel.cancelled() => {
            let message = tool_call_cancelled_message(name);
            log::debug!(target: LOG_TARGET, "{message}");
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

    pub async fn execute_tools(&self, tool_calls: &[ToolCall]) -> Vec<ToolExecution> {
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
        let mut results: Vec<Option<ToolExecution>> = vec![None; total_len];
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
                            let outcome =
                                match call_tool_with_timeout(tool, &name, input, &ctx).await {
                                    Ok(result) => ToolOutcome::from_tool_result(result),
                                    Err(message) => ToolOutcome::error(message),
                                };
                            (
                                pos,
                                ToolExecution::from_parts(id, provider_id, name, outcome),
                            )
                        }
                    })
                })
                .collect();

            let concurrent_results = futures::future::join_all(futures).await;
            for (pos, execution) in concurrent_results {
                results[pos] = Some(execution);
            }
        }

        // Execute non-concurrent tools sequentially
        for (call, &pos) in sequential_calls.iter().zip(sequential_positions.iter()) {
            // Check for cancellation between sequential tool calls
            if self.ctx.cancel.is_cancelled() {
                results[pos] = Some(ToolExecution::new(
                    call,
                    ToolOutcome::error("Cancelled by user"),
                ));
                continue;
            }
            if let Some(tool) = self.registry.get(&call.name) {
                let outcome =
                    match call_tool_with_timeout(tool, &call.name, call.input.clone(), &self.ctx)
                        .await
                    {
                        Ok(result) => ToolOutcome::from_tool_result(result),
                        Err(message) => ToolOutcome::error(message),
                    };
                results[pos] = Some(ToolExecution::new(call, outcome));
            } else {
                results[pos] = Some(ToolExecution::new(
                    call,
                    ToolOutcome::error(format!("unknown tool: {}", call.name)),
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
    ) -> ToolExecution {
        if ctx.cancel.is_cancelled() {
            return ToolExecution::new(call, ToolOutcome::error("Cancelled by user"));
        }
        if let Some(tool) = self.registry.get(&call.name) {
            let outcome =
                match call_tool_with_timeout(tool, &call.name, call.input.clone(), ctx).await {
                    Ok(result) => ToolOutcome::from_tool_result(result),
                    Err(message) => ToolOutcome::error(message),
                };
            ToolExecution::new(call, outcome)
        } else {
            ToolExecution::new(
                call,
                ToolOutcome::error(format!("unknown tool: {}", call.name)),
            )
        }
    }

    /// Execute only the given tool calls (subset of all calls)
    pub async fn execute_tools_filtered(&self, tool_calls: &[&ToolCall]) -> Vec<ToolExecution> {
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
