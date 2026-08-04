//! RuntimeContextFactory — domain-responsible RuntimeContext assembly.
//!
//! The factory holds [`RuntimeServices`] (session-scoped shared ports) and
//! assembles a per-Run [`RuntimeContext`] from a [`RunSpec`], a
//! [`SessionSnapshot`] and optional parent Run capabilities.
//!
//! Capability-semantic decisions (interaction, hook, reasoning) are driven
//! by the binding-mode fields in [`RunSpec`], validated against parent
//! availability.

use std::sync::Arc;

use crate::application::hook::empty::BoundaryHookPort;
use crate::application::interaction::port::{
    ParentMediatedInteractionPort, UnavailableInteractionPort,
};
use crate::application::run::context::{
    RunCapabilityBindings, RuntimeContext, RuntimeContextAssemblyToken, RuntimeServices,
};
use crate::application::run::creation::{
    RunCreationBindings, RunCreationError, RunCreationRequest, SessionSnapshot,
};
use crate::application::run::workspace::RuntimeWorkspaceAccess;
use crate::domain::agent_run::{HookBindingMode, InteractionBindingMode, RunSpec};
use crate::ports::PolicyPort;
use hook::HookPort;
use memory::api::ReflectionHistoryStore;
use task::TaskAccess;
use tools::{
    RegistryScopeName, ToolCatalogError, ToolCatalogPort, ToolCatalogSnapshot, ToolExecutionPort,
    ToolProfileName,
};

// ── Factory-owned immutable services ──

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

struct SessionResolution {
    snapshot: SessionSnapshot,
    lease: Option<context::OwnedSessionSharedPermit>,
}

struct WorkspaceSelection {
    access: Option<RuntimeWorkspaceAccess>,
}

struct ProviderSelection {
    binding: Arc<crate::ports::ProviderBinding>,
}

struct ContextSelection {
    port: Arc<dyn crate::ports::ContextPort>,
}

struct MemorySelection {
    port: Arc<dyn memory::api::MemoryPort>,
}

struct ToolCatalogSelection {
    port: Option<Arc<dyn ToolCatalogPort>>,
}

struct InteractionSelection {
    port: Arc<dyn crate::application::interaction::port::InteractionPort>,
}

struct HookSelection {
    port: Arc<dyn HookPort>,
}

struct ReasoningSelection {
    port: Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>>,
}

struct EventRouteSelection {
    sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
}

#[derive(Clone)]
struct IsolatedRunEventSink;

impl crate::application::loop_engine::chat::ChatEventSink for IsolatedRunEventSink {
    fn send_event<'a>(
        &'a self,
        _event: crate::application::loop_engine::chat::RuntimeStreamEvent,
    ) -> crate::application::loop_engine::chat::EventFuture<'a> {
        Box::pin(std::future::ready(()))
    }

    fn try_send_event(&self, _event: crate::application::loop_engine::chat::RuntimeStreamEvent) {}
}

struct LifecycleSelection {
    cancel: crate::application::run::context::RunCancellationScope,
    usage: crate::application::run::context::RunUsageTracker,
}

struct SkillLoadSelection {
    state: Arc<dyn tools::SkillLoadStatePort>,
    session_id: String,
}

struct RunCreationResources {
    session: SessionResolution,
    workspace: WorkspaceSelection,
}

/// Domain-responsible RuntimeContext assembly.
///
/// Holds [`RuntimeServices`] (session-scoped shared ports) and assembles
/// per-Run [`RuntimeContext`] instances from a [`RunSpec`], grouped
/// [`RunCapabilityBindings`], and an optional parent [`RuntimeContext`].
pub struct RuntimeContextFactory {
    services: RuntimeServices,
    provider_factory: Option<Arc<dyn crate::ports::ProviderFactory>>,
    skill_catalog: Option<Arc<dyn tools::SkillCatalogPort>>,
    use_injected_hooks: bool,
}

