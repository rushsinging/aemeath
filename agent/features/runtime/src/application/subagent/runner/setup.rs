use super::loop_run::SubAgentRun;
use super::CliAgentRunner;
use crate::application::interaction::UnavailableInteractionPort;
use crate::application::runtime_context::{
    RunContextBindings, RunInputBufferHandle, RunUsageTracker, RuntimeContext,
};
use crate::application::runtime_context_factory::RuntimeContextFactory;
use crate::application::subagent::Agent;
use crate::application::workspace_access::RuntimeWorkspaceAccess;
use crate::domain::agent_run::RunSpec;
use crate::ports::ModelId;
use async_trait::async_trait;
use hook::HookDispatchContext;
use provider::RequestSystemBlock;
use share::message::Message;
use std::sync::Arc;
use std::time::Duration;
use tools::{
    AgentProgressEvent, AgentProgressKind, RegistryScopeName, ToolCatalogError, ToolCatalogPort,
    ToolCatalogSnapshot, ToolProfileName,
};
use tools::{AgentRunRequest, AgentRunner, ToolExecutionContext};

// ── Sub-run derivation types ──

/// Minimal request for sub-run derivation — only the fields needed to
/// determine the derived [`RunSpec`] and [`RuntimeContext`].
#[derive(Debug, Clone)]
pub struct SubRunRequest {
    pub role: String,
    pub timeout: Duration,
}

/// Result of [`derive_sub_run`]: a single-source-of-truth bundle for the
/// sub-agent launcher.  Every downstream consumer reads from this bundle —
/// no second `derive_isolated()` call, no separate config snapshot, no
/// parallel catalog query.
///
/// #1385: holds full [`RuntimeWorkspaceAccess`] (not just [`project::WorkspaceViews`]);
/// all scope/read_access/persist/skill query/hook workspace root come from this
/// single derived workspace.
#[derive(Clone)]
pub struct DerivedSubRun {
    pub spec: RunSpec,
    pub context: RuntimeContext,
    /// Full workspace access — used for execution scope, read_access, persist,
    /// skill query factory, and hook workspace root.  Never call
    /// `derive_isolated()` a second time.
    pub workspace: RuntimeWorkspaceAccess,
    /// Resolved role config — avoids re-parsing config snapshot in run_agent.
    pub role_config: share::config::AgentRoleConfig,
    /// Resolved model display string (e.g. "test-provider/test-model").
    pub model_display: String,
    /// Resolved model name (e.g. "test-model").
    pub model_name: String,
    /// Max tokens for this sub-run.
    pub max_tokens: u32,
    /// Requested reasoning level.
    pub reasoning_level: provider::ReasoningLevel,
    /// Session ID for the isolated context (used as SubAgentRun.session_id).
    pub session_id: String,
}

// ── Restricted tool catalog wrapper ──

/// A [`ToolCatalogPort`] backed by a fixed snapshot that ONLY serves the
/// `sub-agent / sub-agent-restricted` scope+profile pair.  Any other
/// query is rejected to prevent semantic masquerading (e.g. a tool
/// requesting a full catalog through a restricted handle).
struct RestrictedToolCatalog {
    snapshot: ToolCatalogSnapshot,
}

impl ToolCatalogPort for RestrictedToolCatalog {
    fn snapshot(
        &self,
        scope: &RegistryScopeName,
        profile: &ToolProfileName,
    ) -> Result<ToolCatalogSnapshot, ToolCatalogError> {
        if scope.as_str() != "sub-agent" || profile.as_str() != "sub-agent-restricted" {
            return Err(ToolCatalogError::UnknownScope {
                scope: format!("{scope}/{profile} — RestrictedToolCatalog only serves sub-agent/sub-agent-restricted"),
            });
        }
        Ok(self.snapshot.clone())
    }
}

/// #1385: Combined cancellation signal — wraps an external signal (from the
/// tools-layer [`AgentRunRequest`]) and an internal [`tokio_util::sync::CancellationToken`].
/// Either source cancelling makes the combined signal fire, so a parent
/// cancellation propagates into tool execution AND the runtime LLM token.
struct CombinedCancellationSignal {
    external: Arc<dyn tools::CancellationSignal>,
    token: tokio_util::sync::CancellationToken,
}

#[async_trait]
impl tools::CancellationSignal for CombinedCancellationSignal {
    fn is_cancelled(&self) -> bool {
        self.external.is_cancelled() || self.token.is_cancelled()
    }

