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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionMode {
    Interactive,
    NonInteractive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventRoute {
    Client,
    ParentRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceMode {
    Shared,
    Isolated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolScope {
    Full,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunSpecError {
    #[error("子 Run 能力不得超过父 Run")]
    CapabilityEscalation,
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
    pub tools: ToolScope,
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
            tools: ToolScope::Full,
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
            tools: ToolScope::Restricted,
        }
    }

    pub fn derive_sub(
        &self,
        name: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, RunSpecError> {
        let _ = self;
        Ok(Self::sub(name, timeout))
    }

    pub fn with_tool_scope(mut self, tools: ToolScope) -> Result<Self, RunSpecError> {
        if self.kind == RunKind::Sub && tools == ToolScope::Full {
            return Err(RunSpecError::CapabilityEscalation);
        }
        self.tools = tools;
        Ok(self)
    }
}
