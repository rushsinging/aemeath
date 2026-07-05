use super::ids::{ChatId, ChatTurnId, ToolCallId, ToolStreamKey};
use super::tool_call::{ToolCall, ToolCallStatus};
use super::tool_result_payload::ToolResultPayload;

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

    pub fn observe_tool_start(
        &mut self,
        id: ToolCallId,
        chat_id: ChatId,
        name: String,
        index: usize,
        provider_id: Option<String>,
        model_id: Option<String>,
        role: Option<String>,
    ) {
        let key = ToolStreamKey::new(chat_id, self.id.clone(), name, index);
        let mut call = ToolCall::pending(id, key);
        call.provider_id = provider_id;
        call.model_id = model_id;
        call.role = role;
        self.tool_calls.push(call);
        self.status = ChatTurnStatus::ToolExecuting;
    }

    pub fn update_tool(
        &mut self,
        id: &str,
        arguments: Option<String>,
        status: ToolCallStatus,
    ) -> Option<String> {
        let call = self
            .tool_calls
            .iter_mut()
            .find(|call| call.id.as_ref().map(AsRef::as_ref) == Some(id))?;
        call.update(arguments, status);
        self.status = match status {
            ToolCallStatus::PendingArgs | ToolCallStatus::Ready => {
                if self.status == ChatTurnStatus::Completed {
                    self.status
                } else {
                    ChatTurnStatus::ToolCalling
                }
            }
            ToolCallStatus::Running => ChatTurnStatus::ToolExecuting,
            ToolCallStatus::Success
            | ToolCallStatus::Error
            | ToolCallStatus::Cancelled
            | ToolCallStatus::Orphaned => self.status,
        };
        Some(call.args_preview.clone())
    }
    pub fn bind_tool(&mut self, id: &str) -> Option<String> {
        let call = self
            .tool_calls
            .iter_mut()
            .find(|call| call.id.as_ref().map(AsRef::as_ref) == Some(id))?;
        call.bind();
        self.status = ChatTurnStatus::ToolExecuting;
        Some(call.args_preview.clone())
    }
    pub fn complete_tool(&mut self, id: &str, result: ToolResultPayload) -> Option<ToolCallStatus> {
        let call = self
            .tool_calls
            .iter_mut()
            .find(|call| call.id.as_ref().map(AsRef::as_ref) == Some(id))?;
        call.complete(result);
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

    fn bound_ids(turn: &ChatTurn) -> Vec<ToolCallId> {
        turn.tool_calls
            .iter()
            .filter_map(|call| call.id.clone())
            .collect()
    }

    #[test]
    fn test_bind_tool_exact_match_binds_correct_placeholder() {
        // 正常路径：(name, index) 精确匹配未绑定占位。
        let mut turn = ChatTurn::new(ChatTurnId::new("t"), 0);
        let chat = ChatId::new("c");
        let call_r = ToolCallId::new("call_r");
        let call_b = ToolCallId::new("call_b");
        turn.observe_tool_start(
            call_r.clone(),
            chat.clone(),
            "Read".into(),
            0,
            None,
            None,
            None,
        );
        turn.observe_tool_start(call_b.clone(), chat, "Bash".into(), 1, None, None, None);

        assert!(turn.bind_tool(call_r.as_str()).is_some());
        assert_eq!(turn.tool_calls[0].id.as_ref(), Some(&call_r));
        assert_eq!(turn.tool_calls[1].id.as_ref(), Some(&call_b));
    }

    #[test]
    fn test_bind_tool_falls_back_to_unbound_when_index_mismatched() {
        // 根因 A：ToolCallStart 用工具序号(0,1)、ObserveToolCall 用 content-block 序号
        // （前置 thinking 时偏移成 1,2）。index=2 无精确占位时应回退绑同名首个未绑定占位，
        // 而非返回 None 成 orphan。
        let mut turn = ChatTurn::new(ChatTurnId::new("t"), 0);
        let chat = ChatId::new("c");
        let call_a = ToolCallId::new("call_a");
        let call_b = ToolCallId::new("call_b");
        turn.observe_tool_start(
            call_a.clone(),
            chat.clone(),
            "Read".into(),
            0,
            None,
            None,
            None,
        );
        turn.observe_tool_start(call_b.clone(), chat, "Read".into(), 1, None, None, None);

        assert!(turn.bind_tool(call_a.as_str()).is_some());
        assert!(
            turn.bind_tool(call_b.as_str()).is_some(),
            "internal id 应直接绑定，不再依赖 provider/content index"
        );

        let ids = bound_ids(&turn);
        assert_eq!(ids.len(), 2, "两个占位都应绑上，实际: {ids:?}");
        assert!(ids.contains(&call_a) && ids.contains(&call_b));
    }

    #[test]
    fn test_bind_tool_never_overwrites_already_bound_placeholder() {
        // 根因 B：agent loop 每轮往同一 turn push 占位，index 跨轮重复（0,1,0,1）。
        // 轮2 bind(index=1) 的 find-first 会命中轮1 已绑占位——绝不能覆盖，否则丢 id 致泄漏。
        let mut turn = ChatTurn::new(ChatTurnId::new("t"), 0);
        let chat = ChatId::new("c");
        let call_1a = ToolCallId::new("call_1a");
        let call_1 = ToolCallId::new("call_1");
        let call_2a = ToolCallId::new("call_2a");
        let call_2 = ToolCallId::new("call_2");
        // 轮 1 占位 + 绑定。
        turn.observe_tool_start(
            call_1a.clone(),
            chat.clone(),
            "Read".into(),
            0,
            None,
            None,
            None,
        );
        turn.observe_tool_start(
            call_1.clone(),
            chat.clone(),
            "Read".into(),
            1,
            None,
            None,
            None,
        );
        turn.bind_tool(call_1.as_str());
        // 轮 2 占位（index 跨轮重复）。
        turn.observe_tool_start(
            call_2a.clone(),
            chat.clone(),
            "Read".into(),
            0,
            None,
            None,
            None,
        );
        turn.observe_tool_start(call_2.clone(), chat, "Read".into(), 1, None, None, None);
        // 轮 2 bind(index=1)：internal id 直连，不会覆盖轮1 已绑 call_1。
        turn.bind_tool(call_2.as_str());

        let ids = bound_ids(&turn);
        assert!(
            ids.contains(&call_1),
            "已绑定的 call_1 不应被覆盖丢失（#87 泄漏根因 B），实际: {ids:?}"
        );
        assert!(
            ids.contains(&call_2),
            "call_2 应绑到另一个未绑定占位，实际: {ids:?}"
        );
    }
}
