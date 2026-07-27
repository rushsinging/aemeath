//! #1248 Task 3: RuntimeContextFactory — domain-responsible RuntimeContext assembly.
//!
//! The factory holds [`RuntimeServices`] (session-scoped shared ports) and
//! assembles a per-Run [`RuntimeContext`] from a [`RunSpec`], a
//! [`RunContextBindings`], and an optional parent [`RuntimeContext`].
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
use crate::application::run::context::{
    RunCapabilityBindings, RuntimeContext, RuntimeContextAssemblyToken, RuntimeServices,
};
use crate::application::run::preparation::{RunPreparationError, RunPreparationRequest};
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

// ── Runtime-owned resolution contract ──

/// Converts a pure-value Run preparation request into one frozen RuntimeContext.
pub trait RuntimeContextResolver: Send + Sync {
    fn resolve(
        &self,
        factory: &RuntimeContextFactory,
        request: &RunPreparationRequest,
    ) -> Result<
        (
            RuntimeContext,
            crate::application::run::preparation::SessionSnapshot,
        ),
        RunPreparationError,
    >;
}

pub(crate) struct MainRunContextResolver {
    wiring: Arc<context::MainSessionWiring>,
    provider: Arc<crate::ports::ProviderBinding>,
    interaction: Arc<dyn crate::application::interaction::port::InteractionPort>,
    reasoning: Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>>,
    event_sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
}

impl MainRunContextResolver {
    pub(crate) fn new(
        wiring: Arc<context::MainSessionWiring>,
        provider: Arc<crate::ports::ProviderBinding>,
        interaction: Arc<dyn crate::application::interaction::port::InteractionPort>,
        reasoning: Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>>,
        event_sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
    ) -> Self {
        Self {
            wiring,
            provider,
            interaction,
            reasoning,
            event_sink,
        }
    }
}

