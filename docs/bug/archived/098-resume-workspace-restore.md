# Bug #98：resume 时没有加载 worktree 配置

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认修复 |
| 修复 | 02046d5 |

## 症状

会话 resume 后，workspace 上下文（worktree 路径、context stack）丢失，runtime handle 仍指向主工作区而非原会话的 worktree 路径。

## 根因

`load_session_impl` 丢弃 workspace 上下文，runtime handle 未同步更新。

## 修复

resume 会话时恢复 workspace 上下文到 TUI 和 runtime handle。

## 验证

- 用户确认修复。

## 涉及路径

- `agent/features/runtime/`（session load / resume 路径）

## 关联提交

- `02046d5 fix(runtime): resume 会话时恢复 workspace 上下文到 TUI 和 runtime handle (refs #98)`
