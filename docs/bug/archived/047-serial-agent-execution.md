# Bug #47: LLM 声称派发多个 reviewer 但 Agent 实际串行执行

- **发现日期**：2026-05
- **归档日期**：2026-05-19
- **状态**：已确认修复
- **优先级**：高

## 修复

v1：execute_non_agent 并行化 + Agent tool 并行指引
v2：请求体添加 parallel_tool_calls=true 让 LLM 一次返回多个 tool calls
v3：OpenAI Compatible 流式中 tool call name delta 实时触发 on_tool_use_start（不再等流结束批量调用）
v4：ToolCall UI 事件从 agent_calls/execute_non_agent 提前到 execute_tool_round 入口统一发送
v5：流式 arguments delta 实时更新 pending 占位行（从 partial JSON 提取关键参数如 file_path、command）
commit: 0c74d82 (v3 revert), 39317bd..9b86f77 (v4), bb8bb25 (v5)
