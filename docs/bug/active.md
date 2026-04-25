# 活动中 Bug

| # | 标题 | 优先级 | 状态 | 发现日期 | 根因类别 |
|---|------|--------|------|----------|----------|
| 1 | Resume 时 Markdown 渲染换行丢失 | 高 | 待回归 | 2026-04 | 路径不一致 |
| 2 | 代码块灰色背景导致内容不可读 | 中 | 待回归 | 2026-04 | 样式对比度 |
| 3 | Tool call 标题行无法鼠标选中 | 高 | 已修复 | 2026-04 | screen_line_map 遗漏 |
| 4 | Tool call 后 Thinking 状态栏卡在 "Calling xxx..." | 高 | 已修复 | 2026-04 | tool_call_active 未同步 |

## 详情

### #1 Resume 时 Markdown 渲染换行丢失
**症状**：Session resume 后 assistant 多段落文本连成一块，streaming 路径正常。
**根因**：`render_history_message` 未 split `\n`，`sanitize_for_display` strip 了换行符。
**修复**：`text.lines()` 逐行 push。
**关联**：路径不一致——streaming 做了 split，resume 没做。

### #2 代码块灰色背景导致内容不可读
**症状**：代码块 `bg(DarkGray) + fg(White) + Dim`，深色终端上看不清。
**根因**：之前 fence 扫描器从未成功触发（整块文本 `trim().starts_with("```")` 返回 false），#1 修复后首次触发，暴露了样式问题。
**修复**：改为 `bg(Rgb(40,44,52)) + fg(Rgb(171,178,191))`（One Dark 色系）。
**关联**：依赖于 #1 的修复。

### #3 Tool call 标题行无法鼠标选中
**症状**：`●`/`✓`/`✗` 打头的 tool call 行无法点击选中，其他行正常。
**根因**：三个 tool call 渲染分支通过 `return` 提前退出，跳过了 `screen_line_map` 构建。
**修复**：将 `char_offsets` + `screen_line_map.push` 提前到所有分支之前。
**关联**：`screen_line_map` 是 selection 的定位依据。

### #4 Tool call 后 Thinking 状态栏卡在 "Calling xxx..."
**症状**：模型完成 tool call 后重新开始 thinking 时，状态栏仍显示 "Calling xxx..."，thinking 文本不显示。
**根因**：`UiEvent::Thinking` 在 `tool_call_active == true` 时被跳过，导致状态未同步。
**修复**：`Thinking` 和 `Text` 事件中检测 `tool_call_active`，若为 true 则重置并更新状态栏。
**关联**：`event_handler.rs` tool_call_active 状态管理。
