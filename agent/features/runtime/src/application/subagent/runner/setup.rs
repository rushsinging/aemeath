use super::loop_run::SubAgentRun;
use super::CliAgentRunner;
use crate::application::subagent::Agent;
use crate::ports::{ModelId, ProviderBinding, ProviderBuildSpec};
use async_trait::async_trait;
use hook::HookDispatchContext;
use provider::RequestSystemBlock;
use share::message::Message;
use std::sync::Arc;
use tools::{AgentProgressEvent, AgentProgressKind};
use tools::{AgentRunRequest, AgentRunner, ToolExecutionContext};

#[async_trait]
impl AgentRunner for CliAgentRunner {
    async fn run_agent(&self, request: AgentRunRequest<'_>) -> tools::AgentRunTerminal {
        let prompt = request.prompt;
        let system = request.system;
        let identity = request.identity;
        let cancellation = request.cancellation.child_signal();
        let runtime_cancellation = tokio_util::sync::CancellationToken::new().child_token();
        let request_progress = request.progress;
        let catalog = request.catalog;
        let plan_mode = request.plan_mode;
        let guidance = request.guidance;
        let timeout = request.timeout;
        let parent_run_id = Some(sdk::RunId::from_legacy_or_new(identity.run_id()));
        let role_name = request.role;
        let memory = request.memory;
        let progress_sink = request_progress.clone();
        let run_config = crate::application::run_config::RunConfigSnapshot::capture(
            self.config_reader.committed_snapshot(),
        );
        let config_snapshot = run_config.config();
        let role = match config_snapshot.agents().roles.get(role_name) {
            Some(role) if role.enabled => role,
            Some(_) => {
                return tools::AgentRunTerminal::Failed {
                    error: format!(
                        "子代理角色 `{role_name}` 已禁用（agents.roles.{role_name}.enabled=false)"
                    ),
                };
            }
            None => {
                let mut available = config_snapshot
                    .agents()
                    .roles
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>();
                available.sort();
                return tools::AgentRunTerminal::Failed {
                    error: format!(
                        "unknown sub-agent role `{role_name}`; configured roles: {}",
                        available.join(", ")
                    ),
                };
            }
        };
        if role.model.trim().is_empty() {
            return tools::AgentRunTerminal::Failed {
                error: format!("sub-agent role `{role_name}` has no configured model"),
            };
        }
        let resolved_spec = role.model.clone();

        // Resolve the model config from ModelsConfig to build a ProviderBuildSpec.
        // Unknown models fail closed — no silent fallback to a default client.
        let model_lookup = config_snapshot.models().find_model(&resolved_spec);

        let (source_key, source_config, model_entry) = match model_lookup {
            Some(found) => found,
            None => {
                return tools::AgentRunTerminal::Failed {
                    error: format!(
                        "unknown model `{resolved_spec}` configured for sub-agent role `{role_name}`"
                    ),
                };
            }
        };

        let max_tokens_override = Self::role_max_tokens_override(role);

        // Determine reasoning for this sub-agent: role config > model config > default
        let role_reasoning = role.reasoning;
        let model_reasoning = model_entry.reasoning;
        // 模型配置的固定推理档位（"off".."max"），优先级高于 reasoning bool。
        let model_effort = model_entry
            .reasoning_effort
            .as_deref()
            .and_then(provider::ReasoningLevel::parse);
        let reasoning = role_reasoning.or(model_reasoning).unwrap_or(self.reasoning);
        // The provider adapter clamps the requested level to the model capability
        // during invoke, so we pass the unclamped level here.
        let level = match model_effort {
            Some(effort) => effort,
            None => {
                if reasoning {
                    provider::ReasoningLevel::Medium
                } else {
                    provider::ReasoningLevel::Off
                }
            }
        };

        let context_size = config_snapshot.resolve_context_size(None, model_entry.context_window);

        // Construct ProviderBuildSpec from the resolved model config and build a binding.
        let max_tokens = max_tokens_override
            .filter(|tokens| *tokens > 0)
            .or_else(|| (model_entry.max_tokens > 0).then_some(model_entry.max_tokens))
            .unwrap_or(8192);
        let build_spec = ProviderBuildSpec {
            driver: source_config.driver.clone(),
            source_key: source_key.clone(),
            api_style: model_entry.api_style.clone(),
            api_key: source_config.api_key.clone(),
            base_url: if source_config.base_url.is_empty() {
                None
            } else {
                Some(source_config.base_url.clone())
            },
            model: ModelId {
                provider: source_key.clone(),
                model: model_entry.id.clone(),
            },
            max_tokens,
            requested_reasoning: level,
            context_window: (model_entry.context_window > 0).then_some(model_entry.context_window),
            timeout: std::time::Duration::from_secs(config_snapshot.api_timeout_secs()),
            user_agent: config_snapshot.user_agent().to_string(),
        };
        let binding: Arc<ProviderBinding> = match self.factory.build(build_spec) {
            Ok(binding) => Arc::new(binding),
            Err(error) => {
                return tools::AgentRunTerminal::Failed {
                    error: error.to_string(),
                };
            }
        };
        let max_tokens = binding.max_tokens;
        let session_id = identity
            .parent_run_id()
            .map(ToString::to_string)
            .or_else(|| {
                self.workspace
                    .views()
                    .read()
                    .current_workspace_root()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| "subagent".to_string());
        let role_name = role_name.to_string();
        let model_name = resolved_spec.clone();
        let sub_run_id = sdk::RunId::new_v7();
        let sub_run_context = super::loop_run::sub_run_log_context(
            &logging::capture(),
            &session_id,
            sub_run_id.as_ref(),
            &model_name,
            &binding.model.provider,
            &role_name,
        );

        logging::instrument(sub_run_context, async move {
        log::info!(target: crate::LOG_TARGET,
            "[SubAgent] reasoning={} level={} max_tokens={:?} (role={:?}, model={:?}, effort={:?}, default={})",
            reasoning,
            level,
            max_tokens_override,
            role_reasoning,
            model_reasoning,
            model_effort,
            self.reasoning
        );

        // Extract hook port to avoid borrow conflicts with closure
        let hook_port = self.hook_runner.clone();

        // Append role-specific system suffix if configured
        let system = match role.system_suffix.as_ref() {
            Some(suffix) => format!("{}\n\n{}", system, suffix),
            None => system.to_string(),
        };

        // Call SubagentStart hook
        let workspace_root = self.workspace.views().read().current_workspace_root();
        let hook_outcome = hook_port
            .dispatch_at(
                hook::HookInvocation::SubRunStart(hook::SubRunInput {
                    prompt: prompt.to_string(),
                    system: system.clone(),
                    model_spec: Some(resolved_spec.clone()),
                }),
                HookDispatchContext::new(&workspace_root),
                &tokio_util::sync::CancellationToken::new(),
            )
            .await;
        // Send any system messages from hook results to progress_tx
        for msg in &hook_outcome.messages {
            if let hook::HookDisplayMessageKind::SystemMessage = msg.kind {
                if let Some(ref sink) = progress_sink {
                    sink.emit(AgentProgressEvent {
                        sequence: 0,
                        kind: AgentProgressKind::Message {
                            text: format!("[hook] {}", msg.text),
                        },
                    });
                }
            }
        }

        // Helper to emit progress — writes to aemeath.log via log::info! for diagnostics.
        let session_id_for_log = session_id.clone();
        let role_name_for_log = role_name.clone();
        let model_name_for_log = model_name.clone();
        let progress = move |turn: Option<usize>, msg: &str| {
            let turn_str = turn
                .map(|t| t.to_string())
                .unwrap_or_else(|| "-".to_string());
            log::debug!(
                target: crate::LOG_TARGET,
                "[role:{} model:{} turn:{}] {}",
                role_name,
                model_name,
                turn_str,
                msg
            );
        };
        // Build a fresh sub-agent registry with all tools except Agent (prevent recursion)
        let sub_workspace = self.workspace.derive_isolated();
        let sub_catalog = match self.tool_catalog.snapshot(
            &tools::RegistryScopeName::new("sub-agent"),
            &tools::ToolProfileName::new("sub-agent-restricted"),
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return tools::AgentRunTerminal::Failed {
                    error: error.to_string(),
                }
            }
        };
        let tool_schemas = sub_catalog.model_schemas();
        let messages = vec![Message::user(prompt)];
        let provider_name_for_log = binding.model.provider.clone();
        let model_name_for_log_closure = model_name_for_log.clone();
        let role_name_for_request_log = role_name_for_log.clone();
        let schema_count = tool_schemas.len();
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
            log::info!(target: crate::LOG_TARGET,
                "[subagent_llm_request] session={}, turn={}, provider={}, model={}, role={}, messages={}, tools={}, latest_roles={}",
                session_id_for_log,
                turn,
                provider_name_for_log,
                model_name_for_log_closure,
                role_name_for_request_log,
                messages.len(),
                schema_count,
                serde_json::to_string(&latest).unwrap_or_default(),
            );
        };
        let sub_run_id = sdk::RunId::new_v7();
        let sub_views = sub_workspace.views();
        let sub_scope = tools::ExecutionScope::builder(
            sub_run_id.to_string(),
            sub_views.read().workspace_id(),
            sub_views.read().current_workspace_root(),
        )
        .parent_run_id(identity.run_id())
        .invocation_source(tools::InvocationSource::SubAgent)
        .registry_scope(tools::RegistryScopeName::new("sub-agent"))
        .profile(tools::ToolProfileName::new("sub-agent-restricted"))
        .build();
        let sub_ctx = ToolExecutionContext::new(
            sub_scope,
            tools::ToolExecutionPorts::new(
                cancellation.clone(),
                sub_workspace.read_access(),
                std::sync::Arc::new(tools::MutexReadSet(std::sync::Arc::new(
                    std::sync::Mutex::new(std::collections::HashSet::new()),
                ))),
                plan_mode,
                memory.clone(),
                guidance,
            )
            .with_user_agent(config_snapshot.user_agent())
            .with_catalog(catalog)
            .with_progress(request_progress),
        );
        let agent = Agent {
            catalog: sub_catalog,
            execution: self.tool_execution.clone(),
            ctx: sub_ctx,
            max_tool_concurrency: self.max_tool_concurrency,
            agent_semaphore: self.agent_semaphore.clone(),
            workspace_persist: sub_workspace.persist(),
            runtime_cancellation: runtime_cancellation.clone(),
        };

        let model_display = resolved_spec.as_str();
        // issue #499：发送 Started 事件，让 TUI 在 Agent 工具 header 显示实际 role/model。
        // 这是 sub-agent 的第一个 progress 事件，早于 ToolCalls/Message。
        if let Some(ref sink) = progress_sink {
            sink.emit(AgentProgressEvent {
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

        let isolated_session_id = sdk::SessionId::new_v7().to_string();
        let isolated_context = crate::application::context_coordination::ContextCoordinator::new(
            context::isolated_context_with_skill(
                &isolated_session_id,
                self.skill_materializer.clone(),
                std::sync::Arc::new(context::adapters::WorkspaceSkillQueryFactory::new(
                    sub_workspace.views().read(),
                )),
            ),
        );
        SubAgentRun {
            prompt,
            system,
            progress_sink,
            binding,
            max_tokens,
            level,
            hook_port,
            workspace_root,
            tool_schemas,
            config_snapshot: config_snapshot.clone(),
            language: config_snapshot.language().to_string(),
            messages,
            committed_message_count: 0,
            context: isolated_context,
            context_request: None,
            accepted_input: Vec::new(),
            context_window: None,
            log_request_messages: Box::new(log_request_messages),
            agent,
            runtime_cancellation,
            timeout,
            turn_count: 0,
            last_total_tokens: None,
            active_run: self.active_run.clone(),
            terminal: None,
            start_time: std::time::Instant::now(),
            session_id: isolated_session_id,
            run_id: sub_run_id,
            parent_run_id,
            role_name_for_log,
            model_name_for_log,
            resolved_spec: Some(resolved_spec),
            progress: Box::new(progress),
            ctx_context_size: context_size,
            tool_result_materializer: self.tool_result_materializer.clone(),
            policy: self.policy.clone(),
            tool_context_binding: self.tool_context_binding.clone(),
            input_strategy: crate::application::loop_engine::input_strategy::SubInputStrategy::new(
                prompt,
            ),
        }
        .run_loop()
        .await
        })
        .await
    }

    async fn complete(
        &self,
        prompt: &str,
        system: &str,
        cancellation: std::sync::Arc<dyn tools::CancellationSignal>,
    ) -> String {
        use crate::ports::{InvocationOptions, InvocationRequest};

        let runtime_cancellation = tokio_util::sync::CancellationToken::new();
        let _signal_propagation = super::loop_run::CancellationPropagationGuard::new(
            cancellation,
            runtime_cancellation.clone(),
        );

        // Resolve the default model for the simple completion path.
        let config_snapshot = self.config_reader.committed_snapshot();
        let default_spec = {
            let d = config_snapshot.models().default.as_str();
            (!d.is_empty()).then_some(d)
        };
        let model_lookup = default_spec.and_then(|spec| config_snapshot.models().find_model(spec));
        let (source_key, source_config, model_entry) = match model_lookup {
            Some(found) => found,
            None => return "LLM error: no default model configured".to_string(),
        };
        let max_tokens = if model_entry.max_tokens > 0 {
            model_entry.max_tokens
        } else {
            8192
        };
        let build_spec = ProviderBuildSpec {
            driver: source_config.driver.clone(),
            source_key: source_key.clone(),
            api_style: model_entry.api_style.clone(),
            api_key: source_config.api_key.clone(),
            base_url: if source_config.base_url.is_empty() {
                None
            } else {
                Some(source_config.base_url.clone())
            },
            model: ModelId {
                provider: source_key.clone(),
                model: model_entry.id.clone(),
            },
            max_tokens,
            requested_reasoning: provider::ReasoningLevel::Off,
            context_window: (model_entry.context_window > 0).then_some(model_entry.context_window),
            timeout: std::time::Duration::from_secs(config_snapshot.api_timeout_secs()),
            user_agent: config_snapshot.user_agent().to_string(),
        };
        let binding = match self.factory.build(build_spec) {
            Ok(binding) => binding,
            Err(error) => return format!("LLM error: {error}"),
        };

        let system_blocks = vec![RequestSystemBlock::Cacheable(system.to_string())];
        let messages = vec![Message::user(prompt)];
        let mut request = InvocationRequest::new(
            binding.model.clone(),
            messages,
            InvocationOptions::new(binding.max_tokens, binding.requested_reasoning),
        );
        request.system = system_blocks;
        request.cancellation = runtime_cancellation.clone();

        let mut stream = match binding
            .provider
            .invoke(request, &runtime_cancellation)
            .await
        {
            Ok(stream) => stream,
            Err(error) => return format!("LLM error: {error}"),
        };
        use futures::StreamExt;
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
