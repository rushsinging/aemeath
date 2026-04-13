# Task Panel (Inline) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 TUI 输出流中以系统消息形式展示实时任务状态，替代之前 TaskCreate tool result 中的内嵌列表。

**Architecture:** 工具执行后检测 TaskCreate/TaskUpdate 结果并插入完整任务快照；扩展现有 timer 轮询机制，在有活跃任务时持续检测状态变更并插入单条更新消息。

**Tech Stack:** Rust, tokio, ratatui TUI

---

### Task 1: 添加任务快照格式化函数

**Files:**
- Create: `aemeath-cli/src/tui/task_display.rs`
- Modify: `aemeath-cli/src/tui/mod.rs`

- [ ] **Step 1: 创建 task_display.rs 模块**

```rust
// aemeath-cli/src/tui/task_display.rs

use aemeath_core::task::{Task, TaskStatus};

/// 格式化完整任务列表快照
pub fn format_task_snapshot(tasks: &[Task]) -> String {
    let completed = tasks.iter().filter(|t| t.status == TaskStatus::Completed).count();
    let total = tasks.iter().filter(|t| t.status != TaskStatus::Deleted).count();

    if total == 0 {
        return String::new();
    }

    let mut lines = vec![format!("━━━ Tasks: {}/{} completed ━━━", completed, total)];

    for t in tasks {
        if t.status == TaskStatus::Deleted {
            continue;
        }
        let icon = match t.status {
            TaskStatus::Completed => "✓",
            TaskStatus::InProgress => "■",
            TaskStatus::Pending => "□",
            TaskStatus::Deleted => continue,
        };
        let owner = t.owner.as_deref().map(|o| format!(" (@{})", o)).unwrap_or_default();
        let blocked = if !t.blocked_by.is_empty() {
            let deps = t.blocked_by.iter().map(|d| format!("#{d}")).collect::<Vec<_>>().join(", ");
            format!(" (blocked by {})", deps)
        } else {
            String::new()
        };
        lines.push(format!("  {} #{} {}{}{}", icon, t.id, t.subject, owner, blocked));
    }

    lines.join("\n")
}

/// 格式化单条任务状态变更
pub fn format_task_change(task: &Task) -> String {
    match task.status {
        TaskStatus::InProgress => {
            let action = task.active_form.as_deref().unwrap_or("started");
            format!("  ■ #{} {} — {}", task.id, task.subject, action)
        }
        TaskStatus::Completed => {
            format!("  ✓ #{} {} — completed", task.id, task.subject)
        }
        _ => String::new(),
    }
}
```

- [ ] **Step 2: 注册模块**

在 `aemeath-cli/src/tui/mod.rs` 中添加 `pub mod task_display;`

- [ ] **Step 3: 编译验证**

Run: `cargo build 2>&1`
Expected: 编译通过

- [ ] **Step 4: Commit**

```bash
git add aemeath-cli/src/tui/task_display.rs aemeath-cli/src/tui/mod.rs
git commit -m "feat: add task_display module for snapshot and change formatting"
```

---

### Task 2: 工具执行后插入任务快照

**Files:**
- Modify: `aemeath-cli/src/tui/app.rs:1575-1612` (tool result 处理后)

- [ ] **Step 1: 在 tool result 发送后、MessagesSync 前，检测 TaskCreate/TaskUpdate 并插入快照**

在 `process_in_background` 中，找到发送 ToolResult 事件的循环（约 1575 行）之后、`messages.push(Message::tool_results_rich(...))` 之前，添加：

```rust
// After sending ToolResult events, check if any task tool was called
// and insert a task snapshot into the output stream
{
    let has_task_create = tool_name_map.values().any(|n| *n == "TaskCreate");
    let has_task_update_completed = tool_name_map.values().any(|n| *n == "TaskUpdate")
        && all_results.iter().any(|(_, output, is_err, _)| !is_err && output.contains("completed"));

    if has_task_create || has_task_update_completed {
        let tasks = _task_store.list().await;
        let snapshot = crate::tui::task_display::format_task_snapshot(&tasks);
        if !snapshot.is_empty() {
            let _ = tx.send(UiEvent::SystemMessage(snapshot)).await;
        }
    }
}
```

具体位置：在 `for (_id, output, ...) in results.iter() { ... }` 循环之后，在 `// Build combined results` 注释之前插入。

- [ ] **Step 2: 编译验证**

Run: `cargo build 2>&1`
Expected: 编译通过

- [ ] **Step 3: Commit**

```bash
git add aemeath-cli/src/tui/app.rs
git commit -m "feat: insert task snapshot after TaskCreate/TaskUpdate completion"
```

---

### Task 3: 扩展轮询机制 — 任何活跃任务时都启动轮询

**Files:**
- Modify: `aemeath-cli/src/tui/app.rs:1503-1562` (timer 轮询逻辑)

- [ ] **Step 1: 修改轮询触发条件和轮询逻辑**

