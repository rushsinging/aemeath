//! 任务工具文案（task_create/get/list/stop/update/list_create/list_complete 的 description）。

/// TaskCreate description。
pub fn task_create(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"仅为复杂的多步骤工作（3+ 步骤、相互依赖的改动或并行子代理）创建任务以跟踪进度；简单单步请求直接执行，不要建任务。调用前须已有 active 任务列表（没有时先用 TaskListCreate 创建），否则会报"当前没有 active 批次"。"#
        }
        _ => {
            r#"Create a task to track progress on complex multi-step work only (3+ steps, dependent changes, or parallel sub-agents). For simple one-step requests, execute directly. Requires an active task list (create one with TaskListCreate first), otherwise the call fails."#
        }
    }
}

/// TaskGet description。
pub fn task_get(lang: &str) -> &'static str {
    match lang {
        "zh" => "按 ID 检索任务。返回任务详情，包括主题、描述、状态和依赖。",
        _ => "Retrieve a task by ID. Returns task details including subject, description, status, and dependencies.",
    }
}

/// TaskListGet description。
pub fn task_list(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"列出所有任务及其状态。仅当最近一次 TaskUpdate(status) 返回的进度摘要不足以决定下一步，或用户明确要求查看完整任务列表时使用。"#
        }
        _ => {
            r#"List all tasks and their status. Use only when the latest TaskUpdate(status) progress summary is insufficient to choose the next step, or when the user explicitly requests the full task list."#
        }
    }
}

/// TaskStop description。
pub fn task_stop(lang: &str) -> &'static str {
    match lang {
        "zh" => "停止运行中或待处理的任务。将任务标记为已删除并取消关联工作。",
        _ => "Stop a running or pending task. Marks the task as deleted and cancels any associated work.",
    }
}

/// TaskUpdate description。
pub fn task_update(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"更新任务的单个字段。用于在工作推进时跟踪任务状态：开始一项任务前标记 in_progress，完成后标记 completed。每次调用只改一个字段，合法字段与取值见参数说明。"#
        }
        _ => {
            r#"Update a single field on a task. Use to track progress through the task lifecycle: mark a task in_progress before starting work on it and completed when finished. Each call changes exactly one field; valid fields and accepted values are documented on the parameters."#
        }
    }
}

/// TaskBlockBy description。
pub fn task_block_by(lang: &str) -> &'static str {
    match lang {
        "zh" => "完整替换任务的前置依赖。所有 ID 必须属于当前任务列表，且更新不得形成环。",
        _ => "Replace all blocking dependencies of a task. All IDs must belong to the current task list, and the update must remain acyclic.",
    }
}

pub fn task_lists(lang: &str) -> &'static str {
    match lang {
        "zh" => "列出当前及历史任务列表，可按 active / paused / archived 状态过滤。使用返回的 ID 调用 TaskListGet 查询具体列表。",
        _ => "List current and historical task lists, optionally filtered by active, paused, or archived status. Use a returned ID with TaskListGet to inspect that list.",
    }
}

/// TaskListCreate description。
pub fn task_list_create(lang: &str) -> &'static str {
    match lang {
        "zh" => "为复杂的多步骤请求创建任务列表（3+ 步骤、多个依赖，或并行子代理协调）。之后创建的任务会自动挂载到此列表。",
        _ => "Create a task list for a complex multi-step request (3+ steps, multiple dependencies, or parallel sub-agent coordination). Tasks created afterwards auto-attach to this list.",
    }
}

/// TaskListComplete description。
pub fn task_list_complete(lang: &str) -> &'static str {
    match lang {
        "zh" => "在当前用户请求的所有任务完成后，完成当前活动任务列表。这会停止该已完成列表的未来提醒。",
        _ => "Complete the current active task list after all tasks for the current user request are done. This stops future reminders for that completed list.",
    }
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
