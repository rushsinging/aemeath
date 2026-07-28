use super::loop_run::DerivedLoopCapabilityAdapter;
use super::CliAgentRunner;
use crate::application::run::context::RuntimeContext;
use crate::application::run::context_factory::RuntimeContextFactory;
use crate::application::run::preparation::{
    ParentRunCapabilities, RunPreparationRequest, SessionState,
};
use crate::application::run::workspace::RuntimeWorkspaceAccess;
use crate::application::tool::agent::Agent;
use crate::domain::agent_run::RunSpec;
use async_trait::async_trait;
use hook::HookDispatchContext;
use std::sync::Arc;
use std::time::Duration;
use tools::{AgentProgressEvent, AgentProgressKind};
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
pub struct PreparedSubRun {
    pub context: RuntimeContext,
    pub run: crate::domain::agent_run::Run,
    pub execution: crate::application::run::execution_state::RunExecutionState,
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
    /// Session ID for the isolated context (used as DerivedLoopCapabilityAdapter.session_id).
    pub session_id: String,
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
///   all downstream access comes from the returned `PreparedSubRun.workspace`.
///   **Never call `derive_isolated` a second time.**
///
/// #1397 P6.2: Uses the pure-value RunPreparer entry; context creation stays factory-private.
pub fn derive_sub_run(
    parent_spec: &RunSpec,
    parent_context: &RuntimeContext,
    parent_workspace: &RuntimeWorkspaceAccess,
    parent_run_id: crate::domain::agent_run::RunId,
    request: &SubRunRequest,
    provider_factory: Arc<dyn crate::ports::ProviderFactory>,
    skill_materializer: Arc<dyn tools::SkillMaterializationPort>,
    runtime_context_factory: Arc<RuntimeContextFactory>,
) -> Result<PreparedSubRun, crate::application::client::RuntimeContextAssemblyError> {
    use crate::application::client::RuntimeContextAssemblyError;

    // 1. Derive the RunSpec from parent.
    let spec = parent_spec
        .derive_sub(&request.role, request.timeout)
        .map_err(|e| RuntimeContextAssemblyError::SubDerivationFailed {
            reason: e.to_string(),
        })?;

    // 2. RuntimeContextFactory binds the derived workspace and live capabilities.
    let config_snapshot = parent_context.config().clone();
    let role = config_snapshot
        .config()
        .agents()
        .roles
        .get(&request.role)
        .ok_or_else(|| RuntimeContextAssemblyError::SubRoleNotFound {
            role: request.role.clone(),
        })?
        .clone();
    let resolved_spec = role.model.clone();
    let isolated_session_id = sdk::SessionId::new_v7().to_string();
    let session = SessionState::new(
        isolated_session_id,
        parent_workspace.views().read().current_workspace_root(),
        resolved_spec.clone(),
        config_snapshot.config().clone(),
    );
    let preparation_request = RunPreparationRequest::new(
        spec.clone(),
        session.snapshot_for_run(),
        Some(ParentRunCapabilities::from_active_run(
            parent_run_id.clone(),
            parent_spec.clone(),
            Arc::new(parent_context.clone()),
            parent_workspace.clone(),
        )),
    )
    .map_err(|error| RuntimeContextAssemblyError::SubDerivationFailed {
        reason: error.to_string(),
    })?;
    runtime_context_factory.bind_derived_factories(provider_factory, skill_materializer);
    let preparer = crate::application::run::preparer::RunPreparer::new(runtime_context_factory);
    let prepared = preparer.prepare(preparation_request).map_err(|error| {
        RuntimeContextAssemblyError::SubDerivationFailed {
            reason: error.to_string(),
        }
    })?;
    let (run, mut execution, session, context, workspace) = prepared.into_parts();
    execution.initialize_for_launch(Vec::new(), 0);
    let workspace = workspace.ok_or_else(|| RuntimeContextAssemblyError::SubDerivationFailed {
        reason: "子 Run workspace 未完成绑定".to_string(),
    })?;
    let context = context.expect("RunPreparer must produce RuntimeContext");
    let provider = context.provider();
    let model_display = role.model.clone();
    let model_name = role.model.clone();
    let max_tokens = provider.max_tokens;
    let reasoning_level = provider.requested_reasoning;

    Ok(PreparedSubRun {
        context,
        run,
        execution,
        workspace,
        role_config: role,
        model_display,
        model_name,
        max_tokens,
        reasoning_level,
        session_id: session.session_id().to_string(),
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
            parent_frame.run_id.clone(),
            &sub_request,
            self.factory.clone(),
            self.skill_materializer.clone(),
            self.runtime_context_factory.clone(),
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
        // not a duplicate static field. PreparedSubRun.reasoning_level is
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

        let sub_run_context = super::loop_run::sub_run_log_context(
            &logging::capture(),
            &session_id,
            derived.run.id().as_ref(),
            &model_name,
            &binding.model.provider,
            &role_name_for_log,
        );

        logging::instrument(sub_run_context, async move {
            // ── Logging ──
            log::info!(target: crate::LOG_TARGET,
                "[SubAgent] derived run_spec={} role={} model={} max_tokens={}",
                derived.run.spec().name, role_name_for_log, model_display, max_tokens
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
            let sub_catalog = match derived.context.tool_catalog().snapshot(
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

            // ── #1385: Execution scope from derived workspace (the single source) ──
            let sub_views = derived.workspace.views();
            let sub_scope = tools::ExecutionScope::builder(
                derived.run.id().to_string(),
                sub_views.read().workspace_id(),
                sub_views.read().current_workspace_root(),
            )
            .parent_run_id(identity.run_id())
            .invocation_source(tools::InvocationSource::SubAgent)
            .registry_scope(tools::RegistryScopeName::new("sub-agent"))
            .profile(tools::ToolProfileName::new("sub-agent-restricted"))
            .build();

            // #1385: Read access and persist from the SAME derived workspace.
            // Never call derive_isolated() a second time.
            let sub_ctx = ToolExecutionContext::new(
                sub_scope,
                tools::ToolExecutionPorts::new(
                    combined_cancel.clone(),
                    derived.workspace.read_access(),
                    Arc::new(tools::MutexReadSet(Arc::new(std::sync::Mutex::new(
                        std::collections::HashSet::new(),
                    )))),
                    plan_mode,
                    derived.context.memory(),
                    guidance,
                )
                .with_user_agent(derived.context.config_ref().config().user_agent())
                .with_progress(progress_sink.clone()),
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
            // ContextCoordinator is constructed on-demand inside DerivedLoopCapabilityAdapter,
            // not stored as a field.

            let context_size = derived
                .context
                .config_ref()
                .config()
                .resolve_context_size(None, 0);

            let config_snapshot = derived.context.config_ref().config().clone();

            super::loop_run::launch_sub_run(
                derived.run,
                derived.execution,
                DerivedLoopCapabilityAdapter {
                    prompt,
                    system,
                    progress_sink,
                    runtime_context: derived.context,
                    max_tokens,
                    workspace_root,
                    tool_schemas,
                    config_snapshot: config_snapshot.clone(),
                    language: config_snapshot.language().to_string(),
                    agent,
                    runtime_cancellation: runtime_token,
                    active_run: self.active_run.clone(),
                    session_id: derived.session_id,
                    role_name_for_log: role_name_for_log.clone(),
                    model_name_for_log: model_name,
                    resolved_spec: Some(model_display),
                    progress: Box::new(progress),
                    ctx_context_size: context_size,
                    tool_result_materializer: self.tool_result_materializer.clone(),
                    input_strategy:
                        crate::application::loop_engine::input_strategy::FixedInputAdapter::new(
                            prompt,
                        ),
                    plan_mode: plan_mode_active,
                },
            )
            .await
        })
        .await
    }
}
