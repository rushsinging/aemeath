//! #1248 Task 3: RuntimeContextFactory — domain-responsible RuntimeContext assembly.
//!
//! The factory holds [`RuntimeServices`] (session-scoped shared ports) and
//! assembles a per-Run [`RuntimeContext`] from a [`RunSpec`], a
//! [`SessionSnapshot`] and optional parent Run capabilities.
//!
//! Capability-semantic decisions (interaction, hook, reasoning) are driven
//! by the binding-mode fields in [`RunSpec`], validated against parent
//! availability.

use std::sync::Arc;

use crate::application::client::RuntimeContextAssemblyError;
use crate::application::hook::empty::BoundaryHookPort;
use crate::application::interaction::port::{
    ParentMediatedInteractionPort, UnavailableInteractionPort,
};
use crate::application::loop_engine::chat::{ChatEventSinkHandle, RunEventSink};
use crate::application::run::context::{
    RunCapabilityBindings, RuntimeContext, RuntimeContextAssemblyToken, RuntimeServices,
};
use crate::application::run::preparation::{
    RunPreparationError, RunPreparationRequest, SessionSnapshot,
};
use crate::application::run::workspace::RuntimeWorkspaceAccess;
use crate::domain::agent_run::{
    HookBindingMode, InteractionBindingMode, ReasoningBindingMode, RunSpec,
};
use crate::ports::PolicyPort;
use hook::HookPort;
use memory::api::ReflectionHistoryStore;
use task::TaskAccess;
use tools::{
    RegistryScopeName, ToolCatalogError, ToolCatalogPort, ToolCatalogSnapshot,
    ToolExecutionContextBindingPort, ToolExecutionPort, ToolProfileName,
};

// ── Factory-owned session bindings ──

#[derive(Clone)]
struct SessionBindings {
    wiring: Arc<std::sync::RwLock<Option<Arc<context::MainSessionWiring>>>>,
    provider: Arc<std::sync::RwLock<Option<Arc<crate::ports::ProviderBinding>>>>,
    interaction: Arc<
        std::sync::RwLock<Option<Arc<dyn crate::application::interaction::port::InteractionPort>>>,
    >,
    reasoning:
        Arc<std::sync::RwLock<Option<Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>>>>>,
    event_sink: RunEventSink,
    workspace: Arc<std::sync::RwLock<Option<RuntimeWorkspaceAccess>>>,
    provider_factory: Arc<std::sync::RwLock<Option<Arc<dyn crate::ports::ProviderFactory>>>>,
    skill_materializer: Arc<std::sync::RwLock<Option<Arc<dyn tools::SkillMaterializationPort>>>>,
}

impl SessionBindings {
    fn new(wiring: Option<Arc<context::MainSessionWiring>>) -> Self {
        Self {
            wiring: Arc::new(std::sync::RwLock::new(wiring)),
            provider: Arc::new(std::sync::RwLock::new(None)),
            interaction: Arc::new(std::sync::RwLock::new(None)),
            reasoning: Arc::new(std::sync::RwLock::new(None)),
            event_sink: RunEventSink::default(),
            workspace: Arc::new(std::sync::RwLock::new(None)),
            provider_factory: Arc::new(std::sync::RwLock::new(None)),
            skill_materializer: Arc::new(std::sync::RwLock::new(None)),
        }
    }
}

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
                scope: format!("{scope}/{profile} — restricted catalog only serves sub-agent"),
            });
        }
        Ok(self.snapshot.clone())
    }
}

// ── Factory ──

/// #1248 Task 3 domain-responsible RuntimeContext assembly.
///
/// Holds [`RuntimeServices`] (session-scoped shared ports) and assembles
/// per-Run [`RuntimeContext`] instances from a [`RunSpec`], a
/// [`RunContextBindings`], and an optional parent [`RuntimeContext`].
pub struct RuntimeContextFactory {
    services: RuntimeServices,
    session: SessionBindings,
}

