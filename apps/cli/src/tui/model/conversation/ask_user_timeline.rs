use super::block::ConversationBlock;
use crate::tui::model::output_timeline::OutputTimelineItem;

pub(super) fn sync_ask_user_timeline_item(
    blocks: &[ConversationBlock],
    timeline: &mut [OutputTimelineItem],
) {
    let Some(block) = blocks
        .iter()
        .find(|block| matches!(block, ConversationBlock::AskUserBatch { .. }))
    else {
        return;
    };
    let Some(item) = timeline
        .iter_mut()
        .find(|item| matches!(item, OutputTimelineItem::AskUserBatch { .. }))
    else {
        return;
    };
    let ConversationBlock::AskUserBatch {
        id,
        slots,
        active_index,
        phase,
        cursor,
        selected,
        chat_input_active,
        chat_input_text,
        confirm_cursor,
        confirmed,
    } = block
    else {
        return;
    };
    *item = OutputTimelineItem::AskUserBatch {
        id: id.clone(),
        slots: slots.clone(),
        active_index: *active_index,
        phase: *phase,
        cursor: *cursor,
        selected: selected.clone(),
        chat_input_active: *chat_input_active,
        chat_input_text: chat_input_text.clone(),
        confirm_cursor: *confirm_cursor,
        confirmed: *confirmed,
    };
}
