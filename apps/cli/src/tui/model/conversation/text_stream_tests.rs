use super::intent::{AssistantText, CompleteBlock, ThinkingText};
use crate::tui::model::conversation::ids::{ChatId, ChatRunId};
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::output_timeline::OutputTimelineItem;

fn chat_id() -> ChatId {
    ChatId::new("chat-1")
}

fn run_id() -> ChatRunId {
    ChatRunId::new("run-1")
}

fn assistant_text_blocks(model: &ConversationModel) -> Vec<&str> {
    model
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            OutputTimelineItem::AssistantText { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn thinking_blocks(model: &ConversationModel) -> Vec<&str> {
    model
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            OutputTimelineItem::Thinking { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn apply_assistant_text(model: &mut ConversationModel, text: &str) {
    model.apply(AssistantText {
        chat_id: chat_id(),
        run_id: run_id(),
        text: text.to_string(),
    });
}

fn apply_thinking_text(model: &mut ConversationModel, text: &str) {
    model.apply(ThinkingText {
        chat_id: chat_id(),
        run_id: run_id(),
        text: text.to_string(),
    });
}

#[test]
fn blank_assistant_delta_creates_no_visible_block() {
    let mut model = ConversationModel::default();

    apply_assistant_text(&mut model, "\n\n");

    assert!(
        assistant_text_blocks(&model).is_empty(),
        "纯空白 assistant delta 不应产生可见文本块"
    );
}

#[test]
fn blank_thinking_delta_creates_no_visible_block() {
    let mut model = ConversationModel::default();

    apply_thinking_text(&mut model, "\n");

    assert!(
        thinking_blocks(&model).is_empty(),
        "纯空白 thinking delta 不应产生可见思考块"
    );
}

#[test]
fn blank_delta_after_visible_text_keeps_paragraph_separator() {
    let mut model = ConversationModel::default();

    apply_assistant_text(&mut model, "first");
    apply_assistant_text(&mut model, "\n\n");
    apply_assistant_text(&mut model, "second");

    assert_eq!(
        assistant_text_blocks(&model),
        vec!["first\n\nsecond"],
        "已有可见块时空白 delta MUST 追加为段落分隔，NEVER 丢弃"
    );
}

#[test]
fn blank_delta_before_visible_text_does_not_prefix_block() {
    let mut model = ConversationModel::default();

    apply_assistant_text(&mut model, "\n\n");
    apply_assistant_text(&mut model, "answer");

    assert_eq!(
        assistant_text_blocks(&model),
        vec!["answer"],
        "前导空白 delta 不应进入文本块正文"
    );
}

#[test]
fn blank_delta_after_completed_block_starts_no_new_block() {
    let mut model = ConversationModel::default();

    apply_assistant_text(&mut model, "answer");
    model.apply(CompleteBlock {
        chat_id: chat_id(),
        run_id: run_id(),
    });
    apply_assistant_text(&mut model, "\n\n");

    assert_eq!(
        assistant_text_blocks(&model),
        vec!["answer"],
        "块结束后收到纯空白 delta 不应新开空块"
    );
}

#[test]
fn blank_thinking_delta_before_visible_thinking_does_not_prefix_block() {
    let mut model = ConversationModel::default();

    apply_thinking_text(&mut model, "\n");
    apply_thinking_text(&mut model, "plan");

    assert_eq!(thinking_blocks(&model), vec!["plan"]);
}

#[test]
fn single_space_thinking_delta_creates_no_visible_block() {
    // 真实 session 证据：模型在 tool_use 前推送 thinking=" "（单个空格），
    // 记录形如 {"type":"thinking","thinking":" "}。非空串但无可见内容。
    let mut model = ConversationModel::default();

    apply_thinking_text(&mut model, " ");

    assert!(
        thinking_blocks(&model).is_empty(),
        "单空格 thinking delta 不应产生可见思考块"
    );
}

#[test]
fn whitespace_only_deltas_of_every_shape_create_no_block() {
    for blank in [" ", "  ", "\t", "\n", "\r\n", "\n\n\n", " \n \t "] {
        let mut model = ConversationModel::default();
        apply_assistant_text(&mut model, blank);
        apply_thinking_text(&mut model, blank);
        assert!(
            assistant_text_blocks(&model).is_empty() && thinking_blocks(&model).is_empty(),
            "纯空白形态 {blank:?} 不应产生任何可见块"
        );
    }
}
