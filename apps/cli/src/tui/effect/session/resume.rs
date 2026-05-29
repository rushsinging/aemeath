use crate::tui::app::App;

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
        self.status_bar.set_session_id(session_id);
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
