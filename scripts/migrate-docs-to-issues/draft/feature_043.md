<!-- Migrated from: docs/feature/archived/043-worktree-cwd-context.md -->
# Feature #43：在 git worktree 中工作时 cwd 应设置为 worktree 目录

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 完成日期 | 2026-05 |
| 归档日期 | 2026-05-24 |
| 状态 | 已确认完成 |

## 需求

当切换到 git worktree 后，工具上下文的 cwd、path_base、working_root 与安全边界应同步到当前工作根，避免文件工具、搜索、构建和提交误作用于 main 工作区。

## 实现结果

1. `ToolContext` 增加可更新的 `working_root`。
2. Bash 执行结束同步 `$PWD` 时更新 `path_base`。
3. 通过 `git rev-parse --show-toplevel` 推导当前 checkout/worktree 根作为安全边界。
4. Read/Edit/Write/Glob/Grep/LSP 改用当前 `path_base` 解析相对路径，并以当前 `working_root` 校验。
5. Agent scope 与子代理系统提示使用当前 `path_base`。
6. HookRunner 支持更新项目目录；hook 执行时 `AEMEATH_PROJECT_DIR`、`CLAUDE_PROJECT_DIR`、`{AEMEATH_PROJECT_DIR}`、`{CLAUDE_PROJECT_DIR}` 占位符和进程 cwd 均使用当前项目目录。
7. TUI 状态栏在 Bash/Agent 改变工作目录后刷新当前工作目录显示。

## 验证

2026-05-24 用户确认 feature #43 已完成。活动列表中移除 #43，并保留此归档记录。
