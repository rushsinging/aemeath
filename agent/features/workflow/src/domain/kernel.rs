//! 确定性 Kernel：Graph 结构不变量校验（设计文档 §2 Kernel 1–10、§11.2 A）。
//!
//! 校验为纯函数，按固定优先级报告首个违规；MVP 一次只返回一个
//! `GraphInvariantViolation`（附细分 reason 与违规对象），不做错误聚合。

use std::collections::{HashMap, HashSet, VecDeque};

use crate::LOG_TARGET;

use super::{GraphError, GraphInvariantReason, NodeKind, WorkflowDefinition};

/// Kernel 校验入口。
pub struct Kernel;

impl Kernel {
    /// 校验完整 WorkflowDefinition；纯函数，无副作用。
    ///
    /// 优先级（固定，测试依赖此顺序断言具体 reason）：
    /// 1. Edge 端点与 root 引用存在（MissingNode）
    /// 2. 自环（SelfLoop）
    /// 3. 环（CycleDetected）
    /// 4. 唯一 CompleteNode（Missing / Multiple）
    /// 5. CompleteNode 无后继（CompleteNodeHasSuccessor）
    /// 6. 唯一 root（MultipleRoots）
    /// 7. root 是普通业务 Node（RootIsSpecialKind）
    /// 8. Role 约束（WorkNodeMissingRole / RoleOnSpecialKind）
    /// 9. 从 root 可达全部 Node（UnreachableNode）
    /// 10. 非 CompleteNode 有后继（NodeWithoutSuccessor）
    /// 11. 非 CompleteNode 可达 CompleteNode（NoPathToCompleteNode）
    pub fn validate(definition: &WorkflowDefinition) -> Result<(), GraphError> {
        if let Err(error) = Self::validate_inner(definition) {
            log::debug!(
                target: LOG_TARGET,
                "workflow_kernel_rejected reason={:?} subject={:?} nodes={} edges={}",
                error.reason,
                error.subject,
                definition.nodes.len(),
                definition.edges.len()
            );
            return Err(error);
        }
        Ok(())
    }

    fn validate_inner(definition: &WorkflowDefinition) -> Result<(), GraphError> {
        Self::check_references(definition)?;
        Self::check_self_loops(definition)?;
        Self::check_cycles(definition)?;
        Self::check_complete_nodes(definition)?;
        Self::check_single_root(definition)?;
        Self::check_root_kind(definition)?;
        Self::check_roles(definition)?;
        Self::check_reachable(definition)?;
        Self::check_successors(definition)?;
        Self::check_path_to_complete(definition)
    }

    /// Kernel 7：Edge 端点与 root 引用必须指向已定义 Node。
    fn check_references(definition: &WorkflowDefinition) -> Result<(), GraphError> {
        let known: HashSet<&str> = definition
            .nodes
            .iter()
            .map(|node| node.node_id.as_str())
            .collect();
        if !known.contains(definition.root_node_id.as_str()) {
            return Err(GraphError {
                reason: GraphInvariantReason::MissingNode,
                subject: Some(definition.root_node_id.clone()),
            });
        }
        for edge in &definition.edges {
            if !known.contains(edge.from_node_id.as_str()) {
                return Err(GraphError {
                    reason: GraphInvariantReason::MissingNode,
                    subject: Some(edge.from_node_id.clone()),
                });
            }
            if !known.contains(edge.to_node_id.as_str()) {
                return Err(GraphError {
                    reason: GraphInvariantReason::MissingNode,
                    subject: Some(edge.to_node_id.clone()),
                });
            }
        }
        Ok(())
    }

    /// Kernel 6：禁止自环。
    fn check_self_loops(definition: &WorkflowDefinition) -> Result<(), GraphError> {
        for edge in &definition.edges {
            if edge.from_node_id == edge.to_node_id {
                return Err(GraphError {
                    reason: GraphInvariantReason::SelfLoop,
                    subject: Some(edge.from_node_id.clone()),
                });
            }
        }
        Ok(())
    }

