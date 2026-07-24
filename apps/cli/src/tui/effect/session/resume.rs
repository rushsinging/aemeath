use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiResumedSessionStep};
use crate::tui::app::App;
use crate::tui::model::input::intent::InputIntent;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::update::intent::AgentIntent;

impl App {
    pub(crate) fn resume_session_messages(
        &mut self,
        session_id: &str,
        steps: Vec<TuiResumedSessionStep>,
        created_at: String,
    ) {
        let messages = steps
            .iter()
            .flat_map(|step| step.messages.iter().cloned())
            .collect::<Vec<_>>();
        let msg_count = messages.len();
        self.session.session_created_at = Some(created_at);
        self.session.rename_session(session_id);
        // session_id 真相归 SessionModel，StatusBar 渲染时直接消费 StatusViewModel。
        self.apply_agent_intent(AgentIntent::Session(SessionIntent::SetCurrentSession {
            id: session_id.to_string(),
        }));
        self.handle_input_intent(crate::tui::model::input::intent::InputIntent::Clear);
        // 走 ResumeConversation intent，不触发 spinner 副作用
        self.apply_agent_intent(AgentIntent::Conversation(
            crate::tui::model::conversation::intent::ConversationIntent::ResumeConversation(
                crate::tui::model::conversation::intent::ResumeConversation { steps },
            ),
        ));
        apply_resume_input_history(self, &messages);
        self.append_system_notice(format!(
            "[resumed session {} ({} messages)]",
            session_id, msg_count
        ));
        self.mark_output_dirty();
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
            }],
            "2026-01-01T00:00:00Z".to_string(),
        );

        assert!(
            !app.model.conversation.runtime.spinner.chat_active,
            "SessionResumed 仅恢复历史，不能表示 Runtime 正在执行"
        );
        assert_eq!(app.model.conversation.runtime.spinner.phase, None);
        assert_eq!(app.model.conversation.runtime.spinner.running_tool_count, 0);
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
