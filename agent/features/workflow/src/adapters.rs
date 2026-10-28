//! Workflow BC adapters：文件系统持久化（#2083）。

mod file_persistence;

pub use file_persistence::FileWorkflowPersistence;

#[cfg(test)]
mod file_persistence_tests;
