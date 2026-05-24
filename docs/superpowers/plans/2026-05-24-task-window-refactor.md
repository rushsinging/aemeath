# Task Window 重构 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 重构 task list TUI 显示逻辑，统一编号来源、简化排序策略、消除 display number 跳跃问题。

**Architecture:** 去掉 TUI 层重复的 `build_display_numbers`，编号完全委托 `TaskStore.get_display_number`（按全局 id 升序，batch 内排除 deleted）。排序策略简化为 `completed(按 id 升序) → in_progress(按 id 升序) → pending(按 id 升序)`，窗口化只做截断 + 折叠提示。

**Tech Stack:** Rust, tokio, ratatui (TUI 框架)

---

## 问题诊断

### 当前缺陷

1. **双重编号逻辑**：`task_window::build_display_numbers` 在 TUI 层按全局 id 排序后分配 1-based 索引；`TaskStore` 层 `display.rs` 的 `get_display_number` / `tasks_in_batch` 做同样的事，但排序实现略有不同（TUI 用 `task_sort_key`，store 用 `id.parse::<u64>`）。两套编号可能不一致。
2. **双重排序冲突**：`list_current_batch` 按 priority+created_at 排序传入 `build_task_window`，但 `build_task_window` 内部完全重新排序（TTL/recency/扩展回退），传入顺序被忽略。
3. **`build_task_window` 过于复杂**：225 行主体 + 多层扩展/回退/合并逻辑，包含 `merge_completed_lines`、`COMPLETED_TTL_SECS`、下限保护、温和扩展等概念，难以理解和维护。
4. **编号不连续**：窗口化只展示部分 task 时，display number 跳跃（如 #1,#2,#3,#11,#12,#4,#5），用户困惑。

### 设计决策

| 决策 | 理由 |
|------|------|
| 编号统一用 `TaskStore.get_display_number` | LLM 工具调用中的 #N 与 TUI 显示必须一致；单一真相来源 |
| 去掉 `build_display_numbers` | 消除双重编号不一致风险 |
| 排序简化为 completed→in_progress→pending | 组内排序：completed 按 updated_at 降序（最近完成在前，反映实际执行顺序 1→2→7→3→…），in_progress 按 updated_at 升序（最早开始的在前），pending 按 display_number 升序（稳定序） |
| 去掉 TTL/recency/扩展回退 | 窗口化只做简单截断，不再按时间过滤或补齐；减少复杂度 |
| 保留窗口化截断 + 折叠提示 | 超出 max_lines 时仍需折叠提示，但逻辑大幅简化 |

---

## 涉及文件

| 文件 | 操作 | 职责 |
|------|------|------|
| `packages/core/src/task/display.rs` | 修改 | 新增 `get_batch_display_map` 批量获取 display number |
| `packages/core/src/task/batch.rs` | 不变 | `tasks_in_batch` 已满足需求 |
| `cli/src/tui/app/task_window.rs` | 重写 | 简化为 ~100 行的 `build_task_window` |
| `cli/src/tui/app/task_window_tests.rs` | 重写 | 覆盖新逻辑 |
| `cli/src/tui/app/task_window_helpers_tests.rs` | 修改 | 测试辅助函数适配新签名 |
| `cli/src/tui/app/task_window_progress_tests.rs` | 修改 | 适配新签名 |
| `cli/src/tui/app/runtime.rs` | 修改 | `update_task_status` 改用 store display map |

---

## Task 1: TaskStore 新增 `get_batch_display_map`

**Files:**
- Modify: `packages/core/src/task/display.rs`
- Test: `packages/core/src/task/display.rs` (inline tests)

批量获取当前 batch 内所有 task 的 display number 映射，供 TUI 一次调用。

- [ ] **Step 1: 在 `display.rs` impl TaskStore 中新增方法**

```rust
/// Batch-get display numbers for all tasks in the current display batch.
/// Returns a map from global task id to 1-based display number.
/// Returns empty map if no active batch.
pub async fn get_batch_display_map(&self) -> std::collections::HashMap<String, usize> {
    let Some(batch_id) = self.display_batch_id().await else {
        return std::collections::HashMap::new();
    };
    let tasks = self
        .tasks_in_batch(
            batch_id,
            &[
                TaskStatus::Pending,
                TaskStatus::InProgress,
                TaskStatus::Completed,
            ],
        )
        .await;
    tasks
        .into_iter()
        .enumerate()
        .map(|(i, t)| (t.id, i + 1))
        .collect()
}
```