    async fn cancelled(&self) {
        // Race: whichever fires first wakes us.
        tokio::select! {
            _ = self.external.cancelled() => {},
            _ = self.token.cancelled() => {},
        }
    }

    fn child_signal(&self) -> Arc<dyn tools::CancellationSignal> {
        // Child tools get the same combined signal.
        Arc::new(Self {
            external: self.external.clone(),
            token: self.token.clone(),
        })
    }
}

/// Derive a sub-run [`RunSpec`], [`RuntimeContext`], isolated
/// [`RuntimeWorkspaceAccess`], and resolved model/role metadata from the
/// parent's capabilities.
///
/// # Rules (per #1385)
///
/// - **cancel**: `parent.child_scope()` — parent cancels child, child does NOT cancel parent.
/// - **tool catalog**: restricted `sub-agent/sub-agent-restricted` snapshot from parent.
/// - **memory**: `NoOpMemory` by default; not the parent's `Arc`.
/// - **context**: skills-wired [`ContextPort`] using the derived workspace query factory.
/// - **policy**: same `Arc` as parent (or stricter in future).
/// - **interaction**: disabled bridge (sub-agents are non-interactive).
///   Returns `InteractionCommandOutcome::NotFound` on any register/reply/cancel attempt.
/// - **provider**: built fresh from role config via factory; does NOT reuse parent transport.
/// - **task/hook/reflection/config**: shared from parent.
/// - **reasoning**: Inherit mode via factory (`workflow::inherited_reasoning`) — independent port.
/// - **workspace**: isolated via `parent_workspace.derive_isolated()` — used exactly once here;
///   all downstream access comes from the returned `DerivedSubRun.workspace`.
///   **Never call `derive_isolated` a second time.**
///
/// #1248 Task 3: Uses `runtime_context_factory.assemble()` for capability-semantic
/// decisions; the same factory instance serves both Main and Sub runs.
pub fn derive_sub_run(
    parent_spec: &RunSpec,
    parent_context: &RuntimeContext,
    parent_workspace: &RuntimeWorkspaceAccess,
    request: &SubRunRequest,
    provider_factory: &dyn crate::ports::ProviderFactory,
    skill_catalog: Arc<dyn tools::SkillCatalogPort>,
    runtime_context_factory: &RuntimeContextFactory,
) -> Result<DerivedSubRun, crate::application::client::RuntimeContextAssemblyError> {
    use crate::application::client::RuntimeContextAssemblyError;

    // 1. Derive the RunSpec from parent.
    let spec = parent_spec
        .derive_sub(&request.role, request.timeout)
        .map_err(|e| RuntimeContextAssemblyError::SubDerivationFailed {
            reason: e.to_string(),
        })?;

    // 2. Derive isolated workspace — exactly once, used for all downstream needs.
    let sub_workspace = parent_workspace.derive_isolated();

    // 3. Look up role config from parent's config snapshot.
    let config_snapshot = parent_context.config().clone();
    let config = config_snapshot.config();
    let role = config.agents().roles.get(&request.role).ok_or_else(|| {
        RuntimeContextAssemblyError::SubRoleNotFound {
            role: request.role.clone(),
        }
    })?;
    if !role.enabled {
        return Err(RuntimeContextAssemblyError::SubRoleDisabled {
            role: request.role.clone(),
        });
    }
    if role.model.trim().is_empty() {
        return Err(RuntimeContextAssemblyError::SubRoleNoModel {
            role: request.role.clone(),
        });
    }
    let resolved_spec = role.model.clone();

    // 4. Build provider binding via provider_factory.
    let sub_binding = {
        let model_lookup = config.models().find_model(&resolved_spec);
        let (_source_key, source_config, model_entry) =
            model_lookup.ok_or_else(|| RuntimeContextAssemblyError::SubUnknownModel {
                model: resolved_spec.clone(),
                role: request.role.clone(),
            })?;

        let max_tokens = CliAgentRunner::role_max_tokens_override(role)
            .filter(|t| *t > 0)
            .or_else(|| (model_entry.max_tokens > 0).then_some(model_entry.max_tokens))
            .unwrap_or(8192);

        // Determine reasoning level.
        let model_reasoning = model_entry.reasoning;
        let model_effort = model_entry
            .reasoning_effort
            .as_deref()
            .and_then(provider::ReasoningLevel::parse);
        let reasoning = model_reasoning.unwrap_or(false);
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

        let build_spec = crate::ports::ProviderBuildSpec {
            driver: source_config.driver.clone(),
            source_key: _source_key.clone(),
            api_style: model_entry.api_style.clone(),
            api_key: source_config.api_key.clone(),
            base_url: if source_config.base_url.is_empty() {
                None
            } else {
                Some(source_config.base_url.clone())
            },
            model: ModelId {
                provider: _source_key.clone(),
                model: model_entry.id.clone(),
            },
            max_tokens,
            requested_reasoning: level,
            context_window: (model_entry.context_window > 0).then_some(model_entry.context_window),
            timeout: std::time::Duration::from_secs(config_snapshot.config().api_timeout_secs()),
            user_agent: config_snapshot.config().user_agent().to_string(),
        };
        provider_factory.build(build_spec).map_err(|e| {
            RuntimeContextAssemblyError::SubProviderBuildFailed {
                role: request.role.clone(),
                message: e.to_string(),
            }
        })?
    };

    let max_tokens = sub_binding.max_tokens;
    let reasoning_level = sub_binding.requested_reasoning;
    // model_name is the full "provider/model" spec (matches logging expectations).
    let model_name = resolved_spec.clone();
    let model_display = resolved_spec.clone();

    // 5. Build restricted tool catalog from parent's catalog (NOT from
    // a separate runner field; derived.context.tool_catalog() is the
    // single source for catalog access).
    let restricted_catalog: Arc<dyn ToolCatalogPort> = {
        let snapshot = parent_context
            .tool_catalog()
            .snapshot_for_run(
                &RegistryScopeName::new("sub-agent"),
                &ToolProfileName::new("sub-agent-restricted"),
                config_snapshot.tool_selection(),
            )
            .map_err(|e| RuntimeContextAssemblyError::SubDerivationFailed {
                reason: e.to_string(),
            })?;
        Arc::new(RestrictedToolCatalog { snapshot })
    };

    // 6. Build skills-wired ContextPort using the derived workspace's
    // query factory.  This is the final port stored in RuntimeContext;
    // run_agent uses `derived.context.context()` directly.
    let isolated_session_id = sdk::SessionId::new_v7().to_string();
    let skills_context_port: Arc<dyn crate::ports::ContextPort> =
        context::isolated_context_with_skill(
            &isolated_session_id,
            skill_catalog,
            Arc::new(context::adapters::WorkspaceSkillQueryFactory::new(
                sub_workspace.views().read(),
            )),
        );

    // 7. #1248 Task 3: Assemble RuntimeContext via factory.
    // All per-Run ports go into RunContextBindings; the factory handles
    // capability-semantic decisions (reasoning Inherit → inherited_reasoning,
    // interaction ParentMediated validation, etc.).
    // Restricted tool catalog is passed as bindings.tool_catalog override.
    let cancel = parent_context.cancel().child_scope();
    let bindings = RunContextBindings {
        context: skills_context_port,
        provider: Arc::new(sub_binding.clone()),
        interaction: Arc::new(UnavailableInteractionPort),
        memory: Arc::new(memory::NoOpMemory),
        config: config_snapshot.clone(),
        cancel,
        event_sink: {
            crate::application::main_loop::ChatEventSinkHandle::new(
                super::loop_run::SubAgentEventSink,
            )
        },
        usage: RunUsageTracker::new(),
        input: RunInputBufferHandle::new(),
        reasoning: Arc::new(std::sync::Mutex::new(
            *parent_context
                .reasoning_ref()
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )),
        tool_catalog: Some(restricted_catalog),
    };

    let context = runtime_context_factory.assemble(&spec, bindings, Some(parent_context))?;

    Ok(DerivedSubRun {
        spec,
        context,
        workspace: sub_workspace,
        role_config: role.clone(),
        model_display,
        model_name,
        max_tokens,
        reasoning_level,
        session_id: isolated_session_id,
    })
}

