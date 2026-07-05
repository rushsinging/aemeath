//! ConversationModel 的 AskUserQuestion 批量交互块逻辑。
//!
//! AskUserBatch 块同一时刻至多一个，使用固定 id `ASK_USER_BLOCK_ID`，
//! 管理多问 + 确认页的状态机（Answering → Confirming → Confirmed）。

use super::block::{AskUserPhase, AskUserSlot};
use super::change::ConversationChange;
use super::model::ConversationModel;
use crate::tui::model::output_timeline::OutputTimelineItem;

/// AskUser 交互块的固定 id（同一时刻至多一个）。
pub const ASK_USER_BLOCK_ID: &str = "ask-user";

/// AskUserBatch 块的可变交互状态快照，供控制器在提交/导航时读取。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AskUserSnapshot {
    pub active_index: usize,
    pub phase: AskUserPhase,
    pub cursor: usize,
    pub selected: Vec<bool>,
    pub chat_input_active: bool,
    /// Type something 输入框的光标位置（byte offset）。
    pub chat_input_cursor: usize,
    pub confirm_cursor: usize,
    /// 当前激活问题的 LLM 选项数。
    pub llm_option_count: usize,
    /// 当前激活问题的全部选项数。
    pub options_count: usize,
    /// 当前激活问题的 multi_select 标志。
    pub multi_select: bool,
    /// 用户已确认提交（block 进入终态）。
    pub confirmed: bool,
}

impl ConversationModel {
    /// 读取当前 AskUserBatch 块的交互状态快照（无块时返回 None）。
    pub fn ask_user_snapshot(&self) -> Option<AskUserSnapshot> {
        self.timeline.items().iter().find_map(|item| {
            if let OutputTimelineItem::AskUserBatch {
                slots,
                active_index,
                phase,
                cursor,
                selected,
                chat_input_active,
                chat_input_cursor,
                confirm_cursor,
                confirmed,
                ..
            } = item
            {
                let slot = slots.get(*active_index);
                Some(AskUserSnapshot {
                    active_index: *active_index,
                    phase: *phase,
                    cursor: *cursor,
                    selected: selected.clone(),
                    chat_input_active: *chat_input_active,
                    chat_input_cursor: *chat_input_cursor,
                    confirm_cursor: *confirm_cursor,
                    llm_option_count: slot.map(|s| s.llm_option_count).unwrap_or(0),
                    options_count: slot.map(|s| s.options.len()).unwrap_or(0),
                    multi_select: slot.map(|s| s.multi_select).unwrap_or(false),
                    confirmed: *confirmed,
                })
            } else {
                None
            }
        })
    }

    /// 显示 AskUserBatch 交互块；若已存在则替换。
    pub(super) fn show_ask_user_batch(
        &mut self,
        slots: Vec<AskUserSlot>,
    ) -> Vec<ConversationChange> {
        let first_total = slots.first().map(|s| s.options.len()).unwrap_or(0);
        self.clear_active_text_blocks();
        self.remove_ask_user_block();
        let n = slots.len();
        self.timeline.push(OutputTimelineItem::AskUserBatch {
            id: ASK_USER_BLOCK_ID.to_string(),
            slots,
            active_index: 0,
            phase: AskUserPhase::Answering,
            cursor: 0,
            selected: vec![false; first_total],
            chat_input_active: false,
            chat_input_text: String::new(),
            chat_input_cursor: 0,
            confirm_cursor: n,
            confirmed: false,
        });
        vec![
            ConversationChange::AskUserShown {
                id: ASK_USER_BLOCK_ID.to_string(),
            },
            ConversationChange::OutputDirty,
        ]
    }

    /// 回答当前激活问题，自动前进到下一题或进入确认页。
    pub(super) fn answer_current_ask_user(&mut self, answer: String) -> Vec<ConversationChange> {
        if let Some(OutputTimelineItem::AskUserBatch {
            slots,
            active_index,
            phase,
            cursor,
            selected,
            chat_input_active,
            chat_input_text,
            confirm_cursor,
            confirmed,
            ..
        }) = self.ask_user_timeline_item_mut()
        {
            // 设置当前问题答案
            if let Some(slot) = slots.get_mut(*active_index) {
                slot.answer = Some(answer);
            }
            // 前进逻辑
            if *active_index < slots.len() - 1 {
                *active_index += 1;
                let new_total = slots
                    .get(*active_index)
                    .map(|s| s.options.len())
                    .unwrap_or(0);
                *cursor = 0;
                *selected = vec![false; new_total];
                *chat_input_active = false;
                chat_input_text.clear();
            } else if slots.len() == 1 {
                // 单问题：直接确认，跳过确认页
                *confirmed = true;
            } else {
                *phase = AskUserPhase::Confirming;
                *confirm_cursor = slots.len(); // 默认停在「全部确认提交」
            }
            return self.ask_user_updated();
        }
        Vec::new()
    }

