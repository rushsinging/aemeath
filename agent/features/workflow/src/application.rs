//! Workflow Application 层（#2085）：聚合状态机、Session 内存储与 Tool 端口实现。

mod aggregate;
mod backend;
mod store;

#[cfg(test)]
mod aggregate_tests;

#[cfg(test)]
mod advance_tests;

#[cfg(test)]
mod backend_tests;

#[cfg(test)]
mod replan_tests;

#[cfg(test)]
mod acceptance_tests;

#[cfg(test)]
mod persistence_tests;

pub use aggregate::WorkflowAggregate;
pub use backend::InMemoryWorkflowBackend;
pub use store::InMemoryWorkflowStore;
