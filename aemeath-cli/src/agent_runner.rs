use aemeath_core::agent::Agent;
use aemeath_core::compact::safe_slice;
use aemeath_core::config::{AgentRoleConfig, AgentsConfig, ModelsConfig};
use aemeath_core::hook::HookRunner;
use aemeath_core::logging::{self, LogFile};
use aemeath_core::message::Message;
use aemeath_core::task::TaskStore;
use aemeath_core::tool::{AgentRunner, ToolContext, ToolRegistry};
use aemeath_llm::client::LlmClient;
use aemeath_llm::pool::LlmClientPool;
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
    /// Default LLM client (used when no model_spec is provided).
    pub client: Arc<LlmClient>,
    /// Client pool for multi-LLM routing. `None` if only one model is configured.
    pub pool: Option<Arc<LlmClientPool>>,
    /// Agent config for role resolution.
    pub agents_config: Arc<AgentsConfig>,
    /// Hook runner for executing sub-agent hooks.
    pub hook_runner: HookRunner,
    /// Default reasoning setting for sub-agents (from config / CLI).
    pub reasoning: bool,
    /// Model entries config for reasoning lookup.
    pub models_config: Arc<ModelsConfig>,
}

impl CliAgentRunner {
    /// Resolve a model spec to a concrete `"provider/model_id"` string.
    ///
    /// The `model_spec` passed in is already resolved by AgentTool:
    ///   - If the user set `model="deepseek/deepseek-chat"`, that comes through directly.
    ///   - If the user set `role="coder"`, that comes through as the role name.
    ///   - If neither was set, it's `None`.
    ///
    /// Resolution order:
    /// 1. If `model_spec` matches a role name in `agents.roles` → use the role's `model` field.
    /// 2. If `model_spec` contains `/` → treat as `"provider/model_id"` directly.
    /// 3. If `model_spec` is `None` → use `agents.default_model` if set.
    fn resolve_model_spec(&self, model_spec: Option<&str>) -> Option<String> {
        match model_spec {
            Some(spec) => {
                // 1. Check if it's a role name
                if let Some(role) = self.agents_config.roles.get(spec) {
                    if !role.model.is_empty() {
                        return Some(role.model.clone());
                    }
                }
                // 2. Already a "provider/model" spec or bare model name
                Some(spec.to_string())
            }
            None => {
                // 3. Use default_model if configured
                if !self.agents_config.default_model.is_empty() {
                    return Some(self.agents_config.default_model.clone());
                }
                None
            }
        }
    }

