<!-- Migrated from: docs/feature/archived/067-changeset-task-project-refresh.md -->
# Feature #67：Task/project window 改为 ChangeSet 驱动刷新

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 归档日期 | 2026-06-02 |
| 状态 | 已确认完成 |
| 实现 | d07d582 |

## 背景

TUI task list window 原本每帧调用 `AgentClient::task_status()` 轮询 Runtime；status bar 的 worktree kind / branch 也只在 init 阶段设置，EnterWorktree/ExitWorktree 后不会事件驱动刷新。

## 完成内容

1. Runtime 中影响 task list window 的 TaskStore 写路径发出 `ChangeSet::TASKS`。
2. project/worktree 上下文变化路径发出 `ChangeSet::PROJECT`。
3. TUI 监听 `AgentClient::changes()`，收到 `TASKS` 后刷新 `RuntimeModel.task_status.lines`，收到 `PROJECT` 后刷新 workspace/status bar。
4. `/clear` 保留本地立即清空，并由后续 `TASKS` change 同步空快照。

## 验收

- TUI 不再每帧无条件刷新 task status。
- Task/Project 变化通过 ChangeSet 驱动刷新。
- 用户确认完成。
