# Bug #68：TUI 丢失 context window 用量显示

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-25 |
| 状态 | 已确认修复 |

## 症状

TUI 中此前可见的 context window 用量显示（如「已用 tokens / 上下文上限 / 剩余比例」）不再出现，用户无法直观判断当前消息量距离自动压缩阈值还有多远。

## 根因

`apps/cli/src/run_orchestration.rs` 中调用 `app.run()` 时 `context_size` 参数硬编码为 `0`。`session_lifecycle.rs` 会调用 `self.status_bar.set_context_size(context_size as u64)`，而 `status_bar.rs` 的渲染条件为 `if self.context_size > 0`，因此 `0` 导致 ctx% 永远不渲染。

## 修复

读取 `resolved_model.model.context_window` 传给 `app.run()`，替代硬编码的 `0`。

## 验证

2026-05-25 用户确认 bug #68 已修复。活动列表中移除 #68，并保留此归档记录。
