use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CancelRunOutcome {
    Accepted,
    AlreadyCancelling,
    AlreadyTerminal,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CancelRunStepOutcome {
    Accepted,
    AlreadyCancelling,
    NoActiveStep,
    RunTerminating,
    RunTerminal,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TerminateRunOutcome {
    Accepted,
    AlreadyTerminating,
    AlreadyTerminal,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum RunTerminationReason {
    UserExit,
    DoubleCtrlC,
    QuitCommand,
    ProcessSignal,
    SessionShutdown,
    ParentStepCancelled,
}

/// Absolute wall-clock deadline used only as wire data.
///
/// Runtime converts this value to its injected monotonic clock at the control boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlDeadline {
    unix_millis: u64,
}

impl ControlDeadline {
    pub const fn from_unix_millis(unix_millis: u64) -> Self {
        Self { unix_millis }
    }

    pub const fn unix_millis(self) -> u64 {
        self.unix_millis
    }
}
