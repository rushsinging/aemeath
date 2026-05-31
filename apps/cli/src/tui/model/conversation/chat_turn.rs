use super::ids::{ChatId, ChatTurnId, ToolCallId, ToolStreamKey};
use super::tool_call::{ToolCall, ToolCallStatus};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatTurn {
    pub id: ChatTurnId,
    pub sequence: usize,
    pub status: ChatTurnStatus,
    pub assistant_stream: String,
    pub tool_calls: Vec<ToolCall>,
}

impl ChatTurn {
    pub fn new(id: ChatTurnId, sequence: usize) -> Self {
        Self {
            id,
            sequence,
            status: ChatTurnStatus::Streaming,
            assistant_stream: String::new(),
            tool_calls: Vec::new(),
        }
    }

    pub fn observe_tool_start(&mut self, chat_id: ChatId, name: String, index: usize) {
        let key = ToolStreamKey::new(chat_id, self.id.clone(), name, index);
        self.tool_calls.push(ToolCall::pending(key));
        self.status = ChatTurnStatus::ToolCalling;
    }

    pub fn bind_tool(
        &mut self,
        id: ToolCallId,
        name: &str,
        index: usize,
        summary: String,
    ) -> Option<String> {
        // agent loop 每轮往同一 turn 追加占位，index 跨轮重复（0,1,0,1…）；且 ToolCallStart 用
        // 工具序号、ObserveToolCall 用 content-block 序号（assistant 前置 thinking/text 时偏移）。
        // 故 bind 只认「未绑定」占位：优先 (name,index) 精确匹配的未绑定占位，否则回退同名首个
        // 未绑定占位。绝不覆盖已绑定占位——否则前一轮的 id 会被冲掉，其结果在 assemble 时找不到
        // 对应 tool call，进而泄漏完整原始 output（#87 根因 B / #86 同步问题）。
        let pos = self
            .tool_calls
            .iter()
            .position(|call| {
                call.id.is_none() && call.stream_key.name == name && call.stream_key.index == index
            })
            .or_else(|| {
                self.tool_calls
                    .iter()
                    .position(|call| call.id.is_none() && call.stream_key.name == name)
            })?;
        let call = &mut self.tool_calls[pos];
        call.bind(id, summary);
        self.status = ChatTurnStatus::ToolExecuting;
        Some(call.args_preview.clone())
    }

    pub fn complete_tool(
        &mut self,
        id: &str,
        output: String,
        is_error: bool,
    ) -> Option<ToolCallStatus> {
        let call = self
            .tool_calls
            .iter_mut()
            .find(|call| call.id.as_ref().map(AsRef::as_ref) == Some(id))?;
        call.complete(output, is_error);
        let status = call.status;
        if self.tool_calls.iter().all(|call| {
            matches!(
                call.status,
                ToolCallStatus::Success
                    | ToolCallStatus::Error
                    | ToolCallStatus::Cancelled
                    | ToolCallStatus::Orphaned
            )
        }) {
            self.status = ChatTurnStatus::Completing;
        }
        Some(status)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatTurnStatus {
    Streaming,
    ToolCalling,
    ToolExecuting,
    Completing,
    Completed,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};

    fn bound_ids(turn: &ChatTurn) -> Vec<String> {
        turn.tool_calls
            .iter()
            .filter_map(|call| call.id.as_ref().map(|id| id.as_ref().to_string()))
            .collect()
    }

    #[test]
    fn test_bind_tool_exact_match_binds_correct_placeholder() {
        // 正常路径：(name, index) 精确匹配未绑定占位。
        let mut turn = ChatTurn::new(ChatTurnId::new("t"), 0);
        let chat = ChatId::new("c");
        turn.observe_tool_start(chat.clone(), "Read".into(), 0);
        turn.observe_tool_start(chat, "Bash".into(), 1);

        assert!(turn
            .bind_tool(ToolCallId::new("call_r"), "Read", 0, String::new())
            .is_some());
        assert_eq!(turn.tool_calls[0].id.as_ref().unwrap().as_ref(), "call_r");
        assert!(turn.tool_calls[1].id.is_none(), "Bash 占位不应被绑");
    }

    #[test]
    fn test_bind_tool_falls_back_to_unbound_when_index_mismatched() {
        // 根因 A：ToolCallStart 用工具序号(0,1)、ObserveToolCall 用 content-block 序号
        // （前置 thinking 时偏移成 1,2）。index=2 无精确占位时应回退绑同名首个未绑定占位，
        // 而非返回 None 成 orphan。
        let mut turn = ChatTurn::new(ChatTurnId::new("t"), 0);
        let chat = ChatId::new("c");
        turn.observe_tool_start(chat.clone(), "Read".into(), 0);
        turn.observe_tool_start(chat, "Read".into(), 1);

        assert!(turn
            .bind_tool(ToolCallId::new("call_a"), "Read", 1, String::new())
            .is_some());
        assert!(
            turn.bind_tool(ToolCallId::new("call_b"), "Read", 2, String::new())
                .is_some(),
            "index 错位应回退绑定，不应成 orphan"
        );

        let ids = bound_ids(&turn);
        assert_eq!(ids.len(), 2, "两个占位都应绑上，实际: {ids:?}");
        assert!(ids.contains(&"call_a".to_string()) && ids.contains(&"call_b".to_string()));
    }

    #[test]
    fn test_bind_tool_never_overwrites_already_bound_placeholder() {
        // 根因 B：agent loop 每轮往同一 turn push 占位，index 跨轮重复（0,1,0,1）。
        // 轮2 bind(index=1) 的 find-first 会命中轮1 已绑占位——绝不能覆盖，否则丢 id 致泄漏。
        let mut turn = ChatTurn::new(ChatTurnId::new("t"), 0);
        let chat = ChatId::new("c");
        // 轮 1 占位 + 绑定。
        turn.observe_tool_start(chat.clone(), "Read".into(), 0);
        turn.observe_tool_start(chat.clone(), "Read".into(), 1);
        turn.bind_tool(ToolCallId::new("call_1"), "Read", 1, String::new());
        // 轮 2 占位（index 跨轮重复）。
        turn.observe_tool_start(chat.clone(), "Read".into(), 0);
        turn.observe_tool_start(chat, "Read".into(), 1);
        // 轮 2 bind(index=1)：first (Read,1) 是轮1 已绑 call_1，不应覆盖。
        turn.bind_tool(ToolCallId::new("call_2"), "Read", 1, String::new());

        let ids = bound_ids(&turn);
        assert!(
            ids.contains(&"call_1".to_string()),
            "已绑定的 call_1 不应被覆盖丢失（#87 泄漏根因 B），实际: {ids:?}"
        );
        assert!(
            ids.contains(&"call_2".to_string()),
            "call_2 应绑到另一个未绑定占位，实际: {ids:?}"
        );
    }
}
