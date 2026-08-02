//! Runtime-owned Activity observation model and coordinator.

mod coordinator;
#[allow(dead_code)]
mod model;
mod model_tool;
mod run_events;
mod runtime_work;

#[cfg(test)]
#[path = "activity/coordinator_tests.rs"]
mod coordinator_tests;

#[cfg(test)]
#[path = "activity/model_tool_tests.rs"]
mod model_tool_tests;

#[cfg(test)]
#[path = "activity/run_events_tests.rs"]
mod run_events_tests;

pub(crate) use coordinator::{
    ActivityChangePublisher, ActivityCoordinator, ActivityError, ActivityTerminal, StartActivity,
    UpdateActivity,
};
#[cfg(test)]
pub(crate) use coordinator::{SystemActivityClock, UuidV7ActivityIdSource};
pub(crate) use model::{ActivityDetail, ActivityKind, ActivitySource};
