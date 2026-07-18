use std::path::Path;

use audit::{
    file_usage_append_store, start_usage_worker, UsageSender, UsageWorkerConfig, UsageWorkerHandle,
};
use share::config::domain::snapshot::ConfigSnapshot;
use storage::SafeStorageRoot;

pub fn usage_worker_config_from_snapshot(snapshot: &ConfigSnapshot) -> UsageWorkerConfig {
    snapshot.usage_worker_config().into()
}

pub struct AuditWorkerAssembly {
    pub sender: UsageSender,
    pub handle: UsageWorkerHandle,
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
