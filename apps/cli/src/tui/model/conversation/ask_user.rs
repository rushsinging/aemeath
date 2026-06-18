//! ConversationModel 的 AskUserQuestion 批量交互块逻辑。
//!
//! AskUserBatch 块同一时刻至多一个，使用固定 id `ASK_USER_BLOCK_ID`，
//! 管理多问 + 确认页的状态机（Answering → Confirming → Confirmed）。

use super::block::{AskUserPhase, AskUserSlot, ConversationBlock};
use super::change::ConversationChange;
use super::model::ConversationModel;
use crate::tui::model::conversation::ask_user_timeline::sync_ask_user_timeline_item;
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
        self.blocks.iter().find_map(|block| {
            if let ConversationBlock::AskUserBatch {
                slots,
                active_index,
                phase,
                cursor,
                selected,
                chat_input_active,
                confirm_cursor,
                confirmed,
                ..
            } = block
            {
                let slot = slots.get(*active_index);
                Some(AskUserSnapshot {
                    active_index: *active_index,
                    phase: *phase,
                    cursor: *cursor,
                    selected: selected.clone(),
                    chat_input_active: *chat_input_active,
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
        self.blocks.push(ConversationBlock::AskUserBatch {
            id: ASK_USER_BLOCK_ID.to_string(),
            slots: slots.clone(),
            active_index: 0,
            phase: AskUserPhase::Answering,
            cursor: 0,
            selected: vec![false; first_total],
            chat_input_active: false,
            chat_input_text: String::new(),
            confirm_cursor: n, // 默认停在「全部确认提交」
            confirmed: false,
        });
        self.timeline.push(OutputTimelineItem::AskUserBatch {
            id: ASK_USER_BLOCK_ID.to_string(),
            slots,
            active_index: 0,
            phase: AskUserPhase::Answering,
            cursor: 0,
            selected: vec![false; first_total],
            chat_input_active: false,
            chat_input_text: String::new(),
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
        if let Some(ConversationBlock::AskUserBatch {
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
        }) = self.ask_user_block_mut()
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
        if let Some(ConversationBlock::AskUserBatch {
            slots,
            active_index,
            phase,
            cursor,
            selected,
            chat_input_active,
            chat_input_text,
            ..
        }) = self.ask_user_block_mut()
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
        if let Some(ConversationBlock::AskUserBatch {
            slots,
            active_index,
            cursor: current,
            ..
        }) = self.ask_user_block_mut()
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
        if let Some(ConversationBlock::AskUserBatch {
            slots,
            active_index,
            selected,
            ..
        }) = self.ask_user_block_mut()
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
        if let Some(ConversationBlock::AskUserBatch {
            chat_input_active,
            chat_input_text,
            ..
        }) = self.ask_user_block_mut()
        {
            *chat_input_active = active;
            if !active {
                chat_input_text.clear();
            }
            return self.ask_user_updated();
        }
        Vec::new()
    }

    /// 追加字符到 Type something 输入框。
    pub(super) fn append_ask_user_chat_char(&mut self, ch: char) -> Vec<ConversationChange> {
        if let Some(ConversationBlock::AskUserBatch {
            chat_input_active,
            chat_input_text,
            ..
        }) = self.ask_user_block_mut()
        {
            if *chat_input_active {
                chat_input_text.push(ch);
                return self.ask_user_updated();
            }
        }
        Vec::new()
    }

    /// 删除 Type something 输入框末尾字符。
    pub(super) fn delete_ask_user_chat_char(&mut self) -> Vec<ConversationChange> {
        if let Some(ConversationBlock::AskUserBatch {
            chat_input_active,
            chat_input_text,
            ..
        }) = self.ask_user_block_mut()
        {
            if *chat_input_active {
                chat_input_text.pop();
                return self.ask_user_updated();
            }
        }
        Vec::new()
    }

    /// 获取 Type something 输入框文本。
    pub fn ask_user_chat_text(&self) -> Option<String> {
        self.blocks.iter().find_map(|block| {
            if let ConversationBlock::AskUserBatch {
                chat_input_active: true,
                chat_input_text,
                ..
            } = block
            {
                Some(chat_input_text.clone())
            } else {
                None
            }
        })
    }

    /// 更新确认页导航光标（范围 0..=N+1）。
    pub(super) fn set_ask_user_confirm_cursor(&mut self, cursor: usize) -> Vec<ConversationChange> {
        if let Some(ConversationBlock::AskUserBatch {
            slots,
            confirm_cursor,
            ..
        }) = self.ask_user_block_mut()
        {
            let max = slots.len() + 1; // N 个问题项 + 提交 + 取消
            *confirm_cursor = cursor.min(max);
            return self.ask_user_updated();
        }
        Vec::new()
    }

    /// 确认提交所有答案（block 进入终态）。
    pub(super) fn confirm_ask_user_batch(&mut self) -> Vec<ConversationChange> {
        if let Some(ConversationBlock::AskUserBatch { confirmed, .. }) = self.ask_user_block_mut() {
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
        sync_ask_user_timeline_item(&self.blocks, self.timeline.items_mut());
        vec![
            ConversationChange::AskUserUpdated {
                id: ASK_USER_BLOCK_ID.to_string(),
            },
            ConversationChange::OutputDirty,
        ]
    }

    fn ask_user_block_mut(&mut self) -> Option<&mut ConversationBlock> {
        self.blocks
            .iter_mut()
            .find(|block| matches!(block, ConversationBlock::AskUserBatch { .. }))
    }

    /// 移除已存在的 AskUserBatch 块，返回是否实际移除。
    fn remove_ask_user_block(&mut self) -> bool {
        let before = self.blocks.len();
        self.blocks
            .retain(|block| !matches!(block, ConversationBlock::AskUserBatch { .. }));
        self.timeline
            .retain(|item| !matches!(item, OutputTimelineItem::AskUserBatch { .. }));
        before != self.blocks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::block::AskUserSlot;
    use crate::tui::model::conversation::intent::ConversationIntent;

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
        model.apply(ConversationIntent::ShowAskUserBatch { slots });
    }

    fn batch_block(model: &ConversationModel) -> &ConversationBlock {
        model
            .blocks
            .iter()
            .find(|b| matches!(b, ConversationBlock::AskUserBatch { .. }))
            .expect("ask user batch block")
    }

    #[test]
    fn test_show_ask_user_batch_initializes_answering_phase() {
        let mut model = ConversationModel::default();
        show_batch(&mut model, vec![make_slot("q1", "问题1", &["A", "B"])]);
        if let ConversationBlock::AskUserBatch {
            phase,
            active_index,
            ..
        } = batch_block(&model)
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
        model.apply(ConversationIntent::AnswerCurrentAskUser {
            answer: "A".to_string(),
        });
        if let ConversationBlock::AskUserBatch {
            active_index,
            phase,
            slots,
            ..
        } = batch_block(&model)
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
        model.apply(ConversationIntent::AnswerCurrentAskUser {
            answer: "A".to_string(),
        });
        model.apply(ConversationIntent::AnswerCurrentAskUser {
            answer: "B".to_string(),
        });
        if let ConversationBlock::AskUserBatch {
            phase,
            confirm_cursor,
            slots,
            ..
        } = batch_block(&model)
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
        model.apply(ConversationIntent::AnswerCurrentAskUser {
            answer: "A".to_string(),
        });
        model.apply(ConversationIntent::ConfirmAskUserBatch);
        if let ConversationBlock::AskUserBatch { confirmed, .. } = batch_block(&model) {
            assert!(*confirmed);
        }
    }

    #[test]
    fn test_single_question_batch_answer_confirmed_immediately() {
        let mut model = ConversationModel::default();
        show_batch(&mut model, vec![make_slot("q1", "问题1", &["A"])]);
        model.apply(ConversationIntent::AnswerCurrentAskUser {
            answer: "A".to_string(),
        });
        if let ConversationBlock::AskUserBatch {
            confirmed, phase, ..
        } = batch_block(&model)
        {
            assert!(*confirmed);
            assert_eq!(*phase, AskUserPhase::Answering); // phase 不变，直接 confirmed
        }
    }

    #[test]
    fn test_single_question_batch_answer_no_options_confirmed_immediately() {
        let mut model = ConversationModel::default();
        show_batch(&mut model, vec![make_slot("q1", "问题1", &[])]);
        model.apply(ConversationIntent::AnswerCurrentAskUser {
            answer: "自由输入".to_string(),
        });
        if let ConversationBlock::AskUserBatch { confirmed, .. } = batch_block(&model) {
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
        model.apply(ConversationIntent::AnswerCurrentAskUser {
            answer: "A".to_string(),
        });
        model.apply(ConversationIntent::AnswerCurrentAskUser {
            answer: "C".to_string(),
        });
        // 导航回第 0 题重新作答
        model.apply(ConversationIntent::NavigateAskUserTo { index: 0 });
        if let ConversationBlock::AskUserBatch {
            active_index,
            phase,
            cursor,
            chat_input_active,
            ..
        } = batch_block(&model)
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
        let changes = model.apply(ConversationIntent::SetAskUserCursor { cursor: 0 });
        assert!(changes.is_empty());
    }

    #[test]
    fn test_dismiss_ask_user_batch_removes_block() {
        let mut model = ConversationModel::default();
        show_batch(&mut model, vec![make_slot("q1", "问题1", &["A"])]);
        let changes = model.apply(ConversationIntent::DismissAskUserBatch);
        assert!(changes
            .iter()
            .any(|c| matches!(c, ConversationChange::AskUserDismissed)));
        assert!(!model
            .blocks
            .iter()
            .any(|b| matches!(b, ConversationBlock::AskUserBatch { .. })));
    }
}
