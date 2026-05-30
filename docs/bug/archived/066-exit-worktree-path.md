# Bug #66：ExitWorktree 带 path 参数报错"已在 worktree 中"

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-30 |
| 状态 | 已确认修复 |
| 根因类别 | 工具语义 / worktree 嵌套检查 |

## 症状

ExitWorktree 工具传入 `path` 参数时，预期行为是退出当前 worktree 后切换到指定路径，但实际报错：

```text
✗ ExitWorktree
  {"path":"/Users/guoyuqi/Nextcloud/work/claudecode/aemeath"}
  ✗ 切换路径失败：已在 worktree 中，请先 ExitWorktree 退出当前 worktree 再进入新的
```

用户想通过 `ExitWorktree(path="/some/target")` 一步切换回目标工作区，却被要求先无参 `ExitWorktree` 退出再 `cd` 目标路径，操作被迫拆为两步。

## 复现

1. 通过 `EnterWorktree` 进入任意 worktree。
2. 调用 `ExitWorktree(path="/Users/guoyuqi/Nextcloud/work/claudecode/aemeath")`。
3. 观察错误返回："已在 worktree 中，请先 ExitWorktree 退出当前 worktree 再进入新的"。
4. 先执行无参 `ExitWorktree` 成功退出 worktree，再执行同样带 path 的 ExitWorktree 或手动 cd 才能到达目标路径。

## 根因

`ExitWorktree(path)` 的处理分支未区分"仅退出"与"退出+切换"两种语义：带 `path` 参数时被内部当作 `EnterWorktree(path)` 处理，触发 worktree 嵌套检查，直接拒绝。

## 修复

- 梳理 `ExitWorktree(path)` 的执行分支：先执行无参 ExitWorktree 的退出逻辑（pop 上下文栈、恢复工作目录），再以恢复后的工作目录为基础执行 path 切换（等价于无条件 cd 目标路径，不再嵌套检查 worktree）。
- 提供 `path` 参数即代表"我要退出并切换"，单步完成。
- 补充回归：从任意 worktree 调用 `ExitWorktree(path)` 应能切换到目标路径；目标路径不存在时给出明确路径错误而非 worktree 嵌套错误。

## 相关提交

- `24e8e9c` docs: 新增 ExitWorktree path 参数报错 bug (refs #66)

## 验证

2026-05-30 用户确认 bug #66 已修复。
