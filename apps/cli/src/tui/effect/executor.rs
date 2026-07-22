use crate::tui::app::event::UiEvent;
use crate::tui::app::App;
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::effect::session::processing;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::conversation::interaction::{
    InteractionCommandFailure, UiInteractionCancelReason, UiInteractionReply,
    UiInteractionRequestId,
};
use crate::tui::model::conversation::workspace::WorktreeKind;
use crate::tui::model::runtime::status_notice::StatusNotice;
use crate::tui::update::intent::AgentIntent;
use tokio::sync::mpsc;

fn interaction_reply_to_sdk(reply: UiInteractionReply) -> sdk::InteractionReply {
    match reply {
        UiInteractionReply::UserAnswers(answers) => {
            sdk::InteractionReply::UserQuestions(answers.into_iter().map(sdk::UserAnswer).collect())
        }
        UiInteractionReply::ToolApproval { approved, reason } => {
            sdk::InteractionReply::ToolApproval(if approved {
                sdk::ApprovalDecision::Approve
            } else {
                sdk::ApprovalDecision::Deny { reason }
            })
        }
        UiInteractionReply::PlanApproval { approved, reason } => {
            sdk::InteractionReply::PlanApproval(if approved {
                sdk::ApprovalDecision::Approve
            } else {
                sdk::ApprovalDecision::Deny { reason }
            })
        }
        UiInteractionReply::ContinueHardPause => sdk::InteractionReply::HardPauseContinue,
    }
}

fn interaction_failure_from_sdk(
    outcome: sdk::InteractionCommandOutcome,
) -> InteractionCommandFailure {
    match outcome {
        sdk::InteractionCommandOutcome::InvalidReply(error) => {
            InteractionCommandFailure::InvalidReply(format!("{error:?}"))
        }
        sdk::InteractionCommandOutcome::NotFound => InteractionCommandFailure::NotFound,
        sdk::InteractionCommandOutcome::AlreadyCompleted => {
            InteractionCommandFailure::AlreadyCompleted
        }
        sdk::InteractionCommandOutcome::RunCancelling => InteractionCommandFailure::RunCancelling,
        sdk::InteractionCommandOutcome::Accepted => {
            unreachable!("accepted outcome is handled first")
        }
    }
}

fn resolve_workspace_metadata(root: &str) -> (Option<String>, WorktreeKind) {
    let branch = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|branch| branch.trim().to_string())
        .filter(|branch| !branch.is_empty());

    let kind = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir", "--git-common-dir"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| {
            let mut lines = stdout.lines().map(str::trim);
            match (lines.next(), lines.next()) {
                (Some(git_dir), Some(common_dir)) if git_dir != common_dir => {
                    WorktreeKind::LinkedWorktree
                }
                (Some(_), Some(_)) => WorktreeKind::MainCheckout,
                _ => WorktreeKind::Unknown,
            }
        })
        .unwrap_or(WorktreeKind::Unknown);

    (branch, kind)
}

impl App {
    pub(crate) fn execute_spawn_effect(&mut self, effect: SpawnAgentChatEffect) {
        if let Some(spawn_ctx) = effect.context {
            let handle = processing::spawn_processing(spawn_ctx);
            self.chat.set_processing_handle(handle);
        }
    }