当前轮询只在 `has_long_running`（Agent 或 TodoRun）时启动。改为：当 TaskStore 中有 pending 或 in_progress 任务时也启动。

将以下代码块：

```rust
let has_long_running = tool_names.iter().any(|n| n == "Agent" || n == "TodoRun");
let has_todo_run = tool_names.iter().any(|n| n == "TodoRun");
```

替换为：

```rust
let has_long_running = tool_names.iter().any(|n| n == "Agent");
// Check if there are active tasks that need polling
let has_active_tasks = {
    let tasks = _task_store.list().await;
    tasks.iter().any(|t| t.status == aemeath_core::task::TaskStatus::Pending
        || t.status == aemeath_core::task::TaskStatus::InProgress)
};
let should_poll = has_long_running || has_active_tasks;
```

- [ ] **Step 2: 修改 timer_handle 条件和轮询体**

将 `let timer_handle = if has_long_running {` 改为 `let timer_handle = if should_poll {`

在轮询循环体中，将原来的 `if has_todo_run { ... }` 整块替换为使用 `task_display::format_task_change` 的通用逻辑：

```rust
// Poll task status changes
let current_tasks = timer_store.list().await;
for t in &current_tasks {
    let prev = last_statuses.get(&t.id);
    let changed = match prev {
        Some(prev_status) => *prev_status != t.status,
        None => t.status == TaskStatus::InProgress || t.status == TaskStatus::Completed,
    };
    if changed {
        let msg = crate::tui::task_display::format_task_change(t);
        if !msg.is_empty() {
            let _ = timer_tx.send(UiEvent::SystemMessage(msg)).await;
        }
        last_statuses.insert(t.id.clone(), t.status.clone());
    }
}
```

同时删除 `if has_todo_run {` 初始化 `last_statuses` 的条件判断 — 始终初始化：

```rust
// Initialize with current statuses
for t in timer_store.list().await {
    last_statuses.insert(t.id.clone(), t.status.clone());
}
```

- [ ] **Step 3: 清理 `has_todo_run` 变量**

删除 `let has_todo_run = ...;` 行（不再需要）。

- [ ] **Step 4: 编译验证**

Run: `cargo build 2>&1`
Expected: 编译通过

- [ ] **Step 5: Commit**

```bash
git add aemeath-cli/src/tui/app.rs
git commit -m "feat: poll task status changes for any active tasks"
```

---

### Task 4: TaskCreate tool result 恢复简短输出

**Files:**
- Modify: `aemeath-tools/src/task_create.rs:97-125`

- [ ] **Step 1: 去掉 TaskCreate 中的任务列表摘要**

将 `task_create.rs` 中从 `let mut output = format!(...)` 到 `ToolResult::success(output)` 的整块替换回简短输出：

```rust
        ToolResult::success(format!(
            "Task #{} created successfully: {} [{}]{progress_str}\nDescription: {}",
            task.id, task.subject, priority_str, task.description
        ))
```

- [ ] **Step 2: 恢复 output_area.rs 中 task tool 的默认显示行数**

将 `output_area.rs` 中的：

```rust
let max_lines = if matches!(tool_name, "TaskCreate" | "TaskUpdate" | "TaskList") {
    20
} else {
    3
};
```

改回统一 3 行（TaskList 保留 20 行，因为用户主动查看时需要看到完整列表）：

```rust
let max_lines = if matches!(tool_name, "TaskList") {
    20
} else {
    3
};
```

- [ ] **Step 3: 编译验证**

Run: `cargo build 2>&1`
Expected: 编译通过

- [ ] **Step 4: Commit**

```bash
git add aemeath-tools/src/task_create.rs aemeath-cli/src/tui/output_area.rs
git commit -m "refactor: simplify TaskCreate output, task list shown via snapshot"
```

---

### Task 5: 清理遗留 TodoRun 引用

**Files:**
- Modify: `aemeath-cli/src/tui/app.rs` (TodoRun 相关的 enrichment 和注释)

- [ ] **Step 1: 删除 TodoRun enrichment 逻辑**

在 `process_in_background` 中（约 1438-1465 行），找到 `// For TodoRun: enrich with pending todo subjects` 块。将整个 if-else 简化：

```rust
for call in &tool_calls {
    let _ = tx.send(UiEvent::ToolCall {
        name: call.name.clone(),
        summary: call.input.to_string(),
    }).await;
}
```

这替换了原来包含 `if call.name == "TodoRun"` 分支的循环。

- [ ] **Step 2: 删除 Plan A 注释**

删除 `// [Plan A disabled] Auto-trigger TodoRun...` 相关注释。

- [ ] **Step 3: 编译验证**

Run: `cargo build 2>&1`
Expected: 编译通过

- [ ] **Step 4: Commit**

```bash
git add aemeath-cli/src/tui/app.rs
git commit -m "cleanup: remove TodoRun references from TUI"
```