#[async_trait]
impl AgentRunner for CliAgentRunner {
    async fn run_agent(&self, request: AgentRunRequest<'_>) -> tools::AgentRunTerminal {
        let prompt = request.prompt;
        let system = request.system;
        let identity = request.identity;
        // ── #1385: external cancellation signal from tools layer ──
        let external_cancel = request.cancellation.child_signal();
        let request_progress = request.progress;
        // #1385: request catalog/memory are NOT used for child;
        // all catalog/memory access comes from derived.context.
        let plan_mode = request.plan_mode;
        let plan_mode_active = plan_mode.is_plan_mode().unwrap_or(false);
        let guidance = request.guidance;
        let timeout = request.timeout;
        let parent_run_id = Some(sdk::RunId::from_legacy_or_new(identity.run_id()));
        let role_name = request.role;
        let progress_sink = request_progress.clone();

        // ── #1385: Read the parent frame from the shared RAII source ──
        let parent_frame = match self.parent_context.get() {
            Some(frame) => frame,
            None => {
                return tools::AgentRunTerminal::Failed {
                    error: "no parent run context — sub-agent invoked outside Main Run".to_string(),
                };
            }
        };

        let sub_request = SubRunRequest {
            role: role_name.to_string(),
            timeout,
        };
        let derived = match derive_sub_run(
            &parent_frame.spec,
            &parent_frame.context,
            &self.workspace,
            &sub_request,
            self.factory.as_ref(),
            self.skill_catalog.clone(),
            self.runtime_context_factory.as_ref(),
        ) {
            Ok(d) => d,
            Err(error) => {
                return tools::AgentRunTerminal::Failed {
                    error: error.to_string(),
                };
            }
        };

        // ── #1385: Every value below comes from `derived` or `derived.context` ──
        // No `config_reader` snapshot, no `self.tool_catalog`, no second
        // `derive_isolated()`, no separate `self.policy`/`self.tool_context_binding`.

        let run_spec = derived.spec.clone();

        // Runtime cancellation: derived from parent's scope, parent-cancels-child.
        let runtime_token = derived.context.cancel().token().clone();

        // Combined cancellation for ToolExecutionPorts: external (tools-layer)
        // OR runtime token cancelling stops executing tools.
        let combined_cancel: Arc<dyn tools::CancellationSignal> =
            Arc::new(CombinedCancellationSignal {
                external: external_cancel,
                token: runtime_token.clone(),
            });

        // Resolved metadata from derived (no config re-parse needed).
        let role_config = &derived.role_config;
        let role_name_for_log = role_name.to_string();
        let model_display = derived.model_display.clone();
        let model_name = derived.model_name.clone();
        let max_tokens = derived.max_tokens;
        // #1248 Task 7: reasoning level from RuntimeContext's ReasoningPort,
        // not a duplicate static field. DerivedSubRun.reasoning_level is
        // still available for diagnostics but no longer used at construction.
        let _reasoning_level = derived.reasoning_level;
        let binding = derived.context.provider();

        let session_id = identity
            .parent_run_id()
            .map(ToString::to_string)
            .or_else(|| {
                derived
                    .workspace
                    .views()
                    .read()
                    .current_workspace_root()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| "subagent".to_string());

        let sub_run_id = sdk::RunId::new_v7();
        let sub_run_context = super::loop_run::sub_run_log_context(
            &logging::capture(),
            &session_id,
            sub_run_id.as_ref(),
            &model_name,
            &binding.model.provider,
            &role_name_for_log,
        );

        logging::instrument(sub_run_context, async move {
            // ── Logging ──
            log::info!(target: crate::LOG_TARGET,
                "[SubAgent] derived run_spec={} role={} model={} max_tokens={}",
                run_spec.name, role_name_for_log, model_display, max_tokens
            );

            let hook_port = derived.context.hooks();

            // Append role-specific system suffix if configured
            let system = match role_config.system_suffix.as_ref() {
                Some(suffix) => format!("{}\n\n{}", system, suffix),
                None => system.to_string(),
            };

            // Call SubagentStart hook — workspace root from derived workspace.
            let workspace_root = derived.workspace.views().read().current_workspace_root();
            let hook_outcome = hook_port
                .dispatch_at(
                    hook::HookInvocation::SubRunStart(hook::SubRunInput {
                        prompt: prompt.to_string(),
                        system: system.clone(),
                        model_spec: Some(model_display.clone()),
                    }),
                    HookDispatchContext::new(&workspace_root),
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await;
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

            // Helper to emit progress
            let session_id_for_log = session_id.clone();
            let progress_role = role_name_for_log.clone();
            let progress_model = model_display.clone();
            let progress = move |turn: Option<usize>, msg: &str| {
                let turn_str = turn
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "-".to_string());
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[role:{} model:{} turn:{}] {}",
                    progress_role,
                    progress_model,
                    turn_str,
                    msg
                );
            };

            // ── #1385: Catalog from derived.context (NOT from self.tool_catalog) ──
            let sub_catalog = match derived
                .context
                .tool_catalog()
                .snapshot_for_run(
                    &tools::RegistryScopeName::new("sub-agent"),
                    &tools::ToolProfileName::new("sub-agent-restricted"),
                    derived.context.config_ref().tool_selection(),
                ) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return tools::AgentRunTerminal::Failed {
                        error: error.to_string(),
                    }
                }
            };
            let tool_schemas = sub_catalog.model_schemas();
            let schema_count = tool_schemas.len();

            // ── Log request messages callback ──
            let log_session_id = session_id_for_log.clone();
            let log_provider = binding.model.provider.clone();
            let log_model = model_name.clone();
            let log_role = role_name_for_log.clone();
            let log_request_messages = move |turn: usize, messages: &[Message]| {
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
                    log_session_id,
                    turn,
                    log_provider,
                    log_model,
                    log_role,
                    messages.len(),
                    schema_count,
                    serde_json::to_string(&latest).unwrap_or_default(),
                );
            };

