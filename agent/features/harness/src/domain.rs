//! Harness 组件树领域模型。

pub mod changeset;
#[cfg(test)]
mod changeset_tests;
pub mod component;
#[cfg(test)]
mod component_tests;
pub mod daemon_protocol;
#[cfg(test)]
mod daemon_protocol_tests;
pub mod errors;
pub mod task;
#[cfg(test)]
mod task_tests;
#[cfg(test)]
pub(crate) mod test_support;
