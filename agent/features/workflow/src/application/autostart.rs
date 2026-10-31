//! Workflow 自动启动决策（KEV / System One 评分接入的领域侧骨架）。
//!
//! 职责边界：本模块只做两件事——
//! 1. `autostart_plan_graph`：构造初始 Graph（第一个节点固定为 `plan`，
//!    Planner 角色，后继 CompleteNode；后续节点由 LLM 经 `workflow_replan`
//!    的 pending_rewrites / continuation 自行补齐）；
//! 2. `should_autostart`：纯决策函数——无 Active Workflow、分数达阈值、
//!    未超时才放行。
//!
//! 评分本身（KEV `ScoringPort`）、turn 边界触发与 SystemMessage 注入归
//! Runtime / Composition 装配；daemon 不可用一律 fail-open（不启动）。

use crate::domain::tool_data::CreateWorkflowSpec;
use crate::domain::{EdgeDefinition, NodeDefinition, NodeKind, Role};

use crate::constants::{AUTOSTART_COMPLETE_NODE_ID, AUTOSTART_PLAN_NODE_ID};

/// 自动启动初始图：`plan (work/Planner) → done (complete)`。
///
/// 第一个节点以 plan 开头是用户拍板口径：评分判定任务复杂后，Runtime 直接
/// 建出最小可推进骨架，节点级规划工作由 LLM 在 plan 节点内完成并经
/// `workflow_replan` 扩展，Runtime NEVER 代 LLM 补齐业务节点。
pub fn autostart_plan_graph(objective: &str) -> CreateWorkflowSpec {
    CreateWorkflowSpec {
        name: "auto".to_owned(),
        objective: objective.to_owned(),
        root_node_id: AUTOSTART_PLAN_NODE_ID.to_owned(),
        nodes: vec![
            NodeDefinition {
                node_id: AUTOSTART_PLAN_NODE_ID.to_owned(),
                kind: NodeKind::Work,
                role: Some(Role::Planner),
                description: "规划整体方案；经 workflow_replan 补齐后续节点".to_owned(),
                completion_criteria: None,
            },
            NodeDefinition {
                node_id: AUTOSTART_COMPLETE_NODE_ID.to_owned(),
                kind: NodeKind::Complete,
                role: None,
                description: "最终交付收口".to_owned(),
                completion_criteria: None,
            },
        ],
        edges: vec![EdgeDefinition {
            from_node_id: AUTOSTART_PLAN_NODE_ID.to_owned(),
            to_node_id: AUTOSTART_COMPLETE_NODE_ID.to_owned(),
        }],
    }
}

/// 自动启动决策输入。
#[derive(Debug, Clone, PartialEq)]
pub struct AutostartEvaluation {
    /// KEV 评分（`ScoringPort` 单题 Score 结果）；无评分（daemon 不可用/超时）为 `None`。
    pub score: Option<f64>,
    /// 配置阈值。
    pub threshold: f64,
    /// 当前 Session 是否已有 Active Workflow（含 Replanning）。
    pub has_active_workflow: bool,
}

/// 是否应自动启动 Workflow。
///
/// - 无评分 → `false`（fail-open：静默不启动，NEVER 猜测）；
/// - 已有 Active Workflow → `false`（已在模式内，不重复触发）；
/// - 分数 < 阈值 → `false`（LLM 手动 `workflow_create` 永远允许，评分只增不锁）。
pub fn should_autostart(evaluation: &AutostartEvaluation) -> bool {
    match evaluation.score {
        Some(score) => score >= evaluation.threshold && !evaluation.has_active_workflow,
        None => false,
    }
}
