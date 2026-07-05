use super::loop_run::SubAgentRun;
use super::{CliAgentRunner, SilentHandler};
use crate::business::agent::Agent;
use crate::LOG_TARGET;
use async_trait::async_trait;
use provider::api::SystemBlock;
use share::message::Message;
use share::tool::{AgentProgressEvent, AgentProgressKind};
use storage::api::TaskStore;
use tools::api::{AgentRunRequest, AgentRunner, ToolExecutionContext, ToolRegistry};

#[async_trait]
impl AgentRunner for CliAgentRunner {
    async fn run_agent(&self, request: AgentRunRequest<'_>) -> String {
        let prompt = request.prompt;
        let system = request.system;
        let ctx = request.ctx;
        let max_turns_override = request.max_turns;
        let model_spec = request.model_spec;
        let progress_tx = request.progress_tx;
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
        let previous_reasoning_level = client.current_reasoning_level();
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
            .and_then(|(_, _, entry)| entry.reasoning);
        let reasoning = role_reasoning.or(model_reasoning).unwrap_or(self.reasoning);
        let level = if reasoning {
            provider::contract::ReasoningLevel::Medium
        } else {
            provider::contract::ReasoningLevel::Off
        };
        client.set_reasoning_level(level);
        log::info!(target: LOG_TARGET,
            "[SubAgent] reasoning={} level={} max_tokens={:?} (role={:?}, model={:?}, default={})",
            reasoning,
            level,
            max_tokens_override,
            role_reasoning,
            model_reasoning,
            self.reasoning
        );

        let restore_max_tokens = max_tokens_override.is_some();

        // Extract hook_runner to avoid borrow conflicts with closure
        let hook_runner = self.hook_runner.clone();

        // Append role-specific system suffix if configured
        let system = match role.and_then(|r| r.system_suffix.as_ref()) {
            Some(suffix) => format!("{}\n\n{}", system, suffix),
            None => system.to_string(),
        };

        // Call SubagentStart hook
        let workspace_root = ctx.workspace_read().current_workspace_root();
        let hook_results = hook_runner
            .on_subagent_start(prompt, &system, resolved_spec.as_deref(), &workspace_root)
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

        // Helper to emit progress — writes to aemeath.log via log::info! for diagnostics.
        let session_id = ctx
            .parent_session_id
            .clone()
            .or_else(|| {
                ctx.workspace_read()
                    .current_workspace_root()
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
        // 将 sub-agent 的 model 同步到日志 context（影响 hook/audit 等共享 sink 的 model 字段）
        logging::context::set_current_model(model_name.clone());
        let progress = move |turn: Option<usize>, msg: &str| {
            let turn_str = turn
                .map(|t| t.to_string())
                .unwrap_or_else(|| "-".to_string());
            log::debug!(
                target: LOG_TARGET,
                "[role:{} model:{} turn:{}] {}",
                role_name,
                model_name,
                turn_str,
                msg
            );
        };
        // Build a fresh sub-agent registry with all tools except Agent (prevent recursion)
        let sub_task_store = std::sync::Arc::new(TaskStore::new());
        let sub_skills =
            std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        let mut sub_registry = ToolRegistry::new();
        tools::api::register_subagent_tools(&mut sub_registry, sub_task_store, sub_skills);
        let sub_schemas = sub_registry.schemas_for(&ctx.resources.lang);
        let messages = vec![Message::user(prompt)];
        let handler = SilentHandler;
        // For sub-agents, use the system prompt as a single cached block
        let system_blocks = vec![SystemBlock::cached(system.clone())];
        let client_for_log = client.clone();
        let role_name_for_request_log = role_name_for_log.clone();
        let model_name_for_request_log = model_name_for_log.clone();
        let schema_count = sub_schemas.len();
        let log_request_messages = move |turn: usize, messages: &[Message]| {
            // 只记录摘要，不 dump 完整消息内容
            let latest: Vec<serde_json::Value> = messages
                .iter()
                .rev()
                .take(3)
                .map(|m| {
                    serde_json::json!({
                        "role": m.role,
                        "len": m.content.len(),
                    })
                })
                .collect();
            log::info!(target: LOG_TARGET,
                "[subagent_llm_request] session={}, turn={}, provider={}, model={}, role={}, model_spec={}, messages={}, tools={}, latest_roles={}",
                session_id_for_log,
                turn,
                client_for_log.provider_name(),
                client_for_log.model_name(),
                role_name_for_request_log,
                model_name_for_request_log,
                messages.len(),
                schema_count,
                serde_json::to_string(&latest).unwrap_or_default(),
            );
        };
        let sub_ctx = ToolExecutionContext {
            resources: tools::api::ToolResources {
                agent_runner: None, // No nested agents
                registry: ctx.resources.registry.clone(),
                memory_config: ctx.resources.memory_config.clone(),
                lang: ctx.resources.lang.clone(),
                allow_all: ctx.resources.allow_all,
            },
            // 子 agent 从父快照派生独立 workspace 实例（继承位置、空栈、独立锁），
            // 子的 worktree 进出不影响父（修隔离 bug，原先 Arc::clone 共享可变状态）。
            workspace: ctx.workspace.seed_isolated(),
            cancel: ctx.cancel.clone(),
            read_files: std::sync::Arc::new(
                std::sync::Mutex::new(std::collections::HashSet::new()),
            ),
            session_reminders: ctx.session_reminders.clone(),
            plan_mode: ctx.plan_mode,
            max_tool_concurrency: ctx.max_tool_concurrency,
            max_agent_concurrency: ctx.max_agent_concurrency,
            agent_semaphore: ctx.agent_semaphore.clone(), // 全局限流共享
            progress_tx: ctx.progress_tx.clone(), // 子 agent 复用父的 progress_tx，内部 tool 调用会通过 AgentProgress 转发到 TUI
            parent_session_id: ctx.parent_session_id.clone(),
        };
        let agent = Agent {
            registry: &sub_registry,
            ctx: sub_ctx,
        };

        let model_display = resolved_spec.as_deref().unwrap_or("default");
        // issue #499：发送 Started 事件，让 TUI 在 Agent 工具 header 显示实际 role/model。
        // 这是 sub-agent 的第一个 progress 事件，早于 ToolCalls/Message。
        if let Some(ref tx) = progress_tx {
            let _ = tx.try_send(AgentProgressEvent {
                sequence: 0,
                kind: AgentProgressKind::Started {
                    role: Some(role_name_for_log.clone()),
                    model: model_display.to_string(),
                },
            });
        }
        progress(
            None,
            &format!("Sub-agent started with model: {}", model_display),
        );

        SubAgentRun {
            prompt,
            system,
            ctx,
            progress_tx,
            client,
            hook_runner,
            sub_schemas,
            messages,
            handler,
            system_blocks,
            log_request_messages: Box::new(log_request_messages),
            agent,
            max_turns: max_turns_override as usize,
            start_time: std::time::Instant::now(),
            max_duration: std::time::Duration::from_secs(1800),
            session_id,
            role_name_for_log,
            model_name_for_log,
            resolved_spec,
            previous_max_tokens,
            previous_reasoning_level,
            restore_max_tokens,
            progress: Box::new(progress),
            ctx_context_size: 200_000,
        }
        .run_loop()
        .await
    }

    async fn complete(&self, prompt: &str, system: &str, ctx: &ToolExecutionContext) -> String {
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