- [ ] **Step 2: 在 display.rs tests 中添加测试**

```rust
#[tokio::test]
async fn test_get_batch_display_map_empty() {
    let store = setup_store_with_batches().await;
    let map = store.get_batch_display_map().await;
    assert!(map.is_empty());
}

#[tokio::test]
async fn test_get_batch_display_map_returns_sequential_numbers() {
    let store = setup_store_with_batches().await;
    store.batches.lock().await.push(Batch::new(1));
    add_task(&store, "8", 1, TaskStatus::Pending).await;
    add_task(&store, "9", 1, TaskStatus::InProgress).await;
    add_task(&store, "10", 1, TaskStatus::Completed).await;
    let map = store.get_batch_display_map().await;
    assert_eq!(map["8"], 1);
    assert_eq!(map["9"], 2);
    assert_eq!(map["10"], 3);
}

#[tokio::test]
async fn test_get_batch_display_map_excludes_deleted() {
    let store = setup_store_with_batches().await;
    store.batches.lock().await.push(Batch::new(1));
    add_task(&store, "1", 1, TaskStatus::Pending).await;
    add_task(&store, "2", 1, TaskStatus::Deleted).await;
    add_task(&store, "3", 1, TaskStatus::InProgress).await;
    let map = store.get_batch_display_map().await;
    assert_eq!(map.len(), 2);
    assert_eq!(map["1"], 1);
    assert_eq!(map["3"], 2);
}
```

- [ ] **Step 3: 验证**

Run: `cargo test -p aemeath-core -- task::display`
Expected: 所有测试通过

---

## Task 2: 重写 `build_task_window`

**Files:**
- Rewrite: `cli/src/tui/app/task_window.rs`

简化为：分组排序 → 截断 → 格式化。display number 由外部传入。

- [ ] **Step 1: 重写 `task_window.rs`**

新签名：

```rust
//! Task list 窗口化显示
//!
//! 排序策略：
//!   completed → 按 updated_at 降序（最近完成在前，反映实际执行顺序）
//!   in_progress → 按 updated_at 升序（最早开始的在前）
//!   pending → 按 display_number 升序（稳定序）
//! 窗口化：超出 max_lines 时截断并显示折叠提示

use aemeath_core::task::{Task, TaskStatus};
use std::collections::HashMap;

/// 构建 task 状态显示行（窗口化，含摘要行）。
///
/// 规则：
/// 1. 摘要行 `━━ Tasks: completed/total ━━` 反映全量
/// 2. 按状态分组排序：completed → in_progress → pending
///    - completed 组内按 updated_at 降序（最近完成在前）
///    - in_progress 组内按 updated_at 升序（最早开始的在前）
///    - pending 组内按 display_number 升序
/// 3. 最多显示 `max_lines` 条 task 行
/// 4. 超出部分折叠提示 `… +N more`
/// 5. 空输入返回空 Vec
pub fn build_task_window(
    tasks: &[Task],
    display_map: &HashMap<String, usize>,
    max_lines: usize,
) -> Vec<String> {
    if tasks.is_empty() || max_lines == 0 {
        return Vec::new();
    }

    let total = tasks.len();
    let completed_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();

    let summary = format!("━━ Tasks: {}/{} ━━", completed_count, total);
    let mut lines = vec![summary];

    let mut completed: Vec<&Task> = Vec::new();
    let mut in_progress: Vec<&Task> = Vec::new();
    let mut pending: Vec<&Task> = Vec::new();

    for t in tasks {
        match t.status {
            TaskStatus::Completed => completed.push(t),
            TaskStatus::InProgress => in_progress.push(t),
            TaskStatus::Pending => pending.push(t),
            _ => {}
        }
    }

    // completed: 最近完成在前
    completed.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    // in_progress: 最早开始在前
    in_progress.sort_by_key(|t| t.updated_at);
    // pending: 按 display_number 稳定序
    sort_by_display_number(&mut pending, display_map);

    let ordered: Vec<&Task> = completed
        .into_iter()
        .chain(in_progress.into_iter())
        .chain(pending.into_iter())
        .collect();

    let shown_count = ordered.len().min(max_lines);
    let hidden_count = ordered.len() - shown_count;

    for t in ordered.iter().take(shown_count) {
        lines.push(format_task_line(t, display_map));
    }

    if hidden_count > 0 {
        lines.push(format!("… +{} more", hidden_count));
    }

    lines
}

fn sort_by_display_number(tasks: &mut [&Task], display_map: &HashMap<String, usize>) {
    tasks.sort_by_key(|t| display_map.get(&t.id).copied().unwrap_or(usize::MAX));
}

fn format_task_line(t: &Task, display_map: &HashMap<String, usize>) -> String {
    let icon = match t.status {
        TaskStatus::Completed => "✓",
        TaskStatus::InProgress => "■",
        TaskStatus::Pending => "□",
        _ => "?",
    };
    let display_id = display_map.get(&t.id).copied().unwrap_or(0);
    let owner = t
        .owner
        .as_deref()
        .map(|o| format!(" (@{})", o))
        .unwrap_or_default();
    format!("{} #{} {}{}", icon, display_id, t.subject, owner)
}

#[cfg(test)]
#[path = "task_window_tests.rs"]
mod tests;
```

