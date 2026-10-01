//! 同批用户消息折叠（#1818）。
//!
//! 一次原子 drain 里的多条用户消息表达的是同一次连续输入，模型侧应当看到
//! 一条消息而不是「用户连发 N 次」。折叠发生在**接纳层**——在事件进入
//! `run_input_buffer` / `accepted_inputs` 之前合并，这样 `user_message_snapshot`
//! 只返回 1 条，SDK 排队快照随之收敛，TUI 的全量替换路径自动显示 1 组排队行。
//!
//! 折叠规则：
//! - 连续 `UserMessage` 段合并为一条，文本以空行分隔；
//! - 图片按拼接顺序并入同一条消息，占位符**全局重编号**（每条消息内部
//!   从 `[Image #1]` 起算，直接拼接会撞号）；
//! - `SkillRequest` 及其余事件是边界，原样保留（SkillRequest 携带
//!   `skill` / `arguments` / `raw_input` metadata，合并会丢结构）。

use sdk::{ChatInputEvent, ChatInputImage};

/// 把一批同批输入折叠为合并后的序列（#1818）。
///
/// 返回值长度 ≤ 输入长度；长度不变时等价于原序列（单条批次零行为变化）。
pub(crate) fn fold_batched_user_inputs(events: Vec<ChatInputEvent>) -> Vec<ChatInputEvent> {
    let mut folded: Vec<ChatInputEvent> = Vec::with_capacity(events.len());
    // 正在累积的连续用户消息段；遇到边界事件才落定。
    let mut pending: Option<MergedUserMessage> = None;

    for event in events {
        match event {
            ChatInputEvent::UserMessage { id, text, images } => {
                pending
                    .get_or_insert_with(|| MergedUserMessage::new(id))
                    .push(text, images);
            }
            boundary => {
                if let Some(merged) = pending.take() {
                    folded.push(merged.into_event());
                }
                folded.push(boundary);
            }
        }
    }
    if let Some(merged) = pending.take() {
        folded.push(merged.into_event());
    }
    folded
}

/// 连续用户消息段的累积器：文本空行拼接，图片占位符连续编号。
struct MergedUserMessage {
    input_id: sdk::InputId,
    text: String,
    images: Vec<ChatInputImage>,
}

impl MergedUserMessage {
    fn new(input_id: sdk::InputId) -> Self {
        Self {
            input_id,
            text: String::new(),
            images: Vec::new(),
        }
    }

    fn push(&mut self, text: String, images: Vec<ChatInputImage>) {
        if !self.text.is_empty() {
            self.text.push_str("\n\n");
        }
        // 占位符在本条消息内从 [Image #1] 起算，合并后要接着已并入的图片继续
        // 编号，否则两条都含 `[Image #1]` 时无法区分是哪张图。
        //
        // 逐条重编号：本条消息的占位符换成接着已有图片继续的全局序号。
        let assignments: Vec<(String, usize)> = images
            .iter()
            .enumerate()
            .map(|(offset, image)| (image.id.clone(), self.images.len() + offset))
            .collect();
        self.text
            .push_str(&renumber_placeholders(&text, &assignments));
        self.images.extend(images);
    }

    fn into_event(self) -> ChatInputEvent {
        ChatInputEvent::UserMessage {
            id: self.input_id,
            text: self.text,
            images: self.images,
        }
    }
}

