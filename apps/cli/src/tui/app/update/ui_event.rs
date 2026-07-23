use super::UpdateResult;
use crate::tui::app::{App, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::conversation::intent::*;
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
            // ── Runtime 事件已走 TuiMsg::Runtime → update_runtime_event，以下分支不再触达 ──
            UiEvent::Text { .. }
            | UiEvent::Thinking { .. }
            | UiEvent::BlockComplete { .. }
            | UiEvent::ToolCallStart { .. }
            | UiEvent::ToolCallUpdate { .. }
            | UiEvent::ToolResult { .. }
            | UiEvent::Usage { .. }
            | UiEvent::LiveTps(_)
            | UiEvent::AgentProgress { .. }
            | UiEvent::HookEvent(_)
            | UiEvent::HookMessage(_)
            | UiEvent::UserMessagesAdopted { .. }
            | UiEvent::UserMessagesQueued { .. }
            | UiEvent::TurnStarted { .. }
            | UiEvent::MicrocompactDone { .. }
            | UiEvent::StopHookBlocked { .. }
            | UiEvent::PostToolExecutionSync { .. }
            | UiEvent::CompactRollback { .. }
            | UiEvent::CompactFinished { .. }
            | UiEvent::GraphPhaseChanged { .. }
            | UiEvent::CompactProgress { .. }
            | UiEvent::UserMessagesWithdrawn(_)
            | UiEvent::SessionReset
            | UiEvent::ApiError { .. } => {
                // 阶段 3：Runtime 事件副作用已迁移到 update_runtime_event
            }
            // ── 本地 Effect 回灌 ──
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
            UiEvent::ModelSwitched { result } => {
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
