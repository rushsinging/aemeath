# Bug #119：TUI tool call 空 summary 覆盖流式参数导致 Skill(?) 与 TaskCreate 缺失

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 发现日期 | 2026-06 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认修复 |
| 修复 | 52b68c7 |

## 症状

Tool call 绑定时空 summary 覆盖 ToolArgumentsDelta 收集的参数预览，导致 Skill(?) 与 TaskCreate 等工具调用在 TUI 中显示为 `Skill(?)` 或缺少参数摘要。

## 根因

`ToolCall` 绑定时空 summary 覆盖 `ToolArgumentsDelta` 收集的参数预览。

## 修复

保留 tool call 流式参数摘要，不在绑定时用空 summary 覆盖已收集的参数预览。

## 验证

- 用户确认修复。

## 涉及路径

- `apps/cli/src/tui/`（tool call 显示/绑定路径）

## 关联提交

- `52b68c7 fix(tui): 保留 tool call 流式参数摘要 (refs #119)`
