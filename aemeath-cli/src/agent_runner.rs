use aemeath_core::agent::Agent;
use aemeath_core::compact::safe_slice;
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
        max_turns_override: Option<u32>,
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
            plan_mode: ctx.plan_mode,
            allow_all: ctx.allow_all,
        };
        let agent = Agent {
            registry: &sub_registry,
            ctx: sub_ctx,
        };

        // Helper to emit progress — writes to log file for diagnostics
        let log_path = dirs::home_dir()
            .unwrap_or_default()
            .join(".aemeath")
            .join("agent.log");
        let progress = move |msg: &str| {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let _ = writeln!(f, "[{}] {}", now, msg);
            }
        };

        // Sub-agents use a conservative context size for compaction decisions
        let ctx_context_size: usize = 128_000;
        let max_turns = max_turns_override.unwrap_or(50) as usize;
        let start_time = std::time::Instant::now();
        let max_duration = std::time::Duration::from_secs(600); // 10 minute hard limit
        for turn in 0..max_turns {
            if ctx.cancel.is_cancelled() {
                progress("Agent cancelled by user");
                return "Cancelled by user".to_string();
            }
            if start_time.elapsed() > max_duration {
                progress(&format!("Agent timed out after {}s", start_time.elapsed().as_secs()));
                // Return whatever text we have so far
                for msg in messages.iter().rev() {
                    let text = msg.text_content();
                    if !text.is_empty() {
                        return format!("{}\n\n[Sub-agent timed out after {}s]", text, start_time.elapsed().as_secs());
                    }
                }
                return format!("Sub-agent timed out after {}s", start_time.elapsed().as_secs());
            }
            let msg_tokens = aemeath_core::compact::estimate_messages_tokens(&messages);
            progress(&format!(
                "Agent turn {}/{}, messages: {}, est_tokens: {}",
                turn + 1, max_turns, messages.len(), msg_tokens
            ));

            let response = self
                .client
                .stream_message(&system_blocks, &messages, &sub_schemas, &mut handler, &ctx.cancel)
                .await;

            match response {
                Ok(resp) => {
                    let api_input = resp.usage.input_tokens as u64;
                    progress(&format!(
                        "API ok: in={} out={} stop={:?}",
                        resp.usage.input_tokens, resp.usage.output_tokens, resp.stop_reason
                    ));
                    messages.push(resp.assistant_message.clone());

                    let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                    if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                        progress("Agent completed");
                        return resp.assistant_message.text_content();
                    }

                    // Report which tools the sub-agent is calling with inputs
                    for call in &tool_calls {
                        let input_summary = call.input.to_string();
                        let input_short = if input_summary.len() > 200 {
                            format!("{}...", safe_slice(&input_summary, 200))
                        } else {
                            input_summary
                        };
                        progress(&format!("  → {}({})", call.name, input_short));
                    }

                    let mut results = agent.execute_tools(&tool_calls).await;
                    progress(&format!(
                        "Tools done ({}s elapsed), {} results",
                        start_time.elapsed().as_secs(), results.len()
                    ));

                    // Log each tool result summary
                    for (id, output, is_error, _) in results.iter() {
                        let label = if *is_error { "ERR" } else { "OK" };
                        let out_short = if output.len() > 300 {
                            format!("{}...[{} chars]", safe_slice(output, 300), output.len())
                        } else {
                            output.clone()
                        };
                        let tool_name = tool_calls.iter()
                            .find(|c| c.id == *id)
                            .map(|c| c.name.as_str())
                            .unwrap_or("?");
                        progress(&format!("  ← {}[{}]: {}", tool_name, label, out_short));
                    }

                    // Truncate oversized tool results to keep sub-agent context lean
                    aemeath_core::compact::truncate_tool_results(&mut results);

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

                    // Auto-compact using actual API token count
                    // Sub-agents compact more aggressively than the main loop
                    let ctx_pct = api_input * 100 / ctx_context_size as u64;
                    let urgency = if ctx_pct >= 50 { 2 } else if ctx_pct >= 35 { 1 } else { 0 };
                    if urgency >= 2 {
                        // Full local compaction — aggressively trim old messages
                        let old_len = messages.len();
                        let (compacted, was_compacted) = aemeath_core::compact::compact_messages(
                            &messages, system, ctx_context_size,
                        );
                        if was_compacted {
                            messages = compacted;
                            progress(&format!("Agent compacted: {} → {} messages", old_len, messages.len()));
                        }
                    } else if urgency >= 1 {
                        aemeath_core::compact::microcompact(&mut messages, 4);
                        progress("Agent microcompacted");
                    }
                }
                Err(e) => {
                    progress(&format!("Agent error: {e}"));
                    return format!("Sub-agent error: {e}");
                }
            }
        }

        progress(&format!("Agent reached max turns ({}), returning partial result", max_turns));
        // Return the last assistant text if available
        for msg in messages.iter().rev() {
            let text = msg.text_content();
            if !text.is_empty() {
                return format!("{}\n\n[Sub-agent reached max turns ({})]", text, max_turns);
            }
        }
        format!("Sub-agent reached max turns ({})", max_turns)
    }

    async fn complete(
        &self,
        prompt: &str,
        system: &str,
        ctx: &ToolContext,
    ) -> String {
        let system_blocks = vec![SystemBlock::cached(system.to_string())];
        let messages = vec![Message::user(prompt)];
        let mut handler = SilentHandler;

        match self
            .client
            .stream_message(&system_blocks, &messages, &[], &mut handler, &ctx.cancel)
            .await
        {
            Ok(resp) => resp.assistant_message.text_content(),
            Err(e) => format!("LLM error: {e}"),
        }
    }
}
