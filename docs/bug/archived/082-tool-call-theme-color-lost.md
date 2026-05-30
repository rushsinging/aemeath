# Bug #82：TUI 渲染 tool call 时丢失 theme 颜色

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-30 |
| 状态 | 已确认修复 |
| 根因类别 | TUI 渲染 / tool call 标题 span 颜色 |

## 症状

#58 渲染管线重构后，TUI 中 tool call（如 Bash/Grep/Read 等）的标题、参数、状态指示器以默认前景色显示，缺少原有的 theme 颜色（如工具名高亮色、运行态动画色、完成态颜色等），所有工具调用看起来像纯文本，无视觉区分。

## 根因

新渲染管线中 `render_tool_call` 已按 `ToolCallBlockView.style` 给状态 icon 应用语义状态色（Running/Success/Error 等），但工具标题 span 固定使用 `theme::TEXT`。因此 `●`/`✓` 仍有颜色，`Bash`/`Grep`/`Read(...)` 等工具名和标题看起来像普通文本，造成 tool call theme 颜色丢失。

## 修复

将 tool call header 标题 span 的前景色从 `theme::TEXT` 改为与 icon 一致的 `icon_color`，即由 `semantic_color(view.style)` 派生。running/success/error/cancelled/orphaned 等状态下，状态指示器和工具标题共享对应 theme 颜色。

## 回归测试

1. `test_tool_call_running_applies_theme_color_to_icon_and_title`
2. `test_tool_call_success_uses_success_icon_color`

## 相关提交

- `2f62a4d` fix(tui): 修复 tool call 标题颜色丢失 (refs #82)

## 验证

2026-05-30 用户确认 bug #82 已修复。
