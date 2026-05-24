# Feature #46：TUI status line 增加第二行并显示 cwd/current worktree

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 完成日期 | 2026-05 |
| 归档日期 | 2026-05-24 |
| 状态 | 已确认完成 |

## 需求

TUI status line 增加第二行，用于展示当前真实工作路径、root、git/worktree、权限模式和完整 session，让用户能明确判断当前工具实际会在哪个目录或 worktree 中执行。

## 实现结果

1. status line V2 已按重新规划实现。
2. 第一行展示状态、模型、token in/out、t/s、ctx%、API calls。
3. 第一行不再显示 cost/session。
4. 第二行展示真实路径，路径保留 `~` 或 `/` 前缀。
5. 仅当路径不一致时展示 root。
6. 第二行展示 git/worktree、权限模式和完整 session。
7. 第二行改为语义化 span 渲染，路径、git、权限、session 使用不同视觉权重。
8. 窄屏时优先保留真实路径前缀、权限和 session。
9. 与 #43/#45 的工作上下文数据源联动。

## 验证

2026-05-24 用户确认 feature #46 已完成。活动列表中移除 #46，并保留此归档记录。
