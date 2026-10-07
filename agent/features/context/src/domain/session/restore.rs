//! Canonical Session 恢复投影。

use super::envelope::RestoreStepSource;
use super::message_integrity::{check_message_integrity, deep_clean_messages, sanitize_messages};
use crate::domain::session::CanonicalSession;
use crate::domain::{ToolCallReceiptData, ToolCallState};
use std::sync::Arc;

use share::message::{ContentBlock, Message, Role};

#[derive(Debug, Clone)]
pub struct SessionRestoreStepData {
    pub run_id: String,
    pub step_id: String,
    pub message_segments: Vec<Arc<[Message]>>,
    pub finalize_cause: Option<crate::domain::FinalizeCause>,
    pub duration_ms: Option<u64>,
}

impl SessionRestoreStepData {
    pub fn messages(&self) -> impl Iterator<Item = &Message> {
        self.message_segments
            .iter()
            .flat_map(|segment| segment.iter())
    }
}

#[derive(Debug, Clone)]
pub struct SessionRestore {
    pub active_messages: Vec<Message>,
    pub display_steps: Vec<SessionRestoreStepData>,
    pub compacted: bool,
    pub created_at: String,
    pub trimmed: usize,
    pub repaired: usize,
}

impl SessionRestore {
    pub fn active_from_canonical(session: &CanonicalSession) -> Self {
        let (active_steps, active_trimmed, active_repaired) =
            clean_steps(session.restore_steps_from_marker());
        let active_messages = active_steps
            .iter()
            .flat_map(SessionRestoreStepData::messages)
            .cloned()
            .collect();
        Self {
            active_messages,
            display_steps: Vec::new(),
            compacted: session.compact.is_some(),
            created_at: session.created_at.clone(),
            trimmed: active_trimmed,
            repaired: active_repaired,
        }
    }

    pub fn from_canonical(session: &CanonicalSession) -> Self {
        let (active_steps, active_trimmed, active_repaired) =
            clean_steps(session.restore_steps_from_marker());
        let (display_steps, _, _) = clean_steps(session.all_restore_steps());
        let active_messages = active_steps
            .iter()
            .flat_map(SessionRestoreStepData::messages)
            .cloned()
            .collect();
        Self {
            active_messages,
            display_steps,
            compacted: session.compact.is_some(),
            created_at: session.created_at.clone(),
            trimmed: active_trimmed,
            repaired: active_repaired,
        }
    }
}

fn clean_steps(raw_steps: Vec<RestoreStepSource>) -> (Vec<SessionRestoreStepData>, usize, usize) {
    let mut steps = Vec::with_capacity(raw_steps.len());
    let mut trimmed = 0;
    let mut repaired = 0;
    for RestoreStepSource {
        cursor,
        message_segments,
        tool_receipts,
        finalize_cause,
        duration_ms,
    } in raw_steps
    {
        let had_unfinished_receipts = has_unfinished_receipts(&tool_receipts);
        let needs_integrity_repair = message_segments_need_repair(&message_segments);
        let finalize_cause = finalize_cause.or_else(|| {
            had_unfinished_receipts.then_some(crate::domain::FinalizeCause::RunTerminated)
        });
        if !had_unfinished_receipts && !needs_integrity_repair {
            if !message_segments.iter().all(|segment| segment.is_empty()) {
                steps.push(SessionRestoreStepData {
                    run_id: cursor.run_id,
                    step_id: cursor.step_id,
                    message_segments,
                    finalize_cause,
                    duration_ms,
                });
            }
            continue;
        }

        let mut messages = message_segments
            .iter()
            .flat_map(|segment| segment.iter().cloned())
            .collect::<Vec<_>>();
        project_unfinished_tool_results(&mut messages, &tool_receipts);
        let before = messages.len();
        sanitize_messages(&mut messages);
        trimmed += before.saturating_sub(messages.len());
        if check_message_integrity(&messages).has_issues() {
            repaired += deep_clean_messages(&mut messages);
        }
        if !messages.is_empty() {
            steps.push(SessionRestoreStepData {
                run_id: cursor.run_id,
                step_id: cursor.step_id,
                message_segments: vec![messages.into()],
                finalize_cause,
                duration_ms,
            });
        }
    }
    (steps, trimmed, repaired)
}

