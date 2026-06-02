//! ConversationModel 的 AskUserQuestion 交互块逻辑。
//!
//! AskUser 块同一时刻至多一个，使用固定 id `ASK_USER_BLOCK_ID`，
//! 选项导航的可变状态（cursor/selected/chat_input_active）只在此块内维护，
//! 作为渲染与交互的单一真相。

use super::block::ConversationBlock;
use super::change::ConversationChange;
use super::model::ConversationModel;

/// AskUser 交互块的固定 id（同一时刻至多一个）。
pub const ASK_USER_BLOCK_ID: &str = "ask-user";

/// AskUser 块的可变交互状态快照，供控制器在提交/导航时读取。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AskUserSnapshot {
    pub cursor: usize,
    pub selected: Vec<bool>,
    pub chat_input_active: bool,
}

impl ConversationModel {
    /// 读取当前 AskUser 块的交互状态快照（无块时返回 None）。
    pub fn ask_user_snapshot(&self) -> Option<AskUserSnapshot> {
        self.blocks.iter().find_map(|block| {
            if let ConversationBlock::AskUser {
                cursor,
                selected,
                chat_input_active,
                ..
            } = block
            {
                Some(AskUserSnapshot {
                    cursor: *cursor,
                    selected: selected.clone(),
                    chat_input_active: *chat_input_active,
                })
            } else {
                None
            }
        })
    }

    /// 显示 AskUser 交互块；若已存在则替换。
    pub(super) fn show_ask_user(
        &mut self,
        question: String,
        options: Vec<sdk::OptionItem>,
        llm_option_count: usize,
        multi_select: bool,
        cursor: usize,
        default: Option<String>,
    ) -> Vec<ConversationChange> {
        let total = options.len();
        let cursor = if total == 0 { 0 } else { cursor.min(total - 1) };
        self.remove_ask_user_block();
        self.blocks.push(ConversationBlock::AskUser {
            id: ASK_USER_BLOCK_ID.to_string(),
            question,
            options,
            llm_option_count,
            multi_select,
            cursor,
            selected: vec![false; total],
            chat_input_active: false,
            chat_input_text: String::new(),
            default,
            answer: None,
        });
        vec![
            ConversationChange::AskUserShown {
                id: ASK_USER_BLOCK_ID.to_string(),
            },
            ConversationChange::OutputDirty,
        ]
    }

    /// 更新 AskUser 块光标位置（越界自动夹取）。
    pub(super) fn set_ask_user_cursor(&mut self, cursor: usize) -> Vec<ConversationChange> {
        if let Some(ConversationBlock::AskUser {
            cursor: current,
            options,
            ..
        }) = self.ask_user_block_mut()
        {
            if options.is_empty() {
                return Vec::new();
            }
            *current = cursor.min(options.len() - 1);
            return Self::ask_user_updated();
        }
        Vec::new()
    }

    /// 切换 AskUser 块中某选项的勾选状态。内建选项（>= llm_option_count）不可勾选。
    pub(super) fn toggle_ask_user_selected(&mut self, index: usize) -> Vec<ConversationChange> {
        if let Some(ConversationBlock::AskUser {
            selected,
            llm_option_count,
            ..
        }) = self.ask_user_block_mut()
        {
            if index >= *llm_option_count {
                return Vec::new();
            }
            if let Some(flag) = selected.get_mut(index) {
                *flag = !*flag;
                return Self::ask_user_updated();
            }
        }
        Vec::new()
    }

    /// 设置 AskUser 块是否处于自由输入子态。
    pub(super) fn set_ask_user_chat_input(&mut self, active: bool) -> Vec<ConversationChange> {
        if let Some(ConversationBlock::AskUser {
            chat_input_active,
            chat_input_text,
            ..
        }) = self.ask_user_block_mut()
        {
            *chat_input_active = active;
            if !active {
                chat_input_text.clear();
            }
            return Self::ask_user_updated();
        }
        Vec::new()
    }

    /// 追加字符到 Type something 输入框。
    pub(super) fn append_ask_user_chat_char(&mut self, ch: char) -> Vec<ConversationChange> {
        if let Some(ConversationBlock::AskUser {
            chat_input_active,
            chat_input_text,
            ..
        }) = self.ask_user_block_mut()
        {
            if *chat_input_active {
                chat_input_text.push(ch);
                return Self::ask_user_updated();
            }
        }
        Vec::new()
    }

    /// 删除 Type something 输入框末尾字符。
    pub(super) fn delete_ask_user_chat_char(&mut self) -> Vec<ConversationChange> {
        if let Some(ConversationBlock::AskUser {
            chat_input_active,
            chat_input_text,
            ..
        }) = self.ask_user_block_mut()
        {
            if *chat_input_active {
                chat_input_text.pop();
                return Self::ask_user_updated();
            }
        }
        Vec::new()
    }

