use std::path::Path;

use audit::{
    file_usage_append_store, start_usage_worker, UsageSender, UsageWorkerConfig, UsageWorkerHandle,
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
    fn try_record(&self, record: audit::UsageRecord) -> audit::UsageEmitOutcome {
        self.sender.try_record(record)
    }
}

pub struct AuditWorkerAssembly {
    pub sender: UsageSender,
    pub handle: UsageWorkerHandle,
}

pub struct SessionAudit {
    sink: std::sync::Arc<dyn runtime::UsageSink>,
    handle: UsageWorkerHandle,
}

impl SessionAudit {
    pub fn usage_sink(&self) -> std::sync::Arc<dyn runtime::UsageSink> {
        std::sync::Arc::clone(&self.sink)
    }

    pub async fn shutdown(&self) -> audit::UsageShutdownOutcome {
        self.handle.shutdown().await
    }
}

pub fn wire_session_audit(
    agents_dir: &Path,
    snapshot: &ConfigSnapshot,
) -> Result<SessionAudit, String> {
    let assembly = wire_audit_worker(agents_dir, snapshot)?;
    let sink = std::sync::Arc::new(AuditUsageSink::new(assembly.sender));
    Ok(SessionAudit {
        sink,
        handle: assembly.handle,
    })
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