impl RuntimeContextFactory {
    /// Narrow crate-root construction entry.
    ///
    /// Accepts six explicit session-scoped port parameters — no opaque
    /// service bag. This is the only constructor callable from outside the
    /// runtime crate.
    pub fn new(
        tool_catalog: Arc<dyn ToolCatalogPort>,
        tool_execution: Arc<dyn ToolExecutionPort>,
        policy: Arc<dyn PolicyPort>,
        reflection_history: Arc<dyn ReflectionHistoryStore>,
        task: Arc<dyn TaskAccess>,
        hooks: Arc<dyn HookPort>,
    ) -> Self {
        Self::from_services(
            tool_catalog,
            tool_execution,
            policy,
            reflection_history,
            task,
            hooks,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_services(
        tool_catalog: Arc<dyn ToolCatalogPort>,
        tool_execution: Arc<dyn ToolExecutionPort>,
        policy: Arc<dyn PolicyPort>,
        reflection_history: Arc<dyn ReflectionHistoryStore>,
        task: Arc<dyn TaskAccess>,
        hooks: Arc<dyn HookPort>,
    ) -> Self {
        Self {
            services: RuntimeServices {
                tool_catalog,
                tool_execution,
                policy,
                reflection_history,
                task,
                hooks,
            },
            provider_factory: None,
            skill_catalog: None,
            use_injected_hooks: cfg!(test),
        }
    }

    #[cfg(test)]
    pub(crate) fn use_snapshot_hooks_for_test(&mut self) {
        self.use_injected_hooks = false;
    }

    pub fn with_derived_bindings(
        &self,
        provider_factory: Arc<dyn crate::ports::ProviderFactory>,
        skill_catalog: Arc<dyn tools::SkillCatalogPort>,
    ) -> Self {
        Self {
            services: self.services.clone(),
            provider_factory: Some(provider_factory),
            skill_catalog: Some(skill_catalog),
            use_injected_hooks: self.use_injected_hooks,
        }
    }

    /// Read-only access to process-stable [`RuntimeServices`].
    pub fn services(&self) -> &RuntimeServices {
        &self.services
    }

    pub(crate) fn prepare(
        &self,
        request: &RunCreationRequest,
        bindings: &RunCreationBindings,
    ) -> Result<
        (
            RuntimeContext,
            SessionSnapshot,
            Option<RuntimeWorkspaceAccess>,
        ),
        RunCreationError,
    > {
        let session = self.resolve_session(request, bindings)?;
        let workspace = self.select_workspace(bindings)?;
        let provider = self.select_provider(request, bindings)?;
        let context = self.select_context(request, bindings, &session, &workspace)?;
        let memory = self.select_memory(bindings)?;
        let tool_catalog = self.select_tool_catalog(bindings)?;
        let parent = bindings.parent().map(|parent| parent.context().clone());
        let interaction =
            self.select_interaction_port(request.spec(), bindings, parent.as_deref())?;
        let run_config = crate::application::run::config::RunConfigSnapshot::capture(
            session.snapshot.config().clone(),
        );
        let hook = self.select_hook_port(request.spec(), &run_config, parent.as_deref())?;
        let reasoning = self.select_reasoning_port(bindings, parent.as_deref())?;
        let event_route = self.select_event_route(bindings)?;
        let activity_publisher = Arc::new(event_route.sink.clone());
        let lifecycle = self.select_lifecycle(request, bindings, parent.as_deref())?;
        let skill_load = self.select_skill_load(&context, parent.as_deref(), &session);
        let bindings = RunCapabilityBindings {
            model: crate::application::run::context::ModelBindings {
                context: context.port,
                provider: provider.binding,
                interaction: interaction.port,
                memory: memory.port,
                config: run_config,
                reasoning: reasoning.port,
                tool_catalog: tool_catalog.port,
            },
            io: crate::application::run::context::IoBindings {
                event_sink: event_route.sink,
                input: crate::application::run::context::RunInputBufferHandle::new(),
            },
            lifecycle: crate::application::run::context::LifecycleBindings {
                cancel: lifecycle.cancel,
                usage: lifecycle.usage,
            },
            skill_load_session_id: skill_load.session_id,
        };
        self.bind_runtime_context(
            request
                .run_id()
                .cloned()
                .unwrap_or_else(crate::domain::agent_run::RunId::new_v7),
            activity_publisher,
            bindings,
            hook,
            skill_load.state,
            RunCreationResources { session, workspace },
        )
    }

    fn bind_runtime_context(
        &self,
        run_id: crate::domain::agent_run::RunId,
        activity_publisher: Arc<dyn crate::application::activity::ActivityChangePublisher>,
        bindings: RunCapabilityBindings,
        hook: HookSelection,
        skill_load_state: Arc<dyn tools::SkillLoadStatePort>,
        resources: RunCreationResources,
    ) -> Result<
        (
            RuntimeContext,
            SessionSnapshot,
            Option<RuntimeWorkspaceAccess>,
        ),
        RunCreationError,
    > {
        let services = RuntimeServices {
            tool_catalog: bindings
                .model
                .tool_catalog
                .clone()
                .unwrap_or_else(|| self.services.tool_catalog.clone()),
            hooks: hook.port,
            ..self.services.clone()
        };
        let context = RuntimeContext::new(
            services,
            bindings,
            skill_load_state,
            Arc::new(
                crate::application::activity::ActivityCoordinator::production(
                    run_id,
                    activity_publisher,
                ),
            ),
            RuntimeContextAssemblyToken::new(),
        );
        let context = match resources.session.lease {
            Some(lease) => context.hold_session_lease(lease),
            None => context,
        };
        Ok((
            context,
            resources.session.snapshot,
            resources.workspace.access,
        ))
    }

    fn resolve_session(
        &self,
        request: &RunCreationRequest,
        bindings: &RunCreationBindings,
    ) -> Result<SessionResolution, RunCreationError> {
        if request.parent().is_some() != bindings.parent().is_some() {
            return Err(RunCreationError::ContextAssembly);
        }
        let Some(session) = bindings.session() else {
            if bindings.parent().is_none() {
                return Err(RunCreationError::ContextAssembly);
            }
            return Ok(SessionResolution {
                snapshot: request.session().clone(),
                lease: None,
            });
        };
        let wiring = session.wiring();
        let lease = wiring
            .gate()
            .try_acquire_shared()
            .map_err(|_| RunCreationError::ContextAssembly)?;
        let committed = wiring.committed_session();
        let config = wiring.committed_config();
        let snapshot = if committed.id == request.session().session_id()
            && config.revision().get() == request.session().revision()
        {
            request.session().clone()
        } else {
            request.session().with_bound_values(
                committed.id.clone(),
                request.session().workspace_root().to_path_buf(),
                request.session().model_key().to_string(),
                config,
            )
        };
        Ok(SessionResolution {
            snapshot,
            lease: Some(lease),
        })
    }

    fn select_workspace(
        &self,
        bindings: &RunCreationBindings,
    ) -> Result<WorkspaceSelection, RunCreationError> {
        Ok(WorkspaceSelection {
            access: bindings
                .parent()
                .map(|parent| parent.workspace().derive_isolated()),
        })
    }

    fn select_provider(
        &self,
        request: &RunCreationRequest,
        bindings: &RunCreationBindings,
    ) -> Result<ProviderSelection, RunCreationError> {
        if let Some(session) = bindings.session() {
            return Ok(ProviderSelection {
                binding: session.provider().clone(),
            });
        }
        let role = self.resolve_derived_role(request)?;
        let (source_key, source, model) = request
            .session()
            .config()
            .models()
            .find_model(&role.model)
            .ok_or_else(|| RunCreationError::SubUnknownModel {
                model: role.model.clone(),
            })?;
        let max_tokens = role
            .max_tokens
            .filter(|tokens| *tokens > 0)
            .or_else(|| (model.max_tokens > 0).then_some(model.max_tokens))
            .unwrap_or(8192);
        let requested_reasoning = model
            .reasoning_effort
            .as_deref()
            .and_then(provider::ReasoningLevel::parse)
            .unwrap_or(if model.reasoning.unwrap_or(false) {
                provider::ReasoningLevel::Medium
            } else {
                provider::ReasoningLevel::Off
            });
        let binding = self
            .provider_factory
            .as_ref()
            .ok_or(RunCreationError::ContextAssembly)?
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
                requested_reasoning,
                context_window: (model.context_window > 0).then_some(model.context_window),
                timeout: std::time::Duration::from_secs(
                    request.session().config().api_timeout_secs(),
                ),
                user_agent: request.session().config().user_agent().to_string(),
            })
            .map_err(|error| RunCreationError::SubProviderBuild {
                message: error.to_string(),
            })?;
        Ok(ProviderSelection {
            binding: Arc::new(binding),
        })
    }

