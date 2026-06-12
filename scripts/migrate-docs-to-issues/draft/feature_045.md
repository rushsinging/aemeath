<!-- Migrated from: docs/feature/archived/045-enter-exit-worktree-tools.md -->
# Feature #45：为 LLM 提供 EnterWorktree / ExitWorktree 工具

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 完成日期 | 2026-05 |
| 归档日期 | 2026-05-24 |
| 状态 | 已确认完成 |

## 需求

新增显式 worktree 上下文切换工具，让 LLM 可调用 EnterWorktree 进入指定 git worktree 并切换 cwd/path_base/working_root，完成后调用 ExitWorktree 恢复原工作区，避免依赖 Bash cd 隐式切换。

## 实现结果

1. 新增 `EnterWorktree` 工具：进入指定 git worktree，将 ToolContext 的 cwd、path_base、working_root 和相关安全边界切换到该 worktree 根目录。
2. 新增 `ExitWorktree` 工具：退出当前 worktree，恢复进入前的工作区上下文，或按参数显式切换回指定路径。
3. 进入前校验目标路径属于当前 git 仓库 worktree。
4. 工具结果展示当前工作根、git branch、repo root，便于用户确认。
5. 系统提示中要求需要在 worktree 中修改、验证、提交时优先使用 `EnterWorktree`，完成后使用 `ExitWorktree`。
6. 与 #43 复用同一套 cwd/path_base/working_root 更新逻辑。

## 验证

2026-05-24 用户确认 feature #45 已完成。活动列表中移除 #45，并保留此归档记录。
