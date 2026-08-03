use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiResumedSessionStep};
use crate::tui::app::App;
use crate::tui::model::input::intent::InputIntent;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::update::intent::AgentIntent;

impl App {
    pub(crate) fn restore_startup_backing(&mut self, resume: sdk::LocalSessionResumeBacking) {
        let session_id = resume.session_id.clone();
        let created_at = resume.created_at.to_string();
        let backing =
            crate::tui::model::conversation::resumed_history::ResumedHistoryBacking::from_sdk(
                resume,
            );
        let message_count = backing.message_count();
        let input_history = backing.user_input_history();
        self.session.session_created_at = Some(created_at);
        self.session.rename_session(&session_id);
        self.apply_agent_intent(AgentIntent::Session(SessionIntent::SetCurrentSession {
            id: session_id.clone(),
        }));
        self.handle_input_intent(InputIntent::Clear);
        self.model.conversation.reset();
        self.model.display_history.replace(backing);
        self.apply_agent_intent(AgentIntent::Input(InputIntent::ReplaceHistory(
            input_history,
        )));
        self.append_system_notice(format!(
            "[resumed session {} ({} messages)]",
            session_id, message_count
        ));
        self.mark_output_dirty();
    }

    pub(crate) fn restore_startup_session(&mut self, resume: sdk::SessionResumeView) {
        crate::tui::log_debug!(
            "resume_lifecycle boundary=tui_startup stage=view_received session_id={} steps={} messages={}",
            resume.session_id,
            resume.steps.len(),
            resume.steps.iter().map(|step| step.messages.len()).sum::<usize>()
        );
        let steps = resume
            .steps
            .into_iter()
            .map(|step| TuiResumedSessionStep {
                run_id: step.run_id,
                step_id: step.step_id,
                messages: step
                    .messages
                    .into_iter()
                    .map(crate::tui::adapter::event_mapping::chat_message)
                    .collect(),
                finalize_cause: step.finalize_cause.map(|cause| match cause {
                    sdk::ResumedStepFinalizeCause::Completed => crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::Completed,
                    sdk::ResumedStepFinalizeCause::UserCancelledStep => crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::UserCancelledStep,
                    sdk::ResumedStepFinalizeCause::RunTerminated => crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::RunTerminated,
                }),
                duration_ms: step.duration_ms,
            })
            .collect();
        self.resume_session_messages(
            &resume.session_id,
            steps,
            None,
            resume.created_at.to_string(),
            resume.compacted,
        );
    }

    pub(crate) fn resume_session_messages(
        &mut self,
        session_id: &str,
        steps: Vec<TuiResumedSessionStep>,
        display_history: Option<crate::tui::adapter::runtime_view::TuiDisplayHistoryIndex>,
        created_at: String,
        compacted: bool,
    ) {
        let messages = steps
            .iter()
            .flat_map(|step| step.messages.iter().cloned())
            .collect::<Vec<_>>();
        let input_history = if display_history.is_some() {
            display_history
                .as_ref()
                .map(|index| {
                    index
                        .steps
                        .iter()
                        .flat_map(|step| step.user_input_history.iter().cloned())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            extract_user_input_history(&messages)
        };
        let msg_count = messages.len();
        let last_role = messages
            .last()
            .map(|message| message.role.as_str())
            .unwrap_or("-");
        let last_text_len = messages
            .last()
            .map(|message| message.text_content().len())
            .unwrap_or(0);
        crate::tui::log_debug!(
            "resume_lifecycle boundary=tui_resume_model stage=apply_started session_id={} steps={} messages={} last_role={} last_text_len={}",
            session_id,
            steps.len(),
            msg_count,
            last_role,
            last_text_len
        );
        self.session.session_created_at = Some(created_at);
        self.session.rename_session(session_id);
        // session_id 真相归 SessionModel，StatusBar 渲染时直接消费 StatusViewModel。
        self.apply_agent_intent(AgentIntent::Session(SessionIntent::SetCurrentSession {
            id: session_id.to_string(),
        }));
        self.handle_input_intent(crate::tui::model::input::intent::InputIntent::Clear);
        if let Some(index) = display_history {
            self.model.conversation.reset();
            self.model.display_history.replace(
                crate::tui::model::conversation::resumed_history::ResumedHistoryBacking::from_tui_index(
                    index,
                ),
            );
        } else {
            // 走 ResumeConversation intent，不触发 spinner 副作用
            self.apply_agent_intent(AgentIntent::Conversation(
                crate::tui::model::conversation::intent::ConversationIntent::ResumeConversation(
                    crate::tui::model::conversation::intent::ResumeConversation { steps },
                ),
            ));
        }
        if compacted {
            self.append_system_notice("✓ 上下文压缩完成");
        }
        self.apply_agent_intent(AgentIntent::Input(InputIntent::ReplaceHistory(
            input_history,
        )));
        self.append_system_notice(format!(
            "[resumed session {} ({} messages)]",
            session_id, msg_count
        ));
        self.mark_output_dirty();
        crate::tui::log_debug!(
            "resume_lifecycle boundary=tui_resume_model stage=apply_completed session_id={} timeline_items={} chats={} revision={} dirty_output={}",
            session_id,
            self.model.conversation.timeline.items().len(),
            self.model.conversation.chats.len(),
            self.model.conversation.revision(),
            self.view_state.dirty.output
        );
    }
}

pub(crate) fn apply_resume_input_history(app: &mut App, messages: &[TuiChatMessage]) {
    let history = extract_user_input_history(messages);
    app.apply_agent_intent(AgentIntent::Input(InputIntent::ReplaceHistory(history)));
}

fn extract_user_input_history(messages: &[TuiChatMessage]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| message.is_user_input())
        .filter_map(extract_user_input_text)
        .filter(|text| !text.is_empty())
        .collect()
}

fn extract_user_input_text(message: &TuiChatMessage) -> Option<String> {
    let text = message.text_content();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiContentBlock, TuiMessageSource};
    use std::path::PathBuf;