    /// 确认页导航到某项（重新作答）。
    pub(super) fn navigate_ask_user_to(&mut self, index: usize) -> Vec<ConversationChange> {
        if let Some(OutputTimelineItem::AskUserBatch {
            slots,
            active_index,
            phase,
            cursor,
            selected,
            chat_input_active,
            chat_input_text,
            ..
        }) = self.ask_user_timeline_item_mut()
        {
            if index >= slots.len() {
                return Vec::new();
            }
            *active_index = index;
            *phase = AskUserPhase::Answering;
            let total = slots.get(index).map(|s| s.options.len()).unwrap_or(0);
            *cursor = 0;
            *selected = vec![false; total];
            *chat_input_active = false;
            chat_input_text.clear();
            return self.ask_user_updated();
        }
        Vec::new()
    }

    /// 更新当前激活问题的选项光标（越界自动夹取）。
    pub(super) fn set_ask_user_cursor(&mut self, cursor: usize) -> Vec<ConversationChange> {
        if let Some(OutputTimelineItem::AskUserBatch {
            slots,
            active_index,
            cursor: current,
            ..
        }) = self.ask_user_timeline_item_mut()
        {
            let total = slots
                .get(*active_index)
                .map(|s| s.options.len())
                .unwrap_or(0);
            if total == 0 {
                return Vec::new();
            }
            *current = cursor.min(total - 1);
            return self.ask_user_updated();
        }
        Vec::new()
    }

    /// 切换当前激活问题中某选项的勾选状态。内建选项不可勾选。
    pub(super) fn toggle_ask_user_selected(&mut self, index: usize) -> Vec<ConversationChange> {
        if let Some(OutputTimelineItem::AskUserBatch {
            slots,
            active_index,
            selected,
            ..
        }) = self.ask_user_timeline_item_mut()
        {
            let llm_count = slots
                .get(*active_index)
                .map(|s| s.llm_option_count)
                .unwrap_or(0);
            if index >= llm_count {
                return Vec::new();
            }
            if let Some(flag) = selected.get_mut(index) {
                *flag = !*flag;
                return self.ask_user_updated();
            }
        }
        Vec::new()
    }

    /// 设置当前激活问题是否处于 Type something 子态。
    pub(super) fn set_ask_user_chat_input(&mut self, active: bool) -> Vec<ConversationChange> {
        if let Some(OutputTimelineItem::AskUserBatch {
            chat_input_active,
            chat_input_text,
            ..
        }) = self.ask_user_timeline_item_mut()
        {
            *chat_input_active = active;
            if !active {
                chat_input_text.clear();
            }
            return self.ask_user_updated();
        }
        Vec::new()
    }

    /// 在 Type something 输入框当前光标位置插入字符。
    pub(super) fn append_ask_user_chat_char(&mut self, ch: char) -> Vec<ConversationChange> {
        if let Some(OutputTimelineItem::AskUserBatch {
            chat_input_active,
            chat_input_text,
            chat_input_cursor,
            ..
        }) = self.ask_user_timeline_item_mut()
        {
            if *chat_input_active {
                let pos = *chat_input_cursor;
                if pos <= chat_input_text.len() {
                    chat_input_text.insert(pos, ch);
                    *chat_input_cursor = pos + ch.len_utf8();
                    return self.ask_user_updated();
                }
            }
        }
        Vec::new()
    }

