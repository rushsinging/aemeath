use super::loop_run::SubAgentRun;
use super::CliAgentRunner;
use crate::application::agent::Agent;
use crate::LOG_TARGET;
use async_trait::async_trait;
use provider::SystemBlock;
use share::message::Message;
use tools::{AgentProgressEvent, AgentProgressKind};
use tools::{AgentRunRequest, AgentRunner, ToolExecutionContext, ToolRegistry};

#[async_trait]
impl AgentRunner for CliAgentRunner {
    async fn run_agent(&self, request: AgentRunRequest<'_>) -> tools::AgentRunTerminal {
        let prompt = request.prompt;
        let system = request.system;
        let ctx = request.ctx;
        let timeout = request.timeout;
        let parent_run_id = Some(sdk::RunId::from_legacy_or_new(&ctx.run_id));
        let model_spec = request.model_spec;
        let progress_tx = request.progress_tx;
        // Resolve role and model
        let role = self.resolve_role(model_spec);
        let resolved_spec = self.resolve_model_spec(model_spec);

        // Clients only own immutable transport/defaults. Every sub Run receives an independent scope.
        let client = match (&self.pool, &resolved_spec) {
            (Some(pool), Some(spec)) => match pool.get_isolated_client(spec) {
                Ok(client) => std::sync::Arc::new(client),
                Err(error) => {
                    return tools::AgentRunTerminal::Failed { error };
                }
            },
            _ => self.client.clone(),
        };

        let max_tokens_override = Self::role_max_tokens_override(role);

        // Determine reasoning for this sub-agent: role config > model config > default
        let role_reasoning = role.and_then(|r| r.reasoning);
        let model_entry = resolved_spec.as_deref().and_then(|spec| {
            // Try find_model to get the ModelEntryConfig for reasoning lookup
            let query = if spec.contains('/') {
                spec.to_string()
            } else {
                format!("{}/{}", self.client.provider_name(), spec)
            };
            self.models_config.find_model(&query)
        });
        let context_size = model_entry
            .as_ref()
            .map(|(_, _, entry)| entry.context_window)
            .filter(|size| *size > 0)
            .unwrap_or(200_000);
        let model_reasoning = model_entry
            .as_ref()
            .and_then(|(_, _, entry)| entry.reasoning);
        // 模型配置的固定推理档位（"off".."max"），优先级高于 reasoning bool。
        let model_effort = model_entry
            .as_ref()
            .and_then(|(_, _, entry)| entry.reasoning_effort.as_deref())
            .and_then(provider::ReasoningLevel::parse);
        let reasoning = role_reasoning.or(model_reasoning).unwrap_or(self.reasoning);
        // effort 存在时取显式档位（clamp 到 provider 上限），否则沿用 bool→Medium/Off。
        let level = match model_effort {
            Some(effort) => effort.clamped_to(client.max_reasoning_level()),
            None => {
                if reasoning {
                    provider::ReasoningLevel::Medium
                } else {
                    provider::ReasoningLevel::Off
                }
            }
        };
        let invocation_scope = match client.invocation_scope(
            client.default_scope().model(),
            max_tokens_override,
            level,
        ) {
            Ok(scope) => scope,
            Err(error) => {
                return tools::AgentRunTerminal::Failed {
                    error: error.to_string(),
                };
            }
        };
        log::info!(target: LOG_TARGET,
            "[SubAgent] reasoning={} level={} max_tokens={:?} (role={:?}, model={:?}, effort={:?}, default={})",
            reasoning,
            level,
            max_tokens_override,
            role_reasoning,
            model_reasoning,
            model_effort,
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
        let role_name = model_spec.map(|s| s.to_string()).unwrap_or_else(|| {
            // 未配 role 时 fallback 到实际 client 的 model 名，而非硬编码 "default"
            resolved_spec
                .clone()
                .unwrap_or_else(|| client.model_name().to_string())
        });
        let model_name = resolved_spec
            .clone()
            .unwrap_or_else(|| client.model_name().to_string());
        let role_name_for_log = role_name.clone();
        let model_name_for_log = model_name.clone();
        // 将 sub-agent 的 model 同步到日志 context（影响 hook/audit 等共享 sink 的 model 字段）
        logging::set_current_model(model_name.clone());
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
        // Sub Run 用独立的 task::TaskStore access，不共享父 Run 的 Task 状态（#889）。
        let sub_task_access: std::sync::Arc<dyn task::TaskAccess> =
            std::sync::Arc::new(task::TaskStore::new());
        let sub_skills =
            std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        let mut sub_registry = ToolRegistry::new();
        tools::register_subagent_tools(&mut sub_registry, sub_task_access, sub_skills);
        let sub_schemas = sub_registry.schemas_for(&ctx.resources.lang);
        let messages = vec![Message::user(prompt)];
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
        let sub_run_id = sdk::RunId::new_v7();
        let sub_ctx = ToolExecutionContext {
            resources: tools::ToolResources {
                agent_runner: None, // No nested agents
                registry: ctx.resources.registry.clone(),
                memory_config: ctx.resources.memory_config.clone(),
                lang: ctx.resources.lang.clone(),
                allow_all: ctx.resources.allow_all,
            },
            // 子 agent 从父快照派生独立 workspace 实例（继承位置、空栈、独立锁），
            // 子的 worktree 进出不影响父（修隔离 bug，原先 Arc::clone 共享可变状态）。
            workspace: ctx.derive_isolated_workspace(),
            run_id: sub_run_id.to_string(),
            cancel: ctx.cancel.child_token(),
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

        let model_display = resolved_spec.as_deref().unwrap_or(&model_name_for_log);
        // issue #499：发送 Started 事件，让 TUI 在 Agent 工具 header 显示实际 role/model。
        // 这是 sub-agent 的第一个 progress 事件，早于 ToolCalls/Message。
        if let Some(ref tx) = progress_tx {
            let _ = tx.try_send(AgentProgressEvent {
                sequence: 0,
                kind: AgentProgressKind::Started {
                    // 未配 role 时发 None，TUI 不显示 [role: ...] 标记。
                    role: model_spec.map(|s| s.to_string()),
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
            progress_tx,
            client,
            invocation_scope,
            hook_runner,
            sub_schemas,
            messages,
            system_blocks,
            log_request_messages: Box::new(log_request_messages),
            agent,
            timeout,
            turn_count: 0,
            last_total_tokens: None,
            active_run: self.active_run.clone(),
            terminal: None,
            start_time: std::time::Instant::now(),
            session_id,
            run_id: sub_run_id,
            parent_run_id,
            role_name_for_log,
            model_name_for_log,
            resolved_spec,
            progress: Box::new(progress),
            ctx_context_size: context_size,
            tool_result_materializer: self.tool_result_materializer.clone(),
        }
        .run_loop()
        .await
    }

    async fn complete(&self, prompt: &str, system: &str, ctx: &ToolExecutionContext) -> String {
        use futures::StreamExt;

        let system_blocks = vec![SystemBlock::cached(system.to_string())];
        let messages = vec![Message::user(prompt)];

        let mut stream = match self
            .client
            .invocation_stream(
                self.client.default_scope(),
                &system_blocks,
                &messages,
                &[],
                &ctx.cancel,
            )
            .await
        {
            Ok(stream) => stream,
            Err(error) => return format!("LLM error: {error}"),
        };
        while let Some(event) = stream.next().await {
            match event {
                provider::InvocationEvent::Completed(completion) => {
                    return completion
                        .output
                        .iter()
                        .filter_map(|block| match block {
                            provider::ProviderContentBlock::Text(text) => Some(text.as_str()),
                            _ => None,
                        })
                        .collect();
                }
                provider::InvocationEvent::Failed(error) => {
                    return format!("LLM error: {error}");
                }
                provider::InvocationEvent::Delta(_) => {}
            }
        }
        "LLM error: provider stream ended without terminal event".to_string()
    }
}
