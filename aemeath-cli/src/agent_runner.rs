use aemeath_core::agent::{Agent, ToolCall};
use aemeath_core::compact::safe_slice;
use aemeath_core::config::{AgentRoleConfig, AgentsConfig, ModelsConfig};
use aemeath_core::hook::HookRunner;
use aemeath_core::logging::{self, LogFile};
use aemeath_core::message::Message;
use aemeath_core::task::TaskStore;
use aemeath_core::tool::{
    AgentProgressEvent, AgentProgressKind, AgentRunner, AgentToolCallProgress, ToolContext,
    ToolRegistry,
};
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

fn build_tool_calls_progress_event(sequence: usize, tool_calls: &[ToolCall]) -> AgentProgressEvent {
    AgentProgressEvent {
        sequence,
        kind: AgentProgressKind::ToolCalls {
            calls: tool_calls
                .iter()
                .map(|call| AgentToolCallProgress {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input: call.input.clone(),
                    summary: summarize_tool_input(&call.name, &call.input),
                })
                .collect(),
        },
    }
}

#[cfg(test)]
fn format_grouped_tool_summaries(tool_calls: &[ToolCall]) -> String {
    let mut grouped: Vec<(&str, Vec<String>)> = Vec::new();
    for call in tool_calls {
        if let Some((_, summaries)) = grouped.iter_mut().find(|(name, _)| *name == call.name) {
            summaries.push(summarize_tool_input(&call.name, &call.input));
        } else {
            grouped.push((
                call.name.as_str(),
                vec![summarize_tool_input(&call.name, &call.input)],
            ));
        }
    }

    grouped
        .into_iter()
        .map(|(name, summaries)| {
            let count = summaries.len();
            let visible = summaries
                .iter()
                .filter(|summary| !summary.is_empty())
                .take(3)
                .cloned()
                .collect::<Vec<_>>();
            let suffix = if visible.is_empty() {
                String::new()
            } else {
                let mut text = visible.join(", ");
                if count > 3 {
                    text.push_str(&format!(" +{} more", count - 3));
                }
                format!(": {text}")
            };
            if count > 1 {
                format!("{name} ×{count}{suffix}")
            } else {
                format!("{name}{suffix}")
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn summarize_tool_input(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Read" | "Write" | "Edit" | "LSP" => extract_display_path(input, &["file_path", "path"]),
        "Grep" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let path = extract_display_path(input, &["path"]);
            match (pattern.is_empty(), path.is_empty()) {
                (false, false) => {
                    format!("\"{}\" in {}", truncate_progress_part(pattern, 48), path)
                }
                (false, true) => format!("\"{}\"", truncate_progress_part(pattern, 48)),
                (true, false) => path,
                (true, true) => fallback_json_summary(input),
            }
        }
        "Glob" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|pattern| truncate_progress_part(pattern, 72))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "Bash" => input
            .get("command")
            .and_then(|v| v.as_str())
            .map(|command| truncate_progress_part(command, 32))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "WebFetch" => input
            .get("url")
            .and_then(|v| v.as_str())
            .map(|url| truncate_progress_part(url, 72))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "TaskUpdate" | "TaskGet" | "TaskOutput" | "TaskStop" => input
            .get("taskId")
            .and_then(|v| v.as_str())
            .map(|id| truncate_progress_part(id, 48))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "TaskCreate" => input
            .get("subject")
            .and_then(|v| v.as_str())
            .map(|subject| truncate_progress_part(subject, 72))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "Memory" => input
            .get("action")
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| fallback_json_summary(input)),
        "Skill" => input
            .get("skill")
            .and_then(|v| v.as_str())
            .map(|skill| truncate_progress_part(skill, 72))
            .unwrap_or_else(|| fallback_json_summary(input)),
        _ => fallback_json_summary(input),
    }
}

fn extract_display_path(input: &serde_json::Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| input.get(*key).and_then(|v| v.as_str()))
        .map(|path| {
            let trimmed = path.trim_start_matches("/repo/");
            let components = trimmed.split('/').collect::<Vec<_>>();
            let compact = if components.len() > 3 {
                components[components.len() - 3..].join("/")
            } else {
                trimmed.to_string()
            };
            truncate_progress_part(&compact, 72)
        })
        .unwrap_or_default()
}

fn fallback_json_summary(input: &serde_json::Value) -> String {
    truncate_progress_part(&input.to_string(), 72)
}

