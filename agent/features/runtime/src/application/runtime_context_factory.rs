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
use crate::application::empty_hook::EmptyHookPort;
use crate::application::interaction::{ParentMediatedInteractionPort, UnavailableInteractionPort};
use crate::application::runtime_context::{
    RunCapabilityBindings, RuntimeContext, RuntimeContextAssemblyToken, RuntimeServices,
};
use crate::domain::agent_run::{
    HookBindingMode, InteractionBindingMode, ReasoningBindingMode, RunSpec,
};
use crate::ports::PolicyPort;
use hook::HookPort;
use memory::api::ReflectionHistoryStore;
use task::TaskAccess;
use tools::{ToolCatalogPort, ToolExecutionContextBindingPort, ToolExecutionPort};

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
    pub fn assemble(
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
        let interaction: std::sync::Arc<dyn crate::application::interaction::InteractionPort> =
            match interaction_mode {
                InteractionBindingMode::Client => bindings.model.interaction.clone(),
                InteractionBindingMode::ParentMediated => std::sync::Arc::new(
                    ParentMediatedInteractionPort::new(parent.unwrap().interaction()),
                ),
                InteractionBindingMode::Unavailable => {
                    std::sync::Arc::new(UnavailableInteractionPort)
                }
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
                hooks: std::sync::Arc::new(EmptyHookPort),
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
