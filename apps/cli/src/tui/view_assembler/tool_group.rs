use crate::tui::model::output_timeline::OutputTimelineItem;
use crate::tui::view_assembler::output_tool_lookup::ToolCallLookup;
use crate::tui::view_model::output::ToolGroupKind;

const MAX_TOOL_CALLS_PER_GROUP: usize = 20;

pub(crate) fn classify_tool_name(tool_name: &str) -> Option<ToolGroupKind> {
    match tool_name {
        "Read" | "Glob" | "Grep" => Some(ToolGroupKind::Explore),
        "Bash" => Some(ToolGroupKind::Run),
        "Write" | "Edit" => Some(ToolGroupKind::Write),
        "TaskCreate" | "TaskUpdate" | "TaskBlockBy" | "TaskListGet" | "TaskLists"
        | "TaskListCreate" | "TaskListComplete" | "TaskGet" | "TaskStop" => {
            Some(ToolGroupKind::Tasks)
        }
        _ => None,
    }
}

pub(super) fn timeline_candidate(
    item: &OutputTimelineItem,
    tool_lookup: &impl ToolCallLookup,
) -> ToolGroupCandidate {
    match item {
        OutputTimelineItem::ToolCall { reference } => {
            let tool_kind = tool_lookup
                .call(
                    &reference.context.chat_id,
                    &reference.context.run_id,
                    &reference.tool_call_id,
                )
                .and_then(|call| classify_tool_name(&call.name));
            ToolGroupCandidate {
                item_id: item.id().into_owned(),
                call_id: Some(reference.tool_call_id.as_ref().to_string()),
                tool_kind,
                step_id: reference.context.run_id.as_ref().to_string(),
                result_call_id: None,
            }
        }
        OutputTimelineItem::ToolResult { reference } => ToolGroupCandidate {
            item_id: item.id().into_owned(),
            call_id: None,
            tool_kind: None,
            step_id: reference.context.run_id.as_ref().to_string(),
            result_call_id: Some(reference.tool_call_id.as_ref().to_string()),
        },
        _ => ToolGroupCandidate {
            item_id: item.id().into_owned(),
            call_id: None,
            tool_kind: None,
            step_id: timeline_step_id(item),
            result_call_id: None,
        },
    }
}

