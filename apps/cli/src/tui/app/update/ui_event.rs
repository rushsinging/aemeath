use super::UpdateResult;
use crate::tui::adapter::hook_notice::hook_spinner_phase;
use crate::tui::app::{App, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::conversation::spinner::SpinnerPhase;
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
            }
            UiEvent::Thinking { .. } => {
                if self.chat.tool_call_active {
                    self.chat.clear_tool_activity();
                }
            }
            UiEvent::BlockComplete { context, text } => {
                let _ = (context, text);
            }
            UiEvent::ToolCallStart {
                name: _, index: _, ..
            } => {
                self.chat.start_tool_activity();
            }
            UiEvent::ToolCallUpdate { name: _, id, .. } => {
                self.chat.register_tool_call(id.clone());
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
                let _remaining = self.chat.finish_tool_call(&id);
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
            }
            UiEvent::HookEvent(event) => {
                // Hook notice 已由 map_agent_event -> AppendHookNotice 注入 ConversationModel，
                // 此处仅更新 spinner 状态（spinner 归 RuntimeModel 管理）。
                self.spinner_phase(hook_spinner_phase(&event));
            }
            UiEvent::Error(msg) => {
                // Error 消息已由 map_agent_event -> AppendError 注入 ConversationModel，
                crate::tui::log_info!("[SPINNER_DEBUG] UiEvent::Error → spinner_stop");
                // 此处不再重复写 output_area（消除双表示）。
                self.spinner_stop();
                self.chat.stop_processing();
                self.chat.clear_processing_handle();
                return UpdateResult::one(Effect::RunHook {
                    message: msg,
                    name: "error".to_string(),
                });
            }
            UiEvent::Cancelled { .. } => {
                // 取消提示改为注入 ConversationModel 的 System notice，经 document 渲染。
                self.append_system_notice("已取消");
                crate::tui::log_info!("[SPINNER_DEBUG] UiEvent::Cancelled → spinner_stop");
                self.spinner_stop();
                self.chat.stop_processing();
                // 不清 processing_handle：cancel_to_idle 只把 loop FSM 带回 Idle，
                // 常驻 loop 任务本身并未退出（等待下一条输入），提前清空会让后续
                // Esc/Ctrl+C 的 abort() 找不到 handle、静默失效。见 #624。
            }
            UiEvent::UserMessagesAdopted { items, queued } => {
                let before_queued = self.model.conversation.queued_submissions.len();
                crate::tui::log_debug!(
                    "UserMessagesAdopted items={} queued={} is_processing={} before_queued={}",
                    items.len(),
                    queued.len(),
                    self.chat.is_processing,
                    before_queued
                );
                for item in items {
                    let preview = item.text_content().chars().take(60).collect::<String>();
                    crate::tui::log_debug!(
                        "UserMessagesAdopted item input_id={:?} text_preview={:?}",
                        item.input_id.as_ref().map(|id| id.as_str().to_string()),
                        preview
                    );
                    if let Some(id) = item.input_id.as_ref() {
                        self.clear_queued_submission_echo_by_id(id);
                    }
                    self.append_user_echo(item.text_content());
                }
                self.model.conversation.sync_queued_from_runtime(&queued);
                let after_queued = self.model.conversation.queued_submissions.len();
                crate::tui::log_debug!("UserMessagesAdopted done after_queued={}", after_queued);
                self.mark_output_dirty();
            }
            UiEvent::UserMessagesQueued { queued } => {
                crate::tui::log_debug!(
                    "UserMessagesQueued count={} is_processing={}",
                    queued.len(),
                    self.chat.is_processing
                );
                self.model.conversation.sync_queued_from_runtime(&queued);
                self.mark_output_dirty();
            }
            UiEvent::TurnStarted { messages: _ } => {
                // Turn 启动：启动 spinner(Thinking)。
                crate::tui::log_info!(
                    "[SPINNER_DEBUG] UiEvent::TurnStarted → spinner_phase(Thinking)"
                );
                self.spinner_phase(SpinnerPhase::Thinking);
                self.mark_output_dirty();
            }
            UiEvent::MicrocompactDone {
                messages: _,
                cleared_count,
            } => {
                // Microcompact 清理陈旧 tool result，turn 仍在进行。
                crate::tui::log_info!(
                    "[SPINNER_DEBUG] UiEvent::MicrocompactDone cleared={} (spinner 不动)",
                    cleared_count
                );
                self.mark_output_dirty();
            }
            UiEvent::StopHookBlocked { messages: _ } => {
                // Stop hook 阻止 turn 结束，追加 reminder 后继续。
                crate::tui::log_info!("[SPINNER_DEBUG] UiEvent::StopHookBlocked (spinner 不动)");
                self.mark_output_dirty();
            }
            UiEvent::PostToolExecutionSync { messages: _ } => {
                // Tool 执行完成后同步消息。
                self.mark_output_dirty();
            }
            UiEvent::ApiError { messages: _, error } => {
                // Provider API 调用失败：stop spinner + 显示错误。
                crate::tui::log_info!(
                    "[SPINNER_DEBUG] UiEvent::ApiError → spinner_stop error={}",
                    error
                );
                self.spinner_stop();
                self.append_system_notice(&error);
                self.mark_output_dirty();
            }
            UiEvent::CompactRollback { messages: _ } => {
                // Compact 失败回滚：不动 spinner（turn 仍在进行）。
                self.model.conversation.runtime.clear_compact_runtime();
                self.mark_output_dirty();
            }
            UiEvent::CompactFinished { messages: _ } => {
                // Compact 成功完成：清 compact 状态。
                // 不停 spinner——compact 后 turn 仍在进行，LLM 会继续生成。
                self.model.conversation.runtime.clear_compact_runtime();
                self.mark_output_dirty();
            }
            UiEvent::ClipboardImage(img) => {
                self.handle_input_intent(
                    crate::tui::model::input::intent::InputIntent::InsertImage(img),
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
            UiEvent::ModelStreamWaiting { .. } => {
                // Transient placeholder 已由 map_agent_event 注入 ConversationModel。
                self.mark_output_dirty();
            }
            UiEvent::SessionSaved { id } => {
                self.append_system_notice(format!("[session saved: {id}]"));
            }
            UiEvent::ReflectionDone { output } => {
                // ...
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
                crate::tui::log_info!("[SPINNER_DEBUG] UiEvent::ReflectionDone → spinner_stop");
                self.spinner_stop();
                self.chat.stop_processing();
                self.chat.clear_processing_handle();
                self.model
                    .conversation
                    .apply(SetStatusNotice(StatusNotice::success("Ready")));
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
            UiEvent::AskUserBatch { items, reply_tx } => {
                // 完成每个 item 关联的 tool_call
                for item in &items {
                    self.chat
                        .finish_tool_call(&sdk::ids::ToolCallId::new(&item.id));
                }
                crate::tui::log_info!(
                    "[SPINNER_DEBUG] UiEvent::AskUserBatch(finish_tool_calls) → spinner_stop"
                );
                self.spinner_stop();

                let n = items.len();
                if n == 1 && items[0].options.is_empty() {
                    // 单问 + 无选项：自由输入模式，走 reply_tx
                    let item = &items[0];
                    let slot = crate::tui::model::conversation::block::AskUserSlot {
                        id: item.id.clone(),
                        question: item.question.clone(),
                        options: Vec::new(),
                        llm_option_count: 0,
                        multi_select: item.multi_select,
                        default: item.default.clone(),
                        answer: None,
                    };
                    self.show_ask_user_batch(vec![slot]);
                    self.input.ask_user_reply_tx = Some(reply_tx);
                } else {
                    // 构建 AskUserSlot 列表
                    let slots: Vec<_> = items
                        .iter()
                        .map(|item| {
                            let llm_count = item.options.len();
                            let mut all_options = item.options.clone();
                            if llm_count >= 1 {
                                all_options.push(sdk::OptionItem::title_only(
                                    crate::tui::app::state::BUILTIN_OPTION_CHAT,
                                ));
                            }
                            crate::tui::model::conversation::block::AskUserSlot {
                                id: item.id.clone(),
                                question: item.question.clone(),
                                options: all_options,
                                llm_option_count: llm_count,
                                multi_select: item.multi_select,
                                default: item.default.clone(),
                                answer: None,
                            }
                        })
                        .collect();
                    self.show_ask_user_batch(slots);
                    self.input.ask_user_state =
                        Some(crate::tui::app::state::AskUserState { reply_tx, items });
                }
                crate::tui::log_info!("[SPINNER_DEBUG] UiEvent::AskUserBatch(show) → spinner_stop");
                self.spinner_stop();
            }
            UiEvent::CurrentTurnChanged(turn) => {
                return UpdateResult::one(Effect::SetCurrentTurn { turn });
            }
            UiEvent::WorkingDirectoryChanged(ctx) => {
                // 工作目录上下文已由 map_agent_event -> RuntimeIntent::WorkspaceSnapshotReceived
                // 注入 RuntimeModel，经 adapter 单向写回 status_bar，此处仅同步会话 cwd。
                self.session.cwd = ctx.raw_path_base.clone();
            }
            UiEvent::TaskStatusChanged(view) => {
                self.model.conversation.apply(UpdateTaskLines(view.lines));
            }
            UiEvent::UpdateAvailable {
                current,
                latest,
                release_url,
            } => {
                self.append_system_notice(format!(
                    "[aemeath v{latest} is available (you have v{current}); run `aemeath update` to upgrade | {release_url}]"
                ));
            }
            // #391 S1-4：runtime idle gate 已清空 messages 并发 SessionReset。
            // TUI 收到后经 Effect 异步执行完整 reset_runtime_state（清 UI + sync + tasks）。
            UiEvent::SessionReset => {
                return UpdateResult::one(Effect::ResetRuntimeState);
            }
            UiEvent::UserMessagesWithdrawn(texts) => {
                self.clear_all_queued_submission_echos();
                if !texts.is_empty() {
                    self.handle_input_intent(
                        crate::tui::model::input::intent::InputIntent::ReplaceText(
                            texts.join("\n"),
                        ),
                    );
                }
            }
            UiEvent::GraphPhaseChanged { node } => {
                // Graph 阶段变化 → 更新 graph_phase（model.apply 会同步 status_notice，
                // 除非当前是临时 notice）
                let phase = if node == "idle" { None } else { Some(node) };
                self.model.conversation.apply(SetGraphPhase(phase));
            }
            UiEvent::CompactProgress {
                stage,
                current,
                total,
            } => {
                self.model.conversation.apply(SetCompactProgress {
                    stage,
                    current,
                    total,
                });
                // #540：进度条嵌在 spinner 行（output 区），dirty 归类为 output_dirty。
                // ui_event.rs 直接 apply 绕开 reduce_agent_event 的 dirty 归类，
                // 必须在此显式 mark_output_dirty 驱动刷新，否则依赖 SpinnerTick 兜底不可靠。
                self.mark_output_dirty();
            }
            UiEvent::ModelSwitched { result } => {
                // #497：模型切换走事件流，TUI 在此更新本地状态（与原 slash.rs RPC 路径对齐）。
                if result.context_window > 0 {
                    self.chat.context_size = result.context_window;
                    self.model
                        .conversation
                        .apply(SetContextSize(result.context_window as u64));
                }
                self.session.current_model_display = result.display_name.clone();
                self.model.conversation.apply(SetProviderModel {
                    provider: self.model.conversation.runtime.provider.clone(),
                    model_id: Some(result.display_name.clone()),
                });
                if let Some(ra) = result.reasoning_active {
                    self.model.conversation.apply(SetThinking(ra));
                }
                self.append_system_notice(format!("[switched to {}]", result.display_name));
            }
            UiEvent::ThinkingChanged { enabled } => {
                // #497：reasoning 模式切换走事件流。SystemMessage("[thinking mode: ON/OFF]")
                // 已由 runtime 发回，TUI 只需更新 thinking 状态。
                self.model.conversation.apply(SetThinking(enabled));
            }
            UiEvent::ContextEstimated {
                estimate,
                message_count,
            } => {
                // #497：上下文估算走事件流。显示格式与旧 slash.rs RPC 路径一致。
                self.append_system_notice(format!(
                    "Context window: ~{} / {} tokens ({:.0}%)",
                    estimate.estimated_tokens, estimate.context_size, estimate.usage_percentage
                ));
                self.append_system_notice(format!("Messages: {}", message_count));
                if estimate.usage_percentage > 80.0 {
                    self.append_system_notice("[auto-compaction will trigger at 80%]");
                }
            }
            UiEvent::CommandResultText { text, is_error } => {
                if is_error {
                    self.append_error_notice(&text);
                } else {
                    self.append_system_notice(&text);
                }
            }
            UiEvent::SessionResumed {
                messages,
                session_id,
                created_at,
            } => {
                self.resume_session_messages(&session_id, messages, created_at.to_string());
            }
            UiEvent::SessionResumeFailed { kind, id, message } => {
                use sdk::SessionResumeFailureKind;
                let prefix = match kind {
                    SessionResumeFailureKind::NotFound => "⚠️ 会话恢复失败（不存在）",
                    SessionResumeFailureKind::Corrupt => "⚠️ 会话恢复失败（文件损坏）",
                    SessionResumeFailureKind::Io => "⚠️ 会话恢复失败（IO 错误）",
                };
                self.append_system_notice(format!("{prefix}: {message}"));
                log::warn!(
                    target: "aemeath:tui",
                    "session resume failed: id={} kind={:?} msg={}",
                    id, kind, message
                );
            }
            UiEvent::Done { .. } => {
                // 不清 processing_handle：Done 只表示这一个 turn 结束，常驻 loop
                // 回 Idle 继续等待下一条输入，任务本身没退出。见 #624。
                effects.extend(self.handle_done(ui_tx, None));
            }
            UiEvent::DoneWithDuration { duration, .. } => {
                // 同上：DoneWithDuration 同样只是「这一回合完成」，不是任务退出。
                effects.extend(self.handle_done(ui_tx, Some(duration)));
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
#[path = "ui_event_tests.rs"]
mod ui_event_tests;
