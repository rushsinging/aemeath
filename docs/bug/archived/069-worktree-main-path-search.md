# Bug #69：worktree 中 LLM 仍尝试搜索主分支路径

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-06-02 |
| 状态 | 已确认修复 |
| 修复 | d865d6d, 3406ce8 |

## 症状

进入 worktree 后，LLM 调用 `Glob` / `Grep` / `Read` 等工具时仍可能传入 main 工作区的绝对路径，触发 workspace 边界保护错误，并可能反复重试同一个越界路径。

## 根因

静态 system prompt 中写入具体 `Current workspace root`，会在会话中途 `EnterWorktree` / `ExitWorktree` 后过期，反而误导 LLM 复用 main checkout 路径。当前 workspace 的实时状态源应是执行中的 workspace context（`path_base` / `working_root` / context stack）和 worktree 工具结果。

## 修复

1. 静态 system prompt 去掉具体 `Current workspace root`，只保留通用路径规则：优先使用相对路径；绝对路径必须位于当前 workspace；不要复用其他 checkout/main/worktree/历史会话中的绝对路径。
2. `EnterWorktree` / `ExitWorktree` 成功结果统一输出当前 `path_base`、`working_root`、分支和后续路径使用规则，让 LLM 通过 tool result 获取最新 workspace context。
3. 文件/搜索工具的越界错误继续提供恢复建议，引导下次使用相对路径或当前 workspace。

## 验证

- 静态 prompt 不再包含固定 workspace root 的回归测试。
- worktree tool result 包含 `path_base` / `working_root` 与路径提示的回归测试。
- 用户确认修复。