fn truncate_progress_part(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let mut truncated = text.chars().take(max_chars).collect::<String>();
    if let Some(idx) = truncated.rfind(" && ") {
        truncated.truncate(idx);
    }
    format!("{truncated}…")
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

    fn role_max_tokens_override(role: Option<&AgentRoleConfig>) -> Option<u32> {
        role.and_then(|r| r.max_tokens).filter(|tokens| *tokens > 0)
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
        progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
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

        let max_tokens_override = Self::role_max_tokens_override(role);
        let previous_max_tokens = client.max_tokens();
        let previous_reasoning = client.is_reasoning();
        if let Some(max_tokens) = max_tokens_override {
            client.set_max_tokens(max_tokens);
        }

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
            .flatten();
        let reasoning = role_reasoning.or(model_reasoning).unwrap_or(self.reasoning);
        client.set_reasoning(reasoning);
        log::info!(
            "[SubAgent] reasoning={} max_tokens={:?} (role={:?}, model={:?}, default={})",
            reasoning,
            max_tokens_override,
            role_reasoning,
            model_reasoning,
            self.reasoning
        );

        let restore_client_settings = || {
            if max_tokens_override.is_some() && previous_max_tokens > 0 {
                client.set_max_tokens(previous_max_tokens);
            }
            client.set_reasoning(previous_reasoning);
        };

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
                        let _ = tx.try_send(AgentProgressEvent {
                            sequence: 0,
                            kind: AgentProgressKind::Message {
                                text: format!("[hook] {sys_msg}"),
                            },
                        });
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
                                let _ = tx.try_send(AgentProgressEvent {
                                    sequence: turns,
                                    kind: AgentProgressKind::Message {
                                        text: format!("[hook] {sys_msg}"),
                                    },
                                });
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
                restore_client_settings();
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
                        restore_client_settings();
                        return result;
                    }
                }
                let result = format!(
                    "Sub-agent timed out after {}s",
                    start_time.elapsed().as_secs()
                );
                call_subagent_stop_hook(result.clone(), turn, true).await;
                restore_client_settings();
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
                            let _ = tx.try_send(AgentProgressEvent {
                                sequence: turn + 1,
                                kind: AgentProgressKind::Message { text: short },
                            });
                        }
                    }

                    let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                    if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                        progress(Some(turn_number), "Agent completed");
                        let result = resp.assistant_message.text_content();
                        call_subagent_stop_hook(result.clone(), turn + 1, false).await;
                        restore_client_settings();
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
                        let _ = tx.try_send(build_tool_calls_progress_event(turn + 1, &tool_calls));
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
                    restore_client_settings();
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
                restore_client_settings();
                return result;
            }
        }
        let result = format!("Sub-agent reached max turns ({})", max_turns);
        call_subagent_stop_hook(result.clone(), max_turns, false).await;
        restore_client_settings();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_max_tokens_override() {
        let role = AgentRoleConfig {
            max_tokens: Some(8192),
            ..Default::default()
        };
        assert_eq!(
            CliAgentRunner::role_max_tokens_override(Some(&role)),
            Some(8192)
        );

        let role = AgentRoleConfig {
            max_tokens: Some(0),
            ..Default::default()
        };
        assert_eq!(CliAgentRunner::role_max_tokens_override(Some(&role)), None);

        let role = AgentRoleConfig {
            max_tokens: None,
            ..Default::default()
        };
        assert_eq!(CliAgentRunner::role_max_tokens_override(Some(&role)), None);

        assert_eq!(CliAgentRunner::role_max_tokens_override(None), None);
    }

    #[test]
    fn test_build_tool_calls_progress_event_preserves_call_data_and_summaries() {
        let calls = vec![
            test_tool_call(
                "1",
                "Read",
                serde_json::json!({"file_path": "/repo/src/lib.rs"}),
            ),
            test_tool_call(
                "2",
                "Grep",
                serde_json::json!({"pattern": "AgentProgress", "path": "/repo/src"}),
            ),
        ];

        let event = build_tool_calls_progress_event(2, &calls);

        assert_eq!(event.sequence, 2);
        match event.kind {
            AgentProgressKind::ToolCalls { calls } => {
                assert_eq!(calls.len(), 2);
                assert_eq!(calls[0].id, "1");
                assert_eq!(calls[0].name, "Read");
                assert_eq!(
                    calls[0].input,
                    serde_json::json!({"file_path": "/repo/src/lib.rs"})
                );
                assert_eq!(calls[0].summary, "src/lib.rs");
                assert_eq!(calls[1].name, "Grep");
                assert_eq!(calls[1].summary, "\"AgentProgress\" in src");
            }
            AgentProgressKind::Message { .. } => panic!("expected ToolCalls event"),
        }
    }

    #[test]
    fn test_build_tool_calls_progress_event_truncates_long_read_groups_at_summary_level() {
        let calls = vec![test_tool_call(
            "1",
            "Bash",
            serde_json::json!({"command": "cargo check -p aemeath-cli && cargo test"}),
        )];

        let event = build_tool_calls_progress_event(1, &calls);

        match event.kind {
            AgentProgressKind::ToolCalls { calls } => {
                assert_eq!(calls[0].summary, "cargo check -p aemeath-cli…");
            }
            AgentProgressKind::Message { .. } => panic!("expected ToolCalls event"),
        }
    }

    #[test]
    fn test_format_grouped_tool_summaries_keeps_existing_display_format() {
        let calls = vec![
            test_tool_call("1", "Read", serde_json::json!({"file_path": "/repo/a.rs"})),
            test_tool_call("2", "Read", serde_json::json!({"file_path": "/repo/b.rs"})),
            test_tool_call("3", "Read", serde_json::json!({"file_path": "/repo/c.rs"})),
            test_tool_call("4", "Read", serde_json::json!({"file_path": "/repo/d.rs"})),
        ];

        let summary = format_grouped_tool_summaries(&calls);

        assert_eq!(summary, "Read ×4: a.rs, b.rs, c.rs +1 more");
    }

    fn test_tool_call(
        id: &str,
        name: &str,
        input: serde_json::Value,
    ) -> aemeath_core::agent::ToolCall {
        aemeath_core::agent::ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            input,
        }
    }
}
