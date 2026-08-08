use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

use audit::{
    file_usage_append_store, start_usage_worker, UsageEmitOutcome, UsageRecord, UsageSender,
    UsageWorkerConfig, UsageWorkerHandle,
};
use share::config::domain::snapshot::ConfigSnapshot;
use storage::SafeStorageRoot;

pub fn usage_worker_config_from_snapshot(snapshot: &ConfigSnapshot) -> UsageWorkerConfig {
    snapshot.usage_worker_config().into()
}

pub struct AuditUsageSink {
    sender: UsageSender,
}

impl AuditUsageSink {
    pub fn new(sender: UsageSender) -> Self {
        Self { sender }
    }
}

impl runtime::UsageSink for AuditUsageSink {
    fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome {
        self.sender.try_record(record)
    }
}

pub struct AuditWorkerAssembly {
    pub sender: UsageSender,
    pub handle: UsageWorkerHandle,
}

#[derive(Clone)]
pub struct AuditClientLifecycle {
    handle: Arc<UsageWorkerHandle>,
}

impl AuditClientLifecycle {
    pub fn new(handle: UsageWorkerHandle) -> Self {
        Self {
            handle: Arc::new(handle),
        }
    }
}

#[async_trait]
impl sdk::ClientLifecycle for AuditClientLifecycle {
    async fn shutdown(&self) -> sdk::ClientShutdownOutcome {
        match self.handle.shutdown().await {
            audit::UsageShutdownOutcome::Drained => sdk::ClientShutdownOutcome::Drained,
            audit::UsageShutdownOutcome::TimedOut { unconfirmed } => {
                sdk::ClientShutdownOutcome::TimedOut { unconfirmed }
            }
        }
    }
}

pub fn wire_audit_worker(
    agents_dir: &Path,
    snapshot: &ConfigSnapshot,
) -> Result<AuditWorkerAssembly, String> {
    let root =
        SafeStorageRoot::open(agents_dir.join("audit")).map_err(|error| error.to_string())?;
    let store = std::sync::Arc::new(file_usage_append_store(root));
    let (sender, handle) = start_usage_worker(store, usage_worker_config_from_snapshot(snapshot));
    Ok(AuditWorkerAssembly { sender, handle })
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod tests;
