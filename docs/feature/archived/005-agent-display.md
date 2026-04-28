# #5 Agent 调用显示优化

**归档日期**：2026-04-27

**实现**：
- `stream.rs` 中 Agent 批处理前发送 `ToolCall` 事件，header 显示 `● Agent(desc) [role: xxx] [model: xxx]`
- Agent 结果输出用 `LineStyle::Assistant`（绿色），区分于普通工具的灰色 `System`
- `AgentProgress` 不再覆盖 header，标题保持稳定
- 删除了不再需要的 `update_agent_progress` 方法

**涉及文件**：`stream.rs`、`update.rs`、`event_handler.rs`、`tool_display.rs`