impl RuntimeContextFactory {
    /// #1248 Task 3: Narrow crate-root construction entry.
    ///
    /// Accepts seven explicit session-scoped port parameters — no opaque
    /// service bag. This is the only constructor callable from outside the
    /// runtime crate.
    pub fn new(
        tool_catalog: Arc<dyn ToolCatalogPort>,
        tool_execution: Arc<dyn ToolExecutionPort>,
        tool_context_binding: Arc<dyn ToolExecutionContextBindingPort>,
        policy: Arc<dyn PolicyPort>,
        reflection_history: Arc<dyn ReflectionHistoryStore>,
        task: Arc<dyn TaskAccess>,
        hooks: Arc<dyn HookPort>,
    ) -> Self {
        Self::from_services(
            tool_catalog,
            tool_execution,
            tool_context_binding,
            policy,
            reflection_history,
            task,
            hooks,
            None,
        )
    }

    pub fn with_session_wiring(
        tool_catalog: Arc<dyn ToolCatalogPort>,
        tool_execution: Arc<dyn ToolExecutionPort>,
        tool_context_binding: Arc<dyn ToolExecutionContextBindingPort>,
        policy: Arc<dyn PolicyPort>,
        reflection_history: Arc<dyn ReflectionHistoryStore>,
        task: Arc<dyn TaskAccess>,
        hooks: Arc<dyn HookPort>,
        wiring: Arc<context::MainSessionWiring>,
    ) -> Self {
        Self::from_services(
            tool_catalog,
            tool_execution,
            tool_context_binding,
            policy,
            reflection_history,
            task,
            hooks,
            Some(wiring),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_services(
        tool_catalog: Arc<dyn ToolCatalogPort>,
        tool_execution: Arc<dyn ToolExecutionPort>,
        tool_context_binding: Arc<dyn ToolExecutionContextBindingPort>,
        policy: Arc<dyn PolicyPort>,
        reflection_history: Arc<dyn ReflectionHistoryStore>,
        task: Arc<dyn TaskAccess>,
        hooks: Arc<dyn HookPort>,
        wiring: Option<Arc<context::MainSessionWiring>>,
    ) -> Self {
        Self {
            services: RuntimeServices {
                tool_catalog,
                tool_execution,
                tool_context_binding,
                policy,
                reflection_history,
                task,
                hooks,
            },
            session: SessionBindings::new(wiring),
        }
    }

    /// Read-only access to session-scoped [`RuntimeServices`].
    ///
    /// Used by TUI launch and other callers that need to project service
    /// ports (tool catalog/execution, hooks) without duplicating them
    /// on [`SessionRuntime`](super::client::SessionRuntime).
    pub fn services(&self) -> &RuntimeServices {
        &self.services
    }

    pub fn bind_session_wiring(&self, wiring: Arc<context::MainSessionWiring>) {
        *self
            .session
            .wiring
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(wiring);
    }

    pub fn bind_session_capabilities(
        &self,
        provider: Arc<crate::ports::ProviderBinding>,
        interaction: Arc<dyn crate::application::interaction::port::InteractionPort>,
        reasoning: Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>>,
        event_sink: ChatEventSinkHandle,
        workspace: RuntimeWorkspaceAccess,
    ) {
        let session = &self.session;
        *session
            .provider
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(provider);
        *session
            .interaction
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(interaction);
        *session
            .reasoning
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(reasoning);
        session.event_sink.bind(event_sink);
        *session
            .workspace
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(workspace);
    }

    pub fn bind_derived_factories(
        &self,
        provider_factory: Arc<dyn crate::ports::ProviderFactory>,
        skill_materializer: Arc<dyn tools::SkillMaterializationPort>,
    ) {
        let session = &self.session;
        *session
            .provider_factory
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(provider_factory);
        *session
            .skill_materializer
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(skill_materializer);
    }

    pub(crate) fn prepare(
        &self,
        request: &RunPreparationRequest,
    ) -> Result<
        (
            RuntimeContext,
            SessionSnapshot,
            Option<RuntimeWorkspaceAccess>,
        ),
        RunPreparationError,
    > {
        let session = &self.session;
        match request.parent() {
            Some(parent) => self.prepare_derived(request, parent, session),
            None => self.prepare_independent(request, session),
        }
    }

    fn prepare_independent(
        &self,
        request: &RunPreparationRequest,
        session: &SessionBindings,
    ) -> Result<
        (
            RuntimeContext,
            SessionSnapshot,
            Option<RuntimeWorkspaceAccess>,
        ),
        RunPreparationError,
    > {
        let wiring = session
            .wiring
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or(RunPreparationError::ContextAssembly)?;
        let permit = wiring
            .gate()
            .try_acquire_shared()
            .map_err(|_| RunPreparationError::ContextAssembly)?;
        let committed = wiring.committed_session();
        let config = wiring.committed_config();
        let resolved_session = if committed.id == request.session().session_id()
            && config.revision().get() == request.session().revision()
        {
            request.session().clone()
        } else {
            request.session().with_bound_values(
                committed.id.clone(),
                request.session().workspace_root().to_path_buf(),
                request.session().model_key().to_string(),
                config.clone(),
            )
        };
        let provider = session
            .provider
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or(RunPreparationError::ContextAssembly)?;
        let interaction = session
            .interaction
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or(RunPreparationError::ContextAssembly)?;
        let reasoning = session
            .reasoning
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or(RunPreparationError::ContextAssembly)?;
        let context = self
            .create(
                request.spec(),
                RunCapabilityBindings {
                    model: crate::application::run::context::ModelBindings {
                        context: wiring.committed_context(),
                        provider,
                        interaction,
                        memory: wiring.committed_memory(),
                        config: crate::application::run::config::RunConfigSnapshot::capture(config),
                        reasoning,
                        tool_catalog: None,
                    },
                    io: crate::application::run::context::IoBindings {
                        event_sink: ChatEventSinkHandle::new(session.event_sink.clone()),
                        input: crate::application::run::context::RunInputBufferHandle::new(),
                    },
                    lifecycle: crate::application::run::context::LifecycleBindings {
                        cancel: crate::application::run::context::RunCancellationScope::new(),
                        usage: crate::application::run::context::RunUsageTracker::new(),
                    },
                },
                None,
            )
            .map_err(|_| RunPreparationError::ContextAssembly)?
            .hold_session_lease(permit);
        Ok((context, resolved_session, None))
    }

    fn prepare_derived(
        &self,
        request: &RunPreparationRequest,
        parent: &crate::application::run::preparation::ParentRunCapabilities,
        session: &SessionBindings,
    ) -> Result<
        (
            RuntimeContext,
            SessionSnapshot,
            Option<RuntimeWorkspaceAccess>,
        ),
        RunPreparationError,
    > {
        let parent_context = parent
            .context()
            .ok_or(RunPreparationError::ContextAssembly)?;
        let parent_workspace = parent
            .workspace()
            .ok_or(RunPreparationError::ContextAssembly)?;
        let role = request
            .session()
            .config()
            .agents()
            .roles
            .get(&request.spec().name)
            .ok_or_else(|| RunPreparationError::SubRoleNotFound {
                role: request.spec().name.clone(),
            })?;
        if !role.enabled {
            return Err(RunPreparationError::SubRoleDisabled {
                role: request.spec().name.clone(),
            });
        }
        if role.model.trim().is_empty() {
            return Err(RunPreparationError::SubRoleNoModel {
                role: request.spec().name.clone(),
            });
        }
        let (source_key, source, model) = request
            .session()
            .config()
            .models()
            .find_model(&role.model)
            .ok_or_else(|| RunPreparationError::SubUnknownModel {
                model: role.model.clone(),
            })?;
        let max_tokens = role
            .max_tokens
            .filter(|tokens| *tokens > 0)
            .or_else(|| (model.max_tokens > 0).then_some(model.max_tokens))
            .unwrap_or(8192);
        let reasoning_level = model
            .reasoning_effort
            .as_deref()
            .and_then(provider::ReasoningLevel::parse)
            .unwrap_or(if model.reasoning.unwrap_or(false) {
                provider::ReasoningLevel::Medium
            } else {
                provider::ReasoningLevel::Off
            });
        let provider_factory = session
            .provider_factory
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or(RunPreparationError::ContextAssembly)?;
        let provider = provider_factory
            .build(crate::ports::ProviderBuildSpec {
                driver: source.driver.clone(),
                source_key: source_key.clone(),
                api_style: model.api_style.clone(),
                api_key: source.api_key.clone(),
                base_url: (!source.base_url.is_empty()).then(|| source.base_url.clone()),
                model: crate::ports::ModelId {
                    provider: source_key,
                    model: model.id.clone(),
                },
                max_tokens,
                requested_reasoning: reasoning_level,
                context_window: (model.context_window > 0).then_some(model.context_window),
                timeout: std::time::Duration::from_secs(
                    request.session().config().api_timeout_secs(),
                ),
                user_agent: request.session().config().user_agent().to_string(),
            })
            .map_err(|error| RunPreparationError::SubProviderBuild {
                message: error.to_string(),
            })?;
        let snapshot = parent_context
            .tool_catalog()
            .snapshot(
                &RegistryScopeName::new("sub-agent"),
                &ToolProfileName::new("sub-agent-restricted"),
            )
            .map_err(|error| RunPreparationError::SubToolCatalog {
                message: error.to_string(),
            })?;
        let workspace = parent_workspace.derive_isolated();
        let resolved_session = request.session().with_bound_values(
            request.session().session_id().to_string(),
            workspace.views().read().current_workspace_root(),
            role.model.clone(),
            request.session().config().clone(),
        );
        let skill_materializer = session
            .skill_materializer
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or(RunPreparationError::ContextAssembly)?;
        let isolated_context = context::api::isolated_context_with_skill(
            resolved_session.session_id(),
            skill_materializer,
            Arc::new(context::adapters::WorkspaceSkillQueryFactory::new(
                workspace.views().read(),
            )),
        );
        let context = self
            .create(
                request.spec(),
                RunCapabilityBindings {
                    model: crate::application::run::context::ModelBindings {
                        context: isolated_context,
                        provider: Arc::new(provider),
                        interaction: parent_context.interaction(),
                        memory: Arc::new(memory::NoOpMemory),
                        config: crate::application::run::config::RunConfigSnapshot::capture(
                            request.session().config().clone(),
                        ),
                        reasoning: Arc::new(std::sync::Mutex::new(
                            *parent_context
                                .reasoning_ref()
                                .lock()
                                .unwrap_or_else(|error| error.into_inner()),
                        )),
                        tool_catalog: Some(Arc::new(RestrictedToolCatalog { snapshot })),
                    },
                    io: crate::application::run::context::IoBindings {
                        event_sink: ChatEventSinkHandle::new(session.event_sink.clone()),
                        input: crate::application::run::context::RunInputBufferHandle::new(),
                    },
                    lifecycle: crate::application::run::context::LifecycleBindings {
                        cancel: parent_context.cancel().child_scope(),
                        usage: crate::application::run::context::RunUsageTracker::new(),
                    },
                },
                Some(parent_context.as_ref()),
            )
            .map_err(|_| RunPreparationError::ContextAssembly)?;
        Ok((context, resolved_session, Some(workspace)))
    }

    /// Assemble a [`RuntimeContext`] from capability-semantic decisions
    /// driven by the [`RunSpec`].
    ///
    /// # Assembly rules
    ///
    /// | Capability | Rule |
    /// |---|---|
    /// | Interaction | `ParentMediated` requires `parent`; `Client` / `Unavailable` are self-sufficient |
    /// | Hook | `BoundaryOnly` requires `parent` (Task 3 validates availability, keeps parent hook; Task 4/6 swaps restricted adapter) |
    /// | Reasoning | `Adaptive` → bindings port; `Fixed` → bindings port; `Inherit` → parent reasoning clone (fails if absent); `NoOp` → no-op port |
    /// | Tool catalog | If `bindings.tool_catalog` is `Some`, use that; otherwise use factory's services catalog |
    ///
    /// # Errors
    ///
    /// - [`RuntimeContextAssemblyError::InteractionUnavailable`] when
    ///   `ParentMediated` is requested without a parent.
    /// - [`RuntimeContextAssemblyError::HookUnavailable`] when
    ///   `BoundaryOnly` is requested without a parent.
    /// - [`RuntimeContextAssemblyError::ReasoningUnavailable`] when
    ///   `Inherit` is requested without a parent.
    ///
    /// Factory-private RuntimeContext construction used only by the unified assembly algorithm.
    pub(crate) fn create(
        &self,
        spec: &RunSpec,
        bindings: impl Into<RunCapabilityBindings>,
        parent: Option<&RuntimeContext>,
    ) -> Result<RuntimeContext, RuntimeContextAssemblyError> {
        let bindings = bindings.into();
        // ── Validate interaction binding mode ──
        let interaction_mode = self.select_interaction(spec);
        match interaction_mode {
            InteractionBindingMode::ParentMediated if parent.is_none() => {
                return Err(RuntimeContextAssemblyError::InteractionUnavailable);
            }
            _ => {}
        }

        // Wire interaction port based on RunSpec mode
        let interaction: std::sync::Arc<
            dyn crate::application::interaction::port::InteractionPort,
        > = match interaction_mode {
            InteractionBindingMode::Client => bindings.model.interaction.clone(),
            InteractionBindingMode::ParentMediated => std::sync::Arc::new(
                ParentMediatedInteractionPort::new(parent.unwrap().interaction()),
            ),
            InteractionBindingMode::Unavailable => std::sync::Arc::new(UnavailableInteractionPort),
        };

        // ── Validate hook binding mode ──
        let hook_mode = self.select_hook(spec);
        match hook_mode {
            HookBindingMode::BoundaryOnly if parent.is_none() => {
                return Err(RuntimeContextAssemblyError::HookUnavailable);
            }
            _ => {}
        }

        // #1248 Task 3: Allow per-Run tool_catalog override (e.g. restricted catalog for sub-runs).
        let services = if let Some(tool_catalog) = bindings.model.tool_catalog.clone() {
            RuntimeServices {
                tool_catalog,
                ..self.services.clone()
            }
        } else {
            self.services.clone()
        };
        let services = match hook_mode {
            HookBindingMode::Full => services,
            HookBindingMode::BoundaryOnly => RuntimeServices {
                hooks: std::sync::Arc::new(BoundaryHookPort::new(services.hooks.clone())),
                ..services
            },
        };

        let mut final_bindings = bindings;
        final_bindings.model.interaction = interaction;

        Ok(RuntimeContext::new(
            services,
            final_bindings,
            RuntimeContextAssemblyToken::new(),
        ))
    }

    // ── Binding-mode selectors (Task 1, preserved) ──

    /// Select the interaction binding mode from the RunSpec.
    pub fn select_interaction(&self, spec: &RunSpec) -> InteractionBindingMode {
        spec.interaction_binding()
    }

    /// Select interaction binding mode with an optional parent capability
    /// snapshot.
    pub fn select_interaction_with_parent(
        &self,
        spec: &RunSpec,
        parent_caps: Option<&RunSpec>,
    ) -> Result<InteractionBindingMode, RuntimeContextAssemblyError> {
        match spec.interaction_binding() {
            InteractionBindingMode::Client | InteractionBindingMode::Unavailable => {
                Ok(spec.interaction_binding())
            }
            InteractionBindingMode::ParentMediated => {
                if parent_caps.is_some() {
                    Ok(InteractionBindingMode::ParentMediated)
                } else {
                    Err(RuntimeContextAssemblyError::InteractionUnavailable)
                }
            }
        }
    }

    /// Select the hook binding mode from the RunSpec.
    pub fn select_hook(&self, spec: &RunSpec) -> HookBindingMode {
        spec.hook_binding()
    }

    /// Static reasoning is supplied directly by RunContextBindings.
    pub fn select_reasoning(&self, _spec: &RunSpec) -> ReasoningBindingMode {
        ReasoningBindingMode::Fixed
    }
}

// ── Test-only helpers ──
// #1248 Task 3: Tests that need to inject a specific hook port should
// construct a new RuntimeContextFactory with the desired RuntimeServices
// rather than mutating SessionRuntime fields directly.

#[cfg(test)]
impl RuntimeContextFactory {
    /// Test-only: create a factory with the given hook port replacing the
    /// default.  Prefer this over mutating `shell.hook_runner`.
    pub(crate) fn with_hooks(&self, hooks: std::sync::Arc<dyn hook::HookPort>) -> Self {
        Self {
            services: RuntimeServices {
                hooks,
                ..self.services.clone()
            },
            session: self.session.clone(),
        }
    }
}