fn message_segments_need_repair(message_segments: &[Arc<[Message]>]) -> bool {
    let mut previous_role = None;
    let mut tool_use_ids = std::collections::HashSet::new();
    let mut tool_result_ids = std::collections::HashSet::new();
    for message in message_segments.iter().flat_map(|segment| segment.iter()) {
        if previous_role
            .as_ref()
            .is_some_and(|role| role == &message.role)
        {
            return true;
        }
        previous_role = Some(message.role.clone());
        tool_use_ids.extend(message.tool_use_ids().into_iter().map(str::to_string));
        tool_result_ids.extend(message.tool_result_ids().into_iter().map(str::to_string));
    }
    tool_result_ids
        .iter()
        .any(|tool_result_id| !tool_use_ids.contains(tool_result_id))
        || tool_use_ids
            .iter()
            .any(|tool_use_id| !tool_result_ids.contains(tool_use_id))
}

fn has_unfinished_receipts(receipts: &[ToolCallReceiptData]) -> bool {
    receipts.iter().any(|receipt| {
        matches!(
            receipt.state,
            ToolCallState::Pending
                | ToolCallState::Running
                // #252：转后台任务在 resume 时必然失效（执行体随原进程消亡）。
                | ToolCallState::Backgrounded
        )
    })
}

fn project_unfinished_tool_results(messages: &mut Vec<Message>, receipts: &[ToolCallReceiptData]) {
    let unresolved: Vec<&ToolCallReceiptData> = receipts
        .iter()
        .filter(|receipt| {
            matches!(
                receipt.state,
                ToolCallState::Pending | ToolCallState::Running | ToolCallState::Backgrounded
            )
        })
        .filter(|receipt| {
            let call_id = provider_call_id(receipt);
            !messages.iter().any(|message| {
                message
                    .tool_result_ids()
                    .into_iter()
                    .any(|id| id == call_id)
            })
        })
        .collect();
    if unresolved.is_empty() {
        return;
    }

    let missing_tool_uses: Vec<ContentBlock> = unresolved
        .iter()
        .filter(|receipt| {
            let call_id = provider_call_id(receipt);
            !messages
                .iter()
                .any(|message| message.tool_use_ids().into_iter().any(|id| id == call_id))
        })
        .map(|receipt| ContentBlock::ToolUse {
            id: provider_call_id(receipt).to_string(),
            name: receipt.identity.tool_name.clone(),
            input: restored_tool_input(receipt),
        })
        .collect();
    if !missing_tool_uses.is_empty() {
        messages.push(Message {
            role: Role::Assistant,
            content: missing_tool_uses,
            metadata: None,
        });
    }

    let blocks = unresolved
        .into_iter()
        .map(|receipt| {
            let call_id = provider_call_id(receipt).to_string();
            // #252：Backgrounded 语义独立——后台任务随会话进程退出失效
            // （执行体消亡、无清理确认问题），与取消不确定（Pending/Running）区分。
            let (outcome, message, text) = if matches!(receipt.state, ToolCallState::Backgrounded) {
                (
                    "BackgroundTaskInvalidated",
                    "background task was lost with the session process;                      inspect side effects via the workspace if relevant",
                    "Background task was lost with the session process.",
                )
            } else {
                (
                    "CancellationUnconfirmed",
                    "tool execution was interrupted; cleanup could not be confirmed",
                    "Tool execution was interrupted; cleanup could not be confirmed.",
                )
            };
            ContentBlock::ToolResult {
                tool_use_id: call_id.clone(),
                content: serde_json::json!({
                    "status": "error",
                    "outcome": outcome,
                    "message": message,
                    "unfinished_call_ids": [call_id],
                    "possible_side_effects": ["tool may still have observable side effects"]
                }),
                is_error: true,
                text: Some(text.to_string()),
            }
        })
        .collect();
    messages.push(Message {
        role: Role::User,
        content: blocks,
        metadata: None,
    });
}

fn restored_tool_input(receipt: &ToolCallReceiptData) -> serde_json::Value {
    serde_json::from_str(&receipt.input_preview)
        .ok()
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}))
}

fn provider_call_id(receipt: &ToolCallReceiptData) -> &str {
    receipt
        .identity
        .provider_call_id
        .as_deref()
        .unwrap_or(&receipt.identity.runtime_call_id)
}

#[cfg(test)]
#[path = "restore_tests.rs"]
mod tests;
