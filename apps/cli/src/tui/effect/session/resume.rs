use crate::tui::adapter::status_widget::apply_runtime_status_to_widget;
use crate::tui::app::App;
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
        self.append_system_notice(format!(
            "[resumed session {} ({} messages)]",
            session_id, msg_count
        ));
    }
}
