use super::change::ConversationChange;
use super::ids::{ChatId, ChatRunId, ToolCallId};
use super::model::ConversationModel;
use super::tool_call::ToolCallStatus;
use super::tool_result_payload::ToolResultPayload;
use crate::tui::model::output_timeline::OutputTimelineItem;

impl ConversationModel {
    pub(super) fn present_cancelled_step(&mut self, confirmed: bool) -> Vec<ConversationChange> {
        let mut cancelled_tools = Vec::new();
        if confirmed {
            if let Some((chat_id, turn)) = self.chats.iter_mut().rev().find_map(|chat| {
                chat.runs
                    .iter_mut()
                    .rev()
                    .find(|turn| {
                        turn.tool_calls.iter().any(|call| {
                            call.status == ToolCallStatus::Running
                                || call.status == ToolCallStatus::Ready
                                || call.status == ToolCallStatus::PendingArgs
                        })
                    })
                    .map(|turn| (chat.id.to_string(), turn))
            }) {
                for call in &mut turn.tool_calls {
                    if call.cancel() {
                        if let Some(id) = call.id.as_ref() {
                            cancelled_tools.push((
                                chat_id.clone(),
                                turn.id.to_string(),
                                id.to_string(),
                            ));
                        }
                    }
                }
            }
        }

        let mut changes = cancelled_tools
            .into_iter()
            .map(
                |(chat_id, run_id, id)| ConversationChange::ToolCallCompleted {
                    chat_id,
                    run_id,
                    id,
                    status: ToolCallStatus::Cancelled,
                },
            )
            .collect::<Vec<_>>();
        changes.extend(self.apply(super::intent::TerminalNotice {
            cause: super::terminal::TerminalCause::UserCancelled,
            duration: None,
        }));
        changes.push(ConversationChange::StyleBoundaryResetRequired);
        changes.push(ConversationChange::OutputDirty);
        changes
    }

    pub(super) fn promote_orphan_tool_result(
        &mut self,
        chat_id: &ChatId,
        run_id: &ChatRunId,
        id: &str,
    ) {
        // 从 timeline 查找 OrphanToolResult 并克隆 payload。
        // O(1) 前置判断：正常路径（result 晚于 call 到达）不存在 orphan，
        // 避免每次 ToolCallUpdate 都全表扫描 timeline（issue #1467）。
        if !self.timeline.contains_orphan(id) {
            return;
        }
        let orphan_payload = self.timeline.items().iter().find_map(|item| {
            if let OutputTimelineItem::OrphanToolResult {
                id: orphan_id,
                output,
                content,
                is_error,
                ..
            } = item
            {
                if orphan_id == id {
                    return Some((output.clone(), content.clone(), *is_error));
                }
            }
            None
        });
        let Some((output, content, is_error)) = orphan_payload else {
            return;
        };
        if self
            .complete_tool_in_context(
                chat_id,
                run_id,
                id,
                ToolResultPayload::new(output, content, is_error, 0),
            )
            .is_some()
        {
            self.timeline.retain(|item| {
                !matches!(item, OutputTimelineItem::OrphanToolResult { id: orphan_id, .. } if orphan_id == id)
            });
            self.insert_tool_result_after_tool_call(
                chat_id.clone(),
                run_id.clone(),
                ToolCallId::from_legacy_or_new(id),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn complete_tool_call(
        &mut self,
        chat_id: ChatId,
        run_id: ChatRunId,
        id: ToolCallId,
        _provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        image_count: usize,
    ) -> Vec<ConversationChange> {
        self.ensure_runtime_turn(chat_id.clone(), run_id.clone());
        if let Some(status) = self.complete_tool_in_context(
            &chat_id,
            &run_id,
            id.as_ref(),
            ToolResultPayload::new(output.clone(), content.clone(), is_error, image_count),
        ) {
            self.insert_tool_result_after_tool_call(chat_id.clone(), run_id.clone(), id.clone());
            crate::tui::log_debug!(
                "model observe tool_result embedded id={} tool_name={} status={:?} is_error={} image_count={} timeline_items_after={}",
                id,
                tool_name,
                status,
                is_error,
                image_count,
                self.timeline.items().len(),
            );
            return vec![
                ConversationChange::ToolCallCompleted {
                    chat_id: chat_id.to_string(),
                    run_id: run_id.to_string(),
                    id: id.to_string(),
                    status,
                },
                ConversationChange::StyleBoundaryResetRequired,
                ConversationChange::OutputDirty,
            ];
        }
        self.timeline.push(OutputTimelineItem::OrphanToolResult {
            id: id.to_string(),
            tool_name: tool_name.clone(),
            output: output.clone(),
            content: content.clone(),
            is_error,
        });
        crate::tui::log_debug!(
            "model observe tool_result orphan id={} is_error={} image_count={} timeline_items_after={}",
            id,
            is_error,
            image_count,
            self.timeline.items().len(),
        );
        vec![
            ConversationChange::OrphanToolResultObserved { id: id.to_string() },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }
    pub(super) fn complete_tool_in_context(
        &mut self,
        chat_id: &ChatId,
        run_id: &ChatRunId,
        id: &str,
        result: ToolResultPayload,
    ) -> Option<ToolCallStatus> {
        self.chats
            .iter_mut()
            .find(|chat| &chat.id == chat_id)
            .and_then(|chat| chat.runs.iter_mut().find(|turn| &turn.id == run_id))
            .and_then(|turn| turn.complete_tool(id, result))
    }
}
