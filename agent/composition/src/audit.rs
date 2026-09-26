use std::path::Path;
use std::sync::Arc;

use audit::{wire_audit_client, wire_audit_store, AuditReader, AuditWriter};
use share::config::domain::snapshot::ConfigSnapshot;
use storage::SafeStorageRoot;

/// runtime 侧用量发送端口适配（UsageSink → AuditWriter 写行为）。
pub struct AuditUsageSink {
    writer: AuditWriter,
}

impl AuditUsageSink {
    pub fn new(writer: AuditWriter) -> Self {
        Self { writer }
    }
}

impl runtime::UsageSink for AuditUsageSink {
    fn try_record(&self, record: audit::UsageRecordData) -> audit::UsageEmitOutcomeData {
        self.writer.try_record(record)
    }
}

/// 会话审计装配：sink（写）+ reader（读）+ 生命周期句柄。
pub struct SessionAudit {
    sink: Arc<dyn runtime::UsageSink>,
    writer: AuditWriter,
    reader: AuditReader,
}

impl SessionAudit {
    pub fn usage_sink(&self) -> Arc<dyn runtime::UsageSink> {
        Arc::clone(&self.sink)
    }

    pub fn reader(&self) -> &AuditReader {
        &self.reader
    }

    pub async fn shutdown(self) {
        self.writer.shutdown().await;
    }
}

pub fn wire_session_audit(
    agents_dir: &Path,
    snapshot: &ConfigSnapshot,
) -> Result<SessionAudit, String> {
    let root =
        SafeStorageRoot::open(agents_dir.join("audit")).map_err(|error| error.to_string())?;
    let store = wire_audit_store(audit::wire_append_store_for(root));
    let (writer, reader) = wire_audit_client(
        &store,
        snapshot.usage_worker_config().capacity(),
        snapshot.usage_worker_config().shutdown_timeout(),
    );
    let sink = Arc::new(AuditUsageSink::new(writer.clone()));
    Ok(SessionAudit {
        sink,
        writer,
        reader,
    })
}