    /// 删除 Type something 输入框光标前一个字符。
    pub(super) fn delete_ask_user_chat_char(&mut self) -> Vec<ConversationChange> {
        if let Some(OutputTimelineItem::AskUserBatch {
            chat_input_active,
            chat_input_text,
            chat_input_cursor,
            ..
        }) = self.ask_user_timeline_item_mut()
        {
            if *chat_input_active && *chat_input_cursor > 0 {
                let pos = *chat_input_cursor;
                // 找到光标前一个 char 的起始 byte 位置
                let prev_start = chat_input_text
                    .get(..pos)
                    .unwrap_or("")
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                chat_input_text.replace_range(prev_start..pos, "");
                *chat_input_cursor = prev_start;
                return self.ask_user_updated();
            }
        }
        Vec::new()
    }

    /// 移动 Type something 输入框光标，delta 为 char 数偏移（负数向左、正数向右）。
    pub(super) fn move_ask_user_chat_cursor(&mut self, delta: isize) -> Vec<ConversationChange> {
        if let Some(OutputTimelineItem::AskUserBatch {
            chat_input_active,
            chat_input_text,
            chat_input_cursor,
            ..
        }) = self.ask_user_timeline_item_mut()
        {
            if *chat_input_active {
                let pos = *chat_input_cursor;
                let text_len = chat_input_text.len();
                let target = if delta < 0 {
                    // 向左回退 |delta| 个 char
                    let back = (-delta) as usize;
                    let bytes_before = chat_input_text.get(..pos).unwrap_or("");
                    let new_byte_pos = bytes_before
                        .char_indices()
                        .rev()
                        .nth(back.saturating_sub(1))
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    new_byte_pos
                } else if delta > 0 {
                    // 向右前进 delta 个 char
                    let bytes_after = chat_input_text.get(pos..).unwrap_or("");
                    let new_byte_pos = bytes_after
                        .char_indices()
                        .nth(delta as usize)
                        .map(|(i, _)| pos + i)
                        .unwrap_or(text_len);
                    new_byte_pos
                } else {
                    pos
                };
                if target != *chat_input_cursor {
                    *chat_input_cursor = target;
                    return self.ask_user_updated();
                }
            }
        }
        Vec::new()
    }

    /// 将光标移到行首或行尾。
    pub(super) fn move_ask_user_chat_cursor_end(
        &mut self,
        to_end: bool,
    ) -> Vec<ConversationChange> {
        if let Some(OutputTimelineItem::AskUserBatch {
            chat_input_active,
            chat_input_text,
            chat_input_cursor,
            ..
        }) = self.ask_user_timeline_item_mut()
        {
            if *chat_input_active {
                let target = if to_end { chat_input_text.len() } else { 0 };
                if target != *chat_input_cursor {
                    *chat_input_cursor = target;
                    return self.ask_user_updated();
                }
            }
        }
        Vec::new()
    }

    /// 删除光标前一个单词（按 char 边界 + 空白）。
    pub(super) fn delete_ask_user_chat_word(&mut self) -> Vec<ConversationChange> {
        if let Some(OutputTimelineItem::AskUserBatch {
            chat_input_active,
            chat_input_text,
            chat_input_cursor,
            ..
        }) = self.ask_user_timeline_item_mut()
        {
            if *chat_input_active && *chat_input_cursor > 0 {
                let pos = *chat_input_cursor;
                let bytes = chat_input_text.as_bytes();
                let mut start = pos;
                // 跳过紧邻光标的空白
                while start > 0 && bytes[start - 1].is_ascii_whitespace() {
                    start -= 1;
                }
                // 回退一个非空白词
                while start > 0 && !bytes[start - 1].is_ascii_whitespace() {
                    start -= 1;
                }
                if start < pos {
                    chat_input_text.replace_range(start..pos, "");
                    *chat_input_cursor = start;
                    return self.ask_user_updated();
                }
            }
        }
        Vec::new()
    }

    /// 获取 Type something 输入框文本。
    pub fn ask_user_chat_text(&self) -> Option<String> {
        self.timeline.items().iter().find_map(|item| {
            if let OutputTimelineItem::AskUserBatch {
                chat_input_active: true,
                chat_input_text,
                ..
            } = item
            {
                Some(chat_input_text.clone())
            } else {
                None
            }
        })
    }

    /// 更新确认页导航光标（范围 0..=N+1）。
    pub(super) fn set_ask_user_confirm_cursor(&mut self, cursor: usize) -> Vec<ConversationChange> {
        if let Some(OutputTimelineItem::AskUserBatch {
            slots,
            confirm_cursor,
            ..
        }) = self.ask_user_timeline_item_mut()
        {
            let max = slots.len() + 1; // N 个问题项 + 提交 + 取消
            *confirm_cursor = cursor.min(max);
            return self.ask_user_updated();
        }
        Vec::new()
    }

