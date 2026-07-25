use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    Main,
    Sub,
}

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
struct CapsCeiling {
    input: InputMode,
    interaction: InteractionMode,
    events: EventRoute,
    context: ResourceMode,
    workspace: ResourceMode,
    memory: MemoryMode,
    tools: ToolScope,
    timeout: Duration,
}

impl CapsCeiling {
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    pub name: String,
    pub timeout: Duration,
    pub kind: RunKind,
    pub input: InputMode,
    pub interaction: InteractionMode,
    pub events: EventRoute,
    pub context: ResourceMode,
    pub workspace: ResourceMode,
    pub memory: MemoryMode,
    pub tools: ToolScope,
    /// When set (for sub specs via `derive_sub`), capability ceilings
    /// inherited from the parent. `with_*` methods validate against this.
    ceiling: Option<CapsCeiling>,
}

impl RunSpec {
    pub fn new(name: impl Into<String>, timeout: Duration) -> Self {
        let name = name.into();
        if timeout.is_zero() && name == "main" {
            return Self::main();
        }
        Self::sub(name, timeout)
    }

    pub fn main() -> Self {
        Self {
            name: "main".to_string(),
            timeout: Duration::ZERO,
            kind: RunKind::Main,
            input: InputMode::SessionQueue,
            interaction: InteractionMode::Interactive,
            events: EventRoute::Client,
            context: ResourceMode::Shared,
            workspace: ResourceMode::Shared,
            memory: MemoryMode::Enabled,
            tools: ToolScope::Full,
            ceiling: None,
        }
    }

    pub fn sub(name: impl Into<String>, timeout: Duration) -> Self {
        Self {
            name: name.into(),
            timeout,
            kind: RunKind::Sub,
            input: InputMode::Fixed,
            interaction: InteractionMode::NonInteractive,
            events: EventRoute::ParentRun,
            context: ResourceMode::Isolated,
            workspace: ResourceMode::Isolated,
            memory: MemoryMode::Disabled,
            tools: ToolScope::Restricted,
            ceiling: None,
        }
    }

    /// Create a sub-run spec derived from `self` (the parent).
    ///
    /// The child starts from Sub defaults (most restricted) and stores
    /// the parent's values as capability ceilings.  Use `with_*` methods
    /// to relax individual fields up to — but never beyond — the parent.
    pub fn derive_sub(
        &self,
        name: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, RunSpecError> {
        // Validate timeout first.
        if !self.timeout.is_zero() && (timeout.is_zero() || timeout > self.timeout) {
            return Err(RunSpecError::CapabilityEscalation);
        }

        let mut sub = Self::sub(name, timeout);

        // Validate every Sub-default field is within the parent's ceiling.
        // (Sub defaults are the most restrictive, so this always passes for
        // well-formed parents.  The check is defensive.)
        if !sub.input.is_within(&self.input)
            || !sub.interaction.is_within(&self.interaction)
            || !sub.events.is_within(&self.events)
            || !sub.context.is_within(&self.context)
            || !sub.workspace.is_within(&self.workspace)
            || !sub.memory.is_within(&self.memory)
            || !sub.tools.is_within(&self.tools)
        {
            return Err(RunSpecError::CapabilityEscalation);
        }

        // Store parent's current values as the ceiling.
        // If parent itself has a ceiling, we inherit the *effective* values
        // (which are already bounded), not the original ceiling.
        sub.ceiling = Some(CapsCeiling::from_spec(self));
        Ok(sub)
    }

    // ── with_* builders ───────────────────────────────────────────

    pub fn with_input(mut self, input: InputMode) -> Result<Self, RunSpecError> {
        self.enforce_sub_fixed(input.is_within(&InputMode::Fixed))?;
        self.check_ceiling(|c| input.is_within(&c.input))?;
        self.input = input;
        Ok(self)
    }

    pub fn with_interaction(mut self, interaction: InteractionMode) -> Result<Self, RunSpecError> {
        self.enforce_sub_fixed(interaction.is_within(&InteractionMode::NonInteractive))?;
        self.check_ceiling(|c| interaction.is_within(&c.interaction))?;
        self.interaction = interaction;
        Ok(self)
    }

    pub fn with_events(mut self, events: EventRoute) -> Result<Self, RunSpecError> {
        self.enforce_sub_fixed(events.is_within(&EventRoute::ParentRun))?;
        self.check_ceiling(|c| events.is_within(&c.events))?;
        self.events = events;
        Ok(self)
    }

    pub fn with_context(mut self, context: ResourceMode) -> Result<Self, RunSpecError> {
        self.enforce_sub_fixed(context.is_within(&ResourceMode::Isolated))?;
        self.check_ceiling(|c| context.is_within(&c.context))?;
        self.context = context;
        Ok(self)
    }

    pub fn with_workspace(mut self, workspace: ResourceMode) -> Result<Self, RunSpecError> {
        self.enforce_sub_fixed(workspace.is_within(&ResourceMode::Isolated))?;
        self.check_ceiling(|c| workspace.is_within(&c.workspace))?;
        self.workspace = workspace;
        Ok(self)
    }

    pub fn with_memory_mode(mut self, memory: MemoryMode) -> Result<Self, RunSpecError> {
        // Standalone sub (no ceiling): memory must stay Disabled.
        if self.kind == RunKind::Sub && self.ceiling.is_none() && memory == MemoryMode::Enabled {
            return Err(RunSpecError::CapabilityEscalation);
        }
        self.check_ceiling(|c| memory.is_within(&c.memory))?;
        self.memory = memory;
        Ok(self)
    }

    pub fn with_tool_scope(mut self, tools: ToolScope) -> Result<Self, RunSpecError> {
        self.enforce_sub_fixed(tools.is_within(&ToolScope::Restricted))?;
        self.check_ceiling(|c| tools.is_within(&c.tools))?;
        self.tools = tools;
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

    /// Fixed sub-profile guard: rejects relaxing any of the six immutable
    /// dimensions (input, interaction, events, context, workspace, tools).
    ///
    /// This check is defined once and is **orthogonal** to `check_ceiling` —
    /// both must pass independently.  It applies to *all* sub runs (standalone
    /// or derived), regardless of the parent capability ceiling.
    fn enforce_sub_fixed(&self, within_fixed: bool) -> Result<(), RunSpecError> {
        if self.kind == RunKind::Sub && !within_fixed {
            return Err(RunSpecError::CapabilityEscalation);
        }
        Ok(())
    }

    /// Check `pred` against the capability ceiling if one exists.
    /// Without a ceiling (Main spec or standalone `sub()`), everything is allowed.
    fn check_ceiling(&self, pred: impl FnOnce(&CapsCeiling) -> bool) -> Result<(), RunSpecError> {
        match &self.ceiling {
            Some(ceiling) if !pred(ceiling) => Err(RunSpecError::CapabilityEscalation),
            _ => Ok(()),
        }
    }
}
