use super::block::ConversationBlock;
use crate::tui::model::output_timeline::OutputTimelineItem;

pub(super) fn sync_ask_user_timeline_item(
    blocks: &[ConversationBlock],
    timeline: &mut [OutputTimelineItem],
) {
    let Some(block) = blocks
        .iter()
        .find(|block| matches!(block, ConversationBlock::AskUser { .. }))
    else {
        return;
    };
    let Some(item) = timeline
        .iter_mut()
        .find(|item| matches!(item, OutputTimelineItem::AskUser { .. }))
    else {
        return;
    };
    let ConversationBlock::AskUser {
        id,
        question,
        options,
        llm_option_count,
        multi_select,
        cursor,
        selected,
        chat_input_active,
        chat_input_text,
        default,
        answer,
    } = block
    else {
        return;
    };
    *item = OutputTimelineItem::AskUser {
        id: id.clone(),
        question: question.clone(),
        options: options.clone(),
        llm_option_count: *llm_option_count,
        multi_select: *multi_select,
        cursor: *cursor,
        selected: selected.clone(),
        chat_input_active: *chat_input_active,
        chat_input_text: chat_input_text.clone(),
        default: default.clone(),
        answer: answer.clone(),
    };
}