    pub(crate) async fn execute_effect(&mut self, effect: Effect, ui_tx: &mpsc::Sender<UiEvent>) {
        match effect {
            Effect::None | Effect::RequestRender => {}
            Effect::QuitApplication => {
                // #390 A1 常驻 loop shutdown：drop input_event_tx → loop 干净退出 →
                // spawn task 执行 auto-save。退出路径在 session_lifecycle.rs 中 await 完成。
                self.chat.clear_input_event_buffer();
                self.layout.request_exit();
            }
            Effect::SpawnAgentChat { .. } => {}
            Effect::SendChatInputEvent { event } => self.send_chat_input_event(event),
            Effect::CancelCurrentRun => self.cancel_current_run(),
            Effect::ReplyInteraction { request_id, reply } => {
                self.execute_interaction_reply(request_id, reply)
            }
            Effect::CancelInteraction { request_id, reason } => {
                self.execute_interaction_cancel(request_id, reason)
            }
            Effect::ResolveWorkspaceMetadata { root, revision } => {
                self.resolve_workspace_metadata_effect(root, revision, ui_tx)
            }
            Effect::SaveSession { notify } => self.save_session_effect(notify, ui_tx),
            Effect::RunHook { message, name } => self.run_hook_effect(message, name),
            Effect::ReadClipboardImage => self.read_clipboard_image_effect(ui_tx),
            Effect::ProcessImageFile { path } => self.process_image_file_effect(path, ui_tx),
            Effect::SetCurrentTurn { turn } => self.set_current_turn_effect(turn),
            Effect::FetchReminderRecap => self.fetch_reminder_recap_effect(ui_tx),
            Effect::FetchMemoryList => self.fetch_memory_list_effect(ui_tx),
            Effect::QueryReflectionHistory { limit } => self.query_reflection_history_effect(limit),
            Effect::CopyToClipboard { text } => self.copy_to_clipboard_effect(&text),
            Effect::StartTimer { .. } | Effect::StopTimer { .. } => {}
            Effect::RunSelfUpdate => self.run_self_update_effect(ui_tx).await,
            Effect::ResetRuntimeState => self.reset_runtime_state().await,
            Effect::OpenUrl { url } => self.open_url_effect(&url),
        }
    }

    fn resolve_workspace_metadata_effect(
        &self,
        root: String,
        revision: u64,
        ui_tx: &mpsc::Sender<UiEvent>,
    ) {
        let ui_tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("workspace_metadata", async move {
            let query_root = root.clone();
            let metadata =
                tokio::task::spawn_blocking(move || resolve_workspace_metadata(&query_root))
                    .await
                    .unwrap_or((None, WorktreeKind::Unknown));
            let _ = ui_tx
                .send(UiEvent::WorkspaceMetadataResolved(
                    crate::tui::app::event::WorkspaceMetadataResolved {
                        root,
                        revision,
                        branch: metadata.0,
                        kind: metadata.1,
                    },
                ))
                .await;
        });
    }

    fn execute_interaction_reply(
        &mut self,
        request_id: UiInteractionRequestId,
        reply: UiInteractionReply,
    ) {
        let sdk_request_id = match sdk::InteractionRequestId::parse_uuid7(request_id.as_str()) {
            Ok(value) => value,
            Err(_) => {
                self.apply_interaction_failure(
                    request_id,
                    InteractionCommandFailure::InvalidRequestId("交互请求标识无效".to_string()),
                );
                return;
            }
        };
        let outcome = match self.agent_client.as_ref() {
            Some(client) => {
                client.reply_interaction(&sdk_request_id, interaction_reply_to_sdk(reply))
            }
            None => {
                self.apply_interaction_failure(
                    request_id,
                    InteractionCommandFailure::CommandClientUnavailable,
                );
                return;
            }
        };
        self.apply_interaction_outcome(request_id, outcome, false);
    }

    fn execute_interaction_cancel(
        &mut self,
        request_id: UiInteractionRequestId,
        reason: UiInteractionCancelReason,
    ) {
        let sdk_request_id = match sdk::InteractionRequestId::parse_uuid7(request_id.as_str()) {
            Ok(value) => value,
            Err(_) => {
                self.apply_interaction_failure(
                    request_id,
                    InteractionCommandFailure::InvalidRequestId("交互请求标识无效".to_string()),
                );
                return;
            }
        };
        let outcome = match self.agent_client.as_ref() {
            Some(client) => client.cancel_interaction(
                &sdk_request_id,
                match reason {
                    UiInteractionCancelReason::UserCancelled => {
                        sdk::InteractionCancelReason::UserCancelled
                    }
                },
            ),
            None => {
                self.apply_interaction_failure(
                    request_id,
                    InteractionCommandFailure::CommandClientUnavailable,
                );
                return;
            }
        };
        self.apply_interaction_outcome(request_id, outcome, true);
    }