            // ── #1385: Execution scope from derived workspace (the single source) ──
            let sub_views = derived.workspace.views();
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

            let available_tools = sub_catalog
              .tools
              .iter()
              .map(|descriptor| descriptor.name.as_str().to_string())
              .collect();

          // #1385: Read access and persist from the SAME derived workspace.
            // Never call derive_isolated() a second time.
            let sub_ctx = ToolExecutionContext::new(
                sub_scope,
                tools::ToolExecutionPorts::new(
                    combined_cancel.clone(),
                    derived.workspace.read_access(),
                    Arc::new(tools::MutexReadSet(Arc::new(
                        std::sync::Mutex::new(std::collections::HashSet::new()),
                    ))),
                    plan_mode,
                    derived.context.memory(),
                    guidance,
                )
                .with_skill_query(tools::SkillQuerySnapshot {
                    extra_dirs: derived.context.config_ref().config().skills().dirs.clone(),
                    available_tools,
                })
                .with_user_agent(derived.context.config_ref().config().user_agent())
                .with_progress(progress_sink.clone())
                .with_catalog(Some(Arc::new(sub_catalog.clone())))
                .with_selection(derived.context.config_ref().tool_selection().clone()),
            );
            let agent = Agent {
                catalog: sub_catalog,
                execution: derived.context.tool_execution(),
                ctx: sub_ctx,
                max_tool_concurrency: self.max_tool_concurrency,
                agent_semaphore: self.agent_semaphore.clone(),
                // #1385: workspace_persist from the same derived workspace.
                workspace_persist: derived.workspace.persist(),
                runtime_cancellation: runtime_token.clone(),
            };