    /// 确认提交所有答案（block 进入终态）。
    pub(super) fn confirm_ask_user_batch(&mut self) -> Vec<ConversationChange> {
        if let Some(OutputTimelineItem::AskUserBatch { confirmed, .. }) =
            self.ask_user_timeline_item_mut()
        {
            *confirmed = true;
            return self.ask_user_updated();
        }
        Vec::new()
    }

    /// 移除 AskUserBatch 交互块。
    pub(super) fn dismiss_ask_user_batch(&mut self) -> Vec<ConversationChange> {
        if self.remove_ask_user_block() {
            return vec![
                ConversationChange::AskUserDismissed,
                ConversationChange::OutputDirty,
            ];
        }
        Vec::new()
    }

    fn ask_user_updated(&mut self) -> Vec<ConversationChange> {
        vec![
            ConversationChange::AskUserUpdated {
                id: ASK_USER_BLOCK_ID.to_string(),
            },
            ConversationChange::OutputDirty,
        ]
    }

    fn ask_user_timeline_item_mut(&mut self) -> Option<&mut OutputTimelineItem> {
        self.timeline
            .items_mut()
            .iter_mut()
            .find(|item| matches!(item, OutputTimelineItem::AskUserBatch { .. }))
    }

    /// 移除已存在的 AskUserBatch 块，返回是否实际移除。
    fn remove_ask_user_block(&mut self) -> bool {
        let before = self.timeline.items().len();
        self.timeline
            .retain(|item| !matches!(item, OutputTimelineItem::AskUserBatch { .. }));
        before != self.timeline.items().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::block::AskUserSlot;
    use crate::tui::model::conversation::intent::*;

    fn make_slot(id: &str, question: &str, options: &[&str]) -> AskUserSlot {
        let llm_count = options.len();
        let mut all = options
            .iter()
            .map(|s| sdk::OptionItem::title_only(s.to_string()))
            .collect::<Vec<_>>();
        if !all.is_empty() {
            all.push(sdk::OptionItem::title_only("Type something...".to_string()));
        }
        AskUserSlot {
            id: id.to_string(),
            question: question.to_string(),
            options: all,
            llm_option_count: llm_count,
            multi_select: false,
            default: None,
            answer: None,
        }
    }

    fn show_batch(model: &mut ConversationModel, slots: Vec<AskUserSlot>) {
        model.apply(ShowAskUserBatch { slots });
    }

    fn timeline_item(model: &ConversationModel) -> &OutputTimelineItem {
        model
            .timeline
            .items()
            .iter()
            .find(|i| matches!(i, OutputTimelineItem::AskUserBatch { .. }))
            .expect("ask user batch timeline item")
    }

    #[test]
    fn test_show_ask_user_batch_initializes_answering_phase() {
        let mut model = ConversationModel::default();
        show_batch(&mut model, vec![make_slot("q1", "问题1", &["A", "B"])]);
        if let OutputTimelineItem::AskUserBatch {
            phase,
            active_index,
            ..
        } = timeline_item(&model)
        {
            assert_eq!(*phase, AskUserPhase::Answering);
            assert_eq!(*active_index, 0);
        }
    }

    #[test]
    fn test_answer_current_advances_to_next_question() {
        let mut model = ConversationModel::default();
        show_batch(
            &mut model,
            vec![
                make_slot("q1", "问题1", &["A"]),
                make_slot("q2", "问题2", &["B"]),
            ],
        );
        model.apply(AnswerCurrentAskUser {
            answer: "A".to_string(),
        });
        if let OutputTimelineItem::AskUserBatch {
            active_index,
            phase,
            slots,
            ..
        } = timeline_item(&model)
        {
            assert_eq!(*active_index, 1);
            assert_eq!(*phase, AskUserPhase::Answering);
            assert_eq!(slots[0].answer.as_deref(), Some("A"));
        }
    }

    #[test]
    fn test_answer_last_question_enters_confirming_phase() {
        let mut model = ConversationModel::default();
        show_batch(
            &mut model,
            vec![
                make_slot("q1", "问题1", &["A"]),
                make_slot("q2", "问题2", &["B"]),
            ],
        );
        model.apply(AnswerCurrentAskUser {
            answer: "A".to_string(),
        });
        model.apply(AnswerCurrentAskUser {
            answer: "B".to_string(),
        });
        if let OutputTimelineItem::AskUserBatch {
            phase,
            confirm_cursor,
            slots,
            ..
        } = timeline_item(&model)
        {
            assert_eq!(*phase, AskUserPhase::Confirming);
            assert_eq!(*confirm_cursor, 2); // 默认在「提交」
            assert_eq!(slots[0].answer.as_deref(), Some("A"));
            assert_eq!(slots[1].answer.as_deref(), Some("B"));
        }
    }

    #[test]
    fn test_confirm_sets_confirmed_flag() {
        let mut model = ConversationModel::default();
        show_batch(&mut model, vec![make_slot("q1", "问题1", &["A"])]);
        model.apply(AnswerCurrentAskUser {
            answer: "A".to_string(),
        });
        model.apply(ConfirmAskUserBatch);
        if let OutputTimelineItem::AskUserBatch { confirmed, .. } = timeline_item(&model) {
            assert!(*confirmed);
        }
    }

    #[test]
    fn test_single_question_batch_answer_confirmed_immediately() {
        let mut model = ConversationModel::default();
        show_batch(&mut model, vec![make_slot("q1", "问题1", &["A"])]);
        model.apply(AnswerCurrentAskUser {
            answer: "A".to_string(),
        });
        if let OutputTimelineItem::AskUserBatch {
            confirmed, phase, ..
        } = timeline_item(&model)
        {
            assert!(*confirmed);
            assert_eq!(*phase, AskUserPhase::Answering); // phase 不变，直接 confirmed
        }
    }

    #[test]
    fn test_single_question_batch_answer_no_options_confirmed_immediately() {
        let mut model = ConversationModel::default();
        show_batch(&mut model, vec![make_slot("q1", "问题1", &[])]);
        model.apply(AnswerCurrentAskUser {
            answer: "自由输入".to_string(),
        });
        if let OutputTimelineItem::AskUserBatch { confirmed, .. } = timeline_item(&model) {
            assert!(*confirmed);
        }
    }

    #[test]
    fn test_navigate_ask_user_to_resets_cursor_and_selected() {
        let mut model = ConversationModel::default();
        show_batch(
            &mut model,
            vec![
                make_slot("q1", "问题1", &["A", "B"]),
                make_slot("q2", "问题2", &["C"]),
            ],
        );
        // 先答完两题进入确认页
        model.apply(AnswerCurrentAskUser {
            answer: "A".to_string(),
        });
        model.apply(AnswerCurrentAskUser {
            answer: "C".to_string(),
        });
        // 导航回第 0 题重新作答
        model.apply(NavigateAskUserTo { index: 0 });
        if let OutputTimelineItem::AskUserBatch {
            active_index,
            phase,
            cursor,
            chat_input_active,
            ..
        } = timeline_item(&model)
        {
            assert_eq!(*active_index, 0);
            assert_eq!(*phase, AskUserPhase::Answering);
            assert_eq!(*cursor, 0);
            assert!(!*chat_input_active);
        }
    }

    #[test]
    fn test_set_cursor_without_batch_is_noop() {
        let mut model = ConversationModel::default();
        let changes = model.apply(SetAskUserCursor { cursor: 0 });
        assert!(changes.is_empty());
    }

    #[test]
    fn test_dismiss_ask_user_batch_removes_block() {
        let mut model = ConversationModel::default();
        show_batch(&mut model, vec![make_slot("q1", "问题1", &["A"])]);
        let changes = model.apply(DismissAskUserBatch);
        assert!(changes
            .iter()
            .any(|c| matches!(c, ConversationChange::AskUserDismissed)));
        assert!(!model.timeline.items().iter().any(|b| matches!(
            b,
            crate::tui::model::output_timeline::OutputTimelineItem::AskUserBatch { .. }
        )));
    }

    // ── chat_input cursor 回归测试 ──

    fn enable_chat_input(model: &mut ConversationModel) {
        model.apply(SetAskUserChatInput { active: true });
    }

    #[test]
    fn test_chat_input_cursor_insert_and_backspace_at_cursor() {
        let mut model = ConversationModel::default();
        show_batch(&mut model, vec![make_slot("q1", "问题1", &[])]);
        enable_chat_input(&mut model);

        // 输入 "abc"
        model.apply(AppendAskUserChatChar { ch: 'a' });
        model.apply(AppendAskUserChatChar { ch: 'b' });
        model.apply(AppendAskUserChatChar { ch: 'c' });
        if let OutputTimelineItem::AskUserBatch {
            chat_input_text,
            chat_input_cursor,
            ..
        } = timeline_item(&model)
        {
            assert_eq!(*chat_input_text, "abc");
            assert_eq!(*chat_input_cursor, 3);
        }

        // 左移到 1，再插入 X 应该是 aXbc
        model.apply(MoveAskUserChatCursor { delta: -2 });
        model.apply(AppendAskUserChatChar { ch: 'X' });
        if let OutputTimelineItem::AskUserBatch {
            chat_input_text,
            chat_input_cursor,
            ..
        } = timeline_item(&model)
        {
            assert_eq!(*chat_input_text, "aXbc");
            assert_eq!(*chat_input_cursor, 2);
        }

        // 在 cursor=2 位置 backspace 删除 X
        model.apply(DeleteAskUserChatChar);
        if let OutputTimelineItem::AskUserBatch {
            chat_input_text,
            chat_input_cursor,
            ..
        } = timeline_item(&model)
        {
            assert_eq!(*chat_input_text, "abc");
            assert_eq!(*chat_input_cursor, 1);
        }
    }

    #[test]
    fn test_chat_input_cursor_move_home_end_word_delete() {
        let mut model = ConversationModel::default();
        show_batch(&mut model, vec![make_slot("q1", "问题1", &[])]);
        enable_chat_input(&mut model);

        // 输入 "hello world"
        for ch in "hello world".chars() {
            model.apply(AppendAskUserChatChar { ch });
        }
        // Home (cursor -> 0)
        model.apply(MoveAskUserChatCursorEnd { to_end: false });
        if let OutputTimelineItem::AskUserBatch {
            chat_input_cursor, ..
        } = timeline_item(&model)
        {
            assert_eq!(*chat_input_cursor, 0);
        }
        // Right 2 次
        model.apply(MoveAskUserChatCursor { delta: 1 });
        model.apply(MoveAskUserChatCursor { delta: 1 });
        if let OutputTimelineItem::AskUserBatch {
            chat_input_cursor, ..
        } = timeline_item(&model)
        {
            assert_eq!(*chat_input_cursor, 2);
        }
        // End (cursor -> 11)
        model.apply(MoveAskUserChatCursorEnd { to_end: true });
        if let OutputTimelineItem::AskUserBatch {
            chat_input_cursor, ..
        } = timeline_item(&model)
        {
            assert_eq!(*chat_input_cursor, "hello world".len());
        }
        // Ctrl+W 删除 "world"
        model.apply(DeleteAskUserChatWord);
        if let OutputTimelineItem::AskUserBatch {
            chat_input_text,
            chat_input_cursor,
            ..
        } = timeline_item(&model)
        {
            assert_eq!(*chat_input_text, "hello ");
            assert_eq!(*chat_input_cursor, "hello ".len());
        }
    }

    #[test]
    fn test_chat_input_cursor_unicode_char_boundary() {
        let mut model = ConversationModel::default();
        show_batch(&mut model, vec![make_slot("q1", "问题1", &[])]);
        enable_chat_input(&mut model);
        // 输入中文 "你好"
        model.apply(AppendAskUserChatChar { ch: '你' });
        model.apply(AppendAskUserChatChar { ch: '好' });
        // 左移一个 char (cursor 从 6 -> 3)
        model.apply(MoveAskUserChatCursor { delta: -1 });
        if let OutputTimelineItem::AskUserBatch {
            chat_input_cursor, ..
        } = timeline_item(&model)
        {
            assert_eq!(*chat_input_cursor, 3); // '你' 占 3 字节
        }
        // 再右移一个 char (cursor 从 3 -> 6)
        model.apply(MoveAskUserChatCursor { delta: 1 });
        if let OutputTimelineItem::AskUserBatch {
            chat_input_cursor, ..
        } = timeline_item(&model)
        {
            assert_eq!(*chat_input_cursor, 6);
        }
    }
}