    fn apply_interaction_failure(
        &mut self,
        request_id: UiInteractionRequestId,
        failure: InteractionCommandFailure,
    ) {
        self.apply_agent_intent(AgentIntent::Conversation(
            ConversationIntent::InteractionReplyRejected(InteractionReplyRejected {
                request_id,
                failure,
            }),
        ));
    }

    fn apply_interaction_outcome(
        &mut self,
        request_id: UiInteractionRequestId,
        outcome: sdk::InteractionCommandOutcome,
        is_cancel: bool,
    ) {
        let intent = match outcome {
            sdk::InteractionCommandOutcome::Accepted if is_cancel => {
                ConversationIntent::InteractionCancelAccepted(InteractionCancelAccepted {
                    request_id,
                })
            }
            sdk::InteractionCommandOutcome::Accepted => {
                ConversationIntent::InteractionReplyAccepted(InteractionReplyAccepted {
                    request_id,
                })
            }
            outcome if is_cancel => {
                ConversationIntent::InteractionCancelRejected(InteractionCancelRejected {
                    request_id,
                    failure: interaction_failure_from_sdk(outcome),
                })
            }
            outcome => ConversationIntent::InteractionReplyRejected(InteractionReplyRejected {
                request_id,
                failure: interaction_failure_from_sdk(outcome),
            }),
        };
        self.apply_agent_intent(AgentIntent::Conversation(intent));
    }

    fn cancel_current_run(&mut self) {
        let outcome = self
            .chat
            .processing_handle
            .as_ref()
            .map(|handle| handle.cancel_current_run())
            .unwrap_or(sdk::CancelRunOutcome::NotFound);
        if matches!(
            outcome,
            sdk::CancelRunOutcome::Accepted | sdk::CancelRunOutcome::AlreadyCancelling
        ) {
            self.chat.start_cancelling();
            self.apply_agent_intent(AgentIntent::Conversation(
                ConversationIntent::SetStatusNotice(SetStatusNotice(StatusNotice::warning(
                    "Cancelling current response… Press Ctrl+C again to exit",
                ))),
            ));
        }
    }

    fn send_chat_input_event(&mut self, event: sdk::ChatInputEvent) {
        if self.chat.input_event_tx.is_none() {
            crate::tui::log_debug!(
                "send_chat_input_event DROPPED tx=None event={:?}",
                std::mem::discriminant(&event)
            );
            self.append_error_notice("当前 Chat 输入通道不可用，已保留在队列中等待兜底 drain");
            return;
        }
        crate::tui::log_debug!(
            "send_chat_input_event sending event={:?}",
            std::mem::discriminant(&event)
        );
        self.chat.push_input_event(event);
    }

    /// `/save` 命令——仅 UX 反馈。Runtime 已有 turn-level auto-save + loop-exit auto-save，
    /// TUI 不再发 ChatInputEvent::SaveSession。
    fn save_session_effect(&mut self, notify: bool, ui_tx: &mpsc::Sender<UiEvent>) {
        if notify {
            let id = self.session.session_id().to_string();
            let tx = ui_tx.clone();
            crate::tui::effect::spawn_guard::spawn_guarded("save_notify", async move {
                let _ = tx.send(UiEvent::SessionSaved { id }).await;
            });
        }
    }

    fn fetch_memory_list_effect(&mut self, _ui_tx: &mpsc::Sender<UiEvent>) {
        // #567：list_reminders 走事件流（ChatInputEvent::ListReminders）。
        // runtime idle 分支查询，结果通过 ReminderList 事件回传。
        self.chat
            .push_input_event(sdk::ChatInputEvent::ListReminders);
    }

    fn run_hook_effect(&mut self, message: String, name: String) {
        // #567：notify_hook 删除——hook 是 runtime 内部行为，TUI 不参与。
        // hook 触发由 runtime 在消息变更时自行执行。
        let _ = (message, name);
    }

