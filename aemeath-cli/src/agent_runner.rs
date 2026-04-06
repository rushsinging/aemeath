use aemeath_core::agent::Agent;
use aemeath_core::message::Message;
use aemeath_core::task::TaskStore;
use aemeath_core::tool::{AgentRunner, ToolContext, ToolRegistry};
use aemeath_llm::client::LlmClient;
use aemeath_llm::stream::StreamHandler;
use aemeath_llm::types::{StopReason, SystemBlock};
use async_trait::async_trait;
use std::sync::Arc;

/// A no-op stream handler for sub-agents (output goes to result, not terminal)
struct SilentHandler;

impl StreamHandler for SilentHandler {
    fn on_text(&mut self, _text: &str) {}
    fn on_tool_use_start(&mut self, _name: &str) {}
    fn on_error(&mut self, _error: &str) {}
}

pub struct CliAgentRunner {
    pub client: Arc<LlmClient>,
}

#[async_trait]
impl AgentRunner for CliAgentRunner {
    async fn run_agent(
        &self,
        prompt: &str,
        system: &str,
        _tool_schemas: &[serde_json::Value],
        _registry: &ToolRegistry,
        ctx: &ToolContext,
    ) -> String {
        // Build a fresh sub-agent registry with all tools except Agent (prevent recursion)
        let sub_task_store = std::sync::Arc::new(TaskStore::new());
        let sub_skills = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        let mut sub_registry = ToolRegistry::new();
        aemeath_tools::register_all_tools_except_agent(&mut sub_registry, sub_task_store, sub_skills);

        let sub_schemas = sub_registry.schemas();
        let mut messages = vec![Message::user(prompt)];
        let mut handler = SilentHandler;

        // For sub-agents, use the system prompt as a single cached block
        let system_blocks = vec![SystemBlock::cached(system.to_string())];

        let sub_ctx = ToolContext {
            cwd: ctx.cwd.clone(),
            cancel: ctx.cancel.clone(),
            read_files: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashSet::new(),
            )),
            agent_runner: None, // No nested agents
            plan_mode: ctx.plan_mode.clone(),
            allow_all: ctx.allow_all,
        };
        let agent = Agent {
            registry: &sub_registry,
            ctx: sub_ctx,
        };

        let max_turns = 20;
        for _ in 0..max_turns {
            let response = self
                .client
                .stream_message(&system_blocks, &messages, &sub_schemas, &mut handler, &ctx.cancel)
                .await;

            match response {
                Ok(resp) => {
                    messages.push(resp.assistant_message.clone());

                    let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                    if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                        // Return the assistant's final text
                        return resp.assistant_message.text_content();
                    }

                    let results = agent.execute_tools(&tool_calls).await;
                    let has_images = results.iter().any(|(_, _, _, imgs)| !imgs.is_empty());
                    if has_images {
                        messages.push(Message::tool_results_rich(results));
                    } else {
                        let simple: Vec<(String, String, bool)> = results
                            .into_iter()
                            .map(|(id, output, is_error, _)| (id, output, is_error))
                            .collect();
                        messages.push(Message::tool_results(simple));
                    }
                }
                Err(e) => {
                    return format!("Sub-agent error: {e}");
                }
            }
        }

        "Sub-agent reached max turns".to_string()
    }
}