    /// Get the resolved role config (if any) for a model spec.
    fn resolve_role(&self, model_spec: Option<&str>) -> Option<&AgentRoleConfig> {
        model_spec.and_then(|spec| self.agents_config.roles.get(spec))
    }
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
        model_spec: Option<&str>,
        progress_tx: Option<tokio::sync::mpsc::Sender<String>>,
    ) -> String {
        // Resolve role and model
        let role = self.resolve_role(model_spec);
        let resolved_spec = self.resolve_model_spec(model_spec);

        // Pick the right client
        let client = match (&self.pool, &resolved_spec) {
            (Some(pool), Some(spec)) => pool.get_client(Some(spec.as_str())).await,
            (Some(pool), None) => pool.get_client(None).await,
            _ => self.client.clone(),
        };

        // Determine reasoning for this sub-agent: role config > model config > default
        let role_reasoning = role.and_then(|r| r.reasoning);
        let model_reasoning = resolved_spec
            .as_deref()
            .and_then(|spec| {
                // Try find_model to get the ModelEntryConfig for reasoning lookup
                let query = if spec.contains('/') {
                    spec.to_string()
                } else {
                    format!("{}/{}", self.client.provider_name(), spec)
                };
                self.models_config.find_model(&query)
            })
            .map(|(_, _, entry)| entry.reasoning)
            .flatten();        let reasoning = role_reasoning.or(model_reasoning).unwrap_or(self.reasoning);
        client.set_reasoning(reasoning);
        log::info!(
            "[SubAgent] reasoning={} (role={:?}, model={:?}, default={})",
            reasoning,
            role_reasoning,
            model_reasoning,
            self.reasoning
        );

        // Extract hook_runner to avoid borrow conflicts with closure
        let hook_runner = self.hook_runner.clone();

        // Append role-specific system suffix if configured
        let system = match role.and_then(|r| r.system_suffix.as_ref()) {
            Some(suffix) => format!("{}\n\n{}", system, suffix),
            None => system.to_string(),
        };

        // Call SubagentStart hook
        let hook_results = hook_runner
            .on_subagent_start(prompt, &system, resolved_spec.as_deref())
            .await;
        // Send any system messages from hook results to progress_tx
        for (_, _, json_output) in &hook_results {
            if let Some(ref output) = json_output {
                if let Some(ref sys_msg) = output.system_message {
                    if let Some(ref tx) = progress_tx {
                        let _ = tx.try_send(format!("[hook] {}", sys_msg));
                    }
                }
            }
        }

        // Helper to emit progress — writes to agent.log for diagnostics.
        let session_id = ctx
            .parent_session_id
            .clone()
            .or_else(|| {
                ctx.cwd
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| "subagent".to_string());
        let session_id_for_log = session_id.clone();
        let role_name = model_spec.unwrap_or("default").to_string();
        let model_name = resolved_spec.as_deref().unwrap_or("default").to_string();
        let role_name_for_log = role_name.clone();
        let model_name_for_log = model_name.clone();
        let progress = move |turn: Option<usize>, msg: &str| {
            let _ = logging::append_agent_line(
                LogFile::Agent,
                &session_id,
                turn,
                &role_name,
                &model_name,
                "INFO",
                "agent",
                msg,
            );
        }; // Helper to call SubagentStop hook and send system messages
        let hook_runner_clone = hook_runner.clone();
        let progress_tx_clone = progress_tx.clone();
        let system_for_hook = system.clone();
        let resolved_spec_for_hook = resolved_spec.clone();
        let call_subagent_stop_hook = move |result: String, turns: usize, is_error: bool| {
            let hook_runner = hook_runner_clone.clone();
            let progress_tx = progress_tx_clone.clone();
            let prompt = prompt.to_string();
            let system = system_for_hook.clone();
            let resolved_spec = resolved_spec_for_hook.clone();
            async move {
                let hook_results = hook_runner
                    .on_subagent_stop(
                        &prompt,
                        &system,
                        resolved_spec.as_deref(),
                        &result,
                        turns,
                        is_error,
                    )
                    .await;
                // Send any system messages from hook results to progress_tx
                for (_, _, json_output) in &hook_results {
                    if let Some(ref output) = json_output {
                        if let Some(ref sys_msg) = output.system_message {
                            if let Some(ref tx) = progress_tx {
                                let _ = tx.try_send(format!("[hook] {}", sys_msg));
                            }
                        }
                    }
                }
            }
        };

        // Build a fresh sub-agent registry with all tools except Agent (prevent recursion)
        let sub_task_store = std::sync::Arc::new(TaskStore::new());
        let sub_skills =
            std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        let mut sub_registry = ToolRegistry::new();
        aemeath_tools::register_all_tools_except_agent(
            &mut sub_registry,
            sub_task_store,
            sub_skills,
        );

        let sub_schemas = sub_registry.schemas();
        let mut messages = vec![Message::user(prompt)];
        let mut handler = SilentHandler;

        // For sub-agents, use the system prompt as a single cached block
        let system_blocks = vec![SystemBlock::cached(system.clone())];
        let log_request_messages = |turn: usize, messages: &[Message]| {
            let payload = serde_json::json!({
                "event": "subagent_llm_request_messages",
                "provider": client.provider_name(),
                "model": client.model_name(),
                "role": role_name_for_log,
                "model_spec": model_name_for_log,
                "system_blocks": system_blocks,
                "messages": messages,
                "tool_schema_count": sub_schemas.len(),
            });
            let _ = logging::append_json_line_with_turn(
                LogFile::Agent,
                &session_id_for_log,
                Some(turn),
                "INFO",
                "llm_request",
                "sub-agent messages sent to LLM",
                payload,
            );
        };

        let sub_ctx = ToolContext {
            cwd: ctx.cwd.clone(),
            cancel: ctx.cancel.clone(),
            read_files: std::sync::Arc::new(
                std::sync::Mutex::new(std::collections::HashSet::new()),
            ),
            agent_runner: None, // No nested agents
            session_reminders: ctx.session_reminders.clone(),
            plan_mode: ctx.plan_mode,
            allow_all: ctx.allow_all,
            max_tool_concurrency: ctx.max_tool_concurrency,
            max_agent_concurrency: ctx.max_agent_concurrency,
            agent_semaphore: ctx.agent_semaphore.clone(),
            progress_tx: None, // sub-agents don't stream progress (yet)
            parent_session_id: ctx.parent_session_id.clone(),
        };
        let agent = Agent {
            registry: &sub_registry,
            ctx: sub_ctx,
        };

        let model_display = resolved_spec.as_deref().unwrap_or("default");
        progress(
            None,
            &format!("Sub-agent started with model: {}", model_display),
        );

        // Sub-agents use a conservative context size for compaction decisions
        let ctx_context_size: usize = 128_000;
        let max_turns = max_turns_override.unwrap_or(50) as usize;
        let start_time = std::time::Instant::now();
        let max_duration = std::time::Duration::from_secs(600); // 10 minute hard limit
        for turn in 0..max_turns {
            let turn_number = turn + 1;
            if ctx.cancel.is_cancelled() {
                progress(Some(turn_number), "Agent cancelled by user");
                let result = "Cancelled by user".to_string();
                call_subagent_stop_hook(result.clone(), turn, true).await;
                return result;
            }
            if start_time.elapsed() > max_duration {
                progress(
                    Some(turn_number),
                    &format!("Agent timed out after {}s", start_time.elapsed().as_secs()),
                );
                // Return whatever text we have so far
                for msg in messages.iter().rev() {
                    let text = msg.text_content();
                    if !text.is_empty() {
                        let result = format!(
                            "{}\n\n[Sub-agent timed out after {}s]",
                            text,
                            start_time.elapsed().as_secs()
                        );
                        call_subagent_stop_hook(result.clone(), turn, true).await;
                        return result;
                    }
                }
                let result = format!(
                    "Sub-agent timed out after {}s",
                    start_time.elapsed().as_secs()
                );
                call_subagent_stop_hook(result.clone(), turn, true).await;
                return result;
            }
            let msg_tokens = aemeath_core::compact::estimate_messages_tokens(&messages);
            progress(
                Some(turn_number),
                &format!(
                    "Agent turn {}/{}, messages: {}, est_tokens: {}",
                    turn + 1,
                    max_turns,
                    messages.len(),
                    msg_tokens
                ),
            );

            log_request_messages(turn_number, &messages);
            let response = client
                .stream_message(
                    &system_blocks,
                    &messages,
                    &sub_schemas,
                    &mut handler,
                    &ctx.cancel,
                )
                .await;

            match response {
                Ok(resp) => {
                    let api_input = resp.usage.input_tokens as u64;
                    progress(
                        Some(turn_number),
                        &format!(
                            "API ok: in={} out={} stop={:?}",
                            resp.usage.input_tokens, resp.usage.output_tokens, resp.stop_reason
                        ),
                    );
                    messages.push(resp.assistant_message.clone());

                    // Send text output to TUI progress channel (if available)
                    if let Some(ref tx) = progress_tx {
                        let text = resp.assistant_message.text_content();
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            let short = if trimmed.len() > 300 {
                                format!("{}...", safe_slice(trimmed, 300))
                            } else {
                                trimmed.to_string()
                            };
                            let _ = tx.try_send(format!("Turn {}: {}", turn + 1, short));
                        }
                    }

                    let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                    if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                        progress(Some(turn_number), "Agent completed");
                        let result = resp.assistant_message.text_content();
                        call_subagent_stop_hook(result.clone(), turn + 1, false).await;
                        return result;
                    }

                    // Build a lookup from tool_use_id to tool call info
                    let call_info: std::collections::HashMap<String, (String, String)> = tool_calls
                        .iter()
                        .map(|c| {
                            let input_summary = c.input.to_string();
                            let input_short = if input_summary.len() > 200 {
                                format!("{}...", safe_slice(&input_summary, 200))
                            } else {
                                input_summary
                            };
                            (c.id.clone(), (c.name.clone(), input_short))
                        })
                        .collect();

                    // Send tool call names before execution so the TUI shows progress
                    // while long-running sub-agent tools are still in flight.
                    if let Some(ref tx) = progress_tx {
                        let tool_names: Vec<&str> =
                            tool_calls.iter().map(|c| c.name.as_str()).collect();
                        let _ = tx.try_send(format!(
                            "[Turn {}] calling: {}",
                            turn + 1,
                            tool_names.join(", ")
                        ));
                    }

                    let mut results = agent.execute_tools(&tool_calls).await;
                    progress(
                        Some(turn_number),
                        &format!(
                            "Tools done ({}s elapsed), {} results",
                            start_time.elapsed().as_secs(),
                            results.len()
                        ),
                    );

                    // Log each call followed by its result (interleaved)
                    for (id, output, is_error, _) in results.iter() {
                        let label = if *is_error { "ERR" } else { "OK" };
                        if let Some((name, input_short)) = call_info.get(id.as_str()) {
                            progress(Some(turn_number), &format!("  → {}({})", name, input_short));
                        }
                        let out_short = if output.len() > 300 {
                            format!("{}...[{} chars]", safe_slice(output, 300), output.len())
                        } else {
                            output.clone()
                        };
                        let tool_name = call_info
                            .get(id.as_str())
                            .map(|(n, _)| n.as_str())
                            .unwrap_or("?");
                        progress(
                            Some(turn_number),
                            &format!("  ← {}[{}]: {}", tool_name, label, out_short),
                        );
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
                    let urgency = if ctx_pct >= 50 {
                        2
                    } else if ctx_pct >= 35 {
                        1
                    } else {
                        0
                    };
                    if urgency >= 2 {
                        // Full local compaction — aggressively trim old messages
                        let old_len = messages.len();
                        let (compacted, was_compacted) = aemeath_core::compact::compact_messages(
                            &messages,
                            &system,
                            ctx_context_size,
                        );
                        if was_compacted {
                            messages = compacted;
                            progress(
                                Some(turn_number),
                                &format!(
                                    "Agent compacted: {} → {} messages",
                                    old_len,
                                    messages.len()
                                ),
                            );
                        }
                    } else if urgency >= 1 {
                        aemeath_core::compact::microcompact(&mut messages, 4);
                        progress(Some(turn_number), "Agent microcompacted");
                    }
                }
                Err(e) => {
                    progress(Some(turn_number), &format!("Agent error: {e}"));
                    let result = format!("Sub-agent error: {e}");
                    call_subagent_stop_hook(result.clone(), turn, true).await;
                    return result;
                }
            }
        }

        progress(
            Some(max_turns),
            &format!(
                "Agent reached max turns ({}), returning partial result",
                max_turns
            ),
        );
        // Return the last assistant text if available
        for msg in messages.iter().rev() {
            let text = msg.text_content();
            if !text.is_empty() {
                let result = format!("{}\n\n[Sub-agent reached max turns ({})]", text, max_turns);
                call_subagent_stop_hook(result.clone(), max_turns, false).await;
                return result;
            }
        }
        let result = format!("Sub-agent reached max turns ({})", max_turns);
        call_subagent_stop_hook(result.clone(), max_turns, false).await;
        result
    }

    async fn complete(&self, prompt: &str, system: &str, ctx: &ToolContext) -> String {
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
