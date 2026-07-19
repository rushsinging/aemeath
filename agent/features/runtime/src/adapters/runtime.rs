//! Port trait adapters for concrete production types.
//!
//! Task 10 migration note: adapter type definitions moved to `share::adapter`.
//! These runtime-local impls remain here temporarily because `share` must not
//! depend on upstream runtime/provider/hook crates.

pub use share::adapter::hook::HookRunnerAdapter;
pub use share::adapter::provider::LlmClientAdapter;

use crate::ports::legacy::{HookNotificationPort, ProviderInfoPort};

impl ProviderInfoPort for LlmClientAdapter<provider::LlmClient> {
    fn provider_name(&self) -> &str {
        self.0.provider_name()
    }

    fn model_name(&self) -> &str {
        self.0.model_name()
    }
}

#[async_trait::async_trait]
impl HookNotificationPort for HookRunnerAdapter<hook::api::HookRunner> {
    async fn on_notification(&self, message: &str, kind: &str, workspace_root: &std::path::Path) {
        let _ = self.0.on_notification(message, kind, workspace_root).await;
    }
}
