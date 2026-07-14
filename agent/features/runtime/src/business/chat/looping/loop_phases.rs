//! 从 `process_chat_loop` 中提取的独立阶段处理函数。
//!
//! 这些函数不包含 `continue`/`break` 等跨循环控制流，
//! 可以安全地从 async 循环体中提取为独立函数。

use crate::business::chat::looping::config_reload::{
    check_config_changes, resolve_guidance_reload_policy,
};
use crate::business::chat::looping::snapshot_registry::SourceSnapshotRegistry;
use crate::business::chat::looping::task_reminder::TaskReminderState;
use crate::business::chat::looping::{ChatEventSink, RuntimeStreamEvent};
use crate::LOG_TARGET;
use share::config::GuidanceReloadPolicy;
use share::message::Message;

/// Turn 边界配置变更检测与 guidance 注入。
///
/// 在每个 turn 开始时轮询配置/指令/guidance 文件是否有外部修改，
/// 检测到变更时通过 sink 发送 `ConfigReloaded` 事件，
/// 并按 `GuidanceReloadPolicy` 注入对应的提醒消息。
pub(crate) async fn handle_turn_boundary_config<S>(
    config_snapshot: &mut SourceSnapshotRegistry,
    turn_count: usize,
    sink: &S,
    chain: &mut context::session::ChatChain,
    language: &str,
    segment_id: &str,
) where
    S: ChatEventSink,
{
    let config_diff = check_config_changes(config_snapshot);
    if config_diff.has_changes() {
        log::info!(target: LOG_TARGET,
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
            let policy = resolve_guidance_reload_policy();
            match policy {
                GuidanceReloadPolicy::Inject => {
                    // 在下一条消息前注入 guidance 更新提示
                    let reminder = Message::user(
                        "[guidance 已更新] guidance 文件已被外部修改，请关注后续 system prompt 中的最新指引。".to_string(),
                    );
                    chain.push(reminder, segment_id);
                    sink.send_event(RuntimeStreamEvent::PostToolExecutionSync {
                        messages: chain.messages_flat(),
                    })
                    .await;
                    log::info!(target: LOG_TARGET, "[config_reload] guidance inject mode: injected reminder into messages");
                }
                GuidanceReloadPolicy::Remind => {
                    // 发 system-reminder 让 LLM 自行决定是否读取
                    let reminder_text = match language {
                        "zh" => "<system-reminder>guidance 文件已被外部修改，请用 Read 工具重新读取 ~/.agents/guidance/_default.md 与本次匹配的模型前缀文件以获取最新指引。</system-reminder>",
                        _ => "<system-reminder>The guidance files have been modified externally. Please use the Read tool to re-read ~/.agents/guidance/_default.md and the matching model prefix file to get the latest instructions.</system-reminder>",
                    };
                    let reminder = Message::user(reminder_text.to_string());
                    chain.push(reminder, segment_id);
                    log::info!(target: LOG_TARGET, "[config_reload] guidance remind mode: injected system-reminder");
                }
                GuidanceReloadPolicy::Confirm => {
                    // 发 system-reminder + 标记等待用户确认
                    let reminder_text = match language {
                        "zh" => "<system-reminder>guidance 文件已被外部修改，等待用户确认后应用。TUI 状态栏已标记 \"guidance 改动未应用\"。</system-reminder>",
                        _ => "<system-reminder>The guidance files have been modified externally and will be applied after user confirmation. The TUI status bar shows \"guidance changes pending\".</system-reminder>",
                    };
                    let reminder = Message::user(reminder_text.to_string());
                    chain.push(reminder, segment_id);
                    sink.send_event(RuntimeStreamEvent::SystemMessage(
                        "[guidance] guidance 文件已变更，等待用户确认后应用".to_string(),
                    ))
                    .await;
                    log::info!(target: LOG_TARGET, "[config_reload] guidance confirm mode: waiting for user confirmation");
                }
            }
        }
    }
}

/// 构建发送给 LLM API 的消息列表。
///
/// 在用户消息前注入 `user_context`（如 claudeMd）包装和任务提醒（如满足条件）。
pub(crate) async fn build_api_messages(
    user_context: &str,
    language: &str,
    task_reminder_state: &mut TaskReminderState,
    turn_count: u64,
    task_store: &storage::api::TaskStore,
    messages: &[Message],
) -> Vec<Message> {
    let mut api_msgs = Vec::new();
    if !user_context.is_empty() {
        let context_wrapper = match language {
            "zh" => format!(
                "<system-reminder>\n在回答用户问题时，你可以使用以下上下文：\n# claudeMd\n{user_context}\n\nIMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>"
            ),
            _ => format!(
                "<system-reminder>\nAs you answer the user's questions, you can use the following context:\n# claudeMd\n{user_context}\n\nIMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>"
            ),
        };
        api_msgs.push(Message::user(context_wrapper));
    }
    // Inject task reminder if conditions are met
    if let Some(reminder) = task_reminder_state
        .build_reminder(turn_count, task_store, language)
        .await
    {
        api_msgs.push(reminder);
    }
    // text-first：仅对发送给 LLM 的克隆做转换，tool_result 降为文本、剥离结构化
    // data；持久化的 `messages` 保持忠实（结构化 content 供 TUI / server）。
    api_msgs.extend(messages.iter().map(Message::to_llm_view));
    api_msgs
}

#[cfg(test)]
mod tests {
    use super::build_api_messages;
    use crate::business::chat::looping::task_reminder::TaskReminderState;
    use share::message::{ContentBlock, Message, Role};

    #[tokio::test]
    async fn test_build_api_messages_tool_result_is_text_first() {
        // 持久化的 tool_result 带结构化 content + text；发送给 LLM 时应降为 text-first。
        let messages = vec![Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: serde_json::json!({"stdout": "hello", "exit_code": 0}),
                is_error: false,
                text: Some("hello".to_string()),
            }],
            metadata: None,
        }];
        let store = storage::api::TaskStore::new();
        let mut reminder = TaskReminderState::new();
        let out = build_api_messages("", "en", &mut reminder, 1, &store, &messages).await;

        let block = out
            .iter()
            .flat_map(|m| &m.content)
            .find(|b| b.is_tool_result())
            .expect("tool_result present");
        let json = serde_json::to_value(block).unwrap();
        assert_eq!(
            json["content"],
            serde_json::json!("hello"),
            "LLM 应收到 text-first 文本而非结构化 data: {json}"
        );
        assert!(json.get("text").is_none(), "wire 不应带 text 字段: {json}");
    }

    #[tokio::test]
    async fn test_build_api_messages_legacy_tool_result_without_text_unchanged() {
        // 向后兼容：旧 session 的 tool_result 无 text → 原样保留结构化 content。
        let messages = vec![Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: serde_json::json!({"stdout": "x"}),
                is_error: false,
                text: None,
            }],
            metadata: None,
        }];
        let store = storage::api::TaskStore::new();
        let mut reminder = TaskReminderState::new();
        let out = build_api_messages("", "en", &mut reminder, 1, &store, &messages).await;
        let block = out
            .iter()
            .flat_map(|m| &m.content)
            .find(|b| b.is_tool_result())
            .expect("tool_result present");
        let json = serde_json::to_value(block).unwrap();
        assert_eq!(json["content"], serde_json::json!({"stdout": "x"}));
    }
}
