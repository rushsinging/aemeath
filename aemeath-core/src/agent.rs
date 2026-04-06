use crate::message::{ContentBlock, Message};
use crate::tool::{ImageData, ToolContext, ToolRegistry};

const MAX_CONCURRENCY: usize = 10;

/// (tool_use_id, output_text, is_error, images)
pub type ToolResultTuple = (String, String, bool, Vec<ImageData>);

pub struct Agent<'a> {
    pub registry: &'a ToolRegistry,
    pub ctx: ToolContext,
}

pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

impl<'a> Agent<'a> {
    pub fn extract_tool_calls(message: &Message) -> Vec<ToolCall> {
        message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { id, name, input } => Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    pub async fn execute_tools(
        &self,
        tool_calls: &[ToolCall],
    ) -> Vec<ToolResultTuple> {
        let mut concurrent_calls: Vec<&ToolCall> = Vec::new();
        let mut sequential_calls: Vec<&ToolCall> = Vec::new();

        for call in tool_calls {
            match self.registry.get(&call.name) {
                Some(tool) if tool.is_concurrency_safe() => concurrent_calls.push(call),
                _ => sequential_calls.push(call),
            }
        }

        let mut results: Vec<ToolResultTuple> = Vec::with_capacity(tool_calls.len());

        // Execute concurrent-safe tools in parallel using join_all
        if !concurrent_calls.is_empty() {
            let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENCY));

            let futures: Vec<_> = concurrent_calls
                .iter()
                .filter_map(|call| {
                    self.registry.get(&call.name).map(|tool| {
                        let input = call.input.clone();
                        let ctx = self.ctx.clone();
                        let id = call.id.clone();
                        let name = call.name.clone();
                        let sem = semaphore.clone();

                        async move {
                            let _permit = sem.acquire().await.expect("semaphore closed");
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(120),
                                tool.call(input, &ctx),
                            ).await {
                                Ok(result) => (id, result.output, result.is_error, result.images),
                                Err(_) => (id, format!("Tool {} timed out after 120s", name), true, Vec::new()),
                            }
                        }
                    })
                })
                .collect();

            let concurrent_results = futures::future::join_all(futures).await;
            results.extend(concurrent_results);
        }

        // Execute non-concurrent tools sequentially
        for call in sequential_calls {
            if let Some(tool) = self.registry.get(&call.name) {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(120),
                    tool.call(call.input.clone(), &self.ctx),
                ).await {
                    Ok(result) => {
                        results.push((call.id.clone(), result.output, result.is_error, result.images));
                    }
                    Err(_) => {
                        results.push((call.id.clone(), format!("Tool {} timed out after 120s", call.name), true, Vec::new()));
                    }
                }
            } else {
                results.push((
                    call.id.clone(),
                    format!("unknown tool: {}", call.name),
                    true,
                    Vec::new(),
                ));
            }
        }

        results
    }

    /// Execute only the given tool calls (subset of all calls)
    pub async fn execute_tools_filtered(
        &self,
        tool_calls: &[&ToolCall],
    ) -> Vec<ToolResultTuple> {
        let owned: Vec<ToolCall> = tool_calls.iter().map(|c| ToolCall {
            id: c.id.clone(),
            name: c.name.clone(),
            input: c.input.clone(),
        }).collect();
        self.execute_tools(&owned).await
    }
}