    #[test]
    fn startup_backing_keeps_window_outside_payload_out_of_conversation_model() {
        let mut app = App::new(
            "session-bootstrap".to_string(),
            PathBuf::from("/tmp"),
            "model".to_string(),
        );
        let shared_messages: std::sync::Arc<[sdk::LocalResumeMessage]> = vec![
            sdk::LocalResumeMessage::user("first"),
            sdk::LocalResumeMessage {
                role: sdk::LocalResumeRole::Assistant,
                content: vec![sdk::LocalResumeContentBlock::Text {
                    text: "second".to_string(),
                }],
                metadata: None,
            },
        ]
        .into();
        app.restore_startup_backing(sdk::LocalSessionResumeBacking {
            steps: vec![sdk::LocalResumedSessionStep {
                run_id: "run-1".to_string(),
                step_id: "step-1".to_string(),
                message_segments: vec![std::sync::Arc::clone(&shared_messages)],
                finalize_cause: Some(sdk::ResumedStepFinalizeCause::Completed),
                duration_ms: Some(10),
            }],
            display_history: None,
            session_id: "session-resumed".to_string(),
            created_at: 42,
            compacted: false,
        });

        assert_eq!(app.model.conversation.chats.len(), 0);
        assert_eq!(app.model.display_history.steps().len(), 1);
        assert!(std::sync::Arc::ptr_eq(
            &shared_messages,
            &app.model
                .display_history
                .step(0)
                .expect("history step")
                .message_segments[0]
        ));
    }

    #[test]
    fn startup_view_restores_history_without_runtime_resume_event() {
        let mut app = App::new(
            "session-bootstrap".to_string(),
            PathBuf::from("/tmp"),
            "model".to_string(),
        );
        app.restore_startup_session(sdk::SessionResumeView {
            steps: vec![sdk::ResumedSessionStep {
                run_id: "run-1".to_string(),
                step_id: "step-1".to_string(),
                messages: vec![sdk::ChatMessage::assistant_text("P5 progress is preserved")],
                finalize_cause: None,
                duration_ms: None,
            }],
            session_id: "session-resumed".to_string(),
            created_at: 42,
            compacted: false,
        });

        assert_eq!(app.session.session_id(), "session-resumed");
        assert_eq!(app.model.conversation.timeline.items().len(), 2);
        assert!(app.model.conversation.revision() > 0);
    }

