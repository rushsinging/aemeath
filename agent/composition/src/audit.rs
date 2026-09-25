use std::path::Path;
use std::sync::Arc;

use audit::{wire_audit_client, wire_audit_store, AuditClient};
use share::config::domain::snapshot::ConfigSnapshot;
use storage::SafeStorageRoot;

/// runtime 侧用量发送端口适配（UsageSink → AuditClient 写行为）。
pub struct AuditUsageSink {
    client: AuditClient,
}

impl AuditUsageSink {
    pub fn new(client: AuditClient) -> Self {
        Self { client }
    }
}

impl runtime::UsageSink for AuditUsageSink {
    fn try_record(&self, record: audit::UsageRecordData) -> audit::UsageEmitOutcomeData {
        self.client.try_record(record)
    }
}

/// 会话审计装配：sink + 生命周期句柄（读写合一 client）。
pub struct SessionAudit {
    sink: Arc<dyn runtime::UsageSink>,
    client: AuditClient,
}

impl SessionAudit {
    pub fn usage_sink(&self) -> Arc<dyn runtime::UsageSink> {
        Arc::clone(&self.sink)
    }

    pub fn client(&self) -> &AuditClient {
        &self.client
    }

    pub async fn shutdown(self) {
        self.client.shutdown().await;
    }
}

pub fn wire_session_audit(
    agents_dir: &Path,
    snapshot: &ConfigSnapshot,
) -> Result<SessionAudit, String> {
    let root =
        SafeStorageRoot::open(agents_dir.join("audit")).map_err(|error| error.to_string())?;
    let store = wire_audit_store(audit::append_store_for(root));
    let client = wire_audit_client(
        &store,
        snapshot.usage_worker_config().capacity(),
        snapshot.usage_worker_config().shutdown_timeout(),
    );
    let sink = Arc::new(AuditUsageSink::new(client.clone()));
    Ok(SessionAudit { sink, client })
}
