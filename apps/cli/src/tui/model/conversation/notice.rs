use super::block::HookNoticeContent;
use super::change::ConversationChange;
use super::model::ConversationModel;
use super::system_reminder::strip_system_reminder_envelope_owned;
use crate::tui::model::output_timeline::OutputTimelineItem;

/// 启动横幅文本，作为对话起始的 System block 注入单一真相源。
pub const BANNER_LINES: [&str; 4] = [
    "Aemeath - AI Agent",
    "",
    "Type /help for available commands",
    "",
];

impl ConversationModel {
    /// 注入启动横幅。横幅纳入 ConversationModel，`/clear` reset 会一并清除。
    pub fn seed_banner(&mut self) -> Vec<ConversationChange> {
        let mut changes = Vec::new();
        for line in BANNER_LINES {
            changes.extend(self.append_system_message(line.to_string()));
        }
        changes
    }

    pub(super) fn append_system_message(&mut self, text: String) -> Vec<ConversationChange> {
        self.clear_active_text_blocks();
        let block_id = self.next_block_id("system");
        let text = strip_system_reminder_envelope_owned(text);
        self.timeline.push(OutputTimelineItem::System {
            id: block_id.clone(),
            text,
        });
        vec![
            ConversationChange::SystemMessageAppended { block_id },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }

    pub(super) fn append_hook_notice(
        &mut self,
        content: HookNoticeContent,
    ) -> Vec<ConversationChange> {
        self.clear_active_text_blocks();
        let block_id = self.next_block_id("hook");
        self.timeline.push(OutputTimelineItem::HookNotice {
            id: block_id.clone(),
            content,
        });
        vec![
            ConversationChange::SystemMessageAppended { block_id },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }

    pub(super) fn append_error(&mut self, text: String) -> Vec<ConversationChange> {
        self.clear_active_text_blocks();
        let block_id = self.next_block_id("error");
        self.timeline.push(OutputTimelineItem::Error {
            id: block_id.clone(),
            text: text.clone(),
        });
        vec![
            ConversationChange::ErrorAppended {
                block_id,
                message: text,
            },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::intent::*;

    #[test]
    fn test_seed_banner_pushes_system_blocks() {
        let mut model = ConversationModel::default();
        let changes = model.seed_banner();
        assert_eq!(
            model
                .timeline
                .items()
                .iter()
                .filter(|b| matches!(b, OutputTimelineItem::System { .. }))
                .count(),
            BANNER_LINES.len()
        );
        assert!(changes
            .iter()
            .any(|c| matches!(c, ConversationChange::SystemMessageAppended { .. })));
    }

    #[test]
    fn test_seed_banner_first_block_is_title() {
        let mut model = ConversationModel::default();
        model.seed_banner();
        let first = model.timeline.items().first().expect("banner block");
        assert!(matches!(
            first,
            OutputTimelineItem::System { text, .. } if text == "Aemeath - AI Agent"
        ));
    }

    #[test]
    fn test_append_system_message_resets_active_text_block() {
        let mut model = ConversationModel::default();
        model.apply(StartChat {
            submission: "hi".to_string(),
        });
        model.apply(AssistantText {
            chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
            turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
            text: "streaming".to_string(),
        });
        model.apply(AppendSystemMessage {
            text: "notice".to_string(),
        });
        model.apply(AssistantText {
            chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
            turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
            text: "after".to_string(),
        });
        let assistant_blocks = model
            .timeline
            .items()
            .iter()
            .filter(|b| matches!(b, OutputTimelineItem::AssistantText { .. }))
            .count();
        assert_eq!(assistant_blocks, 2);
    }

    #[test]
    fn test_append_error_pushes_error_block() {
        let mut model = ConversationModel::default();
        let changes = model.append_error("坏了".to_string());
        assert!(matches!(
            model.timeline.items().last(),
            Some(OutputTimelineItem::Error { text, .. }) if text == "坏了"
        ));
        assert!(changes
            .iter()
            .any(|c| matches!(c, ConversationChange::ErrorAppended { .. })));
    }
}
