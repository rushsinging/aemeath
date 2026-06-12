<!-- Migrated from: docs/feature/archived/018-task-list-batch.md -->
# Feature #18: Task list 跨轮次 batch 机制

- **完成日期**：2026-05
- **归档日期**：2026-05-14
- **状态**：已确认完成

## 完成内容

- Task 跟随 session 持久化，不再每次用户消息清空
- 按 batch 分组显示，新 turn 自动切换到新 batch，旧 batch 隐藏
- 已完成 task 在当前 batch 内继续显示
