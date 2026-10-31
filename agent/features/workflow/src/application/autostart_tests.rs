//! 自动启动决策测试：plan 开头初始图 + fail-open 决策矩阵。

use super::autostart::{autostart_plan_graph, should_autostart, AutostartEvaluation};
use crate::domain::tool_data::CreateWorkflowSpec;
use crate::domain::{NodeKind, Role, WorkflowDefinition};
use crate::Kernel;

fn evaluation(score: Option<f64>, threshold: f64, active: bool) -> AutostartEvaluation {
    AutostartEvaluation {
        score,
        threshold,
        has_active_workflow: active,
    }
}

#[test]
fn autostart_graph_is_minimal_and_plan_rooted() {
    let spec: CreateWorkflowSpec = autostart_plan_graph("目标");
    let definition = WorkflowDefinition {
        name: spec.name.clone(),
        objective: spec.objective.clone(),
        root_node_id: spec.root_node_id.clone(),
        nodes: spec.nodes.clone(),
        edges: spec.edges.clone(),
    };
    // Kernel 全量校验通过（最小合法 DAG）。
    Kernel::validate(&definition).expect("初始图必须过 Kernel 校验");
    assert_eq!(spec.root_node_id, "plan");
    let root = spec.nodes.iter().find(|n| n.node_id == "plan").unwrap();
    assert_eq!(root.kind, NodeKind::Work);
    assert_eq!(root.role, Some(Role::Planner));
    assert_eq!(
        spec.nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Complete)
            .count(),
        1
    );
}

#[test]
fn decision_matrix() {
    // 达阈值且无 Active → 启动。
    assert!(should_autostart(&evaluation(Some(0.7), 0.6, false)));
    // 边界：等于阈值 → 启动。
    assert!(should_autostart(&evaluation(Some(0.6), 0.6, false)));
    // 低于阈值 → 不启动。
    assert!(!should_autostart(&evaluation(Some(0.59), 0.6, false)));
    // 已有 Active → 不启动（不重复触发）。
    assert!(!should_autostart(&evaluation(Some(0.9), 0.6, true)));
    // 无评分（daemon 不可用/超时）→ fail-open 不启动。
    assert!(!should_autostart(&evaluation(None, 0.6, false)));
}
