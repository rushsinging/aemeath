<!-- Migrated from: docs/feature/archived/065-resume-input-history.md -->
# Feature #65: Resume 模式输入历史 — 上下键翻阅过往输入

**状态**：✅ 已完成（2026-05-30 用户确认）

**优先级**：中

## 背景

Resume 模式恢复历史会话后，输入框 `InputModel.history` 为空，用户无法通过上/下键翻阅该会话中曾经发送过的输入，体验与 shell history 行为不一致。

## 解决方案

- 会话 resume 加载完成后，从已 load 的 session messages 中提取所有 user 文本消息，按顺序写入 `InputModel.history`。
- 输入框继续复用既有 `MoveHistoryPrevious` / `MoveHistoryNext` 上下键逻辑翻阅历史，并保留当前草稿恢复行为。
- 新增 `ReplaceHistory` intent，承担"批量替换历史"语义，避免与现有 push/trim 路径耦合。
- 补充单元测试：历史替换/翻阅行为、resume 用户输入提取逻辑。
- 后续在 `4daee99` 合并 `bug/feature-65-not-effective` 时修复实际生效问题（commit `06fd085`，refs #92 #65）。

## 相关提交

- `3f7d30f` feat(tui): Resume 模式加载用户输入历史 (refs #65)
- `1d1be2b` Merge feature/65-resume-input-history
- `99d0d55` docs(feature): 更新 Resume 输入历史状态 (refs #65)
- `06fd085` fix(tui): 修复 --resume 未加载输入历史 (refs #92 #65)
- `4daee99` Merge bug/feature-65-not-effective

## 验证

- `cargo test -p cli`
- `cargo clippy -p cli -- -D warnings`
- `.agents/hooks/check-architecture-guards.sh`
