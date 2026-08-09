mod ask_user_key;
pub(crate) mod done;
mod enter;
mod key;
mod key_nav;
mod key_scroll;
mod notice;
mod reminder;
mod spawn_context;
mod ui_event;

pub(crate) use key::CTRL_C_TIMEOUT_SECS;

use super::event::UiEvent;
use crate::tui::adapter::agent_event::{map_agent_event_for_ui, map_runtime_event};
use crate::tui::adapter::tui_runtime_event::{TuiInteractionBody, TuiRuntimeEvent};
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::conversation::block::AskUserSlot;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::runtime::status_notice::StatusNotice;
use crate::tui::render::output::rendered::RenderedLineAnchor;
use crate::tui::render::output_area::SCROLLBAR_RESERVE_COLS;
use crate::tui::update::intent::AgentIntent;
use crate::tui::update::msg::TuiMsg;
use crate::tui::update::root_reducer::{reduce_agent_event, TuiUpdateResult};
use crate::tui::view_model::LiveStatusViewModel;
use tokio::sync::mpsc;

fn markdown_spacing_overrides_to_sdk(
    overrides: crate::tui::render::output::spacing::MarkdownSpacingOverrides,
) -> sdk::MarkdownSpacingOverridesView {
    fn element(
        value: Option<crate::tui::render::output::spacing::ElementSpacing>,
    ) -> Option<sdk::ElementSpacingView> {
        value.map(|spacing| sdk::ElementSpacingView {
            before: spacing.before,
            after: spacing.after,
        })
    }

    sdk::MarkdownSpacingOverridesView {
        paragraph: element(overrides.paragraph),
        heading: element(overrides.heading),
        list: element(overrides.list),
        code_block: element(overrides.code_block),
        table: element(overrides.table),
        blockquote: element(overrides.blockquote),
    }
}

fn ui_event_name(event: &UiEvent) -> &'static str {
    match event {
        UiEvent::SkillsUpdated(_) => "SkillsUpdated",
        UiEvent::Text { .. } => "Text",
        UiEvent::Thinking { .. } => "Thinking",
        UiEvent::BlockComplete { .. } => "BlockComplete",
        UiEvent::ToolCallStart { .. } => "ToolCallStart",
        UiEvent::ToolCallUpdate { .. } => "ToolCallUpdate",
        UiEvent::ToolResult { .. } => "ToolResult",
        UiEvent::Usage { .. } => "Usage",
        UiEvent::Error(_) => "Error",
        UiEvent::Cancelled { .. } => "Cancelled",
        UiEvent::TurnStarted { .. } => "TurnStarted",
        UiEvent::MicrocompactDone { .. } => "MicrocompactDone",
        UiEvent::SessionMessageStateChanged { .. } => "SessionMessageStateChanged",
        UiEvent::HookNotice(_) => "HookNotice",
        UiEvent::ApiError { .. } => "ApiError",
        UiEvent::CompactOperationRolledBack { .. } => "CompactRollback",
        UiEvent::CompactOperationCompleted { .. } => "CompactFinished",
        UiEvent::UserMessagesAdopted { .. } => "UserMessagesAdopted",
        UiEvent::UserMessagesQueued { .. } => "UserMessagesQueued",
        UiEvent::Done { .. } => "Done",
        UiEvent::DoneWithDuration { .. } => "DoneWithDuration",
        UiEvent::LiveTps(_) => "LiveTps",
        UiEvent::ClipboardImage(_) => "ClipboardImage",
        UiEvent::SystemMessage(_) => "SystemMessage",
        UiEvent::SessionSaved { .. } => "SessionSaved",
        UiEvent::ReflectionHistory { .. } => "ReflectionHistory",
        UiEvent::InteractionRequested { .. } => "InteractionRequested",
        UiEvent::WorkingDirectoryChanged { .. } => "WorkingDirectoryChanged",
        UiEvent::WorkspaceMetadataResolved(_) => "WorkspaceMetadataResolved",
        UiEvent::TaskStateChanged(_) => "TaskStateChanged",
        UiEvent::CurrentRunChanged(_) => "CurrentRunChanged",
        UiEvent::UpdateAvailable { .. } => "UpdateAvailable",
        UiEvent::SessionReset => "SessionReset",
        UiEvent::UserMessagesWithdrawn(_) => "UserMessagesWithdrawn",
        UiEvent::GraphPhaseChanged { .. } => "GraphPhaseChanged",
        UiEvent::ModelSwitched { .. } => "ModelSwitched",
        UiEvent::ThinkingChanged { .. } => "ThinkingChanged",
        UiEvent::ContextEstimated { .. } => "ContextEstimated",
        UiEvent::CommandResultText { .. } => "CommandResultText",
        UiEvent::SessionResumed { .. } => "SessionResumed",
        UiEvent::DisplayHistoryWindowLoaded { .. } => "DisplayHistoryWindowLoaded",
        UiEvent::DisplayHistoryWindowLoadFailed { .. } => "DisplayHistoryWindowLoadFailed",
        UiEvent::SessionResumeFailed { .. } => "SessionResumeFailed",
    }
}