注意：删除了 `build_display_numbers`、`merge_completed_lines`、`display_line_number`、`format_fold_hint`、`task_sort_key`、`COMPLETED_TTL_SECS` 等旧函数。不再需要 `show_last_completed` 参数。

---

## Task 3: 重写测试

**Files:**
- Rewrite: `cli/src/tui/app/task_window_tests.rs`
- Rewrite: `cli/src/tui/app/task_window_helpers_tests.rs`
- Rewrite: `cli/src/tui/app/task_window_progress_tests.rs`

- [ ] **Step 1: 重写 `task_window_helpers_tests.rs`**

```rust
use aemeath_core::task::{Task, TaskPriority, TaskStatus};

pub(crate) fn make_task_with_ts(id: &str, subject: &str, status: TaskStatus, ts: u64) -> Task {
    Task {
        id: id.to_string(),
        subject: subject.to_string(),
        description: String::new(),
        status,
        active_form: None,
        owner: None,
        blocked_by: Vec::new(),
        blocks: Vec::new(),
        priority: TaskPriority::Normal,
        progress: 0,
        progress_message: None,
        created_at: ts,
        updated_at: ts,
        session_id: None,
        tags: Vec::new(),
        batch: 0,
    }
}

pub(crate) fn make_task(id: &str, subject: &str, status: TaskStatus) -> Task {
    make_task_with_ts(id, subject, status, id.parse::<u64>().unwrap_or(100))
}

/// Build a display map from a slice of tasks (sorted by global id ascending).
pub(crate) fn make_display_map(tasks: &[Task]) -> std::collections::HashMap<String, usize> {
    let mut ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    ids.sort_by_key(|id| id.parse::<u64>().unwrap_or(u64::MAX));
    ids.into_iter()
        .enumerate()
        .map(|(i, id)| (id.to_string(), i + 1))
        .collect()
}
```

- [ ] **Step 2: 重写 `task_window_tests.rs`**

核心测试用例：

