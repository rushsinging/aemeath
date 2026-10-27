//! 六个 Workflow Tool 的 description（设计文档 §5）。

/// `workflow_create` description。
pub fn workflow_create(lang: &str) -> &'static str {
    match lang {
        "zh" => "一次创建 Workflow Graph：提交完整 DAG（名称、目标、root 节点、节点集合、边集合）。成功后 Workflow 变为 Active，root 成为唯一初始 Active 节点。当前会话已有 Active Workflow 时会被拒绝，需先 deactivate。",
        _ => "Create a Workflow Graph in one call: submit the complete DAG (name, objective, root node, nodes, edges). On success the workflow becomes Active and the root node becomes the only initial Active node. Rejected when this session already has an Active workflow; deactivate it first.",
    }
}

/// `workflow_list` description。
pub fn workflow_list(lang: &str) -> &'static str {
    match lang {
        "zh" => "列出当前会话可见的 Workflow 摘要，可按状态过滤并分页（limit 默认 20、上限 100）。",
        _ => "List Workflow summaries visible to this session, optionally filtered by status and paginated (limit default 20, max 100).",
    }
}

/// `workflow_query` description。
pub fn workflow_query(lang: &str) -> &'static str {
    match lang {
        "zh" => "读取单个 Workflow 详情：Graph 结构、节点状态与结论、证据引用、可选的执行事实摘要，以及当前状态的 allowed_actions 提示。只读。",
        _ => "Read one Workflow in detail: Graph structure, node statuses and conclusions, evidence references, optional execution fact summaries, and the allowed_actions hint for the current state. Read-only.",
    }
}

/// `workflow_update` description。
pub fn workflow_update(lang: &str) -> &'static str {
    match lang {
        "zh" => "用单一 action 管理 Workflow 生命周期：activate（Inactive → Active，按 pending_mode 恢复模式）、deactivate（Active → Inactive，保留 pending_mode；用户被打断时用它挂起）、complete（仅 Active/Executing；需交付摘要且 CompleteNode 已完成）、cancel（需理由）、fail（仅 Active/Replanning；需失败原因）。",
        _ => "Manage the Workflow lifecycle with one action: activate (Inactive -> Active, restoring the pending mode), deactivate (Active -> Inactive, keeping the pending mode; use it to suspend when the user is interrupted), complete (Active/Executing only; requires a delivery summary and a completed CompleteNode), cancel (requires a reason), or fail (Active/Replanning only; requires a failure reason).",
    }
}

/// `workflow_node_update` description。
pub fn workflow_node_update(lang: &str) -> &'static str {
    match lang {
        "zh" => "更新单个 Active 节点：complete（提交结论、证据引用与一个或多个后继决定；后继必须是静态 Edge 的直接指向且 description 必填）或 fail（提交失败结论；随后整个 Workflow 进入 Replanning，其他并行 Active 节点转为 Stopped）。",
        _ => "Update one Active node: complete (submit conclusion, evidence references and one or more successors; successors must be direct edge targets and their description is required) or fail (submit failure conclusion; the whole workflow then switches to Replanning and other parallel Active nodes become Stopped).",
    }
}

/// `workflow_replan` description。
pub fn workflow_replan(lang: &str) -> &'static str {
    match lang {
        "zh" => "仅可在 Active(Replanning) 中调用：提交 continuation subgraph。给出 boundary（触发节点、被停止节点、冻结事实）、可选的新节点与边、至少一个 entry 节点，以及至少一条 rejoin 边（target 必须是既有 Pending 非 Complete 节点）。新 Plan Graph 合法且可立即推进时，Workflow 返回 Executing。",
        _ => "Only inside Active(Replanning): submit a continuation subgraph. Provide the boundary (trigger node, stopped nodes, frozen facts), optional new nodes and edges, at least one entry node, and at least one rejoin edge whose target is an existing Pending non-Complete node. The workflow returns to Executing when the new Plan Graph is valid and immediately advanceable.",
    }
}
