//! Session metadata 单字段 best-effort 持久化（#2032）。
//!
//! `model` 与 `reasoning_effort` 两条持久化路径共用同一「load 比对去重 +
//! update」内核与同一「活跃会话优先」on_wiring 分派，NEVER 各自复制分派逻辑。

use context::session::SessionMetadata;
use share::session_types::ProjectIdentityData;

/// 支持单字段持久化的 metadata 字段（读取去重 + update payload 构造）。
#[derive(Clone, Copy)]
pub(crate) enum SessionMetadataField {
    Model,
    ReasoningEffort,
}

impl SessionMetadataField {
    fn name(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::ReasoningEffort => "reasoning_effort",
        }
    }

    fn read_current(self, metadata: &SessionMetadata) -> Option<String> {
        match self {
            Self::Model => metadata.model.clone(),
            Self::ReasoningEffort => metadata.reasoning_effort.clone(),
        }
    }

    /// overwrite 语义（SessionMetadataUpdateData 已按拍板合并进 SessionMetadata）：
    /// 在已加载的完整 metadata 上应用单字段，再整体写回。
    fn apply(self, metadata: &mut SessionMetadata, value: String) {
        match self {
            Self::Model => metadata.model = Some(value),
            Self::ReasoningEffort => metadata.reasoning_effort = Some(value),
        }
    }
}

/// Best-effort 单字段写：值与磁盘相同则跳过；失败 log warn 不阻断 chat。
pub(crate) async fn persist_metadata_field(
    session_management: &dyn context::SessionManagementPort,
    session_id: &str,
    project: &ProjectIdentityData,
    field: SessionMetadataField,
    value: &str,
) {
    let field_name = field.name();
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    // overwrite 语义需要完整 metadata：load 失败则跳过本次写（不构造半量载荷）。
    let mut metadata = match session_management
        .load_for_project(session_id, project)
        .await
    {
        Ok(session) => {
            if field.read_current(&session.metadata).as_deref() == Some(value) {
                return;
            }
            session.metadata.clone()
        }
        Err(error) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "persist session {field_name} load failed (skip write): session={session_id} error={error}"
            );
            return;
        }
    };
    field.apply(&mut metadata, value.to_string());
    if let Err(error) = session_management
        .update_metadata_for_project(session_id, project, metadata)
        .await
    {
        log::warn!(
            target: crate::LOG_TARGET,
            "persist session {field_name} failed: session={session_id} value={value} error={error}"
        );
    }
}

/// 活跃会话优先分派：`session_id` 是 coordinator 当前 committed 会话时走
/// `update_active_session_metadata`（mutation_gate + disk + committed revision
/// 同步）；否则 disk-only 落盘（含 load 比对去重）。
pub(crate) async fn persist_metadata_field_on_coordinator(
    coordinator: &crate::SessionSwitchCoordinator,
    session_management: &dyn context::SessionManagementPort,
    session_id: &str,
    field: SessionMetadataField,
    value: &str,
) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    if coordinator.committed_session().id == session_id {
        let mut metadata = coordinator.committed_session().metadata.clone();
        field.apply(&mut metadata, value.to_string());
        if let Err(error) = coordinator.update_active_session_metadata(metadata).await {
            log::warn!(
                target: crate::LOG_TARGET,
                "persist session {} (active sync) failed: session={session_id} value={value} error={error}",
                field.name()
            );
        }
        return;
    }
    persist_metadata_field(
        session_management,
        session_id,
        &coordinator.project_identity(),
        field,
        value,
    )
    .await;
}

#[cfg(test)]
#[path = "session_metadata_field_tests.rs"]
mod tests;
