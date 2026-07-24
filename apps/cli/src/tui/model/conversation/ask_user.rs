//! ConversationModel 的 AskUserQuestion 批量交互块逻辑。
//!
//! AskUserBatch 同一时刻至多一个未完成的交互块；已完成块以首个 slot id
//! 派生稳定身份保留在 timeline 中，管理多问 + 确认页状态机。

use super::block::{AskUserPhase, AskUserSlot};
use super::change::ConversationChange;
use super::model::ConversationModel;
use crate::tui::model::output_timeline::OutputTimelineItem;

/// AskUser 交互块的 id 前缀。
pub const ASK_USER_BLOCK_ID_PREFIX: &str = "ask-user-";

fn ask_user_block_id(slots: &[AskUserSlot]) -> String {
    slots
        .first()
        .map(|slot| format!("{ASK_USER_BLOCK_ID_PREFIX}{}", slot.id))
        .unwrap_or_else(|| "ask-user-empty".to_string())
}

fn slot_starts_in_chat_input(slot: Option<&AskUserSlot>) -> bool {
    slot.is_some_and(|slot| slot.options.is_empty())
}

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
    /// 获取当前 AskUserBatch 块的 slot 数（无块时返回 None）。
    pub fn ask_user_slot_count(&self) -> Option<usize> {
        self.timeline.items().iter().find_map(|item| {
            if let OutputTimelineItem::AskUserBatch {
                slots,
                confirmed: false,
                ..
            } = item
            {
                Some(slots.len())
            } else {
                None
            }
        })
    }

    /// 收集当前 AskUserBatch 块各 slot 的答案（含已完成块）。
    pub fn ask_user_batch_answers(&self) -> Option<Vec<String>> {
        self.timeline.items().iter().find_map(|item| {
            if let OutputTimelineItem::AskUserBatch { slots, .. } = item {
                let answers: Vec<String> = slots.iter().filter_map(|s| s.answer.clone()).collect();
                if answers.len() == slots.len() {
                    Some(answers)
                } else {
                    None
                }
            } else {
                None
            }
        })
    }

    /// 获取当前激活 slot 中指定索引的选项文本。
    pub fn ask_user_batch_option_text(&self, index: usize) -> Option<String> {
        self.timeline.items().iter().find_map(|item| {
            if let OutputTimelineItem::AskUserBatch {
                slots,
                active_index,
                confirmed: false,
                ..
            } = item
            {
                slots
                    .get(*active_index)
                    .and_then(|slot| slot.options.get(index).map(|o| o.title.clone()))
            } else {
                None
            }
        })
    }

    /// 获取当前激活 slot 的全部选项文本。
    pub fn ask_user_batch_active_options(&self) -> Option<Vec<String>> {
        self.timeline.items().iter().find_map(|item| {
            if let OutputTimelineItem::AskUserBatch {
                slots,
                active_index,
                confirmed: false,
                ..
            } = item
            {
                slots
                    .get(*active_index)
                    .map(|slot| slot.options.iter().map(|o| o.title.clone()).collect())
            } else {
                None
            }
        })
    }

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
                confirmed: false,
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
                    confirmed: false,
                })
            } else {
                None
            }
        })
    }

    /// 显示 AskUserBatch 交互块；若已存在则替换。
    /// #944 5B: dead code after AskUserBatch reply_tx retirement.
    #[allow(dead_code)]
    pub(super) fn show_ask_user_batch(
        &mut self,
        slots: Vec<AskUserSlot>,
    ) -> Vec<ConversationChange> {
        let id = ask_user_block_id(&slots);
        let first_total = slots.first().map(|s| s.options.len()).unwrap_or(0);
        let chat_input_active = slot_starts_in_chat_input(slots.first());
        self.clear_active_text_blocks();
        self.remove_active_ask_user_block();
        let n = slots.len();
        self.timeline.push(OutputTimelineItem::AskUserBatch {
            id: id.clone(),
            slots,
            active_index: 0,
            phase: AskUserPhase::Answering,
            cursor: 0,
            selected: vec![false; first_total],
            chat_input_active,
            chat_input_text: String::new(),
            chat_input_cursor: 0,
            confirm_cursor: n,
            confirmed: false,
        });
        vec![
            ConversationChange::AskUserShown { id },
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
                *chat_input_active = slot_starts_in_chat_input(slots.get(*active_index));
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
            *chat_input_active = slot_starts_in_chat_input(slots.get(index));
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
        if self.remove_active_ask_user_block() {
            return vec![
                ConversationChange::AskUserDismissed,
                ConversationChange::OutputDirty,
            ];
        }
        Vec::new()
    }

    /// 追加一个仅用于历史展示的已完成 AskUserBatch。
    pub(crate) fn restore_answered_ask_user_batch(
        &mut self,
        slots: Vec<AskUserSlot>,
    ) -> Vec<ConversationChange> {
        if slots.is_empty() {
            return Vec::new();
        }
        let id = ask_user_block_id(&slots);
        let n = slots.len();
        self.timeline.push(OutputTimelineItem::AskUserBatch {
            id: id.clone(),
            slots,
            active_index: 0,
            phase: AskUserPhase::Answering,
            cursor: 0,
            selected: Vec::new(),
            chat_input_active: false,
            chat_input_text: String::new(),
            chat_input_cursor: 0,
            confirm_cursor: n,
            confirmed: true,
        });
        vec![
            ConversationChange::AskUserShown { id },
            ConversationChange::OutputDirty,
        ]
    }

    fn ask_user_updated(&mut self) -> Vec<ConversationChange> {
        let id = self
            .ask_user_timeline_item_mut()
            .and_then(|item| match item {
                OutputTimelineItem::AskUserBatch { id, .. } => Some(id.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "ask-user-empty".to_string());
        vec![
            ConversationChange::AskUserUpdated { id },
            ConversationChange::OutputDirty,
        ]
    }

    fn ask_user_timeline_item_mut(&mut self) -> Option<&mut OutputTimelineItem> {
        self.timeline.items_mut().iter_mut().find(|item| {
            matches!(
                item,
                OutputTimelineItem::AskUserBatch {
                    confirmed: false,
                    ..
                }
            )
        })
    }

    /// 移除当前未完成的 AskUserBatch 块，返回是否实际移除。
    fn remove_active_ask_user_block(&mut self) -> bool {
        let before = self.timeline.items().len();
        self.timeline.retain(|item| {
            !matches!(
                item,
                OutputTimelineItem::AskUserBatch {
                    confirmed: false,
                    ..
                }
            )
        });
        before != self.timeline.items().len()
    }
}

#[cfg(test)]
#[path = "ask_user_tests.rs"]
mod tests;
