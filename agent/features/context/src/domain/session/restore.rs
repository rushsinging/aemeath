//! Canonical Session 恢复投影。

use super::envelope::SessionRestoreStepRecord;
use super::message_integrity::{check_message_integrity, deep_clean_messages, sanitize_messages};
use crate::domain::session::CanonicalSession;
use crate::domain::{ToolCallReceipt, ToolCallState};
use share::message::{ContentBlock, Message, Role};

#[derive(Debug, Clone)]
pub struct SessionRestoreStep {
    pub run_id: String,
    pub step_id: String,
    pub messages: Vec<Message>,
    pub finalize_cause: Option<crate::domain::FinalizeCause>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct SessionRestore {
    pub active_messages: Vec<Message>,
    pub display_steps: Vec<SessionRestoreStep>,
    pub created_at: String,
    pub trimmed: usize,
    pub repaired: usize,
}

impl SessionRestore {
    pub fn from_canonical(session: &CanonicalSession) -> Self {
        let (active_steps, active_trimmed, active_repaired) =
            clean_steps(session.restore_steps_from_marker());
        let (display_steps, _, _) = clean_steps(session.all_restore_steps());
        let active_messages = active_steps
            .iter()
            .flat_map(|step| step.messages.iter().cloned())
            .collect();
        Self {
            active_messages,
            display_steps,
            created_at: session.created_at.clone(),
            trimmed: active_trimmed,
            repaired: active_repaired,
        }
    }
}

fn clean_steps(
    raw_steps: Vec<SessionRestoreStepRecord>,
) -> (Vec<SessionRestoreStep>, usize, usize) {
    let mut steps = Vec::with_capacity(raw_steps.len());
    let mut trimmed = 0;
    let mut repaired = 0;
    for SessionRestoreStepRecord {
        cursor,
        mut messages,
        tool_receipts,
        finalize_cause,
        duration_ms,
    } in raw_steps
    {
        let had_unfinished_receipts = has_unfinished_receipts(&tool_receipts);
        project_unfinished_tool_results(&mut messages, &tool_receipts);
        let finalize_cause = finalize_cause.or_else(|| {
            had_unfinished_receipts.then_some(crate::domain::FinalizeCause::RunTerminated)
        });
        let before = messages.len();
        sanitize_messages(&mut messages);
        trimmed += before.saturating_sub(messages.len());
        if check_message_integrity(&messages).has_issues() {
            repaired += deep_clean_messages(&mut messages);
        }
        if !messages.is_empty() {
            steps.push(SessionRestoreStep {
                run_id: cursor.run_id,
                step_id: cursor.step_id,
                messages,
                finalize_cause,
                duration_ms,
            });
        }
    }
    (steps, trimmed, repaired)
}

fn has_unfinished_receipts(receipts: &[ToolCallReceipt]) -> bool {
    receipts.iter().any(|receipt| {
        matches!(
            receipt.state,
            ToolCallState::Pending | ToolCallState::Running
        )
    })
}

fn project_unfinished_tool_results(messages: &mut Vec<Message>, receipts: &[ToolCallReceipt]) {
    let unresolved: Vec<&ToolCallReceipt> = receipts
        .iter()
        .filter(|receipt| {
            matches!(
                receipt.state,
                ToolCallState::Pending | ToolCallState::Running
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
            ContentBlock::ToolResult {
                tool_use_id: call_id.clone(),
                content: serde_json::json!({
                    "status": "error",
                    "outcome": "CancellationUnconfirmed",
                    "message": "tool execution was interrupted; cleanup could not be confirmed",
                    "unfinished_call_ids": [call_id],
                    "possible_side_effects": ["tool may still have observable side effects"]
                }),
                is_error: true,
                text: Some(
                    "Tool execution was interrupted; cleanup could not be confirmed.".to_string(),
                ),
            }
        })
        .collect();
    messages.push(Message {
        role: Role::User,
        content: blocks,
        metadata: None,
    });
}

fn restored_tool_input(receipt: &ToolCallReceipt) -> serde_json::Value {
    serde_json::from_str(&receipt.input_preview)
        .ok()
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}))
}

fn provider_call_id(receipt: &ToolCallReceipt) -> &str {
    receipt
        .identity
        .provider_call_id
        .as_deref()
        .unwrap_or(&receipt.identity.runtime_call_id)
}

#[cfg(test)]
#[path = "restore_tests.rs"]
mod tests;