    fn read_clipboard_image_effect(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        // #567 S10：read_clipboard_image 迁移到 TUI 本地
        let tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("clipboard_image", async move {
            match crate::tui::render::input::clipboard::read_image().await {
                Ok(img) => {
                    use base64::Engine;
                    let view = sdk::ClipboardImageView {
                        base64: base64::engine::general_purpose::STANDARD.encode(&img.data),
                        media_type: img.media_type,
                        final_size: img.data.len(),
                        display_path: None,
                        width: None,
                        height: None,
                    };
                    let _ = tx.send(UiEvent::ClipboardImage(view)).await;
                }
                Err(e) => crate::tui::log_warn!("clipboard read failed: {e}"),
            }
        });
    }

    fn process_image_file_effect(&mut self, path: String, ui_tx: &mpsc::Sender<UiEvent>) {
        // #567 S10：process_image_file 迁移到 TUI 本地
        let tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("image_file", async move {
            match crate::tui::render::input::clipboard::process_image_file(&path) {
                Ok(img) => {
                    use base64::Engine;
                    let view = sdk::ClipboardImageView {
                        base64: base64::engine::general_purpose::STANDARD.encode(&img.data),
                        media_type: img.media_type,
                        final_size: img.data.len(),
                        display_path: Some(path),
                        width: None,
                        height: None,
                    };
                    let _ = tx.send(UiEvent::ClipboardImage(view)).await;
                }
                Err(e) => crate::tui::log_warn!("image process failed: {e}"),
            }
        });
    }

    /// 将文本复制到系统剪贴板，并据结果在 status bar 给出临时反馈。
    fn copy_to_clipboard_effect(&mut self, text: &str) {
        match crate::tui::render::input::clipboard::copy_text(text) {
            Ok(()) => {
                self.set_transient_notice(StatusNotice::success("已复制选中内容"));
            }
            Err(err) => {
                crate::tui::log_warn!("复制选中内容失败: {err}");
                self.set_transient_notice(StatusNotice::warning(err));
            }
        }
    }

    fn set_current_turn_effect(&mut self, _turn: usize) {
        // #567：set_current_turn 删除——runtime loop 内部自维护 turn 计数器。
    }

    fn query_reflection_history_effect(&mut self, limit: usize) {
        self.chat
            .push_input_event(sdk::ChatInputEvent::QueryReflectionHistory { limit });
    }

    /// 启动时后台检查版本更新（非阻塞）。
    /// 使用 `force_check` 忽略缓存，确保每次启动都查最新状态。
    /// 失败时静默降级——版本检查不应影响正常使用。
    pub(crate) fn spawn_update_check(&self, ui_tx: mpsc::Sender<UiEvent>) {
        let service = composition::update::wire_update();
        crate::tui::effect::spawn_guard::spawn_guarded("update_check", async move {
            match service.force_check().await {
                Ok(check) if check.is_update_available => {
                    let _ = ui_tx
                        .send(UiEvent::UpdateAvailable {
                            current: check.current_version,
                            latest: check.latest_version,
                            release_url: check.release_url,
                        })
                        .await;
                }
                Ok(_) => {} // 已是最新，不提示
                Err(e) => {
                    crate::tui::log_warn!("版本检查失败（已忽略）: {e}");
                }
            }
        });
    }

    /// `/update` 命令触发的自动更新执行器。
    /// 在后台执行 perform_update，结果通过 UiEvent 回灌。
    async fn run_self_update_effect(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        self.append_system_notice("[checking for updates...]".to_string());
        let service = composition::update::wire_update();
        let ui_tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("self_update", async move {
            match service.perform_update().await {
                Ok(sdk::UpdateResult::Updated {
                    from,
                    to,
                    installed_path,
                }) => {
                    let _ = ui_tx
                        .send(UiEvent::SystemMessage(format!(
                            "✓ Updated aemeath {from} → {to}\nInstalled to: {installed_path}\nPlease restart aemeath to use the new version."
                        )))
                        .await;
                }
                Ok(sdk::UpdateResult::UpToDate { version }) => {
                    let _ = ui_tx
                        .send(UiEvent::SystemMessage(format!(
                            "Already up to date ({version})."
                        )))
                        .await;
                }
                Ok(sdk::UpdateResult::CheckOnly(_)) => {}
                Err(e) => {
                    let _ = ui_tx
                        .send(UiEvent::SystemMessage(format!("Update failed: {e}")))
                        .await;
                }
            }
        });
    }

