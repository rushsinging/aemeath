use super::block::{ConversationBlock, HookNoticeContent};
use super::change::ConversationChange;
use super::model::ConversationModel;
use super::system_reminder::strip_system_reminder_envelope_owned;

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
        self.blocks.push(ConversationBlock::System {
            id: block_id.clone(),
            text: strip_system_reminder_envelope_owned(text),
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
        self.blocks.push(ConversationBlock::HookNotice {
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
        self.blocks.push(ConversationBlock::Error {
            id: block_id.clone(),
            text,
        });
        vec![
            ConversationChange::ErrorAppended { block_id },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::intent::ConversationIntent;

    #[test]
    fn test_seed_banner_pushes_system_blocks() {
        let mut model = ConversationModel::default();
        let changes = model.seed_banner();
        assert_eq!(
            model
                .blocks
                .iter()
                .filter(|b| matches!(b, ConversationBlock::System { .. }))
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
        let first = model.blocks.first().expect("banner block");
        assert!(matches!(
            first,
            ConversationBlock::System { text, .. } if text == "Aemeath - AI Agent"
        ));
    }

    #[test]
    fn test_append_system_message_resets_active_text_block() {
        let mut model = ConversationModel::default();
        model.apply(ConversationIntent::StartChat {
            submission: "hi".to_string(),
        });
        model.apply(ConversationIntent::ObserveAssistantText {
            text: "streaming".to_string(),
        });
        model.apply(ConversationIntent::AppendSystemMessage {
            text: "notice".to_string(),
        });
        model.apply(ConversationIntent::ObserveAssistantText {
            text: "after".to_string(),
        });
        let assistant_blocks = model
            .blocks
            .iter()
            .filter(|b| matches!(b, ConversationBlock::AssistantText { .. }))
            .count();
        assert_eq!(assistant_blocks, 2);
    }

    #[test]
    fn test_append_error_pushes_error_block() {
        let mut model = ConversationModel::default();
        let changes = model.append_error("坏了".to_string());
        assert!(matches!(
            model.blocks.last(),
            Some(ConversationBlock::Error { text, .. }) if text == "坏了"
        ));
        assert!(changes
            .iter()
            .any(|c| matches!(c, ConversationChange::ErrorAppended { .. })));
    }
}
