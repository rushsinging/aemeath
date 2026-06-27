use crate::tui::app::App;
use crate::tui::model::input::intent::InputIntent;
use crate::tui::model::runtime::session_intent::SessionIntent;

impl App {
    pub(crate) fn resume_session_messages(
        &mut self,
        session_id: &str,
        messages: Vec<sdk::ChatMessage>,
        created_at: String,
    ) {
        let msg_count = messages.len();
        self.session.session_created_at = Some(created_at);
        self.session.rename_session(session_id);
        // session_id 真相归 SessionModel，StatusBar 渲染时直接消费 StatusViewModel。
        self.model.session.apply(SessionIntent::SetCurrentSession {
            id: session_id.to_string(),
        });
        self.chat.messages.clear();
        self.handle_input_intent(crate::tui::model::input::intent::InputIntent::Clear);
        for (i, message) in messages.iter().enumerate() {
            let subsequent = messages.get(i + 1);
            self.render_history_message(message, subsequent);
        }
        self.chat.messages = messages;
        apply_resume_input_history(self, &self.chat.messages.clone());
        self.append_system_notice(format!(
            "[resumed session {} ({} messages)]",
            session_id, msg_count
        ));
    }
}

pub(crate) fn apply_resume_input_history(app: &mut App, messages: &[sdk::ChatMessage]) {
    let history = extract_user_input_history(messages);
    app.model.input.apply(InputIntent::ReplaceHistory(history));
}

fn extract_user_input_history(messages: &[sdk::ChatMessage]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| message.role == "user")
        .filter_map(extract_user_input_text)
        .filter(|text| !text.is_empty())
        .collect()
}

fn extract_user_input_text(message: &sdk::ChatMessage) -> Option<String> {
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
    use std::path::PathBuf;

    #[test]
    fn test_extract_user_input_history_keeps_user_text_in_order() {
        let messages = vec![
            sdk::ChatMessage::user_text("first"),
            sdk::ChatMessage::assistant_text("answer"),
            sdk::ChatMessage::user_text("second"),
        ];

        let history = extract_user_input_history(&messages);

        assert_eq!(history, vec!["first".to_string(), "second".to_string()]);
    }

    #[test]
    fn test_extract_user_input_history_skips_empty_user_text() {
        let messages = vec![
            sdk::ChatMessage::user_text(""),
            sdk::ChatMessage::user_text("   "),
            sdk::ChatMessage::user_text("keep"),
        ];

        let history = extract_user_input_history(&messages);

        assert_eq!(history, vec!["keep".to_string()]);
    }

    #[test]
    fn test_extract_user_input_history_joins_text_blocks_only() {
        let messages = vec![sdk::ChatMessage {
            role: "user".to_string(),
            content: vec![
                sdk::ContentBlock::text("hello "),
                sdk::ContentBlock::Image {
                    source: sdk::ImageSource::Base64 {
                        media_type: "image/png".to_string(),
                        data: "abc".to_string(),
                    },
                    placeholder: None,
                },
                sdk::ContentBlock::text("world"),
            ],
            metadata: None,
            input_id: None,
        }];

        let history = extract_user_input_history(&messages);

        assert_eq!(history, vec!["hello world".to_string()]);
    }

    #[test]
    fn test_apply_resume_input_history_populates_app_history() {
        let mut app = App::new(
            "new-session".to_string(),
            PathBuf::from("/tmp/aemeath"),
            "test-model".to_string(),
        );
        let messages = vec![
            sdk::ChatMessage::user_text("first"),
            sdk::ChatMessage::assistant_text("answer"),
            sdk::ChatMessage::user_text("second"),
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