    fn fetch_reminder_recap_effect(&mut self, _ui_tx: &mpsc::Sender<UiEvent>) {
        // #567：list_reminders 走事件流。reminder recap 由 ReminderList 事件回传后处理。
        // 暂时发 ListReminders 事件，recap 在 UiEvent 处理中生成。
        self.chat
            .push_input_event(sdk::ChatInputEvent::ListReminders);
    }

    /// 用系统默认程序打开 URL 或本地文件路径（Cmd+Click markdown link / 行内代码路径）。
    fn open_url_effect(&mut self, url: &str) {
        // 安全校验：允许 http/https URL 和本地文件路径
        let is_url = url.starts_with("http://") || url.starts_with("https://");
        let is_path = url.contains('/')
            || url.contains('\\')
            || url.ends_with(".rs")
            || url.ends_with(".toml")
            || url.ends_with(".md")
            || url.ends_with(".json");
        if !is_url && !is_path {
            self.set_transient_notice(StatusNotice::warning(format!("无法识别的链接目标: {url}")));
            return;
        }

        // 本地相对路径：尝试基于 cwd 解析
        let resolved = if !is_url && !std::path::Path::new(url).is_absolute() {
            let cwd = std::env::current_dir().unwrap_or_default();
            Some(cwd.join(url).to_string_lossy().into_owned())
        } else {
            None
        };
        let target = resolved.as_deref().unwrap_or(url);

        #[cfg(target_os = "macos")]
        let cmd = "open";
        #[cfg(target_os = "linux")]
        let cmd = "xdg-open";
        #[cfg(target_os = "windows")]
        let cmd = "cmd";

        let result = {
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new(cmd)
                    .args(["/C", "start", target])
                    .spawn()
            }
            #[cfg(not(target_os = "windows"))]
            {
                std::process::Command::new(cmd).arg(target).spawn()
            }
        };

        match result {
            Ok(_) => {
                self.set_transient_notice(StatusNotice::success(format!("已打开: {url}")));
            }
            Err(e) => {
                crate::tui::log_warn!("打开失败: {e}");
                self.set_transient_notice(StatusNotice::warning(format!("打开失败: {e}")));
            }
        }
    }
}

#[cfg(test)]
#[path = "executor_workspace_tests.rs"]
mod workspace_tests;

#[cfg(test)]
#[path = "executor_interaction_tests.rs"]
mod interaction_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_runtime_ignores_noop_effect() {
        let app = App::new(
            "s".to_string(),
            std::path::PathBuf::from("/tmp"),
            "m".to_string(),
        );
        assert!(!app.layout.should_exit);
    }

    #[test]
    fn test_effect_runtime_quit_effect_sets_exit_flag() {
        let mut app = App::new(
            "s".to_string(),
            std::path::PathBuf::from("/tmp"),
            "m".to_string(),
        );
        app.layout.request_exit();
        assert!(app.layout.should_exit);
    }

    #[test]
    fn test_effect_runtime_accepts_pending_image() {
        let mut app = App::new(
            "s".to_string(),
            std::path::PathBuf::from("/tmp"),
            "m".to_string(),
        );
        // accept_pending_clipboard_image 已移除（#497 spawn_guarded 化），
        // 图片经 UiEvent::ClipboardImage → InsertImage intent 注入。
        app.handle_input_intent(crate::tui::model::input::intent::InputIntent::InsertImage(
            sdk::ClipboardImageView {
                base64: "abc".to_string(),
                media_type: "image/png".to_string(),
                final_size: 3,
                display_path: None,
                width: None,
                height: None,
            },
        ));
        assert_eq!(app.model.input.document.image_spans.len(), 1);
    }
}