    /// 获取 Type something 输入框文本。
    pub fn ask_user_chat_text(&self) -> Option<String> {
        self.blocks.iter().find_map(|block| {
            if let ConversationBlock::AskUser {
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

    /// 移除 AskUser 交互块。
    pub(super) fn dismiss_ask_user(&mut self) -> Vec<ConversationChange> {
        if self.remove_ask_user_block() {
            return vec![
                ConversationChange::AskUserDismissed,
                ConversationChange::OutputDirty,
            ];
        }
        Vec::new()
    }

    /// 设置用户回答内容，block 进入已回答状态（不再显示选项和键盘提示）。
    pub(super) fn answer_ask_user(&mut self, answer: String) -> Vec<ConversationChange> {
        if let Some(ConversationBlock::AskUser {
            answer: ans, ..
        }) = self.ask_user_block_mut()
        {
            *ans = Some(answer);
            return Self::ask_user_updated();
        }
        Vec::new()
    }

    fn ask_user_updated() -> Vec<ConversationChange> {
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
            .find(|block| matches!(block, ConversationBlock::AskUser { .. }))
    }

    /// 移除已存在的 AskUser 块，返回是否实际移除。
    fn remove_ask_user_block(&mut self) -> bool {
        let before = self.blocks.len();
        self.blocks
            .retain(|block| !matches!(block, ConversationBlock::AskUser { .. }));
        before != self.blocks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::intent::ConversationIntent;

    fn show(model: &mut ConversationModel, options: &[&str], llm: usize, multi: bool) {
        model.apply(ConversationIntent::ShowAskUser {
            question: "选哪个?".to_string(),
            options: options
                .iter()
                .map(|s| sdk::OptionItem::title_only(s.to_string()))
                .collect(),
            llm_option_count: llm,
            multi_select: multi,
            cursor: 0,
            default: None,
        });
    }

    fn ask_block(model: &ConversationModel) -> &ConversationBlock {
        model
            .blocks
            .iter()
            .find(|b| matches!(b, ConversationBlock::AskUser { .. }))
            .expect("ask user block")
    }

    #[test]
    fn test_show_ask_user_inserts_single_block() {
        let mut model = ConversationModel::default();
        show(&mut model, &["A", "B"], 2, false);
        // 再次 show 应替换而非新增
        show(&mut model, &["C"], 1, false);
        let count = model
            .blocks
            .iter()
            .filter(|b| matches!(b, ConversationBlock::AskUser { .. }))
            .count();
        assert_eq!(count, 1);
        if let ConversationBlock::AskUser { options, .. } = ask_block(&model) {
            assert_eq!(options[0].title, "C");
        }
    }

    #[test]
    fn test_set_ask_user_cursor_clamps_out_of_range() {
        let mut model = ConversationModel::default();
        show(&mut model, &["A", "B"], 2, false);
        model.apply(ConversationIntent::SetAskUserCursor { cursor: 99 });
        if let ConversationBlock::AskUser { cursor, .. } = ask_block(&model) {
            assert_eq!(*cursor, 1);
        }
    }

    #[test]
    fn test_set_ask_user_cursor_without_block_is_noop() {
        let mut model = ConversationModel::default();
        let changes = model.apply(ConversationIntent::SetAskUserCursor { cursor: 0 });
        assert!(changes.is_empty());
    }

    #[test]
    fn test_toggle_ask_user_selected_toggles_llm_option() {
        let mut model = ConversationModel::default();
        show(&mut model, &["A", "B"], 2, true);
        model.apply(ConversationIntent::ToggleAskUserSelected { index: 1 });
        if let ConversationBlock::AskUser { selected, .. } = ask_block(&model) {
            assert_eq!(selected, &vec![false, true]);
        }
    }

    #[test]
    fn test_toggle_ask_user_selected_rejects_builtin_option() {
        let mut model = ConversationModel::default();
        // 2 个 LLM 选项 + 内建项位于索引 2
        show(&mut model, &["A", "B", "All"], 2, true);
        let changes = model.apply(ConversationIntent::ToggleAskUserSelected { index: 2 });
        assert!(changes.is_empty());
        if let ConversationBlock::AskUser { selected, .. } = ask_block(&model) {
            assert_eq!(selected, &vec![false, false, false]);
        }
    }

    #[test]
    fn test_dismiss_ask_user_removes_block() {
        let mut model = ConversationModel::default();
        show(&mut model, &["A"], 1, false);
        let changes = model.apply(ConversationIntent::DismissAskUser);
        assert!(changes
            .iter()
            .any(|c| matches!(c, ConversationChange::AskUserDismissed)));
        assert!(!model
            .blocks
            .iter()
            .any(|b| matches!(b, ConversationBlock::AskUser { .. })));
    }

    #[test]
    fn test_dismiss_ask_user_without_block_is_noop() {
        let mut model = ConversationModel::default();
        let changes = model.apply(ConversationIntent::DismissAskUser);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_ask_user_snapshot_reflects_block_state() {
        let mut model = ConversationModel::default();
        // 无块时返回 None（错误路径）
        assert!(model.ask_user_snapshot().is_none());
        show(&mut model, &["A", "B"], 2, true);
        model.apply(ConversationIntent::SetAskUserCursor { cursor: 1 });
        model.apply(ConversationIntent::ToggleAskUserSelected { index: 0 });
        let snap = model.ask_user_snapshot().expect("snapshot");
        assert_eq!(snap.cursor, 1);
        assert_eq!(snap.selected, vec![true, false]);
        assert!(!snap.chat_input_active);
    }

    #[test]
    fn test_set_ask_user_chat_input_toggles_flag() {
        let mut model = ConversationModel::default();
        show(&mut model, &["A"], 1, false);
        model.apply(ConversationIntent::SetAskUserChatInput { active: true });
        if let ConversationBlock::AskUser {
            chat_input_active, ..
        } = ask_block(&model)
        {
            assert!(*chat_input_active);
        }
    }
}
