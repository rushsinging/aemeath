//! Workflow 工具族 TUI 显示：六工具专属 header + typed result 解析渲染
//! （Create·Update·NodeUpdate·Replan 状态结论行 / List 逐行摘要 / Query 节点表），
//! 替代 fallback 的 JSON 原文截断。关键状态行置于末行（tail 截断保结论）。

use super::super::common::{truncate_ellipsis, typed_from_content};
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use serde::Deserialize;
use std::path::Path;

fn parse_input<T: serde::de::DeserializeOwned>(input: &serde_json::Value) -> Option<T> {
    serde_json::from_value(input.clone()).ok()
}

fn family_policy(max_lines: usize) -> ToolRenderPolicy {
    ToolRenderPolicy {
        header: HeaderPolicy::Compact,
        details: DetailsPolicy::Hidden,
        result: ResultPolicy::Visible {
            max_lines: Some(max_lines),
            render_kind: ResultRender::Plain,
            tail_mode: true,
        },
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn mode_suffix(active_mode: &Option<String>, pending_mode: &Option<String>) -> String {
    let mut parts = Vec::new();
    if let Some(mode) = active_mode {
        parts.push(mode.clone());
    }
    if let Some(mode) = pending_mode {
        parts.push(format!("pending={mode}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

// ── 轻量协议投影（仅显示所需字段；TUI 不依赖 tools crate） ──────────

#[derive(Deserialize)]
struct CreateInputJson {
    name: String,
}

#[derive(Deserialize)]
struct ListInputJson {
    status: Option<String>,
}

#[derive(Deserialize)]
struct IdInputJson {
    #[allow(dead_code)]
    workflow_id: String,
}

#[derive(Deserialize)]
struct UpdateInputJson {
    workflow_id: String,
    action: String,
}

#[derive(Deserialize)]
struct NodeUpdateInputJson {
    node_id: String,
    action: String,
}

#[derive(Deserialize)]
struct CreateResultJson {
    workflow_id: String,
    status: String,
    active_mode: Option<String>,
    node_count: usize,
}

#[derive(Deserialize)]
struct NodeSummaryJson {
    pending: usize,
    active: usize,
    completed: usize,
    failed: usize,
    stopped: usize,
}

#[derive(Deserialize)]
struct SummaryJson {
    workflow_id: String,
    name: String,
    status: String,
    active_mode: Option<String>,
    pending_mode: Option<String>,
    node_summary: NodeSummaryJson,
}

#[derive(Deserialize)]
struct ListResultJson {
    workflows: Vec<SummaryJson>,
    total: usize,
}

#[derive(Deserialize)]
struct NodeViewJson {
    node_id: String,
    status: String,
    kind: Option<String>,
    conclusion: Option<String>,
}

#[derive(Deserialize)]
struct QueryResultJson {
    status: String,
    active_mode: Option<String>,
    nodes: Vec<NodeViewJson>,
    allowed_actions: Vec<String>,
}

#[derive(Deserialize)]
struct UpdateResultJson {
    status: String,
    active_mode: Option<String>,
    pending_mode: Option<String>,
}

#[derive(Deserialize)]
struct NodeUpdateResultJson {
    node_id: String,
    node_status: String,
    status: String,
    active_mode: Option<String>,
    activated_node_ids: Vec<String>,
    stopped_node_ids: Vec<String>,
}

#[derive(Deserialize)]
struct ReplanResultJson {
    status: String,
    active_mode: Option<String>,
    activated_node_ids: Vec<String>,
    applied_node_ids: Vec<String>,
    retired_pending_node_ids: Vec<String>,
}

// ── WorkflowCreate ──────────────────────────────────────────────────

struct WorkflowCreateDisplay;
impl ToolDisplay for WorkflowCreateDisplay {
    fn name(&self) -> &str {
        "workflow_create"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let preview = parse_input::<CreateInputJson>(input)
            .map(|args| truncate_ellipsis(&args.name, 40))
            .unwrap_or_default();
        if preview.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {preview}", self.display_name())
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: CreateResultJson = typed_from_content(content)?;
        Some(vec![
            format!("workflow {}", short_id(&result.workflow_id)),
            format!("{} nodes", result.node_count),
            // 关键结论行置于末尾：tail 截断时保留状态。
            format!(
                "{}{}",
                result.status,
                mode_suffix(&result.active_mode, &None)
            ),
        ])
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(6)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "workflow_create",
    display: || Box::new(WorkflowCreateDisplay)
});

// ── WorkflowList ────────────────────────────────────────────────────

struct WorkflowListDisplay;
impl ToolDisplay for WorkflowListDisplay {
    fn name(&self) -> &str {
        "workflow_list"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let filter = parse_input::<ListInputJson>(input)
            .and_then(|args| args.status)
            .unwrap_or_default();
        if filter.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} [{filter}]", self.display_name())
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: ListResultJson = typed_from_content(content)?;
        let mut lines = Vec::new();
        for workflow in result.workflows.iter().take(10) {
            let mode = mode_suffix(&workflow.active_mode, &workflow.pending_mode);
            lines.push(format!(
                "{} {}{} · {} — {}a/{}p/{}c/{}f/{}s",
                workflow.status,
                short_id(&workflow.workflow_id),
                mode,
                truncate_ellipsis(&workflow.name, 24),
                workflow.node_summary.active,
                workflow.node_summary.pending,
                workflow.node_summary.completed,
                workflow.node_summary.failed,
                workflow.node_summary.stopped,
            ));
        }
        // 关键结论行置于末尾。
        lines.push(format!("total {}", result.total));
        Some(lines)
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(12)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "workflow_list",
    display: || Box::new(WorkflowListDisplay)
});

// ── WorkflowQuery ───────────────────────────────────────────────────

struct WorkflowQueryDisplay;
impl ToolDisplay for WorkflowQueryDisplay {
    fn name(&self) -> &str {
        "workflow_query"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let id = parse_input::<IdInputJson>(input)
            .map(|args| short_id(&args.workflow_id))
            .unwrap_or_default();
        if id.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {id}", self.display_name())
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: QueryResultJson = typed_from_content(content)?;
        let mut lines = Vec::new();
        for node in result.nodes.iter().take(15) {
            let kind = node.kind.as_deref().unwrap_or("");
            let conclusion = node
                .conclusion
                .as_deref()
                .map(|text| truncate_ellipsis(text, 32))
                .unwrap_or_default();
            let kind_part = if kind.is_empty() {
                String::new()
            } else {
                format!("/{kind}")
            };
            let conclusion_part = if conclusion.is_empty() {
                String::new()
            } else {
                format!(" — {conclusion}")
            };
            lines.push(format!(
                "{} {}{}{}",
                node.status, node.node_id, kind_part, conclusion_part
            ));
        }
        if !result.allowed_actions.is_empty() {
            lines.push(format!("allowed: {}", result.allowed_actions.join(", ")));
        }
        // 关键结论行置于末尾。
        lines.push(format!(
            "{}{}",
            result.status,
            mode_suffix(&result.active_mode, &None)
        ));
        Some(lines)
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(20)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "workflow_query",
    display: || Box::new(WorkflowQueryDisplay)
});

// ── WorkflowUpdate ──────────────────────────────────────────────────

struct WorkflowUpdateDisplay;
impl ToolDisplay for WorkflowUpdateDisplay {
    fn name(&self) -> &str {
        "workflow_update"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        match parse_input::<UpdateInputJson>(input) {
            Some(args) => format!(
                "{} {} {}",
                self.display_name(),
                args.action,
                short_id(&args.workflow_id)
            ),
            None => self.display_name().to_string(),
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: UpdateResultJson = typed_from_content(content)?;
        Some(vec![format!(
            "{}{}",
            result.status,
            mode_suffix(&result.active_mode, &result.pending_mode)
        )])
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(4)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "workflow_update",
    display: || Box::new(WorkflowUpdateDisplay)
});

// ── WorkflowNodeUpdate ──────────────────────────────────────────────

struct WorkflowNodeUpdateDisplay;
impl ToolDisplay for WorkflowNodeUpdateDisplay {
    fn name(&self) -> &str {
        "workflow_node_update"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        match parse_input::<NodeUpdateInputJson>(input) {
            Some(args) => format!("{} {} {}", self.display_name(), args.action, args.node_id),
            None => self.display_name().to_string(),
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: NodeUpdateResultJson = typed_from_content(content)?;
        let mut lines = vec![format!("{} is {}", result.node_id, result.node_status)];
        if !result.activated_node_ids.is_empty() {
            lines.push(format!(
                "activated: {}",
                result.activated_node_ids.join(", ")
            ));
        }
        if !result.stopped_node_ids.is_empty() {
            lines.push(format!("stopped: {}", result.stopped_node_ids.join(", ")));
        }
        // 关键结论行置于末尾：Replanning 转换必须在截断后可见。
        lines.push(format!(
            "{}{}",
            result.status,
            mode_suffix(&result.active_mode, &None)
        ));
        Some(lines)
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(8)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "workflow_node_update",
    display: || Box::new(WorkflowNodeUpdateDisplay)
});

// ── WorkflowReplan ──────────────────────────────────────────────────

struct WorkflowReplanDisplay;
impl ToolDisplay for WorkflowReplanDisplay {
    fn name(&self) -> &str {
        "workflow_replan"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let id = parse_input::<IdInputJson>(input)
            .map(|args| short_id(&args.workflow_id))
            .unwrap_or_default();
        if id.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {id}", self.display_name())
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: ReplanResultJson = typed_from_content(content)?;
        let mut lines = Vec::new();
        if !result.activated_node_ids.is_empty() {
            lines.push(format!(
                "activated: {}",
                result.activated_node_ids.join(", ")
            ));
        }
        if !result.applied_node_ids.is_empty() {
            lines.push(format!("applied: {}", result.applied_node_ids.join(", ")));
        }
        if !result.retired_pending_node_ids.is_empty() {
            lines.push(format!(
                "retired: {}",
                result.retired_pending_node_ids.join(", ")
            ));
        }
        // 关键结论行置于末尾。
        lines.push(format!(
            "{}{}",
            result.status,
            mode_suffix(&result.active_mode, &None)
        ));
        Some(lines)
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(8)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "workflow_replan",
    display: || Box::new(WorkflowReplanDisplay)
});
