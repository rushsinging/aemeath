use super::UpdateResult;
use crate::tui::adapter::hook_notice::hook_spinner_phase;
use crate::tui::app::{App, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::conversation::spinner::SpinnerPhase;
use crate::tui::model::runtime_presentation::RuntimePresentationIntent;
use crate::tui::update::intent::AgentIntent;
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
            UiEvent::ToolCallStart { .. } => {
                self.chat.start_tool_activity();
            }
            UiEvent::ToolCallUpdate { id, .. } => {
                self.chat.register_tool_call(id.clone());
            }
            UiEvent::ToolResult { id, .. } => {
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
            UiEvent::HookMessage(_) => {
                // Hook message 已由 map_agent_event -> AppendHookNotice 注入 ConversationModel。
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
            UiEvent::RunCancelled => {
                self.chat.stop_processing();
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
                    let text_len = item.text_content().chars().count();
                    crate::tui::log_debug!(
                        "UserMessagesAdopted item input_id={:?} text_len={}",
                        item.input_id.as_ref().map(|id| id.as_str().to_string()),
                        text_len
                    );
                    if let Some(id) = item.input_id.as_ref() {
                        self.clear_queued_submission_echo_by_id(id);
                    }
                    self.append_user_echo(item.text_content());
                }
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::SyncQueuedSubmissions(SyncQueuedSubmissions { queued }),
                ));
                let after_queued = self.model.conversation.queued_submissions.len();
                crate::tui::log_debug!("UserMessagesAdopted done after_queued={}", after_queued);
            }
            UiEvent::UserMessagesQueued { queued } => {
                crate::tui::log_debug!(
                    "UserMessagesQueued count={} is_processing={}",
                    queued.len(),
                    self.chat.is_processing
                );
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::SyncQueuedSubmissions(SyncQueuedSubmissions { queued }),
                ));
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
                // #749：ApiError 退化为纯展示——仅追加错误 notice + stop spinner。
                // processing 状态收口统一交给随后 runtime 发出的 DoneWithDuration
                // （handle_done → stop_processing），避免各终止路径各自清理导致漏清。
                // NOT 在此 stop_processing，保持与 Done 单一收口点一致。
                crate::tui::log_info!(
                    "[SPINNER_DEBUG] UiEvent::ApiError → spinner_stop（processing 收口交给 Done）error={}",
                    error
                );
                self.spinner_stop();
                self.append_system_notice(&error);
                self.mark_output_dirty();
            }
            UiEvent::CompactRollback { messages: _ } => {
                // Compact 失败回滚：不动 spinner（turn 仍在进行）。
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::ClearCompactRuntime(ClearCompactRuntime),
                ));
            }
            UiEvent::CompactFinished { messages: _ } => {
                // Compact 成功完成：清 compact 状态。
                // 不停 spinner——compact 后 turn 仍在进行，LLM 会继续生成。
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::ClearCompactRuntime(ClearCompactRuntime),
                ));
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
            UiEvent::ReflectionHistory { records } => {
                if records.is_empty() {
                    self.append_system_notice("No reflection history.");
                } else {
                    self.append_system_notice(format_reflection_history(&records));
                }
            }
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

                let slots: Vec<_> = items
                    .iter()
                    .map(|item| {
                        let llm_count = item.options.len();
                        let mut all_options = item.options.clone();
                        if item.allow_free_input && llm_count >= 1 {
                            all_options.push(sdk::OptionItem::title_only(
                                crate::tui::app::state::BUILTIN_OPTION_CHAT,
                            ));
                        }
                        crate::tui::model::conversation::block::AskUserSlot {
                            id: item.id.clone(),
                            question_seq: item.question_seq,
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
                crate::tui::log_info!("[SPINNER_DEBUG] UiEvent::AskUserBatch(show) → spinner_stop");
                self.spinner_stop();
            }
            UiEvent::InteractionRequested { request } => {
                // #1246: Map SDK typed interaction request to TUI model intent.
                let tui_request = sdk_interaction_to_tui(&request);
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::ShowInteraction(ShowInteraction {
                        request: tui_request,
                    }),
                ));
                self.spinner_stop();
            }
            UiEvent::CurrentTurnChanged(turn) => {
                return UpdateResult::one(Effect::SetCurrentTurn { turn });
            }
            UiEvent::WorkingDirectoryChanged(ctx) => {
                self.session.cwd = ctx.raw_path_base;
            }
            UiEvent::WorkspaceMetadataResolved(_) => {}
            UiEvent::TaskStatusChanged(view) => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::UpdateTaskLines(UpdateTaskLines(view.lines)),
                ));
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
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::SetGraphPhase(SetGraphPhase(phase)),
                ));
            }
            UiEvent::CompactProgress {
                stage,
                current,
                total,
            } => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::SetCompactProgress(SetCompactProgress {
                        stage,
                        current,
                        total,
                    }),
                ));
            }
            UiEvent::ModelSwitched { result } => {
                // #497：模型切换走事件流，TUI 在此更新本地状态（与原 slash.rs RPC 路径对齐）。
                if result.context_window > 0 {
                    self.apply_agent_intent(AgentIntent::RuntimePresentation(
                        RuntimePresentationIntent::ContextSize(result.context_window as u64),
                    ));
                }
                self.session.current_model_display = result.display_name.clone();
                self.apply_agent_intent(AgentIntent::RuntimePresentation(
                    RuntimePresentationIntent::ProviderModel {
                        provider: self
                            .model
                            .runtime_presentation
                            .provider()
                            .map(ToOwned::to_owned),
                        model_id: Some(result.display_name.clone()),
                    },
                ));
                if let Some(ra) = result.reasoning_active {
                    self.apply_agent_intent(AgentIntent::RuntimePresentation(
                        RuntimePresentationIntent::Thinking(ra),
                    ));
                }
                self.append_system_notice(format!("[switched to {}]", result.display_name));
            }
            UiEvent::ThinkingChanged { enabled } => {
                // #497：reasoning 模式切换走事件流。SystemMessage("[thinking mode: ON/OFF]")
                // 已由 runtime 发回，TUI 只需更新 thinking 状态。
                self.apply_agent_intent(AgentIntent::RuntimePresentation(
                    RuntimePresentationIntent::Thinking(enabled),
                ));
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
                    target: crate::LOG_TARGET,
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

fn format_reflection_history(records: &[sdk::ReflectionHistoryView]) -> String {
    let mut lines = vec![format!("Reflection history ({}):", records.len())];
    for record in records {
        let tokens = record.token_usage.map_or_else(
            || "n/a".to_string(),
            |usage| format!("{}/{}", usage.input_tokens, usage.output_tokens),
        );
        let error = record
            .error_category
            .map_or_else(|| "none".to_string(), |category| format!("{category:?}"));
        lines.push(format!(
            "- timestamp={} trigger={:?} status={:?} counts(deviations/suggestions/outdated)={}/{}/{} apply={:?} error={} tokens(in/out)={} duration={}ms",
            record.timestamp,
            record.trigger,
            record.status,
            record.deviations,
            record.suggestions,
            record.outdated,
            record.apply_status,
            error,
            tokens,
            record.duration_ms,
        ));
    }
    lines.join("\n")
}

/// #1246: Convert SDK typed InteractionRequest to TUI model's InteractionRequest.
fn sdk_interaction_to_tui(
    request: &sdk::InteractionRequest,
) -> crate::tui::model::conversation::interaction::InteractionRequest {
    use crate::tui::model::conversation::interaction::*;

    InteractionRequest {
        request_id: UiInteractionRequestId::from(request.id.as_str()),
        run_id: UiRunId::from(request.run_id.as_str()),
        body: match &request.body {
            sdk::InteractionRequestBody::UserQuestions(questions) => {
                InteractionBody::UserQuestions(
                    questions
                        .iter()
                        .map(|q| UiUserQuestion {
                            prompt: q.prompt.clone(),
                            options: q.options.clone(),
                            allow_multi: q.allow_multi,
                        })
                        .collect(),
                )
            }
            sdk::InteractionRequestBody::ToolApproval(prompt) => {
                InteractionBody::ToolApproval(UiApprovalPrompt {
                    title: prompt.tool_name.clone(),
                    detail: prompt.args_summary.clone(),
                    risk: match prompt.risk_level {
                        sdk::RiskLevel::Low => UiRiskLevel::Low,
                        sdk::RiskLevel::Medium => UiRiskLevel::Medium,
                        sdk::RiskLevel::High => UiRiskLevel::High,
                    },
                })
            }
            sdk::InteractionRequestBody::PlanApproval(prompt) => {
                InteractionBody::PlanApproval(UiPlanApprovalPrompt {
                    title: prompt.plan_title.clone(),
                    steps: prompt.steps.clone(),
                })
            }
            sdk::InteractionRequestBody::HardPause(diag) => {
                InteractionBody::HardPause(UiStuckDiagnostic {
                    reason: diag.reason.clone(),
                    recent_actions: diag.recent_actions.clone(),
                })
            }
        },
    }
}

#[cfg(test)]
#[path = "ui_event_tests.rs"]
mod ui_event_tests;