pub(crate) fn output_visible_height(area_height: u16, live_status: &LiveStatusViewModel) -> usize {
    let spinner_line_count = usize::from(live_status.spinner.is_some());
    let task_line_count = if live_status.spinner.is_some() {
        live_status.task_lines.len()
    } else {
        0
    };
    // No-spinner path reserves exactly queued line count; empty queue naturally reserves 0.
    let reserved = if live_status.spinner.is_some() {
        live_status.queued_lines.len() + spinner_line_count + task_line_count
    } else {
        live_status.queued_lines.len()
    };
    (area_height as usize).saturating_sub(reserved)
}

/// Return type for update: effects plus optional slash command continuation.
pub struct UpdateResult {
    pub effects: Vec<Effect>,
    pub spawn_effect: Option<SpawnAgentChatEffect>,
    pub pending_slash: Option<String>,
}

impl UpdateResult {
    pub fn none() -> Self {
        Self {
            effects: Vec::new(),
            spawn_effect: None,
            pending_slash: None,
        }
    }

    pub fn one(effect: Effect) -> Self {
        Self {
            effects: vec![effect],
            spawn_effect: None,
            pending_slash: None,
        }
    }

    fn append(&mut self, mut other: Self) {
        self.effects.append(&mut other.effects);
        debug_assert!(
            other.spawn_effect.is_none(),
            "runtime events must not emit spawn effects"
        );
        debug_assert!(
            other.pending_slash.is_none(),
            "runtime events must not emit slash continuations"
        );
    }

    fn dedupe_render_requests(&mut self) {
        let mut saw_render_request = false;
        self.effects.retain(|effect| {
            if !matches!(effect, Effect::RequestRender) {
                return true;
            }
            if saw_render_request {
                false
            } else {
                saw_render_request = true;
                true
            }
        });
    }
}

