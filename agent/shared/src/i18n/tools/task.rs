//! 任务工具文案（task_create/get/list/stop/update/list_create/list_complete 的 description）。

/// TaskCreate description。
pub fn task_create(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"仅为复杂的多步骤工作创建任务以跟踪进度。

仅当用户请求需要至少 3 个实质性执行步骤、多个相互依赖的改动，或并行子代理协调时，才使用任务管理。不要为简单的单步请求创建任务，例如回答问题、查看文件、检查 bug 状态、运行单个命令或做微小的局部编辑。对简单请求直接执行。

重要：每个任务必须是单一、具体、可验证的步骤。糟糕的任务会把多个改动混在一起，如"实施并验证功能"或"修复所有相关问题"。好的任务是具体的："读 X.rs 理解当前错误处理"、"为 Y::send 加重试逻辑"、"为 Z 边界用例加单元测试"、"运行 cargo clippy 并修复警告"。当任务涉及实现时，拆成按文件或按函数的改动，外加单独的验证步骤。

真正需要任务管理时的重要工作流：
1. 首先，以文字描述完整计划——列出所有计划任务，让用户看到全貌
2. 对新的复杂多步骤用户请求，先调用 TaskListCreate 再调用 TaskCreate，使任务挂载到请求摘要
3. 然后用 TaskCreate 逐个创建任务
4. 用 TaskUpdate 设置依赖并分配代理

创建任务后，用 TaskUpdate：
- 在任务间设置依赖（addBlockedBy/addBlocks）
- 开始工作前标记为 in_progress
- 完成后标记为 completed——系统会显示哪些任务已解除阻塞

用 TaskList 发现无未解决依赖的待处理任务。
为可并行执行的独立任务启动 Agent。"#
        }
        _ => {
            r#"Create a task to track progress on complex multi-step work only.

Use task management only when the user request requires at least 3 substantial execution steps,
multiple dependent changes, or parallel sub-agent coordination. Do NOT create tasks for simple
one-step requests such as answering a question, inspecting a file, checking bug status, running a
single command, or making a tiny localized edit. For simple requests, execute directly.

IMPORTANT: each task must be a SINGLE, CONCRETE, VERIFIABLE step. BAD tasks lump multiple
changes together, such as "Implement and verify feature" or "Fix all related issues". GOOD tasks
are specific: "Read X.rs to understand current error handling", "Add retry logic to Y::send",
"Add unit test for Z edge case", "Run cargo clippy and fix warnings". When a task involves
implementation, split it into per-file or per-function changes plus separate verification steps.

IMPORTANT workflow when task management is actually needed:
1. First, describe your complete plan as text — list ALL planned tasks so the user can see the full picture
2. For a new complex multi-step user request, call TaskListCreate before TaskCreate so tasks attach to a request summary
3. Then create tasks one by one with TaskCreate
4. Use TaskUpdate to set dependencies and assign agents

After creating tasks, use TaskUpdate to:
- Set dependencies (addBlockedBy/addBlocks) between tasks
- Mark as in_progress before starting work
- Mark as completed when done — the system will show which tasks are unblocked

Use TaskList to discover pending tasks with no unresolved dependencies.
Launch Agent for independent tasks that can run in parallel."#
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

/// TaskList description。
pub fn task_list(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            r#"列出所有任务及其状态。用于发现可用工作。

寻找 pending 且无未解决 blocked_by 依赖的任务——这些已就绪可执行。可以直接处理，或为可并行的任务启动 Agent。

完成一个任务后调用此工具查找下一个要处理的任务。"#
        }
        _ => {
            r#"List all tasks and their status. Use to discover available work.

Look for tasks that are pending with no unresolved blocked_by dependencies —
these are ready to execute. You can work on them directly or launch Agent
for tasks that can run in parallel.

Call this after completing a task to find the next one to work on."#
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
            r#"更新任务的状态、主题、描述或依赖。

状态工作流：pending → in_progress → completed。用 'deleted' 删除。

标记任务为 completed 时，系统会显示哪些下游任务已解除阻塞、可执行。据此决定下一步处理什么。

完成任务后，检查解除阻塞列表或调用 TaskList 查找下一个可用任务。"#
        }
        _ => {
            r#"Update a task's status, subject, description, or dependencies.

Status workflow: pending → in_progress → completed. Use 'deleted' to remove.

When you mark a task as completed, the system will show which downstream tasks
are now unblocked and ready to execute. Use this to decide what to work on next.

After completing a task, check the unblocked list or call TaskList to find the next available task."#
        }
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
mod tests {
    use super::*;

    #[test]
    fn task_bilingual_and_fallback() {
        assert!(task_create("zh").contains("仅为复杂的多步骤工作"));
        assert!(task_create("en").contains("Create a task to track progress"));
        assert_eq!(task_create("fr"), task_create("en"));
        assert!(task_get("zh").contains("按 ID 检索任务"));
        assert!(task_list("zh").contains("列出所有任务"));
        assert!(task_stop("zh").contains("停止"));
        assert!(task_update("zh").contains("更新任务"));
        assert!(task_list_create("zh").contains("创建任务列表"));
        assert!(task_list_complete("zh").contains("完成当前活动任务列表"));
    }
}