/// 把 `text` 里的图片占位符按 `assignments`（`原占位符 → 新序号`）重写。
///
/// 必须逐位置重建而不能逐个 `str::replace`：把 `[Image #1]` 改成
/// `[Image #2]` 之后，下一个待替换的旧占位符恰好就是 `[Image #2]`，
/// 替换结果会被二次匹配。这里先收集全部匹配位置再单遍重建，
/// 已写出的区间不会重新进入匹配范围。
fn renumber_placeholders(text: &str, assignments: &[(String, usize)]) -> String {
    if assignments.is_empty() {
        return text.to_string();
    }
    let mut matches: Vec<(usize, usize, usize)> = Vec::new();
    for (placeholder, order) in assignments {
        for (start, _) in text.match_indices(placeholder.as_str()) {
            matches.push((start, placeholder.len(), *order));
        }
    }
    matches.sort_unstable();
    matches.dedup();

    let mut rewritten = String::with_capacity(text.len());
    let mut copied_up_to = 0usize;
    for (start, placeholder_len, order) in matches {
        // 防御重叠匹配（理论上占位符互不为前缀）：已复制过的区间不再重复输出。
        if start < copied_up_to {
            continue;
        }
        push_char_range(&mut rewritten, text, copied_up_to, start);
        rewritten.push_str(&image_placeholder(order));
        copied_up_to = start + placeholder_len;
    }
    push_char_range(&mut rewritten, text, copied_up_to, text.len());
    rewritten
}

/// 把 `text` 的 `[from, to)` 字节区间逐字符追加到 `out`。
///
/// 区间边界来自 `match_indices`，必落在字符边界上；用 `char_indices` 遍历
/// 而非字符串切片，RELEASE 下也不会有潜在 panic 路径。
fn push_char_range(out: &mut String, text: &str, from: usize, to: usize) {
    for (offset, character) in text.char_indices() {
        if offset >= to {
            break;
        }
        if offset >= from {
            out.push(character);
        }
    }
}