    fn select_context(
        &self,
        _request: &RunCreationRequest,
        bindings: &RunCreationBindings,
        session: &SessionResolution,
        workspace: &WorkspaceSelection,
    ) -> Result<ContextSelection, RunCreationError> {
        if let Some(bindings) = bindings.session() {
            return Ok(ContextSelection {
                port: bindings.wiring().committed_context(),
            });
        }
        let workspace = workspace
            .access
            .as_ref()
            .ok_or(RunCreationError::ContextAssembly)?;
        let skill_catalog = self
            .skill_catalog
            .as_ref()
            .ok_or(RunCreationError::ContextAssembly)?
            .clone();
        Ok(ContextSelection {
            port: context::isolated_context_with_skill(
                session.snapshot.session_id(),
                skill_catalog,
                Arc::new(context::adapters::WorkspaceSkillQueryFactory::new(
                    workspace.views().read(),
                )),
            ),
        })
    }

    fn select_memory(
        &self,
        bindings: &RunCreationBindings,
    ) -> Result<MemorySelection, RunCreationError> {
        let port = match bindings.session() {
            Some(session) => session.wiring().committed_memory(),
            None => Arc::new(memory::NoOpMemory),
        };
        Ok(MemorySelection { port })
    }

    fn select_tool_catalog(
        &self,
        bindings: &RunCreationBindings,
    ) -> Result<ToolCatalogSelection, RunCreationError> {
        let Some(parent) = bindings.parent() else {
            return Ok(ToolCatalogSelection { port: None });
        };
        let snapshot = parent
            .context()
            .tool_catalog()
            .snapshot(
                &RegistryScopeName::new("sub-agent"),
                &ToolProfileName::new("sub-agent-restricted"),
            )
            .map_err(|error| RunCreationError::SubToolCatalog {
                message: error.to_string(),
            })?;
        Ok(ToolCatalogSelection {
            port: Some(Arc::new(RestrictedToolCatalog { snapshot })),
        })
    }