impl App {
    /// TEA-style update: pure state transition based on a message.
    /// Returns commands for the runtime to execute.
    pub(crate) fn update(
        &mut self,
        msg: TuiMsg,
        ui_tx: &mpsc::Sender<UiEvent>,
        spawn_refs: &SpawnContextRefs,
    ) -> UpdateResult {
        match msg {
            TuiMsg::Ui(ev) => self.update_agent_event(ev, ui_tx, spawn_refs),
            TuiMsg::Runtime(ev) => self.update_runtime_event(ev),
            TuiMsg::RuntimeBatch(events) => {
                let mut batch_result = UpdateResult::none();
                for event in events {
                    batch_result.append(self.update_runtime_event(event));
                }
                batch_result.dedupe_render_requests();
                batch_result
            }
            TuiMsg::AgentEvent(ev) => self.update_agent_event(ev, ui_tx, spawn_refs),
            TuiMsg::Key(key) => self.update_key(key, spawn_refs),
            TuiMsg::Mouse(mouse) => {
                let history_window_before = (
                    self.view_state.output.render_line_limit(),
                    self.view_state.output.history_window_tail_offset,
                );
                let effects = self.handle_mouse_event(mouse, self.layout.output_area_rect);
                // 懒加载：预算增长或 3000 行窗口向更早历史滑动时都必须重建文档。
                let history_window_after = (
                    self.view_state.output.render_line_limit(),
                    self.view_state.output.history_window_tail_offset,
                );
                if history_window_after != history_window_before {
                    self.mark_output_dirty();
                    crate::tui::log_debug!(
                        "tui.output.scroll_dirty source=mouse_event reason=history_window_changed before_limit={} after_limit={} before_tail_offset={} after_tail_offset={} dirty_output=true",
                        history_window_before.0,
                        history_window_after.0,
                        history_window_before.1,
                        history_window_after.1
                    );
                }
                UpdateResult {
                    effects,
                    spawn_effect: None,
                    pending_slash: None,
                }
            }
            TuiMsg::Paste(text) if !self.chat.is_processing => {
                self.handle_paste_event(text, ui_tx);
                UpdateResult::none()
            }
            TuiMsg::Paste(text) => {
                // Paste while processing: insert into input area so it can be queued
                match sdk::classify_paste(&text) {
                    sdk::PasteKind::Empty => {
                        self.input.just_pasted = true;
                        // 删：[reading clipboard image...] —— 同 paste_handler.rs 路径（#fix-tui-image-input-output）
                        return UpdateResult::one(Effect::ReadClipboardImage);
                    }
                    sdk::PasteKind::ImageFile => {
                        // 删：[loading image: ...] —— 同上（#fix-tui-image-input-output）
                        self.input.just_pasted = true;
                        return UpdateResult::one(Effect::ProcessImageFile {
                            path: text.trim().to_string(),
                        });
                    }
                    sdk::PasteKind::Text => {
                        self.input.just_pasted = true;
                        self.handle_input_intent(
                            crate::tui::model::input::intent::InputIntent::InsertText(text),
                        );
                    }
                }
                UpdateResult::none()
            }
            TuiMsg::Resize { width, height } => {
                self.handle_resize(width, height);
                UpdateResult::none()
            }
            TuiMsg::SpinnerTick => {
                // 动画帧真相归 view_state；spinner 是否可见由 Model 决定，
                // 镜像写回统一在每帧渲染前的 refresh_live_status_from_model。
                let before_frame = self.view_state.animation.spinner_frame;
                let before_version = self.view_state.animation.version;
                self.view_state.animation.spinner_frame =
                    self.view_state.animation.spinner_frame.wrapping_add(1);
                self.view_state.animation.version =
                    self.view_state.animation.version.wrapping_add(1);
                let before_silent = self
                    .view_state
                    .run_activity
                    .is_model_silent(std::time::Instant::now());
                self.view_state.spinner.advance();
                self.view_state.run_activity.advance_frame();
                let after_silent = self
                    .view_state
                    .run_activity
                    .is_model_silent(std::time::Instant::now());
                if before_silent != after_silent || after_silent {
                    self.mark_output_dirty();
                }
                // 临时 status notice 过期检查：到期回退到 graph_phase 派生态。
                if self
                    .model
                    .conversation
                    .expire_transient_notice(std::time::Instant::now())
                {
                    self.mark_output_dirty();
                }
                let request_render = self.view_state.run_activity.is_active();
                crate::tui::log_trace!(
                    "tui.spinner.tick before_frame={} after_frame={} before_version={} after_version={} anim_frame={} active={} verb={} dirty_output={}",
                    before_frame,
                    self.view_state.animation.spinner_frame,
                    before_version,
                    self.view_state.animation.version,
                    self.view_state.spinner.frame,
                    self.view_state.run_activity.is_active(),
                    self.view_state.spinner.verb,
                    self.view_state.dirty.output
                );
                if request_render {
                    UpdateResult::one(Effect::RequestRender)
                } else {
                    UpdateResult::none()
                }
            }
            TuiMsg::TerminalKey(key) => self.update_key(key, spawn_refs),
            TuiMsg::TerminalMouse(mouse) => {
                let history_window_before = (
                    self.view_state.output.render_line_limit(),
                    self.view_state.output.history_window_tail_offset,
                );
                let effects = self.handle_mouse_event(mouse, self.layout.output_area_rect);
                let history_window_after = (
                    self.view_state.output.render_line_limit(),
                    self.view_state.output.history_window_tail_offset,
                );
                if history_window_after != history_window_before {
                    self.mark_output_dirty();
                    crate::tui::log_debug!(
                        "tui.output.scroll_dirty source=terminal_mouse reason=history_window_changed before_limit={} after_limit={} before_tail_offset={} after_tail_offset={} dirty_output=true",
                        history_window_before.0,
                        history_window_after.0,
                        history_window_before.1,
                        history_window_after.1
                    );
                }
                UpdateResult {
                    effects,
                    spawn_effect: None,
                    pending_slash: None,
                }
            }
            TuiMsg::TerminalResize { width, height } => {
                self.handle_resize(width, height);
                UpdateResult::none()
            }
            TuiMsg::EffectCompleted(_) | TuiMsg::TimerTick { .. } | TuiMsg::RenderTick => {
                UpdateResult::none()
            }
        }
    }

