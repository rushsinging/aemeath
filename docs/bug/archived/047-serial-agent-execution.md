# Bug #47: LLM 声称派发多个 reviewer 但 Agent 实际串行执行

- **发现日期**：2026-05
- **归档日期**：2026-05-19
- **状态**：已确认修复
- **优先级**：高

## 修复

v1：execute_non_agent 并行化 + Agent tool 并行指引
v2：请求体添加 parallel_tool_calls=true 让 LLM 一次返回多个 tool calls