    fn select_interaction_port(
        &self,
        spec: &RunSpec,
        bindings: &RunCreationBindings,
        parent: Option<&RuntimeContext>,
    ) -> Result<InteractionSelection, RunCreationError> {
        let port = match spec.interaction_binding() {
            InteractionBindingMode::Client => bindings
                .session()
                .map(|session| session.interaction().clone())
                .or_else(|| parent.map(RuntimeContext::interaction))
                .ok_or(RunCreationError::ContextAssembly)?,
            InteractionBindingMode::ParentMediated => Arc::new(ParentMediatedInteractionPort::new(
                parent
                    .ok_or(RunCreationError::ContextAssembly)?
                    .interaction(),
            )),
            InteractionBindingMode::Unavailable => Arc::new(UnavailableInteractionPort),
        };
        Ok(InteractionSelection { port })
    }

    fn select_hook_port(
        &self,
        spec: &RunSpec,
        config: &crate::application::run::config::RunConfigSnapshot,
        parent: Option<&RuntimeContext>,
    ) -> Result<HookSelection, RunCreationError> {
        let run_hooks: Arc<dyn HookPort> = if cfg!(test) && self.use_injected_hooks {
            self.services.hooks.clone()
        } else {
            Arc::new(
                hook::build_dispatcher(config.config().hooks())
                    .map_err(|_| RunCreationError::ContextAssembly)?,
            )
        };
        let port = match spec.hook_binding() {
            HookBindingMode::Full => run_hooks,
            HookBindingMode::BoundaryOnly => {
                parent.ok_or(RunCreationError::ContextAssembly)?;
                Arc::new(BoundaryHookPort::new(run_hooks))
            }
        };
        Ok(HookSelection { port })
    }

