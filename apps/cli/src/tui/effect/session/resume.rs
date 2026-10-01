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
#[path = "resume_tests.rs"]
mod tests;
