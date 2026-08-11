use share::message::{ContentBlock, Message};

use super::context_read_candidate::{ContextReadCandidate, ContextReadStep};
use crate::domain::{ToolCallReceipt, ToolCallState, ToolOutcomeKind};

const EXPLORATORY_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "Glob",
    "WebFetch",
    "WebSearch",
    "LS",
    "ToolSearch",
];

pub fn microcompact_exploration(candidate: &ContextReadCandidate) -> ContextReadCandidate {
    candidate.map_unprotected_outcomes(|run_index, _, step, messages| {
        let eligible_calls =
            successful_exploration_calls(candidate.runs()[run_index].run_id(), step);
        replace_exploration_results(messages, &eligible_calls)
    })
}

struct EligibleExplorationCall {
    call_id: String,
    tool_name: String,
}

fn successful_exploration_calls(
    run_id: &str,
    step: &ContextReadStep,
) -> Vec<EligibleExplorationCall> {
    step.tool_receipts()
        .iter()
        .filter(|receipt| receipt.identity.run_id.as_ref() == run_id)
        .filter(|receipt| receipt.identity.step_id.as_str() == step.step_id())
        .filter(|receipt| EXPLORATORY_TOOLS.contains(&receipt.identity.tool_name.as_str()))
        .filter(|receipt| is_success(receipt))
        .map(|receipt| EligibleExplorationCall {
            call_id: receipt_call_id(receipt),
            tool_name: receipt.identity.tool_name.clone(),
        })
        .collect()
}

fn receipt_call_id(receipt: &ToolCallReceipt) -> String {
    receipt
        .identity
        .provider_call_id
        .as_deref()
        .unwrap_or(&receipt.identity.runtime_call_id)
        .to_string()
}

fn is_success(receipt: &ToolCallReceipt) -> bool {
    matches!(
        &receipt.state,
        ToolCallState::Terminal(terminal) if terminal.outcome == ToolOutcomeKind::Success
    )
}

fn replace_exploration_results(
    messages: &[Message],
    eligible_calls: &[EligibleExplorationCall],
) -> Option<Vec<Message>> {
    let paired_calls = messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, .. } => eligible_calls
                .iter()
                .find(|call| call.call_id == *id && call.tool_name == *name),
            _ => None,
        })
        .collect::<Vec<_>>();
    if paired_calls.is_empty() {
        return None;
    }
    let mut changed = false;
    let messages = messages
        .iter()
        .map(|message| {
            let mut message = message.clone();
            message.content = message
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                        ..
                    } if !is_error => {
                        let Some(call) = paired_calls
                            .iter()
                            .find(|call| call.call_id == *tool_use_id)
                        else {
                            return block.clone();
                        };
                        if is_already_compacted(content) {
                            return block.clone();
                        }
                        changed = true;
                        ContentBlock::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: serde_json::json!({
                                "aemeath_context": {
                                    "kind": "microcompacted_exploration",
                                    "tool": call.tool_name,
                                }
                            }),
                            is_error: false,
                            text: Some(format!("[Microcompacted tool result: {}]", call.tool_name)),
                        }
                    }
                    _ => block.clone(),
                })
                .collect();
            message
        })
        .collect();
    changed.then_some(messages)
}

fn is_already_compacted(content: &serde_json::Value) -> bool {
    content
        .get("aemeath_context")
        .and_then(|metadata| metadata.get("kind"))
        .and_then(serde_json::Value::as_str)
        .is_some()
}