```rust
use super::helpers_tests::{make_display_map, make_task, make_task_with_ts};
use super::*;

#[test]
fn test_empty() {
    let result = build_task_window(&[], &Default::default(), 7);
    assert!(result.is_empty());
}

#[test]
fn test_max_lines_zero() {
    let tasks = vec![make_task("1", "test", TaskStatus::Pending)];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 0);
    assert!(result.is_empty());
}

#[test]
fn test_single_pending() {
    let tasks = vec![make_task("1", "do thing", TaskStatus::Pending)];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert_eq!(result.len(), 2);
    assert!(result[0].contains("0/1"));
    assert!(result[1].contains("□ #1 do thing"));
}

#[test]
fn test_single_in_progress() {
    let tasks = vec![make_task("1", "in progress", TaskStatus::InProgress)];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("■ #1"));
}

#[test]
fn test_single_completed() {
    let tasks = vec![make_task("1", "done", TaskStatus::Completed)];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("✓ #1 done"));
}

#[test]
fn test_status_group_ordering() {
    // make_task_with_ts(id, subject, status, updated_at)
    // completed 按 updated_at 降序：task 7 (ts=700) → task 3 (ts=300) → task 1 (ts=100)
    // in_progress 按 updated_at 升序：task 4 (ts=400)
    // pending 按 display_number 升序：task 2 (display=2) → task 5 (display=5)
    let tasks = vec![
        make_task_with_ts("1", "done x", TaskStatus::Completed, 100),
        make_task_with_ts("2", "pending a", TaskStatus::Pending, 200),
        make_task_with_ts("3", "done y", TaskStatus::Completed, 300),
        make_task_with_ts("4", "doing a", TaskStatus::InProgress, 400),
        make_task_with_ts("5", "pending b", TaskStatus::Pending, 500),
        make_task_with_ts("7", "done z", TaskStatus::Completed, 700),
    ];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert_eq!(result.len(), 7); // summary + 6 tasks
    assert!(result[0].contains("3/6"));
    // completed group: updated_at desc → #6(done z,ts=700), #3(done y,ts=300), #1(done x,ts=100)
    assert!(result[1].contains("✓ #6 done z"));
    assert!(result[2].contains("✓ #3 done y"));
    assert!(result[3].contains("✓ #1 done x"));
    // in_progress group
    assert!(result[4].contains("■ #4 doing a"));
    // pending group: display_number asc → #2, #5
    assert!(result[5].contains("□ #2 pending a"));
    assert!(result[6].contains("□ #5 pending b"));
}

#[test]
fn test_truncation_with_fold_hint() {
    let tasks: Vec<Task> = (1..=20)
        .map(|i| make_task(&i.to_string(), &format!("task {}", i), TaskStatus::Pending))
        .collect();
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    // summary + 7 tasks + fold hint
    assert_eq!(result.len(), 9);
    assert!(result[0].contains("0/20"));
    assert!(result.last().unwrap().contains("+13 more"));
}

#[test]
fn test_all_completed() {
    // make_task uses id as updated_at, so id=10 has ts=10 (most recent) → first
    let tasks: Vec<Task> = (1..=10)
        .map(|i| make_task(&i.to_string(), &format!("task {}", i), TaskStatus::Completed))
        .collect();
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert_eq!(result.len(), 9); // summary + 7 + fold
    assert!(result[0].contains("10/10"));
    // completed sorted by updated_at desc → #10, #9, #8, ...
    assert!(result[1].contains("✓ #10"));
    assert!(result[2].contains("✓ #9"));
    assert!(result.last().unwrap().contains("+3 more"));
}

#[test]
fn test_display_numbers_match_store_numbering() {
    // Global ids are non-sequential: 8, 9, 10
    // Display numbers should be 1, 2, 3 (batch-local)
    let tasks = vec![
        make_task("8", "first", TaskStatus::Pending),
        make_task("9", "second", TaskStatus::InProgress),
        make_task("10", "third", TaskStatus::Completed),
    ];
    let map = make_display_map(&tasks);
    assert_eq!(map["8"], 1);
    assert_eq!(map["9"], 2);
    assert_eq!(map["10"], 3);
    let result = build_task_window(&tasks, &map, 7);
    // completed (id=10, display=3) → in_progress (id=9, display=2) → pending (id=8, display=1)
    assert!(result[1].contains("✓ #3 third"));
    assert!(result[2].contains("■ #2 second"));
    assert!(result[3].contains("□ #1 first"));
}

#[test]
fn test_deleted_tasks_excluded() {
    let tasks = vec![
        make_task("1", "done", TaskStatus::Completed),
        make_task("2", "deleted", TaskStatus::Deleted),
        make_task("3", "pending", TaskStatus::Pending),
    ];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    // Deleted is excluded from display, so 2 task lines
    let task_lines: Vec<_> = result.iter().skip(1).collect();
    assert_eq!(task_lines.len(), 2);
    assert!(task_lines[0].contains("✓ #1"));
    assert!(task_lines[1].contains("□ #2"));
}

#[test]
fn test_owner_display() {
    let mut task = make_task("1", "owned task", TaskStatus::InProgress);
    task.owner = Some("agent-1".to_string());
    let tasks = vec![task];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("@agent-1"));
}
```

