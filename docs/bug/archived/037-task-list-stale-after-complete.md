# Bug #37: Task list 全部完成后切换对话仍显示旧 task

- **发现日期**：2026-05
- **归档日期**：2026-05-14
- **状态**：已确认修复
- **优先级**：中

## 症状

当前 batch 所有 task 已 completed，但下一轮新用户消息开始时未清空/隐藏旧 task list。

## 修复

TaskListComplete 后自动归档当前 batch，新用户消息触发新 batch 时旧 task list 不再显示。
