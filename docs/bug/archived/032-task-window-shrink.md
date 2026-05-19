# Bug #32: Task list 窗口化显示异常

- **发现日期**：2026-05
- **归档日期**：2026-05-19
- **状态**：已确认修复
- **优先级**：高

## 症状

Task list 窗口化显示多种异常：窗口收缩为 1-2 条、completed 排序错乱、pending 跳号、TTL 过滤导致旧 completed 被丢弃无法补齐窗口。

## 修复历程（A~F 轮）

- A~D 轮：TTL 过滤、温和扩展、下限保护、pending 排序
- E 轮：温和扩展/下限保护从未过滤 completed 回退补齐
- F 轮：`merge_completed_lines()` 按 display id 排序，修复 completed 扩展后顺序错乱

## 涉及路径

- `aemeath-cli/src/tui/app/task_window.rs`
