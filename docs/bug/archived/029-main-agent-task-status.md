# Bug #29: 主 agent tool call 执行后 task list 状态不更新

- **发现日期**：2026-05
- **归档日期**：2026-05-14
- **状态**：已确认修复
- **优先级**：高

## 症状

主 agent tool call 执行后，task list 状态不更新。

## 根因

system prompt 引用不存在的 TodoWrite/TodoRun，缺少 TaskUpdate 强约束。

## 涉及路径

- `aemeath-core/src/prompt.rs`
- `aemeath-tools/src/task_update.rs`
