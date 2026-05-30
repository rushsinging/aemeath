use crate::tui::adapter::status_widget::apply_runtime_status_to_widget;
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
        // session_id 真相归 SessionModel，经 adapter 单向写回 status_bar。
        self.model.session.apply(SessionIntent::SetCurrentSession {
            id: session_id.to_string(),
        });
        apply_runtime_status_to_widget(&self.model, &mut self.status_bar);
        self.chat.messages.clear();
        self.chat.clear_pending_images();
        for i in 0..messages.len() {
            let subsequent = if i + 1 < messages.len() {
                Some(&messages[i + 1])
            } else {
                None
            };
            self.render_history_message(&messages[i], subsequent);
        }
        self.chat.messages = messages;
        self.model
            .input
            .apply(InputIntent::ReplaceHistory(extract_user_input_history(
                &self.chat.messages,
            )));
        self.append_system_notice(format!(
            "[resumed session {} ({} messages)]",
            session_id, msg_count
        ));
    }
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
    use serde_json::json;

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
            content: json!([
                { "type": "text", "text": "hello " },
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } },
                { "type": "text", "text": "world" }
            ]),
        }];

        let history = extract_user_input_history(&messages);

        assert_eq!(history, vec!["hello world".to_string()]);
    }
}
