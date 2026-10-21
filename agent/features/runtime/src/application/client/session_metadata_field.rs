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

    fn build_update(self, value: String) -> context::SessionMetadataUpdateData {
        match self {
            Self::Model => context::SessionMetadataUpdateData {
                model: Some(value),
                ..Default::default()
            },
            Self::ReasoningEffort => context::SessionMetadataUpdateData {
                reasoning_effort: Some(value),
                ..Default::default()
            },
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
    match session_management
        .load_for_project(session_id, project)
        .await
    {
        Ok(session) => {
            if field.read_current(&session.metadata).as_deref() == Some(value) {
                return;
            }
        }
        Err(error) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "persist session {field_name} load failed (continuing write): session={session_id} error={error}"
            );
        }
    }
    if let Err(error) = session_management
        .update_metadata_for_project(session_id, project, field.build_update(value.to_string()))
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
        if let Err(error) = coordinator
            .update_active_session_metadata(field.build_update(value.to_string()))
            .await
        {
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