fn timeline_step_id(item: &OutputTimelineItem) -> String {
    match item {
        OutputTimelineItem::AssistantText { context, .. }
        | OutputTimelineItem::Thinking { context, .. } => context
            .as_ref()
            .map(|context| context.run_id.as_ref().to_string())
            .unwrap_or_else(|| "unscoped".to_string()),
        _ => "unscoped".to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolGroupCandidate {
    pub(crate) item_id: String,
    pub(crate) call_id: Option<String>,
    pub(crate) tool_kind: Option<ToolGroupKind>,
    pub(crate) step_id: String,
    pub(crate) result_call_id: Option<String>,
}

impl ToolGroupCandidate {
    #[cfg(test)]
    pub(crate) fn tool_call(item_id: &str, call_id: &str, tool_name: &str, step_id: &str) -> Self {
        Self {
            item_id: item_id.to_string(),
            call_id: Some(call_id.to_string()),
            tool_kind: classify_tool_name(tool_name),
            step_id: step_id.to_string(),
            result_call_id: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn boundary(item_id: &str, step_id: &str) -> Self {
        Self {
            item_id: item_id.to_string(),
            call_id: None,
            tool_kind: None,
            step_id: step_id.to_string(),
            result_call_id: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn result(item_id: &str, call_id: &str, step_id: &str) -> Self {
        Self {
            item_id: item_id.to_string(),
            call_id: None,
            tool_kind: None,
            step_id: step_id.to_string(),
            result_call_id: Some(call_id.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttachedToolResult {
    pub(crate) item_id: String,
    pub(crate) call_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DisplayUnitPlan {
    Single {
        item_id: String,
        attached_results: Vec<AttachedToolResult>,
    },
    ToolGroup {
        group_id: String,
        kind: ToolGroupKind,
        member_ids: Vec<String>,
        attached_results: Vec<AttachedToolResult>,
    },
}

impl DisplayUnitPlan {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Single { item_id, .. } => item_id,
            Self::ToolGroup { group_id, .. } => group_id,
        }
    }

    pub(crate) fn source_item_ids(&self) -> impl Iterator<Item = &str> {
        let ids = match self {
            Self::Single {
                item_id,
                attached_results,
            } => std::iter::once(item_id.as_str())
                .chain(
                    attached_results
                        .iter()
                        .map(|result| result.item_id.as_str()),
                )
                .collect::<Vec<_>>(),
            Self::ToolGroup {
                member_ids,
                attached_results,
                ..
            } => member_ids
                .iter()
                .map(String::as_str)
                .chain(
                    attached_results
                        .iter()
                        .map(|result| result.item_id.as_str()),
                )
                .collect(),
        };
        ids.into_iter()
    }
}

pub(crate) fn plan_display_units(candidates: &[ToolGroupCandidate]) -> Vec<DisplayUnitPlan> {
    let mut display_units = Vec::new();
    let mut pending_calls: Vec<&ToolGroupCandidate> = Vec::new();
    let mut pending_results = Vec::new();

    for candidate in candidates {
        if let Some(result_call_id) = candidate.result_call_id.as_deref() {
            if pending_calls
                .iter()
                .any(|call| call.call_id.as_deref() == Some(result_call_id))
            {
                pending_results.push(AttachedToolResult {
                    item_id: candidate.item_id.clone(),
                    call_id: result_call_id.to_string(),
                });
            } else {
                flush_pending_calls(&mut display_units, &mut pending_calls, &mut pending_results);
                display_units.push(DisplayUnitPlan::Single {
                    item_id: candidate.item_id.clone(),
                    attached_results: Vec::new(),
                });
            }
            continue;
        }

        let Some(tool_kind) = candidate.tool_kind else {
            flush_pending_calls(&mut display_units, &mut pending_calls, &mut pending_results);
            display_units.push(DisplayUnitPlan::Single {
                item_id: candidate.item_id.clone(),
                attached_results: Vec::new(),
            });
            continue;
        };

        let can_extend_pending = pending_calls.last().is_some_and(|last_call| {
            last_call.step_id == candidate.step_id
                && last_call.tool_kind == Some(tool_kind)
                && candidate.call_id.is_some()
        });
        if !can_extend_pending || pending_calls.len() == MAX_TOOL_CALLS_PER_GROUP {
            flush_pending_calls(&mut display_units, &mut pending_calls, &mut pending_results);
        }
        pending_calls.push(candidate);
    }

    flush_pending_calls(&mut display_units, &mut pending_calls, &mut pending_results);
    display_units
}

fn flush_pending_calls(
    display_units: &mut Vec<DisplayUnitPlan>,
    pending_calls: &mut Vec<&ToolGroupCandidate>,
    pending_results: &mut Vec<AttachedToolResult>,
) {
    if pending_calls.is_empty() {
        pending_results.clear();
        return;
    }

    if pending_calls.len() < 2 {
        let call = pending_calls[0];
        display_units.push(DisplayUnitPlan::Single {
            item_id: call.item_id.clone(),
            attached_results: std::mem::take(pending_results),
        });
    } else {
        let first_call = pending_calls[0];
        let kind = first_call
            .tool_kind
            .expect("pending calls always have a classified tool kind");
        let group_id = format!(
            "tool-group:{}:{}",
            first_call.step_id,
            first_call
                .call_id
                .as_deref()
                .expect("pending calls always have a stable call ID")
        );
        let member_ids = pending_calls
            .iter()
            .map(|call| {
                call.call_id
                    .as_ref()
                    .expect("pending calls always have a stable call ID")
                    .clone()
            })
            .collect();
        display_units.push(DisplayUnitPlan::ToolGroup {
            group_id,
            kind,
            member_ids,
            attached_results: std::mem::take(pending_results),
        });
    }
    pending_calls.clear();
}

#[cfg(test)]
#[path = "tool_group_tests.rs"]
mod tests;
