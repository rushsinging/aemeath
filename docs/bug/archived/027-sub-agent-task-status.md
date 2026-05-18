# Bug #27: Sub-agent 已执行 tool call 但 task list 状态不更新

- **发现日期**：2026-05
- **归档日期**：2026-05-14
- **状态**：已确认修复
- **优先级**：高

## 症状

Sub-agent 执行完 tool call 后，TUI 中 task list 状态不更新。

## 根因

AgentTool::call() 未读取 taskId 参数，未在 run_agent 前后管理 task 状态转换；sub-agent TaskStore 与父隔离。

## 涉及路径

- `aemeath-tools/src/agent.rs`
- `aemeath-core/src/task.rs`
