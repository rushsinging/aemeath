//! 从 Main session command driver 中提取的独立阶段处理函数。
//!
//! 这些函数不包含 `continue`/`break` 等跨循环控制流，
//! 可以安全地从 async 循环体中提取为独立函数。

use crate::application::loop_engine::chat::config_reload::check_config_changes;
use crate::application::loop_engine::chat::snapshot_registry::SourceSnapshotRegistry;
use crate::application::loop_engine::chat::{ChatEventSink, RuntimeStreamEvent};
use config::{ConfigReader, ConfigRefreshOutcomeData};

/// Turn 边界配置与 Prompt source 变更检测结果。
pub(crate) struct TurnBoundaryConfigOutcome {
    pub refresh: Result<ConfigRefreshOutcomeData, share::error::DomainError>,
    /// 变更的 guidance / instruction 文件路径（空 = 无变化）。
    pub guidance_changed_paths: Vec<String>,
    /// 防御复检被拒的组件变更（#2015 三段式 validate 段产出；
    /// 被拒条目保留旧版，注入侧排除由各组件装配接入）。
    pub component_rejections:
        Vec<crate::application::loop_engine::chat::config_reload::ComponentReloadRejection>,
}

/// Turn 边界配置变更检测与 diagnostic 通知。
///
/// Provider-visible reminder 只返回 typed fact，由后续 ContextRequestData 携带；
/// 此函数 **NEVER** 修改 Run message state。
pub(crate) async fn handle_turn_boundary_config<S>(
    config_snapshot: &mut SourceSnapshotRegistry,
    config_reader: &dyn ConfigReader,
    session_switch: &crate::SessionSwitchCoordinator,
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
        Ok(ConfigRefreshOutcomeData::Unchanged) => {}
        Ok(ConfigRefreshOutcomeData::Reloaded { scopes, .. }) => {
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
            // 用户可见回显由 runtime 生成（language 在此可用）；只报「已重载 +
            // 生效边界」，不做字段级 diff。
            sink.send_event(RuntimeStreamEvent::SystemMessage(
                share::i18n::runtime::config_reload::config_reload_notice(language, scopes),
            ))
            .await;
            if scopes.contains(
                &share::config::domain::scope::ConfigApplicationScope::SessionRestartRequired,
            ) {
                let revision = config_reader.committed_snapshot().revision();
                session_switch.mark_session_restart_required(revision);
            }
        }
        // 原 `Rejected` 分支等价行为：仅发一条降级 SystemMessage 通知，
        // 继续使用已提交配置（绝不 `?` 早退、不改变 turn 流程）。
        Err(error) => {
            sink.send_event(RuntimeStreamEvent::SystemMessage(format!(
                "[config] 配置重载失败，继续使用已提交配置：{error}"
            )))
            .await;
        }
    }

    let config_diff = check_config_changes(config_snapshot);
    // #2015 三段式 detect→validate→生效：组件类文件变更先过防御复检，
    // 被拒条目保留旧版（NEVER 带病生效），从对外事件中剔除并 warn。
    let component_rejections =
        crate::application::loop_engine::chat::config_reload::validate_component_changes(
            &config_diff.changes,
        );
    let rejected_paths: Vec<String> = component_rejections
        .iter()
        .map(|rejection| rejection.path.display().to_string())
        .collect();
    for rejection in &component_rejections {
        log::warn!(target: crate::LOG_TARGET,
            "[config_reload] 组件变更校验失败，保留旧版：{}（{}）",
            rejection.path.display(),
            rejection.reason
        );
    }
    // changed_keys 与 changes 同序（classify_change_key 逐条映射），zip 过滤
    // guidance / instruction 前缀并保留完整路径（Read 引导目标）；
    // 被拒路径的 key 一并剔除。
    let guidance_changed_paths = config_diff
        .changes
        .iter()
        .zip(config_diff.changed_keys.iter())
        .filter(|(change, key)| {
            (key.starts_with("guidance:") || key.starts_with("instruction:"))
                && !rejected_paths.contains(&change.path.display().to_string())
        })
        .map(|(change, _)| change.path.display().to_string())
        .collect::<Vec<_>>();
    if config_diff.has_changes() {
        log::info!(target: crate::LOG_TARGET,
            "[config_reload] run step {} detected changes: {:?}",
            step_count,
            config_diff.changed_keys
        );
        // 通过 sink 发送 ConfigReloaded 事件通知客户端（被拒条目剔除后）
        let effective_keys: Vec<String> = config_diff
            .changed_keys
            .iter()
            .filter(|key| {
                !component_rejections.iter().any(|rejection| {
                    rejection
                        .path
                        .file_name()
                        .is_some_and(|name| key.ends_with(name.to_string_lossy().as_ref()))
                })
            })
            .cloned()
            .collect();
        if !effective_keys.is_empty() {
            sink.send_event(RuntimeStreamEvent::ConfigReloaded {
                changed_keys: effective_keys.clone(),
                view: crate::application::client::config_snapshot_to_sdk(
                    &config_reader.committed_snapshot(),
                ),
            })
            .await;
            // 组件热载（guidance / instructions / skills）用户可见回显：
            // 按来源分组只报文件数，NEVER 把组件变更标成 [config]。
            for notice in
                crate::application::loop_engine::chat::config_reload::component_reload_notices(
                    language,
                    &effective_keys,
                )
            {
                sink.send_event(RuntimeStreamEvent::SystemMessage(notice))
                    .await;
            }
        }
    }
    TurnBoundaryConfigOutcome {
        refresh,
        guidance_changed_paths,
        component_rejections,
    }
}