    fn update_runtime_event(&mut self, event: TuiRuntimeEvent) -> UpdateResult {
        let diagnostic_kind = match &event {
            TuiRuntimeEvent::AssistantTextDelta { .. } => Some("AssistantTextDelta"),
            TuiRuntimeEvent::BlockComplete { .. } => Some("BlockComplete"),
            TuiRuntimeEvent::UserMessagesAdopted { .. } => Some("UserMessagesAdopted"),
            TuiRuntimeEvent::HookNotice(_) => Some("HookNotice"),
            TuiRuntimeEvent::Done { .. } => Some("Done"),
            _ => None,
        };
        if let Some(kind) = diagnostic_kind {
            crate::tui::log_trace!(
                "event_delivery boundary=tui_channel_to_reducer kind={} outcome=received timeline_items={} queued={} revision={}",
                kind,
                self.model.conversation.timeline.items().len(),
                self.model.conversation.queued_submissions.len(),
                self.model.conversation.revision()
            );
        }
        // UserMessagesAdopted 需要在 mapper/reducer 之外执行清占位 + 用户回显，
        // 因为这些副作用依赖 App 级方法且不产生 Intent。
        match &event {
            TuiRuntimeEvent::SkillsUpdated {
                revision,
                skills,
                slash_routes,
            } => {
                self.set_tui_skill_snapshot(revision.clone(), skills.clone(), slash_routes.clone());
            }
            TuiRuntimeEvent::UserMessagesAdopted { items, queued } => {
                crate::tui::log_debug!(
                    "skill_request boundary=tui_adopted_event items={} queued={} skill_items={} user_items={}",
                    items.len(),
                    queued.len(),
                    items
                        .iter()
                        .filter(|item| matches!(item.source, crate::tui::adapter::runtime_view::TuiMessageSource::SkillRequest))
                        .count(),
                    items
                        .iter()
                        .filter(|item| matches!(item.source, crate::tui::adapter::runtime_view::TuiMessageSource::User))
                        .count()
                );
                for item in items {
                    if let Some(id) = item.input_id.as_ref() {
                        self.clear_queued_submission_echo_by_id(id);
                    }
                    match item.source {
                        crate::tui::adapter::runtime_view::TuiMessageSource::User => {
                            self.append_user_echo(item.text_content());
                        }
                        crate::tui::adapter::runtime_view::TuiMessageSource::SkillRequest => {
                            if let Some(payload) = item.skill_request.as_ref() {
                                crate::tui::log_debug!(
                                    "skill_request boundary=tui_adopted_item source=skill input_id={:?} content_blocks={} content_text_len={} metadata_present=true skill={} arguments_len={} raw_input_len={} raw_input_preview={:?}",
                                    item.input_id,
                                    item.content.len(),
                                    item.text_content().len(),
                                    payload.skill,
                                    payload.arguments.len(),
                                    payload.raw_input.len(),
                                    payload.raw_input.chars().take(120).collect::<String>()
                                );
                                self.append_user_echo(payload.raw_input.clone());
                            } else {
                                crate::tui::log_debug!(
                                    "skill_request boundary=tui_adopted_item source=skill input_id={:?} content_blocks={} content_text_len={} metadata_present=false action=skip_echo",
                                    item.input_id,
                                    item.content.len(),
                                    item.text_content().len()
                                );
                            }
                        }
                        crate::tui::adapter::runtime_view::TuiMessageSource::Hook => {
                            if let Some(notice) = item.hook_notice.as_ref() {
                                let mapping = crate::tui::adapter::agent_event::AgentEventMapping {
                                    conversation: vec![
                                        crate::tui::model::conversation::intent::ConversationIntent::AppendHookNotice(
                                            crate::tui::model::conversation::intent::AppendHookNotice {
                                                title: notice.title(),
                                                text: notice.display_text(),
                                                kind: notice.kind.clone(),
                                            },
                                        ),
                                    ],
                                    ..Default::default()
                                };
                                let reduced = crate::tui::update::root_reducer::reduce_agent_event(
                                    &mut self.model,
                                    mapping,
                                );
                                crate::tui::update::dirty::merge_dirty(
                                    &mut self.view_state.dirty,
                                    reduced.dirty,
                                );
                            }
                        }
                        crate::tui::adapter::runtime_view::TuiMessageSource::SystemGenerated => {}
                    }
                }
                // 用户消息已经成为已提交的会话尾部内容。即使 resume 后用户先向上
                // 浏览过历史，也必须把视图恢复到最新窗口，否则新消息只进入 model，
                // 仍会被旧的 history_window_tail_offset 裁掉。
                self.view_state.output.scroll_to_bottom();
                self.mark_output_dirty();
            }
            TuiRuntimeEvent::TurnStarted { .. } => {
                self.mark_output_dirty();
            }
            TuiRuntimeEvent::ApiError { error, .. } => {
                self.append_system_notice(error);
                self.mark_output_dirty();
            }
            TuiRuntimeEvent::CompactOperationCompleted { .. } => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::ClearCompactRuntime(ClearCompactRuntime),
                ));
            }
            TuiRuntimeEvent::CompactOperationRolledBack { .. } => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::ClearCompactRuntime(ClearCompactRuntime),
                ));
            }
            TuiRuntimeEvent::SessionReset => {
                return UpdateResult::one(Effect::ResetRuntimeState);
            }
            TuiRuntimeEvent::ConfigChanged { view, .. }
            | TuiRuntimeEvent::ConfigReloaded { view, .. } => {
                self.config_view.model_name = view.model_name.clone();
                self.config_view.provider = view.provider.clone();
                self.config_view.has_api_key = view.has_api_key;
                self.config_view.permission_mode = view.permission_mode.clone();
                self.config_view.markdown = view.markdown;
                self.config_view.verbose = view.verbose;
                self.config_view.context_size = view.context_size;
                self.config_view.logging_level = view.logging_level.clone();
                self.config_view.markdown_spacing = match view.markdown_spacing.mode() {
                    crate::tui::render::output::spacing::MarkdownSpacingMode::Normal => {
                        sdk::MarkdownSpacingModeView::Normal
                    }
                    crate::tui::render::output::spacing::MarkdownSpacingMode::Compact => {
                        sdk::MarkdownSpacingModeView::Compact
                    }
                };
                self.config_view.markdown_spacing_overrides =
                    markdown_spacing_overrides_to_sdk(view.markdown_spacing.overrides());
            }
            TuiRuntimeEvent::SessionResumed {
                steps,
                display_history,
                session_id,
                created_at,
                compacted,
            } => {
                crate::tui::log_debug!(
                    "resume_lifecycle boundary=tui_runtime stage=session_resumed_received session_id={} steps={} messages={}",
                    session_id,
                    steps.len(),
                    steps.iter().map(|step| step.messages.len()).sum::<usize>()
                );
                self.resume_session_messages(
                    session_id,
                    steps.clone(),
                    display_history.clone(),
                    created_at.to_string(),
                    *compacted,
                );
                crate::tui::log_debug!(
                    "resume_lifecycle boundary=tui_runtime stage=session_resumed_applied session_id={} timeline_items={} chats={} revision={}",
                    session_id,
                    self.model.conversation.timeline.items().len(),
                    self.model.conversation.chats.len(),
                    self.model.conversation.revision()
                );
                return UpdateResult {
                    effects: Vec::new(),
                    spawn_effect: None,
                    pending_slash: None,
                };
            }
            TuiRuntimeEvent::InteractionRequested(ref req) => {
                self.mark_output_dirty();
                // 桥接到已有的 ask_user_batch inline block 渲染
                if let TuiInteractionBody::UserQuestions(questions) = &req.body {
                    let slots: Vec<AskUserSlot> = questions
                        .iter()
                        .enumerate()
                        .map(|(i, q)| {
                            let llm_count = q.options.len();
                            let mut options: Vec<sdk::OptionItem> = q
                                .options
                                .iter()
                                .map(|o| sdk::OptionItem::title_only(o.clone()))
                                .collect();
                            // 追加 "Type something..." 内建选项 —— cursor 超出
                            // llm_option_count 时切换到自由输入子态
                            options.push(sdk::OptionItem::title_only(
                                "Type something… (自由输入)".to_string(),
                            ));
                            AskUserSlot {
                                id: req
                                    .tool_call_id
                                    .clone()
                                    .unwrap_or_else(|| format!("{}-{i}", req.request_id.as_str())),
                                question_seq: i,
                                question: q.prompt.clone(),
                                options,
                                llm_option_count: llm_count,
                                multi_select: q.allow_multi,
                                default: None,
                                answer: None,
                            }
                        })
                        .collect();
                    self.show_ask_user_batch(req.request_id.clone(), slots);
                }
            }
            TuiRuntimeEvent::RunStep {
                run_id,
                parent_run_id: None,
                step_id,
                event: crate::tui::adapter::tui_runtime_event::TuiRunStepEvent::Started,
            } => {
                self.chat.active_run_step = Some((
                    sdk::RunId::from_legacy_or_new(run_id.as_str()),
                    sdk::RunStepId::from_legacy_or_new(step_id.as_str()),
                ));
            }
            TuiRuntimeEvent::Done { .. } | TuiRuntimeEvent::Cancelled { .. } => {
                // Done/Cancelled 只收敛 App 级 processing；活动展示由 typed Run status 收敛。
                self.chat.active_run_step = None;
                self.chat.stop_processing();
                self.mark_output_dirty();
            }
            _ => {}
        }
        let mapping = map_runtime_event(&event);
        if let Some(kind) = diagnostic_kind {
            crate::tui::log_trace!(
              "event_delivery boundary=tui_mapper kind={} outcome=mapped conversation_intents={} diagnostic_intents={} session_intents={}",
              kind,
              mapping.conversation.len(),
              mapping.diagnostic.len(),
              mapping.session.len()
          );
        }
        let model_result = reduce_agent_event(&mut self.model, mapping);
        self.refresh_live_status_from_model();
        let valid_model_activity = match &event {
            TuiRuntimeEvent::AssistantTextDelta { delta, .. }
            | TuiRuntimeEvent::ThinkingDelta { delta, .. } => !delta.is_empty(),
            TuiRuntimeEvent::ToolCallStarted { .. } => true,
            TuiRuntimeEvent::ToolCallArgumentsDelta { delta, .. } => !delta.is_empty(),
            TuiRuntimeEvent::ToolCallStateChanged { arguments, .. } => arguments.is_some(),
            _ => false,
        };
        if valid_model_activity {
            let active_run_id = self
                .model
                .conversation
                .activity_observations()
                .activities()
                .iter()
                .find(|activity| {
                    activity.kind == crate::tui::adapter::tui_runtime_event::TuiActivityKind::Run
                        && matches!(
                            activity.detail,
                            crate::tui::adapter::tui_runtime_event::TuiActivityDetail::Run {
                                purpose:
                                    crate::tui::adapter::tui_runtime_event::TuiRunPurpose::Main
                            }
                        )
                })
                .map(|activity| activity.run_id.clone());
            if let Some(run_id) = active_run_id.as_ref() {
                if self
                    .view_state
                    .run_activity
                    .observe_main_model_activity(run_id, std::time::Instant::now())
                {
                    self.mark_output_dirty();
                }
            }
        }
        if let Some(kind) = diagnostic_kind {
            crate::tui::log_trace!(
                "event_delivery boundary=tui_reducer kind={} outcome=reduced timeline_items={} queued={} revision={} dirty_output={} effects={}",
                kind,
                self.model.conversation.timeline.items().len(),
                self.model.conversation.queued_submissions.len(),
                self.model.conversation.revision(),
                model_result.dirty.output,
                model_result.effects.len()
            );
        }
        crate::tui::update::dirty::merge_dirty(&mut self.view_state.dirty, model_result.dirty);
        UpdateResult {
            effects: model_result.effects,
            spawn_effect: None,
            pending_slash: None,
        }
    }

    fn update_agent_event(
        &mut self,
        ev: UiEvent,
        ui_tx: &mpsc::Sender<UiEvent>,
        spawn_refs: &SpawnContextRefs,
    ) -> UpdateResult {
        let mapping = map_agent_event_for_ui(&ev);
        crate::tui::log_trace!(
            "tui.agent_event mapped event={} conversation_intents={} diagnostic_intents={} session_intents={}",
            ui_event_name(&ev),
            mapping.conversation.len(),
            mapping.diagnostic.len(),
            mapping.session.len()
        );
        let model_result = if mapping == Default::default() {
            TuiUpdateResult::default()
        } else {
            reduce_agent_event(&mut self.model, mapping)
        };
        crate::tui::log_trace!(
            "tui.agent_event reduced event={} dirty_output={} dirty_status={} dirty_dialog={} dirty_input={} effects={} timeline_items={} chats={}",
            ui_event_name(&ev),
            model_result.dirty.output,
            model_result.dirty.status,
            model_result.dirty.dialog,
            model_result.dirty.input,
            model_result.effects.len(),
            self.model.conversation.timeline.items().len(),
            self.model.conversation.chats.len()
        );
        let mut result = self.update_ui(ev, ui_tx, spawn_refs);
        crate::tui::update::dirty::merge_dirty(&mut self.view_state.dirty, model_result.dirty);
        result.effects.extend(model_result.effects);
        result
    }

    pub(crate) fn output_document_width(&self) -> u16 {
        self.layout
            .output_area_rect
            .width
            .saturating_sub(SCROLLBAR_RESERVE_COLS)
            .max(1)
    }

    pub(crate) fn refresh_output_document_from_model(&mut self) -> Option<Effect> {
        let before_lines = self.output_area.document().total_lines();
        let revision = self.model.conversation.revision();
        let current_workspace_root: Option<String> = self
            .model
            .workspace_provider
            .workspace_root()
            .map(ToOwned::to_owned);
        let width = self.output_document_width();
        let cache = &mut self.output_view;
        let workspace_root = current_workspace_root.as_deref().map(std::path::Path::new);
        let requested_window = crate::tui::view_model::OutputRenderWindow {
            line_limit: self.view_state.output.render_line_limit(),
            tail_offset: self.view_state.output.history_window_tail_offset,
        };
        let requested_window = if requested_window.line_limit >= usize::MAX / 2 {
            crate::tui::view_model::OutputRenderWindow::all()
        } else {
            requested_window
        };
        let materialized = cache.retained.materialize_window(
            &self.model.conversation,
            &self.model.display_history,
            workspace_root,
            requested_window,
        );
        let indexed_items = materialized.indexed_items;
        if let Some(request) = materialized.missing_history_request {
            let request_key = (
                request.session_id.clone(),
                request.generation_revision,
                request.member_names.clone(),
            );
            if cache.loading_history_window.as_ref() != Some(&request_key) {
                cache.loading_history_window = Some(request_key);
                return Some(Effect::LoadDisplayHistoryWindow { request });
            }
        } else {
            cache.loading_history_window = None;
        }
        let sync_stats = materialized.stats;
        #[cfg(test)]
        crate::tui::render::performance::record_retained_view_sync(
            sync_stats.touched_roots,
            sync_stats.created_roots,
            sync_stats.reused_roots,
            sync_stats.rebuilt_roots,
        );
        let need_rebuild = sync_stats.did_rebuild || sync_stats.touched_roots > 0;
        if need_rebuild {
            self.assemble_count = self.assemble_count.saturating_add(1);
        }
        let view_model = &materialized.view_model;
        let root_count = view_model.roots.len();
        // 文档构建（含各 block 的字符串处理）放在 draw 之外，draw 循环的 catch_unwind
        let rebuilt_anchor: Option<(RenderedLineAnchor, usize)> =
            if !self.view_state.output.auto_scroll {
                let old_total_lines = self.output_area.document().total_lines();
                let old_max_start =
                    old_total_lines.saturating_sub(self.view_state.output.last_visible_height);
                let old_visible_start = old_max_start
                    .saturating_sub(self.view_state.output.scroll_offset)
                    .min(old_max_start);
                let old_visible_end = old_visible_start
                    .saturating_add(self.view_state.output.last_visible_height)
                    .min(old_total_lines);
                self.output_area
                    .document()
                    .stable_line_anchor_in_range(old_visible_start..old_visible_end)
            } else {
                None
            };
        crate::tui::log_debug!(
            "tui.output.window_rebuild stage=request revision={} width={} term_width={} indexed_items={} roots={} before_document_lines={} requested_line_limit={} requested_tail_offset={} scroll_offset={} auto_scroll={} visible_height={} source_total_lines={} need_view_model_rebuild={}",
            revision,
            width,
            self.output_area.term_width,
            indexed_items,
            root_count,            before_lines,
            requested_window.line_limit,
            requested_window.tail_offset,
            self.view_state.output.scroll_offset,
            self.view_state.output.auto_scroll,
            self.view_state.output.last_visible_height,
            self.view_state.output.source_total_lines,
            need_rebuild
        );
        let render_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.output_document_renderer.render_model_window(
                view_model,
                width,
                self.output_area.term_width,
                self.view_state.animation.spinner_frame,
                self.model.ui_preferences.markdown_spacing(),
                requested_window,
            )
        }));
        let document = match render_result {
            Ok(result) => {
                let source_total_lines = result.source_total_lines;
                let result_document_lines = result.document.total_lines();
                self.view_state
                    .output
                    .observe_source_document(source_total_lines);
                if let Some((anchor, screen_row)) = rebuilt_anchor.as_ref() {
                    if let Some(anchor_line) = result.document.line_index_for_anchor(anchor) {
                        self.view_state.output.pin_rebuilt_document_to_anchor(
                            result_document_lines,
                            anchor_line.saturating_sub(*screen_row),
                        );
                    } else {
                        crate::tui::log_debug!(
                            "tui.output.window_anchor matched=false result_document_lines={} scroll_offset={} tail_offset={}",
                            result_document_lines,
                            self.view_state.output.scroll_offset,
                            self.view_state.output.history_window_tail_offset
                        );
                    }
                }
                let window_changed = self.view_state.output.render_line_limit()
                    != requested_window.line_limit
                    || self.view_state.output.history_window_tail_offset
                        != requested_window.tail_offset;
                if window_changed {
                    self.mark_output_dirty();
                }
                crate::tui::log_debug!(
                    "tui.output.window_rebuild stage=result requested_line_limit={} requested_tail_offset={} source_total_lines={} result_document_lines={} after_line_limit={} after_tail_offset={} window_changed={} dirty_output={} scroll_offset={} auto_scroll={}",
                    requested_window.line_limit,
                    requested_window.tail_offset,
                    source_total_lines,
                    result_document_lines,
                    self.view_state.output.render_line_limit(),
                    self.view_state.output.history_window_tail_offset,
                    window_changed,
                    self.view_state.dirty.output,
                    self.view_state.output.scroll_offset,
                    self.view_state.output.auto_scroll
                );
                result.document
            }
            Err(_) => {
                crate::tui::log_warn!(
                    "tui.output.refresh_document panicked; keeping previous document"
                );
                self.apply_agent_intent(crate::tui::update::intent::AgentIntent::Conversation(
                    ConversationIntent::SetStatusNotice(SetStatusNotice(StatusNotice::warning(
                        "渲染失败，已记录 panic.log",
                    ))),
                ));
                return None;
            }
        };
        crate::tui::log_trace!(
            "tui.output.history_metrics source_lines={} render_limit={} tail_offset={} before_lines={} scroll_offset={} auto_scroll={} visible_height={} pending_load_older={}",
            self.view_state.output.source_total_lines,
            self.view_state.output.render_line_limit(),
            self.view_state.output.history_window_tail_offset,
            before_lines,
            self.view_state.output.scroll_offset,
            self.view_state.output.auto_scroll,
            self.view_state.output.last_visible_height,
            self.view_state.output.pending_load_older
        );
        let after_lines = document.total_lines();
        crate::tui::log_trace!(
            "tui.output.refresh_document revision={} width={} term_width={} spinner_frame={} roots={} timeline_items={} chats={} before_lines={} after_lines={} rebuilt={}",
            revision,
            width,
            self.output_area.term_width,            self.view_state.animation.spinner_frame,
            root_count,
            self.model.conversation.timeline.items().len(),
            self.model.conversation.chats.len(),
            before_lines,
            after_lines,
            need_rebuild
        );
        self.output_area.replace_document(document);
        None
    }

    pub(crate) fn flush_dirty_view_models(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.view_state.dirty.output {
            if let Some(effect) = self.refresh_output_document_from_model() {
                effects.push(effect);
            }
            self.view_state.dirty.clear_output();
        }
        if self.view_state.dirty.status {
            self.view_state.dirty.clear_status();
        }
        effects
    }
    pub(crate) fn apply_agent_intent(
        &mut self,
        intent: crate::tui::update::intent::AgentIntent,
    ) -> crate::tui::update::root_reducer::TuiUpdateResult {
        let result = crate::tui::update::root_reducer::reduce_intent(&mut self.model, intent);
        crate::tui::update::dirty::merge_dirty(&mut self.view_state.dirty, result.dirty.clone());
        result
    }

    pub(crate) fn mark_output_dirty(&mut self) {
        self.view_state.dirty.mark_output();
    }

    pub(crate) fn status_view_model(&self) -> crate::tui::view_model::StatusViewModel {
        crate::tui::view_assembler::status::StatusViewAssembler::assemble_status_view(
            &self.model.conversation,
            &self.model.runtime_presentation,
            &self.model.workspace_provider,
            Some(&self.model.session),
            &self.model.diagnostic,
            &self.config_view.permission_mode,
        )
    }

    pub(crate) fn dialog_view_model(&self) -> Option<crate::tui::view_model::DialogViewModel> {
        crate::tui::view_assembler::dialog::DialogViewAssembler::assemble_from_diagnostic(
            &self.model.diagnostic,
        )
    }

    /// 据 typed Main Run snapshot、task lines、queued submissions 与纯动画态
    /// 派生实时状态行 ViewModel。
    pub(crate) fn live_status_view_model(&self) -> crate::tui::view_model::LiveStatusViewModel {
        let queued_texts: Vec<String> = self
            .model
            .conversation
            .queued_submissions
            .iter()
            .map(|q| q.text.clone())
            .collect();
        crate::tui::view_assembler::live_status::LiveStatusAssembler::assemble(
            &self.model.conversation,
            &self.view_state.run_activity,
            &self.view_state.spinner,
            &queued_texts,
        )
    }

    /// 渲染前维护 live-status 相关 view_state：
    /// - active 且 verb 为空时选择动词；
    /// - active 时同步 phase，phase 变化只重置 phase 计时；
    /// - inactive 时清空动画状态，保证下次激活重新计时。
    ///
    /// OutputArea render 直接消费 `live_status_view_model()`，不再写 widget mirror。
    ///
    /// verb/active 检测属 effectful 边界（rng/激活检测），故放在此渲染前的副作用处，
    /// 而非纯 reducer。
    pub(crate) fn refresh_live_status_from_model(&mut self) {
        let activity_summary =
            crate::tui::view_assembler::activity_summary::ActivitySummaryAssembler::assemble(
                self.model.conversation.activity_observations(),
            );
        if let Some(summary) = activity_summary.as_ref() {
            let state = &self.view_state.run_activity;
            let root_changed = state.root_timing_identity()
                != Some((
                    summary.root_activity_id.as_str(),
                    summary.root_timing_revision,
                ));
            let primary_changed = state.phase_timing_identity()
                != Some((
                    summary.primary_activity_id.as_str(),
                    summary.phase_timing_revision,
                ));
            if root_changed || primary_changed {
                crate::tui::log_debug!(
                    "[ACTIVITY_TIMING] summary_selected run_id={} root_activity_id={} root_revision={} total_elapsed_ms={} primary_activity_id={} phase_revision={} phase_elapsed_ms={} root_changed={} phase_changed={}",
                    summary.run_id.as_str(),
                    summary.root_activity_id,
                    summary.root_timing_revision,
                    summary.total_elapsed_ms,
                    summary.primary_activity_id,
                    summary.phase_timing_revision,
                    summary.phase_elapsed_ms,
                    root_changed,
                    primary_changed,
                );
            }
        }
        self.view_state
            .run_activity
            .sync_activity_summary(activity_summary.as_ref(), std::time::Instant::now());
        if self.view_state.run_activity.verb.is_empty() && activity_summary.is_some() {
            self.view_state.spinner.pick_verb();
            self.view_state.run_activity.verb = self.view_state.spinner.verb.clone();
        }
        self.view_state.run_activity.frame = self.view_state.spinner.frame;
    }
    /// 根据当前 document 与 layout/live-status 投影同步 OutputViewState 滚动真相。
    /// 每帧渲染前调用；OutputArea render 直接消费 view_state.output，不再写 widget 镜像。
    pub(crate) fn refresh_output_scroll_from_view_state(&mut self) {
        let visible_height = output_visible_height(
            self.layout.output_area_rect.height,
            &self.live_status_view_model(),
        );
        self.view_state
            .output
            .sync_document_metrics(self.output_area.document().total_lines(), visible_height);
        // #70 phase 2：output selection/scroll render 直接消费 view_state.output，无 widget 镜像写回。
        // #70 phase 2：status 选区 render 直接消费 view_state.status_sel，无 widget 镜像写回。
        // #70 phase 2：input 选区 render 直接消费 view_state.input_sel，无 widget 镜像写回。
    }
}

/// Type alias so update.rs can use `App` without circular path
use super::App;
