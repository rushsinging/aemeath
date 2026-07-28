use std::time::Duration;

/// Capability policy applied when a spec is created without a parent ceiling.
///
/// This policy expresses the effective capability contract directly; it is not
/// a run-origin or lifecycle role marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapabilityPolicy {
    Full,
    Restricted,
}

// ── #1248 Task 1: capability-semantic enums ──

/// Interaction binding mode — who mediates user interaction for this run.
///
/// Ordering (most → least permissive): `Client` > `ParentMediated` > `Unavailable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionBindingMode {
    /// Direct client interaction (Main run; historical `Interactive`).
    Client,
    /// Interaction mediated through the parent run (Sub that can AskUser /
    /// request approval through the parent seam).
    ParentMediated,
    /// No interaction available. Any attempt to register a request immediately
    /// fails with a typed unavailable error.
    Unavailable,
}

impl InteractionBindingMode {
    /// `self` must not be more permissive than `ceiling`.
    pub fn is_within(&self, ceiling: &Self) -> bool {
        !matches!(
            (self, ceiling),
            (
                InteractionBindingMode::Client,
                InteractionBindingMode::ParentMediated
            ) | (
                InteractionBindingMode::Client,
                InteractionBindingMode::Unavailable
            ) | (
                InteractionBindingMode::ParentMediated,
                InteractionBindingMode::Unavailable
            )
        )
    }
}

/// Hook binding mode — which lifecycle hooks are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookBindingMode {
    /// All hooks active (Main run).
    Full,
    /// No Hook capability. Runtime binds a no-op HookPort for this mode.
    BoundaryOnly,
}

impl HookBindingMode {
    pub fn is_within(&self, ceiling: &Self) -> bool {
        !matches!(
            (self, ceiling),
            (HookBindingMode::Full, HookBindingMode::BoundaryOnly)
        )
    }
}

/// Reasoning binding mode — how the run selects its reasoning effort.
///
/// Ordering (most → least permissive):
/// `Adaptive` > `Fixed` > `Inherit` > `NoOp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningBindingMode {
    /// Graph-driven adaptive reasoning (Main run; historical `GraphDriven`).
    Adaptive,
    /// Fixed effort level declared by role / RunSpec.
    Fixed,
    /// Inherit parent's effective requested level at construction time;
    /// `observe` does not advance the parent graph.
    Inherit,
    /// No reasoning — `observe` / `set_level` are no-ops.
    NoOp,
}

impl ReasoningBindingMode {
    pub fn is_within(&self, ceiling: &Self) -> bool {
        !matches!(
            (self, ceiling),
            (ReasoningBindingMode::Adaptive, ReasoningBindingMode::Fixed)
                | (
                    ReasoningBindingMode::Adaptive,
                    ReasoningBindingMode::Inherit
                )
                | (ReasoningBindingMode::Adaptive, ReasoningBindingMode::NoOp)
                | (ReasoningBindingMode::Fixed, ReasoningBindingMode::Inherit)
                | (ReasoningBindingMode::Fixed, ReasoningBindingMode::NoOp)
                | (ReasoningBindingMode::Inherit, ReasoningBindingMode::NoOp)
        )
    }
}

// ── legacy capability enums (retained for backward compat until #1397) ──
//
// Migration note: these will be removed when #1397 completes the capability
// wiring.  New code should use the *BindingMode enums above for
// capability-semantic dimensions.  Do NOT add #[deprecated] — the workspace
// still uses these for existing fixed-profile dimensions until #1397 lands.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    SessionQueue,
    Fixed,
}

impl InputMode {
    /// `self` must not be more permissive than `ceiling`.
    pub fn is_within(&self, ceiling: &Self) -> bool {
        !matches!((self, ceiling), (InputMode::SessionQueue, InputMode::Fixed))
    }
}

/// Legacy interaction mode — governs whether the run can prompt a human user.
///
/// Retained for the six legacy fixed-profile dimensions until #1397.
/// New capability-semantic interaction decisions use [`InteractionBindingMode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionMode {
    Interactive,
    NonInteractive,
}

