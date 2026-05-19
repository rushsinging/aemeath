# Bug #48: Output area 选中复制文本内容错位（含 CJK）

- **发现日期**：2026-05
- **归档日期**：2026-05-19
- **状态**：已确认修复
- **优先级**：高

## 症状

选中 `parallel_tool_calls` 复制出 `留**：\`parallel_tool_`，选中内容与实际文本不一致。

## 根因

screen_line_map 或 selection 的字符索引/显示列偏移计算在 CJK 宽字符或 Markdown 渲染处错位。
