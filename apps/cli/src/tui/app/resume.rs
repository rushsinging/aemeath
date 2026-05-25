use super::App;
use ::runtime::api::core::message::Message;

impl App {
    pub(super) fn resume_session_messages(
        &mut self,
        session_id: &str,
        messages: Vec<Message>,
        created_at: String,
    ) {
        let msg_count = messages.len();
        self.session_created_at = Some(created_at);
        self.session_id = session_id.to_string();
        self.status_bar.set_session_id(session_id);
        self.messages.clear();
        self.pending_images.clear();
        let mut msgs = messages;
        ::runtime::api::core::message::sanitize_messages(&mut msgs);
        let trimmed = msg_count - msgs.len();
        // Check for deeper integrity issues
        let integrity = ::runtime::api::core::message::check_message_integrity(&msgs);
        let auto_repaired = if integrity.has_issues() {
            ::runtime::api::core::message::deep_clean_messages(&mut msgs)
        } else {
            0
        };
        // Render history into output_area
        for i in 0..msgs.len() {
            let subsequent = if i + 1 < msgs.len() {
                Some(&msgs[i + 1])
            } else {
                None
            };
            self.render_history_message(&msgs[i], subsequent);
        }
        self.messages = msgs;
        self.output_area.push_system(&format!(
            "[resumed session {} ({} messages)]",
            session_id, msg_count
        ));
        if trimmed > 0 {
            self.output_area.push_system(&format!(
                "[trimmed {} incomplete tool-call message(s)]",
                trimmed
            ));
        }
        if auto_repaired > 0 {
            self.output_area.push_system(&format!(
                "[repaired {} message(s): removed orphaned tool results and fixed role ordering]",
                auto_repaired
            ));
        }
    }
}
