# Bug #96：EnterWorktree 上下文栈与 git 实际状态不一致，导致误报"已在 worktree 中"

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认修复 |
| 修复 | 6a8228c, 8844a29, 667ef2d |

## 症状

1. 用户在 `main` 分支主工作区（`git branch --show-current` → `main`，`pwd` 不在 `.worktrees/` 下），UI 显示也不在 worktree 中。
2. 调用 `EnterWorktree { branch: "feature/xxx" }`（不给 `path`，走自动创建模式）时报错：`进入 worktree 失败：已在 worktree 中，请先 ExitWorktree 退出当前 worktree 再进入新的`。
3. 直接给 `path` 参数指定已存在的 worktree 路径则成功进入。

## 根因

`enter_worktree()` 将 `context_stack.is_empty()` 作为"是否在 worktree 中"的唯一判断依据，完全不校验 git 实际状态。

`context_stack` 是内存 `Arc<Mutex<Vec<WorkingContext>>>`，通过 `workspace_context_from_tool_context()` → `WorkingDirectoryChanged` 事件持久化到会话存储。会话恢复时从 `WorkspaceContext.context_stack` 还原。

触发链条：
1. Session N：EnterWorktree 成功 → context_stack.push → 会话自动持久化时栈非空
2. Session N 异常结束 / 未调用 ExitWorktree → 残留条目持久化
3. Session N+1：恢复到 main，但 context_stack 从持久化恢复后仍非空 → `enter_worktree()` 误判为"已在 worktree 中"

## 修复

`enter_worktree()` 栈非空时，通过 `git rev-parse --git-dir` 校验当前路径是否真实在 `.worktrees/` 下。若栈非空但 git 确认在主工作区，自动清理残留栈并允许进入；仅当 git 也确认在 worktree 中时才拒绝嵌套。

## 验证

- `cargo test -p project` 通过
- 用户确认修复。

## 涉及路径

- `agent/project/src/business/worktree.rs`（`enter_worktree` 的栈校验逻辑）

## 关联提交

- `6a8228c fix(project): enter_worktree() 增加 git 状态校验，防止上下文栈残留导致误判 (refs #96)`
- `8844a29 fix(project): isolate stale worktree stack test from cwd (refs #96)`
- `667ef2d test(project): 稳定 worktree 残留栈测试 (refs #96)`