    /// Kernel 8：Graph 必须是 DAG（三色 DFS 检测回边）。
    fn check_cycles(definition: &WorkflowDefinition) -> Result<(), GraphError> {
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &definition.edges {
            adjacency
                .entry(edge.from_node_id.as_str())
                .or_default()
                .push(edge.to_node_id.as_str());
        }
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            White,
            Grey,
            Black,
        }
        let marks: HashMap<&str, Mark> = definition
            .nodes
            .iter()
            .map(|node| (node.node_id.as_str(), Mark::White))
            .collect();
        let mut marks = marks;
        fn visit<'a>(
            node_id: &'a str,
            adjacency: &HashMap<&'a str, Vec<&'a str>>,
            marks: &mut HashMap<&'a str, Mark>,
        ) -> bool {
            match marks.get(node_id) {
                Some(Mark::Grey) => return false,
                Some(Mark::Black) => return true,
                _ => {}
            }
            marks.insert(node_id, Mark::Grey);
            if let Some(successors) = adjacency.get(node_id) {
                for successor in successors {
                    if !visit(successor, adjacency, marks) {
                        return false;
                    }
                }
            }
            marks.insert(node_id, Mark::Black);
            true
        }
        for node in &definition.nodes {
            if !visit(node.node_id.as_str(), &adjacency, &mut marks) {
                return Err(GraphError {
                    reason: GraphInvariantReason::CycleDetected,
                    subject: None,
                });
            }
        }
        Ok(())
    }

    /// Kernel 2：有且只有一个显式 CompleteNode，且无后继。
    fn check_complete_nodes(definition: &WorkflowDefinition) -> Result<(), GraphError> {
        let complete_ids: Vec<&str> = definition
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Complete)
            .map(|node| node.node_id.as_str())
            .collect();
        match complete_ids.len() {
            0 => Err(GraphError {
                reason: GraphInvariantReason::MissingCompleteNode,
                subject: None,
            }),
            1 => {
                if definition
                    .edges
                    .iter()
                    .any(|edge| edge.from_node_id == complete_ids[0])
                {
                    return Err(GraphError {
                        reason: GraphInvariantReason::CompleteNodeHasSuccessor,
                        subject: Some(complete_ids[0].to_owned()),
                    });
                }
                Ok(())
            }
            _ => Err(GraphError {
                reason: GraphInvariantReason::MultipleCompleteNodes,
                subject: None,
            }),
        }
    }

    /// Kernel 1：唯一 root——恰好一个无前驱 Node，且必须是 root_node_id。
    fn check_single_root(definition: &WorkflowDefinition) -> Result<(), GraphError> {
        let has_predecessor: HashSet<&str> = definition
            .edges
            .iter()
            .map(|edge| edge.to_node_id.as_str())
            .collect();
        let root_candidates: Vec<&str> = definition
            .nodes
            .iter()
            .map(|node| node.node_id.as_str())
            .filter(|node_id| !has_predecessor.contains(node_id))
            .collect();
        if root_candidates.len() != 1 || root_candidates[0] != definition.root_node_id {
            return Err(GraphError {
                reason: GraphInvariantReason::MultipleRoots,
                subject: None,
            });
        }
        Ok(())
    }

    /// Kernel 3：root 必须是普通业务 Node（WorkNode）。
    fn check_root_kind(definition: &WorkflowDefinition) -> Result<(), GraphError> {
        let root = definition
            .nodes
            .iter()
            .find(|node| node.node_id == definition.root_node_id);
        if let Some(root) = root {
            if root.kind != NodeKind::Work {
                return Err(GraphError {
                    reason: GraphInvariantReason::RootIsSpecialKind,
                    subject: Some(root.node_id.clone()),
                });
            }
        }
        Ok(())
    }

    /// Kernel 10（Role 部分）：work 必须有 Role；confirm / complete 禁止 Role。
    fn check_roles(definition: &WorkflowDefinition) -> Result<(), GraphError> {
        for node in &definition.nodes {
            match node.kind {
                NodeKind::Work => {
                    if node.role.is_none() {
                        return Err(GraphError {
                            reason: GraphInvariantReason::WorkNodeMissingRole,
                            subject: Some(node.node_id.clone()),
                        });
                    }
                }
                NodeKind::Confirm | NodeKind::Complete => {
                    if node.role.is_some() {
                        return Err(GraphError {
                            reason: GraphInvariantReason::RoleOnSpecialKind,
                            subject: Some(node.node_id.clone()),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Kernel 4：当前有效 DAG 的全部 Node 从 root 可达（BFS）。
    fn check_reachable(definition: &WorkflowDefinition) -> Result<(), GraphError> {
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &definition.edges {
            adjacency
                .entry(edge.from_node_id.as_str())
                .or_default()
                .push(edge.to_node_id.as_str());
        }
        let mut visited: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<&str> = VecDeque::new();
        queue.push_back(definition.root_node_id.as_str());
        visited.insert(definition.root_node_id.as_str());
        while let Some(current) = queue.pop_front() {
            if let Some(successors) = adjacency.get(current) {
                for successor in successors {
                    if visited.insert(successor) {
                        queue.push_back(successor);
                    }
                }
            }
        }
        for node in &definition.nodes {
            if !visited.contains(node.node_id.as_str()) {
                return Err(GraphError {
                    reason: GraphInvariantReason::UnreachableNode,
                    subject: Some(node.node_id.clone()),
                });
            }
        }
        Ok(())
    }

    /// Kernel 5：每个非 CompleteNode 至少一个后继。
    fn check_successors(definition: &WorkflowDefinition) -> Result<(), GraphError> {
        let has_successor: HashSet<&str> = definition
            .edges
            .iter()
            .map(|edge| edge.from_node_id.as_str())
            .collect();
        for node in &definition.nodes {
            if node.kind != NodeKind::Complete && !has_successor.contains(node.node_id.as_str()) {
                return Err(GraphError {
                    reason: GraphInvariantReason::NodeWithoutSuccessor,
                    subject: Some(node.node_id.clone()),
                });
            }
        }
        Ok(())
    }

    /// Kernel 5：每个非 CompleteNode 存在到达 CompleteNode 的有向路径。
    fn check_path_to_complete(definition: &WorkflowDefinition) -> Result<(), GraphError> {
        let complete_id: Option<&str> = definition
            .nodes
            .iter()
            .find(|node| node.kind == NodeKind::Complete)
            .map(|node| node.node_id.as_str());
        let Some(complete_id) = complete_id else {
            return Ok(());
        };
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &definition.edges {
            adjacency
                .entry(edge.from_node_id.as_str())
                .or_default()
                .push(edge.to_node_id.as_str());
        }
        // 逆向 BFS：从 CompleteNode 沿反向边求可到达它的节点集合。
        let mut reverse: HashMap<&str, Vec<&str>> = HashMap::new();
        for (from, successors) in &adjacency {
            for successor in successors {
                reverse.entry(successor).or_default().push(from);
            }
        }
        let mut reaches_complete: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<&str> = VecDeque::new();
        queue.push_back(complete_id);
        reaches_complete.insert(complete_id);
        while let Some(current) = queue.pop_front() {
            if let Some(predecessors) = reverse.get(current) {
                for predecessor in predecessors {
                    if reaches_complete.insert(predecessor) {
                        queue.push_back(predecessor);
                    }
                }
            }
        }
        for node in &definition.nodes {
            if node.kind != NodeKind::Complete && !reaches_complete.contains(node.node_id.as_str())
            {
                return Err(GraphError {
                    reason: GraphInvariantReason::NoPathToCompleteNode,
                    subject: Some(node.node_id.clone()),
                });
            }
        }
        Ok(())
    }
}
