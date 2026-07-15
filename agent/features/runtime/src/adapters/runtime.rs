//! Port trait adapters for concrete production types.
//!
//! Task 10 migration note: adapter type definitions moved to `share::adapter`.
//! These runtime-local impls remain here temporarily because `share` must not
//! depend on upstream runtime/provider/hook crates.

pub use share::adapter::hook::HookRunnerAdapter;
pub use share::adapter::provider::LlmClientAdapter;

use crate::ports::legacy::{HookNotificationPort, ProviderInfoPort, TaskStorePort};
use std::collections::HashMap;
use storage::{Task, TaskSnapshot, TaskStore};

impl ProviderInfoPort for LlmClientAdapter<provider::api::LlmClient> {
    fn provider_name(&self) -> &str {
        self.0.provider_name()
    }

    fn model_name(&self) -> &str {
        self.0.model_name()
    }

    fn current_reasoning_level(&self) -> provider::contract::ReasoningLevel {
        self.0.current_reasoning_level()
    }

    fn set_reasoning_level(&self, level: provider::contract::ReasoningLevel) {
        self.0.set_reasoning_level(level)
    }
}

#[async_trait::async_trait]
impl HookNotificationPort for HookRunnerAdapter<hook::api::HookRunner> {
    async fn on_notification(&self, message: &str, kind: &str, workspace_root: &std::path::Path) {
        let _ = self.0.on_notification(message, kind, workspace_root).await;
    }
}

// TaskStore 在 share（共享内核）中，runtime 可直接为其实现 port trait
#[async_trait::async_trait]
impl TaskStorePort for TaskStore {
    async fn snapshot(&self) -> TaskSnapshot {
        TaskStore::snapshot(self).await
    }

    async fn restore(&self, snapshot: TaskSnapshot) {
        TaskStore::restore(self, snapshot).await
    }

    async fn list_current_batch(&self) -> Vec<Task> {
        TaskStore::list_current_batch(self).await
    }

    async fn get_batch_display_map(&self) -> HashMap<String, usize> {
        TaskStore::get_batch_display_map(self).await
    }
}
