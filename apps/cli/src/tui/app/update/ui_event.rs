use super::UpdateResult;
use crate::tui::adapter::hook_notice::{hook_event_notice, hook_spinner_phase};
use crate::tui::app::{App, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime::spinner::SpinnerPhase;
use crate::tui::model::runtime::status_notice::StatusNotice;
use tokio::sync::mpsc;

impl App {
    /// Handle UI events from background processing
    pub(super) fn update_ui(
        &mut self,
        ev: UiEvent,
        ui_tx: &mpsc::Sender<UiEvent>,
        _spawn_refs: &SpawnContextRefs,
    ) -> UpdateResult {
        let mut effects = Vec::new();
        match ev {
            UiEvent::Text { .. } => {
                if self.chat.tool_call_active {
                    self.chat.clear_tool_activity();
                }
                self.spinner_phase(SpinnerPhase::Generating);
            }
            UiEvent::Thinking { .. } => {
                if self.chat.tool_call_active {
                    self.chat.clear_tool_activity();
                }
                self.spinner_phase(SpinnerPhase::Thinking);
            }
            UiEvent::BlockComplete { context, text } => {
                let _ = (context, text);
            }
            UiEvent::ToolCallStart { name, index: _, .. } => {
                self.chat.start_tool_activity(); // AskUserQuestion 等待用户回复期间不应显示 spinner
                if name != "AskUserQuestion" {
                    self.spinner_phase(SpinnerPhase::CallingTool(name));
                }
            }
            UiEvent::ToolCallUpdate { name, id, .. } => {
                self.chat.register_tool_call(id.clone());
                self.spinner_phase(SpinnerPhase::CallingTool(name));
            }
            UiEvent::ToolResult {
                id,
                tool_name: _,
                output: _,
                is_error: _,
                images: _,
                ..
            } => {
                let _had_active_id = self.chat.has_active_tool_call(&id);
                let remaining = self.chat.finish_tool_call(&id);
                if remaining == 0 {
                    // All tool results received — agent loop will continue with next API call.
                    // Restart spinner to show "waiting for next response" state.
                    self.spinner_phase(SpinnerPhase::Thinking);
                } else {
                    self.spinner_phase(SpinnerPhase::CallingTools { remaining });
                }
            }
            UiEvent::Usage { .. } => {
                // token/api/tps 真相归 RuntimeModel，经 StatusViewAssembler + adapter 单向写回 status_bar。
            }
            UiEvent::LiveTps(_tps) => {
                // tps 已由 map_agent_event -> RuntimeIntent::RecordLiveTps 注入 RuntimeModel，
                // 经 adapter 单向写回 status_bar。
            }
            UiEvent::AgentProgress { .. } => {
                // AgentProgress 已由 map_agent_event -> RecordAgentProgress 注入
                // ConversationModel，经 document 渲染（消除命令式写 output_area.lines）。
                self.spinner_phase(SpinnerPhase::AgentWorking);
            }
            UiEvent::HookEvent(event) => {
                self.spinner_phase(hook_spinner_phase(&event));
                if let Some(notice) = hook_event_notice(&event) {
                    self.append_hook_notice(notice);
                }
            }
            UiEvent::Error(msg) => {
                // Error 消息已由 map_agent_event -> AppendError 注入 ConversationModel，                // 此处不再重复写 output_area（消除双表示）。
                self.spinner_stop();
                self.chat.stop_processing();
                self.chat.clear_processing_handle();
                return UpdateResult::one(Effect::RunHook {
                    message: msg,
                    name: "error".to_string(),
                });
            }
            UiEvent::Cancelled => {
                // 取消提示改为注入 ConversationModel 的 System notice，经 document 渲染。
                self.append_system_notice("已取消");
                self.spinner_stop();
                self.chat.stop_processing();
                self.chat.clear_processing_handle();
            }
            UiEvent::MessagesSync(msgs) => {
                // 比较新旧 messages，提取新增的 user messages 用于回显
                let old_len = self.chat.messages.len();
                let new_user_texts: Vec<String> = msgs
                    .iter()
                    .skip(old_len)
                    .filter_map(|m| {
                        if m.role == "user" {
                            let t = m.text_content();
                            if t.is_empty() {
                                None
                            } else {
                                Some(t)
                            }
                        } else {
                            None
                        }
                    })
                    .collect();
                self.chat.messages = msgs;
                self.input.clear_queue();
                self.clear_queued_submission_echo();
                // 将新增的 user messages 正式回显到 conversation model
                for text in new_user_texts {
                    self.append_user_echo(text);
                }
                return UpdateResult::one(Effect::SaveSession { notify: false });
            }
            UiEvent::ClipboardImage(img) => {
                let count = self.chat.add_pending_image(img);
                self.handle_input_intent(
                    crate::tui::model::input::intent::InputIntent::SetAttachmentCount(count),
                );
            }
            UiEvent::SystemMessage(msg) => {
                // SystemMessage 已由 map_agent_event -> AppendSystemMessage 注入
                // ConversationModel；此处仅触发 hook（副作用经 Cmd 描述）。
                return UpdateResult::one(Effect::RunHook {
                    message: msg,
                    name: "system_message".to_string(),
                });
            }
            UiEvent::ReminderRecap(_line) => {
                // ReminderRecap 已由 map_agent_event -> AppendSystemMessage 注入
                // ConversationModel，无需在此重复写入。
            }
            UiEvent::MemoryList(reminders) => {
                self.handle_memory_list(&reminders);
            }
            UiEvent::SessionSaved { id } => {
                self.append_system_notice(format!("[session saved: {id}]"));
            }
            UiEvent::SlashCommandFailed { message } => {
                self.append_error_notice(message);
            }
            UiEvent::ReflectionStarted => {
                self.spinner_phase(SpinnerPhase::Reflecting);
                self.chat.start_processing();
            }
            UiEvent::ReflectionUsage => {
                // token/api 真相归 RuntimeModel，经 StatusViewAssembler + adapter 单向写回 status_bar。
            }
            UiEvent::ReflectionDone { output } => {
                self.append_system_notice(output.content.clone());
                if output.auto_applied {
                    self.chat.pending_reflection = None;
                    self.append_system_notice(
                        "[reflection: memory 建议已自动应用，无需重复 /reflect apply]",
                    );
                } else {
                    let suggestion_count = output.suggested_memories.len();
                    let outdated_count = output.outdated_memories.len();
                    self.chat.pending_reflection = Some(output);
                    if suggestion_count > 0 || outdated_count > 0 {
                        self.append_system_notice("可运行 /reflect apply 应用这些 memory 建议");
                    }
                }
                self.spinner_stop();
                self.chat.stop_processing();
                self.chat.clear_processing_handle();
                self.model
                    .runtime
                    .apply(RuntimeIntent::SetStatusNotice(StatusNotice::success(
                        "Ready",
                    )));
            }
            UiEvent::ReflectionApplyDone { output, result } => match result {
                Ok(message) => {
                    if reflection_outputs_same(self.chat.applying_reflection.as_ref(), &output) {
                        self.chat.applying_reflection = None;
                    }
                    self.append_system_notice(format!("[reflection apply 成功: {message}]"));
                }
                Err(message) => {
                    if reflection_outputs_same(self.chat.applying_reflection.as_ref(), &output) {
                        self.chat.applying_reflection = None;
                        if self.chat.pending_reflection.is_none() {
                            self.chat.pending_reflection = Some(output);
                        }
                    }
                    self.append_error_notice(format!(
                        "Reflection apply 失败: {message}。已保留待应用建议，可重试 /reflect apply"
                    ));
                }
            },
            UiEvent::AskUser {
                id,
                question,
                options,
                allow_free_input,
                multi_select,
                default,
                reply_tx,
            } => {
                self.chat.finish_tool_call(&id);
                self.spinner_stop();

                // 构建内建选项：始终追加 Type something
                // - ≥1 LLM 选项：Type something
                // - 0 个选项：无内建选项（纯自由输入）
                let llm_option_count = options.len();
                let mut all_options = options.clone();
                if llm_option_count >= 1 {
                    all_options.push(sdk::OptionItem::title_only(
                        crate::tui::app::state::BUILTIN_OPTION_CHAT,
                    ));
                }

                if all_options.is_empty() {
                    // 无选项：仍以 AskUser 块渲染问题（自由输入模式），应答走 reply_tx；
                    // 携带 default 以渲染 `(default: ...)` 提示行。
                    self.show_ask_user_block(question, Vec::new(), 0, multi_select, 0, default);
                    self.input.ask_user_reply_tx = Some(reply_tx);
                } else {
                    let cursor = default
                        .as_ref()
                        .and_then(|d| all_options.iter().position(|o| o.title == *d))
                        .unwrap_or(0);
                    self.show_ask_user_block(
                        question,
                        all_options.clone(),
                        llm_option_count,
                        multi_select,
                        cursor,
                        None,
                    );
                    self.input.ask_user_state = Some(crate::tui::app::state::AskUserState {
                        reply_tx,
                        options: all_options,
                        llm_option_count,
                        multi_select,
                        allow_free_input,
                    });
                }
                self.spinner_stop();
            }
            UiEvent::DrainQueuedInput { reply_tx } => {
                let queued = self.input.drain_queue();
                if !queued.is_empty() {
                    // 先清除「排队中」显示块（QueuedUserMessage），再以正式 UserMessage
                    // 回显，避免「排队块」与「已发送回显」双显示。
                    self.clear_queued_submission_echo();
                    for msg in &queued {
                        self.append_user_echo(msg.clone());
                    }
                    self.spinner_phase(SpinnerPhase::ThinkingQueued);
                }
                let _ = reply_tx.send(queued);
            }
            UiEvent::CurrentTurnChanged(turn) => {
                return UpdateResult::one(Effect::SetCurrentTurn { turn });
            }
            UiEvent::WorkingDirectoryChanged(ctx) => {
                // 工作目录上下文已由 map_agent_event -> RuntimeIntent::WorkspaceSnapshotReceived
                // 注入 RuntimeModel，经 adapter 单向写回 status_bar，此处仅同步会话 cwd。
                self.session.cwd = ctx.raw_path_base.clone();
            }
            UiEvent::TaskStatusChanged => {
                effects.push(Effect::FetchTaskStatus);
            }
            UiEvent::Done => {
                effects.extend(self.handle_done(ui_tx, None));
                self.chat.clear_processing_handle();
            }
            UiEvent::DoneWithDuration(elapsed) => {
                effects.extend(self.handle_done(ui_tx, Some(elapsed)));
                self.chat.clear_processing_handle();
            }
        }

        UpdateResult {
            effects,
            spawn_effect: None,
            pending_slash: None,
        }
    }
}

fn reflection_outputs_same(
    left: Option<&sdk::ReflectionOutputView>,
    right: &sdk::ReflectionOutputView,
) -> bool {
    left.is_some_and(|left| format!("{left:?}") == format!("{right:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::effect::session::processing::SpawnContextRefs;
    use crate::tui::model::conversation::block::ConversationBlock;
    use std::path::PathBuf;

    fn test_app() -> App {
        App::new(
            "test-session".to_string(),
            PathBuf::from("/tmp"),
            "test-model".to_string(),
        )
    }

    #[test]
    fn test_update_ui_drain_queued_input_echoes_original_queued_text() {
        let mut app = test_app();
        app.input.push_queue("a\nb\nc".to_string());
        app.enqueue_submission_echo("[Copied Text 1]");
        let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel();
        let (ui_tx, _ui_rx) = mpsc::channel(1);
        let spawn_refs = SpawnContextRefs { agent_client: None };

        app.update_ui(UiEvent::DrainQueuedInput { reply_tx }, &ui_tx, &spawn_refs);

        assert_eq!(reply_rx.try_recv(), Ok(vec!["a\nb\nc".to_string()]));
        assert!(app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::UserMessage { text, .. } if text == "a\nb\nc")
        }));
        assert!(app
            .model
            .conversation
            .blocks
            .iter()
            .all(|block| !matches!(block, ConversationBlock::QueuedUserMessage { .. })));
    }

    #[test]
    fn test_update_ui_messages_sync_echoes_original_user_message() {
        let mut app = test_app();
        app.chat.messages.push(sdk::ChatMessage::user_text("first"));
        app.input.push_queue("a\nb\nc".to_string());
        app.enqueue_submission_echo("[Copied Text 1]");
        let messages = vec![
            sdk::ChatMessage::user_text("first"),
            sdk::ChatMessage::user_text("a\nb\nc"),
        ];
        let (ui_tx, _ui_rx) = mpsc::channel(1);
        let spawn_refs = SpawnContextRefs { agent_client: None };

        app.update_ui(UiEvent::MessagesSync(messages), &ui_tx, &spawn_refs);

        assert_eq!(app.input.queue_len(), 0);
        assert!(app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::UserMessage { text, .. } if text == "a\nb\nc")
        }));
        assert!(app
            .model
            .conversation
            .blocks
            .iter()
            .all(|block| !matches!(block, ConversationBlock::QueuedUserMessage { .. })));
    }
}
