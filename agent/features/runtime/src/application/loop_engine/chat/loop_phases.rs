//! 从 Main session command driver 中提取的独立阶段处理函数。
//!
//! 这些函数不包含 `continue`/`break` 等跨循环控制流，
//! 可以安全地从 async 循环体中提取为独立函数。

use crate::application::loop_engine::chat::config_reload::check_config_changes;
use crate::application::loop_engine::chat::snapshot_registry::SourceSnapshotRegistry;
use crate::application::loop_engine::chat::{ChatEventSink, RuntimeStreamEvent};
use config::{ConfigReader, ConfigRefreshOutcome};

/// Turn 边界配置与 Prompt source 变更检测结果。
pub(crate) struct TurnBoundaryConfigOutcome {
    pub refresh: ConfigRefreshOutcome,
    pub guidance_sources_changed: bool,
}

/// Turn 边界配置变更检测与 diagnostic 通知。
///
/// Provider-visible reminder 只返回 typed fact，由后续 ContextRequest 携带；
/// 此函数 **NEVER** 修改 Run message state。
pub(crate) async fn handle_turn_boundary_config<S>(
    config_snapshot: &mut SourceSnapshotRegistry,
    config_reader: &dyn ConfigReader,
    session_wiring: &context::MainSessionWiring,
    step_count: usize,
    sink: &S,
    language: &str,
    _segment_id: &str,
) -> TurnBoundaryConfigOutcome
where
    S: ChatEventSink,
{
    let refresh = config_reader.refresh_if_sources_changed().await;
    match &refresh {
        ConfigRefreshOutcome::Unchanged => {}
        ConfigRefreshOutcome::Reloaded { scopes, .. } => {
            let mut changed_keys = vec!["config:reloaded".to_string()];
            changed_keys.extend(
                scopes
                    .iter()
                    .map(|scope| format!("config:scope:{}", scope.as_str())),
            );
            sink.send_event(RuntimeStreamEvent::ConfigReloaded {
                changed_keys,
                view: crate::application::client::config_snapshot_to_sdk(
                    &config_reader.committed_snapshot(),
                ),
            })
            .await;
            if scopes.contains(
                &share::config::domain::scope::ConfigApplicationScope::SessionRestartRequired,
            ) {
                let revision = config_reader.committed_snapshot().revision();
                session_wiring.mark_session_restart_required(revision);
                let message = match language {
                    "zh" => "[config] 部分配置将在重启 Session 后生效。当前 Session 继续使用既有基础设施。",
                    _ => "[config] Some configuration changes take effect after restarting the session. The current session keeps its existing infrastructure.",
                };
                sink.send_event(RuntimeStreamEvent::SystemMessage(message.to_string()))
                    .await;
            }
        }
        ConfigRefreshOutcome::Rejected { error } => {
            sink.send_event(RuntimeStreamEvent::SystemMessage(format!(
                "[config] 配置重载失败，继续使用已提交配置：{error:?}"
            )))
            .await;
        }
    }

    let config_diff = check_config_changes(config_snapshot);
    let guidance_sources_changed = config_diff
        .changed_keys
        .iter()
        .any(|key| key.starts_with("guidance:") || key.starts_with("instruction:"));
    if config_diff.has_changes() {
        log::info!(target: crate::LOG_TARGET,
            "[config_reload] run step {} detected changes: {:?}",
            step_count,
            config_diff.changed_keys
        );
        // 通过 sink 发送 ConfigReloaded 事件通知客户端
        sink.send_event(RuntimeStreamEvent::ConfigReloaded {
            changed_keys: config_diff.changed_keys.clone(),
            view: crate::application::client::config_snapshot_to_sdk(
                &config_reader.committed_snapshot(),
            ),
        })
        .await;
    }
    TurnBoundaryConfigOutcome {
        refresh,
        guidance_sources_changed,
    }
}