    #[test]
    fn startup_index_restores_input_history_from_all_display_steps() {
        let mut app = App::new(
            "session-bootstrap".to_string(),
            PathBuf::from("/tmp"),
            "model".to_string(),
        );
        app.restore_startup_backing(sdk::LocalSessionResumeBacking {
            steps: Vec::new(),
            display_history: Some(sdk::DisplayHistoryIndex {
                session_id: "session-resumed".to_string(),
                generation_revision: 42,
                steps: vec![
                    sdk::DisplayHistoryStepReference {
                        run_id: "run-1".to_string(),
                        step_id: "step-1".to_string(),
                        member_name: "step-1.json".to_string(),
                        estimated_lines: 1,
                        user_input_history: vec!["older input".to_string()],
                        finalize_cause: None,
                        duration_ms: None,
                    },
                    sdk::DisplayHistoryStepReference {
                        run_id: "run-2".to_string(),
                        step_id: "step-2".to_string(),
                        member_name: "step-2.json".to_string(),
                        estimated_lines: 1,
                        user_input_history: vec!["latest input".to_string()],
                        finalize_cause: None,
                        duration_ms: None,
                    },
                ],
            }),
            session_id: "session-resumed".to_string(),
            created_at: 42,
            compacted: false,
        });

        assert_eq!(
            app.model.input.history.entries,
            vec!["older input".to_string(), "latest input".to_string()]
        );
        assert_eq!(app.model.input.history.selected_index, None);
        assert_eq!(app.model.input.history.saved_input, "");
    }

    #[test]
    fn test_extract_user_input_history_keeps_user_text_in_order() {
        let messages = vec![
            TuiChatMessage::user_text("first"),
            TuiChatMessage::assistant_text("answer"),
            TuiChatMessage::user_text("second"),
        ];

        let history = extract_user_input_history(&messages);

        assert_eq!(history, vec!["first".to_string(), "second".to_string()]);
    }

    #[test]
    fn test_extract_user_input_history_skips_empty_user_text() {
        let messages = vec![
            TuiChatMessage::user_text(""),
            TuiChatMessage::user_text("   "),
            TuiChatMessage::user_text("keep"),
        ];

        let history = extract_user_input_history(&messages);

        assert_eq!(history, vec!["keep".to_string()]);
    }

    #[test]
    fn test_extract_user_input_history_joins_text_blocks_only() {
        let messages = vec![TuiChatMessage {
            role: "user".to_string(),
            content: vec![
                TuiContentBlock::text("hello "),
                TuiContentBlock::Image {
                    media_type: "image/png".to_string(),
                    base64: "abc".to_string(),
                    placeholder: None,
                },
                TuiContentBlock::text("world"),
            ],
            source: TuiMessageSource::User,
            stop_hook: None,
            skill_request: None,
            input_id: None,
        }];

        let history = extract_user_input_history(&messages);

        assert_eq!(history, vec!["hello world".to_string()]);
    }

    #[test]
    fn resume_session_history_leaves_runtime_spinner_idle() {
        let mut app = App::new(
            "new-session".to_string(),
            PathBuf::from("/tmp/aemeath"),
            "test-model".to_string(),
        );

        app.resume_session_messages(
            "resumed-session",
            vec![TuiResumedSessionStep {
                run_id: "run-1".to_string(),
                step_id: "step-1".to_string(),
                messages: vec![
                    TuiChatMessage::user_text("历史问题"),
                    TuiChatMessage::assistant_text("历史回答"),
                ],
                finalize_cause: None,
                duration_ms: None,
            }],
            None,
            "2026-01-01T00:00:00Z".to_string(),
            false,
        );
        assert!(app
            .model
            .conversation
            .activity_observations()
            .activities()
            .is_empty());
    }
    #[test]
    fn test_apply_resume_input_history_populates_app_history() {
        let mut app = App::new(
            "new-session".to_string(),
            PathBuf::from("/tmp/aemeath"),
            "test-model".to_string(),
        );
        let messages = vec![
            TuiChatMessage::user_text("first"),
            TuiChatMessage::assistant_text("answer"),
            TuiChatMessage::user_text("second"),
        ];

        apply_resume_input_history(&mut app, &messages);

        assert_eq!(
            app.model.input.history.entries,
            vec!["first".to_string(), "second".to_string()]
        );
        assert_eq!(app.model.input.history.selected_index, None);
        assert_eq!(app.model.input.history.saved_input, "");
    }
}