fn image_placeholder(order: usize) -> String {
    format!("[Image #{}]", order + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(placeholder: &str) -> ChatInputImage {
        ChatInputImage {
            id: placeholder.to_string(),
            base64: format!("data-{placeholder}"),
            media_type: "image/png".to_string(),
        }
    }

    fn user_message(id: &str, text: &str) -> ChatInputEvent {
        ChatInputEvent::UserMessage {
            id: sdk::InputId::new(id),
            text: text.to_string(),
            images: Vec::new(),
        }
    }

    fn folded_texts(events: Vec<ChatInputEvent>) -> Vec<String> {
        fold_batched_user_inputs(events)
            .into_iter()
            .map(|event| match event {
                ChatInputEvent::UserMessage { text, .. } => text,
                other => format!("<{other:?}>"),
            })
            .collect()
    }

    #[test]
    fn consecutive_user_messages_merge_with_blank_line_separator() {
        let texts = folded_texts(vec![
            user_message("a", "第一段"),
            user_message("b", "第二段"),
            user_message("c", "第三段"),
        ]);

        assert_eq!(texts, vec!["第一段\n\n第二段\n\n第三段"]);
    }

    #[test]
    fn merged_message_keeps_first_input_id() {
        let first = user_message("a", "第一段");
        let expected_id = match &first {
            ChatInputEvent::UserMessage { id, .. } => id.clone(),
            other => panic!("测试输入应为 UserMessage，实际 {other:?}"),
        };
        let folded = fold_batched_user_inputs(vec![first, user_message("b", "第二段")]);

        match &folded[0] {
            ChatInputEvent::UserMessage { id, .. } => assert_eq!(
                id, &expected_id,
                "合并消息沿用批次首条的 InputId，其余 id 随合并失去独立意义"
            ),
            other => panic!("应折叠为一条 UserMessage，实际 {other:?}"),
        }
    }

    #[test]
    fn skill_request_is_a_merge_boundary() {
        let skill = ChatInputEvent::SkillRequest(sdk::SkillRequest {
            input_id: sdk::InputId::new("skill"),
            skill: "superpowers:brainstorming".to_string(),
            arguments: "scope".to_string(),
            raw_input: "/superpowers:brainstorming scope".to_string(),
        });
        let events = vec![
            user_message("a", "前面"),
            user_message("b", "紧邻"),
            skill,
            user_message("c", "技能之后"),
            user_message("d", "紧邻之后"),
        ];

        let folded = fold_batched_user_inputs(events);

        assert_eq!(folded.len(), 3, "两段合并后的消息 + 技能");
        assert_eq!(folded_texts(folded.clone())[0], "前面\n\n紧邻");
        assert!(matches!(folded[1], ChatInputEvent::SkillRequest(_)));
        assert_eq!(folded_texts(folded)[2], "技能之后\n\n紧邻之后");
    }

    #[test]
    fn control_events_are_merge_boundaries() {
        let events = vec![
            user_message("a", "第一段"),
            ChatInputEvent::Compact,
            user_message("b", "第二段"),
        ];

        let folded = fold_batched_user_inputs(events);

        assert_eq!(folded.len(), 3);
        assert!(matches!(folded[1], ChatInputEvent::Compact));
    }

    #[test]
    fn single_message_batch_is_unchanged() {
        let folded = fold_batched_user_inputs(vec![user_message("a", "只有一条")]);

        assert_eq!(folded_texts(folded), vec!["只有一条"]);
    }

    #[test]
    fn empty_batch_stays_empty() {
        assert!(fold_batched_user_inputs(Vec::new()).is_empty());
    }

    #[test]
    fn image_placeholders_are_renumbered_across_merged_messages() {
        // 两条消息各自都从 [Image #1] 起算，直接拼接会撞号。
        let first = ChatInputEvent::UserMessage {
            id: sdk::InputId::new("a"),
            text: "看这张 [Image #1]".to_string(),
            images: vec![image("[Image #1]")],
        };
        let second = ChatInputEvent::UserMessage {
            id: sdk::InputId::new("b"),
            text: "再看 [Image #1] 和 [Image #2]".to_string(),
            images: vec![image("[Image #1]"), image("[Image #2]")],
        };

        let folded = fold_batched_user_inputs(vec![first, second]);

        match &folded[0] {
            ChatInputEvent::UserMessage { text, images, .. } => {
                assert_eq!(text, "看这张 [Image #1]\n\n再看 [Image #2] 和 [Image #3]");
                assert_eq!(images.len(), 3);
            }
            other => panic!("应折叠为一条 UserMessage，实际 {other:?}"),
        }
    }

    #[test]
    fn image_order_follows_message_concatenation_order() {
        let first = ChatInputEvent::UserMessage {
            id: sdk::InputId::new("a"),
            text: "A [Image #1]".to_string(),
            images: vec![image("[Image #1]")],
        };
        let second = ChatInputEvent::UserMessage {
            id: sdk::InputId::new("b"),
            text: "B [Image #1]".to_string(),
            images: vec![image("[Image #1]")],
        };

        let folded = fold_batched_user_inputs(vec![first, second]);

        match &folded[0] {
            ChatInputEvent::UserMessage { images, .. } => {
                assert_eq!(images[0].base64, "data-[Image #1]");
                assert_eq!(images[1].base64, "data-[Image #1]");
            }
            other => panic!("应折叠为一条 UserMessage，实际 {other:?}"),
        }
    }

    #[test]
    fn placeholder_rewrite_does_not_rematch_rewritten_tokens() {
        // 旧占位符 [Image #1] 被改成 [Image #2] 后，不能再被「旧的
        // [Image #2]」规则二次匹配——这正是逐个 str::replace 会出错的地方。
        let rewritten = renumber_placeholders(
            "旧 [Image #1] 然后 [Image #2]",
            &[("[Image #1]".to_string(), 1), ("[Image #2]".to_string(), 2)],
        );

        assert_eq!(rewritten, "旧 [Image #2] 然后 [Image #3]");
    }

    #[test]
    fn placeholder_rewrite_keeps_text_untouched_when_no_assignment_matches() {
        assert_eq!(
            renumber_placeholders("没有图片", &[("[Image #1]".to_string(), 0)]),
            "没有图片"
        );
    }
}
