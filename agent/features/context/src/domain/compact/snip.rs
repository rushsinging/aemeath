use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use share::message::{ContentBlock, Message};

use super::context_read_candidate::{ContextReadCandidate, ContextReadStep};
use crate::domain::{ToolCallReceipt, ToolCallState, ToolOutcomeKind};

const SNIPPABLE_TOOLS: &[&str] = &["Read", "Grep", "Glob"];
const WRITE_TOOLS: &[&str] = &["Edit", "Write"];

pub fn snip_superseded_exploration(candidate: &ContextReadCandidate) -> ContextReadCandidate {
    let successful_writes = successful_writes(candidate);
    candidate.map_unprotected_outcomes(|run_index, step_index, step, messages| {
        let eligible_calls = snippable_calls(
            candidate.runs()[run_index].run_id(),
            run_index,
            step_index,
            step,
            messages,
            &successful_writes,
        );
        replace_snippable_results(messages, &eligible_calls)
    })
}

#[derive(Debug)]
struct LocatedToolCall {
    call_id: String,
    tool_name: String,
    path: PathBuf,
}

#[derive(Debug)]
struct SuccessfulWrite {
    run_index: usize,
    step_index: usize,
    path: PathBuf,
}

fn successful_writes(candidate: &ContextReadCandidate) -> Vec<SuccessfulWrite> {
    candidate
        .runs()
        .iter()
        .enumerate()
        .flat_map(|(run_index, run)| {
            run.steps()
                .iter()
                .enumerate()
                .flat_map(move |(step_index, step)| {
                    let successful_ids = successful_call_ids(run.run_id(), step);
                    let messages = step.outcome_messages();
                    tool_calls(messages.as_ref())
                        .filter(move |call| successful_ids.iter().any(|id| id == &call.call_id))
                        .map(move |call| SuccessfulWrite {
                            run_index,
                            step_index,
                            path: call.path,
                        })
                        .collect::<Vec<_>>()
                })
        })
        .collect()
}

fn snippable_calls(
    run_id: &str,
    run_index: usize,
    step_index: usize,
    step: &ContextReadStep,
    messages: &[Message],
    writes: &[SuccessfulWrite],
) -> Vec<LocatedToolCall> {
    let successful_ids = successful_call_ids_for_tools(run_id, step, SNIPPABLE_TOOLS);
    tool_calls(messages)
        .filter(|call| successful_ids.iter().any(|id| id == &call.call_id))
        .filter(|call| {
            writes.iter().any(|write| {
                (write.run_index, write.step_index) > (run_index, step_index)
                    && paths_overlap(&call.path, &write.path, &call.tool_name)
            })
        })
        .collect()
}

fn successful_call_ids(run_id: &str, step: &ContextReadStep) -> Vec<String> {
    step.tool_receipts()
        .iter()
        .filter(|receipt| receipt.identity.run_id.as_ref() == run_id)
        .filter(|receipt| receipt.identity.step_id.as_str() == step.step_id())
        .filter(|receipt| WRITE_TOOLS.contains(&receipt.identity.tool_name.as_str()))
        .filter(|receipt| is_success(receipt))
        .map(receipt_call_id)
        .collect()
}

fn successful_call_ids_for_tools(
    run_id: &str,
    step: &ContextReadStep,
    tools: &[&str],
) -> Vec<String> {
    step.tool_receipts()
        .iter()
        .filter(|receipt| receipt.identity.run_id.as_ref() == run_id)
        .filter(|receipt| receipt.identity.step_id.as_str() == step.step_id())
        .filter(|receipt| tools.contains(&receipt.identity.tool_name.as_str()))
        .filter(|receipt| is_success(receipt))
        .map(receipt_call_id)
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

fn tool_calls(messages: &[Message]) -> impl Iterator<Item = LocatedToolCall> + '_ {
    messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => Some(LocatedToolCall {
                call_id: id.clone(),
                tool_name: name.clone(),
                path: tool_input_path(input)?,
            }),
            _ => None,
        })
}

fn replace_snippable_results(
    messages: &[Message],
    eligible_calls: &[LocatedToolCall],
) -> Option<Vec<Message>> {
    if eligible_calls.is_empty() {
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
                        is_error,
                        ..
                    } if !is_error => {
                        let Some(call) = eligible_calls
                            .iter()
                            .find(|call| call.call_id == *tool_use_id)
                        else {
                            return block.clone();
                        };
                        changed = true;
                        ContentBlock::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: serde_json::json!({
                                "aemeath_context": {
                                    "kind": "superseded_exploration",
                                    "path": call.path.to_string_lossy(),
                                    "tool": call.tool_name,
                                }
                            }),
                            is_error: false,
                            text: Some(format!(
                                "[Superseded tool result: {} {}]",
                                call.tool_name,
                                call.path.display()
                            )),
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

fn is_success(receipt: &ToolCallReceipt) -> bool {
    matches!(
        &receipt.state,
        ToolCallState::Terminal(terminal) if terminal.outcome == ToolOutcomeKind::Success
    )
}

fn tool_input_path(input: &Value) -> Option<PathBuf> {
    let path = input
        .get("file_path")
        .or_else(|| input.get("path"))?
        .as_str()?;
    Some(normalize_path(path))
}

fn normalize_path(path: &str) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn paths_overlap(exploration: &Path, write: &Path, tool_name: &str) -> bool {
    match tool_name {
        "Read" => exploration == write,
        "Grep" | "Glob" => write.starts_with(exploration),
        _ => false,
    }
}
