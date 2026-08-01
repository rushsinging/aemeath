//! Runtime-owned Activity observation model and coordinator.

mod coordinator;
#[allow(dead_code)]
mod model;
mod run_events;

#[cfg(test)]
#[path = "activity/coordinator_tests.rs"]
mod coordinator_tests;

#[cfg(test)]
#[path = "activity/run_events_tests.rs"]
mod run_events_tests;

pub(crate) use coordinator::ActivityCoordinator;