            if let Some(ref sink) = progress_sink {
                sink.emit(AgentProgressEvent {
                    sequence: 0,
                    kind: AgentProgressKind::Started {
                        role: Some(role_name_for_log.clone()),
                        model: model_display.clone(),
                    },
                });
            }
            progress(
                None,
                &format!("Sub-agent started with model: {}", model_display),
            );

            // ── #1385: Context port from derived.context (skills-wired) ──
            // ContextCoordinator is constructed on-demand inside SubAgentRun,
            // not stored as a field.

            let messages = vec![Message::user(prompt)];
            let context_size = derived.context.config_ref().config().resolve_context_size(None, 0);

            let config_snapshot = derived.context.config_ref().config().clone();

            SubAgentRun {
                  prompt,
                  system,
                  progress_sink,
                  // #1385 Task 12: runtime_context is owned (not Arc) — derived context
                  // is already owned by the caller.
                  runtime_context: derived.context,
                  max_tokens,
                workspace_root,
                tool_schemas,
                config_snapshot: config_snapshot.clone(),
                language: config_snapshot.language().to_string(),
                messages,
                committed_message_count: 0,
                context_request: None,
                accepted_input: Vec::new(),
                context_window: None,
                log_request_messages: Box::new(log_request_messages),
                agent,
                runtime_cancellation: runtime_token,
                turn_count: 0,
                // #1385 Task 12: last_total_tokens eliminated — usage tracker is single source.
                active_run: self.active_run.clone(),
                terminal: None,
                start_time: std::time::Instant::now(),
                session_id: derived.session_id,
                run_id: sub_run_id,
                parent_run_id,
                role_name_for_log: role_name_for_log.clone(),
                model_name_for_log: model_name,
                resolved_spec: Some(model_display),
                progress: Box::new(progress),
                ctx_context_size: context_size,
                  tool_result_materializer: self.tool_result_materializer.clone(),
                  run_spec,
                input_strategy:
                    crate::application::loop_engine::input_strategy::SubInputStrategy::new(
                        prompt,
                    ),
                plan_mode: plan_mode_active,
                interaction_receivers: Vec::new(),
                pending_work: None,
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
        let build_spec = crate::ports::ProviderBuildSpec {
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