    fn select_reasoning_port(
        &self,
        bindings: &RunCreationBindings,
        parent: Option<&RuntimeContext>,
    ) -> Result<ReasoningSelection, RunCreationError> {
        let port = match bindings.session() {
            Some(session) => session.reasoning().clone(),
            None => Arc::new(std::sync::Mutex::new(
                *parent
                    .ok_or(RunCreationError::ContextAssembly)?
                    .reasoning_ref()
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()),
            )),
        };
        Ok(ReasoningSelection { port })
    }

    fn select_event_route(
        &self,
        bindings: &RunCreationBindings,
    ) -> Result<EventRouteSelection, RunCreationError> {
        let sink = match bindings.session() {
            Some(session) => session.event_sink().clone(),
            None => {
                bindings.parent().ok_or(RunCreationError::ContextAssembly)?;
                crate::application::loop_engine::chat::ChatEventSinkHandle::new(
                    IsolatedRunEventSink,
                )
            }
        };
        Ok(EventRouteSelection { sink })
    }

    fn select_lifecycle(
        &self,
        _request: &RunCreationRequest,
        bindings: &RunCreationBindings,
        parent: Option<&RuntimeContext>,
    ) -> Result<LifecycleSelection, RunCreationError> {
        Ok(LifecycleSelection {
            cancel: parent
                .map(|context| context.cancel().child_scope())
                .unwrap_or_default(),
            // Session Runs share a per-Session usage tracker so that a new Run
            // inherits the last known API total tokens from the previous Run.
            // Sub-Runs (Parent variant) get an isolated tracker.
            usage: match bindings.session() {
                Some(session) => session.usage().clone(),
                None => crate::application::run::context::RunUsageTracker::new(),
            },
        })
    }

    fn select_skill_load(
        &self,
        context: &ContextSelection,
        parent: Option<&RuntimeContext>,
        session: &SessionResolution,
    ) -> SkillLoadSelection {
        match parent {
            Some(parent) => SkillLoadSelection {
                state: parent.skill_load_state(),
                session_id: parent.skill_load_session_id().to_string(),
            },
            None => SkillLoadSelection {
                state: Arc::new(
                    crate::application::context::skill_load_state::ContextSkillLoadState::new(
                        context.port.clone(),
                    ),
                ),
                session_id: session.snapshot.session_id().to_string(),
            },
        }
    }

    fn resolve_derived_role<'a>(
        &self,
        request: &'a RunCreationRequest,
    ) -> Result<&'a share::config::AgentRoleConfig, RunCreationError> {
        let role = request
            .session()
            .config()
            .agents()
            .roles
            .get(&request.spec().name)
            .ok_or_else(|| RunCreationError::SubRoleNotFound {
                role: request.spec().name.clone(),
            })?;
        if !role.enabled {
            return Err(RunCreationError::SubRoleDisabled {
                role: request.spec().name.clone(),
            });
        }
        if role.model.trim().is_empty() {
            return Err(RunCreationError::SubRoleNoModel {
                role: request.spec().name.clone(),
            });
        }
        Ok(role)
    }
}
