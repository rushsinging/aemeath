# Feature #29: Task reminder 被动注入

- **完成日期**：2026-05
- **归档日期**：2026-05-14
- **状态**：已确认完成

## 目标

TUI 路径下每轮扫描上一条 assistant 消息中的 TaskCreate/TaskUpdate，节流（≥5轮间隔）后注入极简 `<system-reminder>` 摘要，提醒 agent 当前 task 进度。

## 完成内容

- TUI 路径已实现被动注入
- REPL 路径暂未注入
