# 活动中 Bug

| # | 标题 | 优先级 | 状态 | 发现日期 | 根因类别 |
|---|------|--------|------|----------|----------|
| 1 | Resume 时 Markdown 渲染换行丢失 | 高 | 待回归 | 2026-04 | 路径不一致 |
| 2 | 代码块灰色背景导致内容不可读 | 中 | 待回归 | 2026-04 | 样式对比度 |
| 4 | Tool call 后 Thinking 状态栏卡在 "Calling xxx..." | 高 | 待修复 | 2026-04 | tool_call_active 未同步 |
| 5 | 鼠标选中时位置错位 | 中 | 已修复 | 2026-04 | screen_line_map 一致性 |
| 6 | Output Area panic 导致进程卡死 | 高 | 待修复 | 2026-04 | catch_unwind 外 panic / 状态不一致 |
| 7 | 新增的 /think 命令无法自动补全 | 中 | 已修复 | 2026-04 | 硬编码 handler 未注册到 CommandRegistry |

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

### #4 Tool call 后 Thinking 状态栏卡在 "Calling xxx..."
**症状**：模型完成 tool call 后重新开始 thinking 时，状态栏仍显示 "Calling xxx..."，thinking 文本不显示。
**根因**：`UiEvent::Thinking` 在 `tool_call_active == true` 时被跳过，导致状态未同步。
**修复**：`Thinking` 和 `Text` 事件中检测 `tool_call_active`，若为 true 则重置并更新状态栏。
**关联**：`event_handler.rs` tool_call_active 状态管理。

### #5 鼠标选中时位置错位
**症状**：对话后在 output area 中鼠标选中文字时，选择起点不在点击位置，有偏移。非必现。
**根因**：`lines` 滚动裁剪（`skip(offset)`）时，`screen_line_map` 未同步裁剪。鼠标点击用 `rel_row` 索引 `screen_line_map`，但滚动后可见行 0 对应的是 `screen_line_map[offset]` 而非 `screen_line_map[0]`。
**修复**：在 `lines` 裁剪的同时，用 `screen_line_map.split_off(offset)` 同步裁剪前缀。点渲染后处理也改用裁剪后的长度。
**关联**：关联 #3（screen_line_map 遗漏，已修复）。

### #6 Output Area panic 导致进程卡死
**症状**：Output area 渲染时触发 panic，TUI 进程卡死无响应，需 kill。
**根因**：待调查。可能方向：screen_line_map 索引越界、CharIdx 运算溢出、catch_unwind 未覆盖的路径。
**关联**：可能与 #3/#5 涉及的 screen_line_map 重构有关。
