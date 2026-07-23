//! 从 `process_chat_loop` 中提取的独立阶段处理函数。
//!
//! 这些函数不包含 `continue`/`break` 等跨循环控制流，
//! 可以安全地从 async 循环体中提取为独立函数。

use crate::application::chat::looping::config_reload::check_config_changes;
use crate::application::chat::looping::snapshot_registry::SourceSnapshotRegistry;
use crate::application::chat::looping::{ChatEventSink, RuntimeStreamEvent};
use config::{ConfigReader, ConfigRefreshOutcome};
use share::config::GuidanceReloadPolicy;
use share::message::Message;

/// Turn 边界配置变更检测与 guidance 注入。
///
/// 在每个 turn 开始时轮询配置/指令/guidance 文件是否有外部修改，
/// 检测到变更时通过 sink 发送 `ConfigReloaded` 事件，
/// 并按 `GuidanceReloadPolicy` 注入对应的提醒消息。
pub(crate) async fn handle_turn_boundary_config<S>(
    config_snapshot: &mut SourceSnapshotRegistry,
    config_reader: &dyn ConfigReader,
    turn_count: usize,
    sink: &S,
    messages: &mut Vec<Message>,
    language: &str,
    _segment_id: &str,
) -> ConfigRefreshOutcome
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
            sink.send_event(RuntimeStreamEvent::ConfigReloaded { changed_keys })
                .await;
            if scopes.contains(
                &share::config::domain::scope::ConfigApplicationScope::SessionRestartRequired,
            ) {
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
    if config_diff.has_changes() {
        log::info!(target: crate::LOG_TARGET,
            "[config_reload] turn {} detected changes: {:?}",
            turn_count,
            config_diff.changed_keys
        );
        // 通过 sink 发送 ConfigReloaded 事件通知客户端
        sink.send_event(RuntimeStreamEvent::ConfigReloaded {
            changed_keys: config_diff.changed_keys.clone(),
        })
        .await;

        // Guidance 变更处理：按 reload_policy 配置注入通知
        let has_guidance_change = config_diff
            .changed_keys
            .iter()
            .any(|k| k.starts_with("guidance:"));
        if has_guidance_change {
            let policy = config_reader.committed_snapshot().guidance_reload_policy();
            match policy {
                GuidanceReloadPolicy::Inject => {
                    // 在下一条消息前注入 guidance 更新提示
                    let reminder = Message::user(
                        "[guidance 已更新] guidance 文件已被外部修改，请关注后续 system prompt 中的最新指引。".to_string(),
                    );
                    messages.push(reminder);
                    sink.send_event(RuntimeStreamEvent::PostToolExecutionSync {
                        messages: messages.clone(),
                    })
                    .await;
                    log::info!(target: crate::LOG_TARGET, "[config_reload] guidance inject mode: injected reminder into messages");
                }
                GuidanceReloadPolicy::Remind => {
                    // 发 system-reminder 让 LLM 自行决定是否读取
                    let reminder_text = match language {
                        "zh" => "<system-reminder>guidance 文件已被外部修改，请用 Read 工具重新读取 ~/.agents/guidance/_default.md 与本次匹配的模型前缀文件以获取最新指引。</system-reminder>",
                        _ => "<system-reminder>The guidance files have been modified externally. Please use the Read tool to re-read ~/.agents/guidance/_default.md and the matching model prefix file to get the latest instructions.</system-reminder>",
                    };
                    let reminder = Message::user(reminder_text.to_string());
                    messages.push(reminder);
                    log::info!(target: crate::LOG_TARGET, "[config_reload] guidance remind mode: injected system-reminder");
                }
                GuidanceReloadPolicy::Confirm => {
                    // 发 system-reminder + 标记等待用户确认
                    let reminder_text = match language {
                        "zh" => "<system-reminder>guidance 文件已被外部修改，等待用户确认后应用。TUI 状态栏已标记 \"guidance 改动未应用\"。</system-reminder>",
                        _ => "<system-reminder>The guidance files have been modified externally and will be applied after user confirmation. The TUI status bar shows \"guidance changes pending\".</system-reminder>",
                    };
                    let reminder = Message::user(reminder_text.to_string());
                    messages.push(reminder);
                    sink.send_event(RuntimeStreamEvent::SystemMessage(
                        "[guidance] guidance 文件已变更，等待用户确认后应用".to_string(),
                    ))
                    .await;
                    log::info!(target: crate::LOG_TARGET, "[config_reload] guidance confirm mode: waiting for user confirmation");
                }
            }
        }
    }
    refresh
}