impl InteractionMode {
    pub fn is_within(&self, ceiling: &Self) -> bool {
        !matches!(
            (self, ceiling),
            (
                InteractionMode::Interactive,
                InteractionMode::NonInteractive
            )
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventRoute {
    Client,
    ParentRun,
}

impl EventRoute {
    pub fn is_within(&self, ceiling: &Self) -> bool {
        !matches!((self, ceiling), (EventRoute::Client, EventRoute::ParentRun))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceMode {
    Shared,
    Isolated,
}

impl ResourceMode {
    pub fn is_within(&self, ceiling: &Self) -> bool {
        !matches!(
            (self, ceiling),
            (ResourceMode::Shared, ResourceMode::Isolated)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMode {
    Enabled,
    Disabled,
}

impl MemoryMode {
    pub fn is_within(&self, ceiling: &Self) -> bool {
        !matches!((self, ceiling), (MemoryMode::Enabled, MemoryMode::Disabled))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolScope {
    Full,
    Restricted,
}

impl ToolScope {
    pub fn is_within(&self, ceiling: &Self) -> bool {
        !matches!((self, ceiling), (ToolScope::Full, ToolScope::Restricted))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunSpecError {
    #[error("子 Run 能力不得超过父 Run")]
    CapabilityEscalation,
}

/// Parent capability ceiling for sub-run specs.
/// Stored when created via [`RunSpec::derive_sub`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct CapabilityCeiling {
    input: InputMode,
    interaction: InteractionMode,
    events: EventRoute,
    context: ResourceMode,
    workspace: ResourceMode,
    memory: MemoryMode,
    tools: ToolScope,
    timeout: Duration,
    // #1248 Task 1: new capability-semantic dimensions
    interaction_kind: InteractionBindingMode,
    hooks: HookBindingMode,
    reasoning: ReasoningBindingMode,
}

impl CapabilityCeiling {
    fn from_spec(spec: &RunSpec) -> Self {
        Self {
            input: spec.input,
            interaction: spec.interaction,
            events: spec.events,
            context: spec.context,
            workspace: spec.workspace,
            memory: spec.memory,
            tools: spec.tools,
            timeout: spec.timeout,
            interaction_kind: spec.interaction_kind,
            hooks: spec.hooks,
            reasoning: spec.reasoning,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    pub name: String,
    pub timeout: Duration,
    pub input: InputMode,
    pub interaction: InteractionMode,
    pub events: EventRoute,
    pub context: ResourceMode,
    pub workspace: ResourceMode,
    pub memory: MemoryMode,
    pub tools: ToolScope,
    /// #1248 Task 1: capability-semantic dimensions (private — use accessors).
    /// Private to prevent direct mutation that would bypass the capability
    /// ceiling check.
    interaction_kind: InteractionBindingMode,
    hooks: HookBindingMode,
    reasoning: ReasoningBindingMode,
    /// Capability policy used when no parent ceiling is present.
    policy: CapabilityPolicy,
    /// When set (for derived specs via `derive_sub`), capability ceilings
    /// inherited from the parent. `with_*` methods validate against this.
    ceiling: Option<CapabilityCeiling>,
}

impl RunSpec {
    pub fn main() -> Self {
        Self::full("main", Duration::ZERO)
    }

    fn full(name: impl Into<String>, timeout: Duration) -> Self {
        Self {
            name: name.into(),
            timeout,
            input: InputMode::SessionQueue,
            interaction: InteractionMode::Interactive,
            events: EventRoute::Client,
            context: ResourceMode::Shared,
            workspace: ResourceMode::Shared,
            memory: MemoryMode::Enabled,
            tools: ToolScope::Full,
            interaction_kind: InteractionBindingMode::Client,
            hooks: HookBindingMode::Full,
            reasoning: ReasoningBindingMode::Adaptive,
            policy: CapabilityPolicy::Full,
            ceiling: None,
        }
    }

    pub fn sub(name: impl Into<String>, timeout: Duration) -> Self {
        Self::restricted(name, timeout)
    }

    fn restricted(name: impl Into<String>, timeout: Duration) -> Self {
        Self {
            name: name.into(),
            timeout,
            input: InputMode::Fixed,
            interaction: InteractionMode::NonInteractive,
            events: EventRoute::ParentRun,
            context: ResourceMode::Isolated,
            workspace: ResourceMode::Isolated,
            memory: MemoryMode::Disabled,
            tools: ToolScope::Restricted,
            interaction_kind: InteractionBindingMode::ParentMediated,
            hooks: HookBindingMode::BoundaryOnly,
            reasoning: ReasoningBindingMode::Inherit,
            policy: CapabilityPolicy::Restricted,
            ceiling: None,
        }
    }

    /// Create a run spec derived from `self` (the parent).
    ///
    /// The derived spec starts from the restricted capability policy and stores
    /// the parent's effective values as ceilings. Use `with_*` methods to relax
    /// individual fields up to — but never beyond — the parent.
    pub fn derive_sub(
        &self,
        name: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, RunSpecError> {
        // Validate timeout first.
        if !self.timeout.is_zero() && (timeout.is_zero() || timeout > self.timeout) {
            return Err(RunSpecError::CapabilityEscalation);
        }

        let mut derived = Self::restricted(name, timeout);

        // Validate every restricted-default field against the parent's ceiling.
        if !derived.input.is_within(&self.input)
            || !derived.interaction.is_within(&self.interaction)
            || !derived.events.is_within(&self.events)
            || !derived.context.is_within(&self.context)
            || !derived.workspace.is_within(&self.workspace)
            || !derived.memory.is_within(&self.memory)
            || !derived.tools.is_within(&self.tools)
            || !derived.interaction_kind.is_within(&self.interaction_kind)
            || !derived.hooks.is_within(&self.hooks)
            || !derived.reasoning.is_within(&self.reasoning)
        {
            return Err(RunSpecError::CapabilityEscalation);
        }

        // Store parent's current values as the ceiling.
        // If parent itself has a ceiling, we inherit the *effective* values
        // (which are already bounded), not the original ceiling.
        derived.ceiling = Some(CapabilityCeiling::from_spec(self));
        Ok(derived)
    }

    // ── with_* builders ───────────────────────────────────────────

    pub fn with_input(mut self, input: InputMode) -> Result<Self, RunSpecError> {
        self.enforce_policy(input.is_within(&InputMode::Fixed))?;
        self.check_ceiling(|c| input.is_within(&c.input))?;
        self.input = input;
        Ok(self)
    }

    pub fn with_interaction(mut self, interaction: InteractionMode) -> Result<Self, RunSpecError> {
        self.enforce_policy(interaction.is_within(&InteractionMode::NonInteractive))?;
        self.check_ceiling(|c| interaction.is_within(&c.interaction))?;
        self.interaction = interaction;
        Ok(self)
    }

    pub fn with_events(mut self, events: EventRoute) -> Result<Self, RunSpecError> {
        self.enforce_policy(events.is_within(&EventRoute::ParentRun))?;
        self.check_ceiling(|c| events.is_within(&c.events))?;
        self.events = events;
        Ok(self)
    }

    pub fn with_context(mut self, context: ResourceMode) -> Result<Self, RunSpecError> {
        self.enforce_policy(context.is_within(&ResourceMode::Isolated))?;
        self.check_ceiling(|c| context.is_within(&c.context))?;
        self.context = context;
        Ok(self)
    }

    pub fn with_workspace(mut self, workspace: ResourceMode) -> Result<Self, RunSpecError> {
        self.enforce_policy(workspace.is_within(&ResourceMode::Isolated))?;
        self.check_ceiling(|c| workspace.is_within(&c.workspace))?;
        self.workspace = workspace;
        Ok(self)
    }

    pub fn with_memory_mode(mut self, memory: MemoryMode) -> Result<Self, RunSpecError> {
        if self.ceiling.is_none() {
            self.enforce_policy(memory.is_within(&MemoryMode::Disabled))?;
        }
        self.check_ceiling(|c| memory.is_within(&c.memory))?;
        self.memory = memory;
        Ok(self)
    }

    pub fn with_tool_scope(mut self, tools: ToolScope) -> Result<Self, RunSpecError> {
        self.enforce_policy(tools.is_within(&ToolScope::Restricted))?;
        self.check_ceiling(|c| tools.is_within(&c.tools))?;
        self.tools = tools;
        Ok(self)
    }

    // ── #1248 Task 1: capability-semantic builders ──

    pub fn with_interaction_kind(
        mut self,
        interaction_kind: InteractionBindingMode,
    ) -> Result<Self, RunSpecError> {
        self.check_ceiling(|c| interaction_kind.is_within(&c.interaction_kind))?;
        self.interaction_kind = interaction_kind;
        Ok(self)
    }

    pub fn with_hooks(mut self, hooks: HookBindingMode) -> Result<Self, RunSpecError> {
        self.check_ceiling(|c| hooks.is_within(&c.hooks))?;
        self.hooks = hooks;
        Ok(self)
    }

    pub fn with_reasoning(mut self, reasoning: ReasoningBindingMode) -> Result<Self, RunSpecError> {
        self.check_ceiling(|c| reasoning.is_within(&c.reasoning))?;
        self.reasoning = reasoning;
        Ok(self)
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self, RunSpecError> {
        self.check_ceiling(|c| {
            if c.timeout.is_zero() {
                return true; // parent unlimited → child any
            }
            !timeout.is_zero() && timeout <= c.timeout
        })?;
        self.timeout = timeout;
        Ok(self)
    }

    // ── #1248 Task 1: read-only capability accessors ──
    //
    // Fields are private to prevent mutation that bypasses the ceiling check.
    // Use the `with_*` builders for mutation; use these accessors for
    // read-only inspection.

    /// Read-only access to the interaction binding mode.
    pub fn interaction_binding(&self) -> InteractionBindingMode {
        self.interaction_kind
    }

    /// Read-only access to the hook binding mode.
    pub fn hook_binding(&self) -> HookBindingMode {
        self.hooks
    }

    /// Read-only access to the reasoning binding mode.
    pub fn reasoning_binding(&self) -> ReasoningBindingMode {
        self.reasoning
    }

    /// Validate every effective capability against another spec used as a
    /// parent ceiling. This is the preparation-boundary guard for specs that
    /// were not produced through `derive_sub`.
    pub fn validate_against(&self, parent: &RunSpec) -> Result<(), RunSpecError> {
        let within_timeout =
            parent.timeout.is_zero() || (!self.timeout.is_zero() && self.timeout <= parent.timeout);
        if self.input.is_within(&parent.input)
            && self.interaction.is_within(&parent.interaction)
            && self.events.is_within(&parent.events)
            && self.context.is_within(&parent.context)
            && self.workspace.is_within(&parent.workspace)
            && self.memory.is_within(&parent.memory)
            && self.tools.is_within(&parent.tools)
            && within_timeout
            && self.interaction_kind.is_within(&parent.interaction_kind)
            && self.hooks.is_within(&parent.hooks)
            && self.reasoning.is_within(&parent.reasoning)
        {
            Ok(())
        } else {
            Err(RunSpecError::CapabilityEscalation)
        }
    }

    /// Restricted policy guard: rejects relaxing the legacy fixed-profile
    /// dimensions independently of run origin or parent presence.
    ///
    /// The capability-semantic dimensions (interaction binding, hooks and
    /// reasoning) remain independently configurable up to the parent ceiling.
    fn enforce_policy(&self, within_restricted: bool) -> Result<(), RunSpecError> {
        if self.policy == CapabilityPolicy::Restricted && !within_restricted {
            return Err(RunSpecError::CapabilityEscalation);
        }
        Ok(())
    }

    /// Check `pred` against the capability ceiling if one exists.
    /// Without a ceiling, the explicit capability policy is authoritative.
    fn check_ceiling(
        &self,
        pred: impl FnOnce(&CapabilityCeiling) -> bool,
    ) -> Result<(), RunSpecError> {
        match &self.ceiling {
            Some(ceiling) if !pred(ceiling) => Err(RunSpecError::CapabilityEscalation),
            _ => Ok(()),
        }
    }
}