- [ ] **Step 3: 重写 `task_window_progress_tests.rs`**

适配新签名（`display_map` 参数）：

```rust
use super::helpers_tests::{make_display_map, make_task};
use super::*;

#[test]
fn test_completed_group_sorted_by_updated_at_desc() {
    // make_task_with_ts: updated_at set explicitly
    // completed: updated_at desc → step three(500) → step two(300) → step one(100)
    let tasks = vec![
        make_task_with_ts("1", "step one", TaskStatus::Completed, 100),
        make_task_with_ts("2", "step two", TaskStatus::Completed, 300),
        make_task_with_ts("3", "step three", TaskStatus::Completed, 500),
        make_task_with_ts("4", "current", TaskStatus::InProgress, 400),
        make_task_with_ts("5", "next", TaskStatus::Pending, 600),
    ];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("✓ #3 step three")); // ts=500
    assert!(result[2].contains("✓ #2 step two"));   // ts=300
    assert!(result[3].contains("✓ #1 step one"));   // ts=100
    assert!(result[4].contains("■ #4"));
    assert!(result[5].contains("□ #5"));
}

#[test]
fn test_window_truncates_across_groups() {
    let mut tasks: Vec<Task> = (1..=5)
        .map(|i| make_task(&i.to_string(), &format!("done {}", i), TaskStatus::Completed))
        .collect();
    tasks.push(make_task("6", "doing", TaskStatus::InProgress));
    tasks.push(make_task("7", "pending", TaskStatus::Pending));
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 4);
    // summary + 4 tasks + fold hint
    assert_eq!(result.len(), 6);
    // First 4 are completed (id 1-4), rest truncated
    assert!(result[1].contains("✓ #1"));
    assert!(result[4].contains("✓ #4"));
    assert!(result[5].contains("+3 more"));
}
```

---

## Task 4: 更新 `runtime.rs` 调用方

**Files:**
- Modify: `cli/src/tui/app/runtime.rs`

- [ ] **Step 1: 更新 `update_task_status` 方法**

```rust
pub(crate) async fn update_task_status(
    &mut self,
    task_store: &Arc<aemeath_core::task::TaskStore>,
    _is_processing: bool,
) {
    let tasks = task_store.list_current_batch().await;
    let active: Vec<_> = tasks
        .iter()
        .filter(|t| t.status != aemeath_core::task::TaskStatus::Deleted)
        .cloned()
        .collect();

    if active.is_empty() {
        self.output_area.set_task_status(Vec::new());
    } else {
        let display_map = task_store.get_batch_display_map().await;
        let task_list_config = aemeath_core::config::TaskListConfig::default();
        let lines = task_window::build_task_window(
            &active,
            &display_map,
            task_list_config.max_lines,
        );
        self.output_area.set_task_status(lines);
    }
}
```

- [ ] **Step 2: 验证**

Run: `cargo check -p aemeath-cli`
Expected: 编译通过

---

## Task 5: 清理旧代码

**Files:**
- Modify: `cli/src/tui/app/task_window.rs`（已重写，确认无旧残留）
- Check: `cli/src/tui/output_area/render_status.rs`（引用 task_window 的测试）

- [ ] **Step 1: 检查 render_status.rs 中是否有旧 task_window 测试引用**

搜索 `build_task_window` 的所有调用点，确认参数已更新。

- [ ] **Step 2: 删除 `TaskListConfig` 中不再需要的 `show_last_completed` 字段**

如果 `show_last_completed` 仅被旧 `build_task_window` 使用，从 config 中删除。

- [ ] **Step 3: 全量验证**

Run: `cargo test -p aemeath-cli`
Run: `cargo test -p aemeath-core -- task::display`
Expected: 全部通过

---

## Task 6: 提交

- [ ] **Step 1: 格式检查**

Run: `cargo fmt --check`

- [ ] **Step 2: 提交**

```
refactor(tui): simplify task window display logic

- Remove dual display number calculation (TUI + TaskStore)
- Use TaskStore.get_batch_display_map as single source of truth
- Simplify sort: completed→in_progress→pending, grouped by display id
- Remove TTL/recency/expansion/fallback complexity
- Remove show_last_completed config parameter
```
