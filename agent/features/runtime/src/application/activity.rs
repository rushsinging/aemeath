//! Runtime-owned Activity observation model and coordinator.

mod coordinator;
mod model;

#[cfg(test)]
#[path = "activity/coordinator_tests.rs"]
mod coordinator_tests;

pub(crate) use coordinator::ActivityCoordinator;
pub(crate) use model::*;
