//! Port trait 适配器——为 runtime 内部 port trait 提供对具体类型的实现。
//!
//! P16 目标：core/ 层不直接依赖 provider/hook 等外部 crate 的具体类型方法。
//! 适配器封装这些调用，使 core/ 层只需依赖 port trait。

mod hook_adapter;
mod provider_adapter;

pub use hook_adapter::HookRunnerAdapter;
pub use provider_adapter::LlmClientAdapter;

use crate::core::port::TaskStorePort;
use std::collections::HashMap;
use storage::api::{Task, TaskSnapshot, TaskStore};

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
