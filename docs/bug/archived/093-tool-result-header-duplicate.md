# Bug #93：TUI 工具结果块内重复显示工具名和图标

| 字段 | 值 |
|------|-----|
| 优先级 | 低 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-30 |
| 状态 | 已确认修复 |
| 根因类别 | TUI 渲染 / Edit DIFF 路径重复 header |

## 症状

工具结果子块在 TUI 中重复显示工具名和图标，例如 `Edit /path/to/file` 行在 diff 视图前后出现两次。

## 根因

Edit DIFF 渲染路径在子块结果体内额外打印一行工具 header（`Edit <path>`），与外层 tool_call header 重复。

## 修复

去除 Edit DIFF 路径在结果体内的 header 行，工具名仅由外层 `render_tool_call` 的 header span 渲染。

## 相关提交

- 与 #90、#94 一同在 G2/Edit 渲染重构中收口（具体 commit 见 #90 archive）

## 验证

2026-05-30 用户确认 bug #93 已修复。
