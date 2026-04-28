# #6 Task 调用显示优化

**归档日期**：2026-04-27

**实现**：
- TaskCreate/TaskUpdate 脱离 `skip_ui`，发送 UI 事件展示关键信息
- TaskList 结果格式化为状态表格
- Task 生命周期状态变更可视化（completed 绿色、in_progress spinner、TaskStop 黄色）

**涉及文件**：`tool_display.rs`、`stream.rs`
