use super::change::ConversationChange;
use super::constants::BANNER_LINES;
use super::model::ConversationModel;
use super::system_reminder::strip_system_reminder_envelope_owned;
use crate::tui::model::output_timeline::OutputTimelineItem;

impl ConversationModel {
    /// 注入启动横幅。横幅纳入 ConversationModel，`/clear` reset 会一并清除。
    pub fn seed_banner(&mut self) -> Vec<ConversationChange> {
        let mut changes = Vec::new();
        for line in BANNER_LINES {
            changes.extend(self.append_system_message(line.to_string()));
        }
        changes
    }

    pub(super) fn append_hook_notice(
        &mut self,
        title: String,
        text: String,
        kind: crate::tui::adapter::runtime_view::TuiHookNoticeKind,
    ) -> Vec<ConversationChange> {
        self.clear_active_text_blocks();
        let block_id = self.next_block_id("hook-notice");
        self.timeline.push(OutputTimelineItem::HookNotice {
            id: block_id.clone(),
            title,
            text,
            kind,
        });
        vec![
            ConversationChange::SystemMessageAppended { block_id },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }

    pub(super) fn append_system_message(&mut self, text: String) -> Vec<ConversationChange> {
        let text = strip_system_reminder_envelope_owned(text);
        if let Some(OutputTimelineItem::System { text: existing, .. }) =
            self.timeline.items_mut().last_mut()
        {
            if existing == "✻ Cancelled" && text.starts_with("✻ Cancelled, ran ") {
                *existing = text;
                return vec![ConversationChange::OutputDirty];
            }
            if existing == &text {
                return Vec::new();
            }
        }
        self.clear_active_text_blocks();
        let block_id = self.next_block_id("system");
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
#[path = "notice_tests.rs"]
mod tests;
