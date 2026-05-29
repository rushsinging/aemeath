impl super::super::App {
    pub(super) async fn handle_save_command(&mut self) {
        let result = if let Some(agent_client) = &self.agent_client {
            if let Err(e) = agent_client
                .sync_current_messages(self.chat.messages.clone())
                .await
            {
                log::warn!("failed to sync session messages: {e}");
            }
            agent_client.save_current_session().await
        } else {
            Err(sdk::SdkError::Internal(
                "SDK agent client is unavailable".to_string(),
            ))
        };
        match result {
            Ok(()) => {
                self.append_system_notice(format!("[session saved: {}]", self.session.session_id))
            }
            Err(e) => self.append_error_notice(format!("Failed to save session: {e}")),
        }
    }
}