impl RuntimeContextResolver for MainRunContextResolver {
    fn resolve(
        &self,
        factory: &RuntimeContextFactory,
        request: &RunPreparationRequest,
    ) -> Result<
        (
            RuntimeContext,
            crate::application::run::preparation::SessionSnapshot,
        ),
        RunPreparationError,
    > {
        let _permit = self
            .wiring
            .gate()
            .try_acquire_shared()
            .map_err(|_| RunPreparationError::ContextAssembly)?;
        let committed = self.wiring.committed_session();
        let config = self.wiring.committed_config();
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
        factory
            .create(
                request.spec(),
                RunCapabilityBindings {
                    model: crate::application::run::context::ModelBindings {
                        context: self.wiring.committed_context(),
                        provider: self.provider.clone(),
                        interaction: self.interaction.clone(),
                        memory: self.wiring.committed_memory(),
                        config: crate::application::run::config::RunConfigSnapshot::capture(config),
                        reasoning: self.reasoning.clone(),
                        tool_catalog: None,
                    },
                    io: crate::application::run::context::IoBindings {
                        event_sink: self.event_sink.clone(),
                        input: crate::application::run::context::RunInputBufferHandle::new(),
                    },
                    lifecycle: crate::application::run::context::LifecycleBindings {
                        cancel: crate::application::run::context::RunCancellationScope::new(),
                        usage: crate::application::run::context::RunUsageTracker::new(),
                    },
                },
                None,
            )
            .map(|context| (context.hold_session_lease(_permit), resolved_session))
            .map_err(|_| RunPreparationError::ContextAssembly)
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

pub(crate) struct SubRunContextResolver {
    parent: Arc<RuntimeContext>,
    workspace: crate::application::workspace::access::RuntimeWorkspaceAccess,
    derived_workspace: Arc<
        std::sync::Mutex<Option<crate::application::workspace::access::RuntimeWorkspaceAccess>>,
    >,
    provider_factory: Arc<dyn crate::ports::ProviderFactory>,
    skill_materializer: Arc<dyn tools::SkillMaterializationPort>,
}

impl SubRunContextResolver {
    pub(crate) fn new(
        parent: Arc<RuntimeContext>,
        workspace: crate::application::workspace::access::RuntimeWorkspaceAccess,
        provider_factory: Arc<dyn crate::ports::ProviderFactory>,
        skill_materializer: Arc<dyn tools::SkillMaterializationPort>,
    ) -> Self {
        Self {
            parent,
            workspace,
            derived_workspace: Arc::new(std::sync::Mutex::new(None)),
            provider_factory,
            skill_materializer,
        }
    }

    pub(crate) fn take_workspace(
        &self,
    ) -> Option<crate::application::workspace::access::RuntimeWorkspaceAccess> {
        self.derived_workspace
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }
}

impl RuntimeContextResolver for SubRunContextResolver {
    fn resolve(
        &self,
        factory: &RuntimeContextFactory,
        request: &RunPreparationRequest,
    ) -> Result<
        (
            RuntimeContext,
            crate::application::run::preparation::SessionSnapshot,
        ),
        RunPreparationError,
    > {
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
        let (_, source, model) = request
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
        let provider = self
            .provider_factory
            .build(crate::ports::ProviderBuildSpec {
                driver: source.driver.clone(),
                source_key: self.parent.provider().model.provider.clone(),
                api_style: model.api_style.clone(),
                api_key: source.api_key.clone(),
                base_url: (!source.base_url.is_empty()).then(|| source.base_url.clone()),
                model: crate::ports::ModelId {
                    provider: self.parent.provider().model.provider.clone(),
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
        let snapshot = self
            .parent
            .tool_catalog()
            .snapshot(
                &RegistryScopeName::new("sub-agent"),
                &ToolProfileName::new("sub-agent-restricted"),
            )
            .map_err(|error| RunPreparationError::SubToolCatalog {
                message: error.to_string(),
            })?;
        let workspace = self.workspace.derive_isolated();
        *self
            .derived_workspace
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(workspace.clone());
        let session_id = sdk::SessionId::new_v7().to_string();
        let resolved_session = request.session().with_identity(
            session_id,
            workspace.views().read().current_workspace_root(),
            role.model.clone(),
        );
        let context = context::api::isolated_context_with_skill(
            resolved_session.session_id(),
            self.skill_materializer.clone(),
            Arc::new(context::adapters::WorkspaceSkillQueryFactory::new(
                workspace.views().read(),
            )),
        );
        factory
            .create(
                request.spec(),
                RunCapabilityBindings {
                    model: crate::application::run::context::ModelBindings {
                        context,
                        provider: Arc::new(provider),
                        interaction: self.parent.interaction(),
                        memory: Arc::new(memory::NoOpMemory),
                        config: crate::application::run::config::RunConfigSnapshot::capture(
                            request.session().config().clone(),
                        ),
                        reasoning: Arc::new(std::sync::Mutex::new(
                            *self
                                .parent
                                .reasoning_ref()
                                .lock()
                                .unwrap_or_else(|error| error.into_inner()),
                        )),
                        tool_catalog: Some(Arc::new(RestrictedToolCatalog { snapshot })),
                    },
                    io: crate::application::run::context::IoBindings {
                        event_sink: crate::application::loop_engine::chat::ChatEventSinkHandle::new(
                            crate::application::run::derived::loop_run::SubAgentEventSink,
                        ),
                        input: crate::application::run::context::RunInputBufferHandle::new(),
                    },
                    lifecycle: crate::application::run::context::LifecycleBindings {
                        cancel: self.parent.cancel().child_scope(),
                        usage: crate::application::run::context::RunUsageTracker::new(),
                    },
                },
                Some(self.parent.as_ref()),
            )
            .map(|context| (context, resolved_session))
            .map_err(|_| RunPreparationError::ContextAssembly)
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
    /// Factory-private context construction used only by RuntimeContextResolver implementations.
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
        }
    }
}
