# 活动中 Bug

| # | 标题 | 优先级 | 状态 | 发现日期 | 根因类别 |
|---|------|--------|------|----------|----------|
| 2 | 代码块灰色背景导致内容不可读 | 中 | 待确认 | 2026-04 | 样式对比度 |
| 3 | Tool call 状态栏卡住 + 长时间 tool call 无流式输出 | 高 | 待确认 | 2026-04 | tool_call_active 未同步 + tool call 输出未流式化 |
| 4 | Output Area panic 导致进程卡死 | 高 | 活动中 | 2026-04 | catch_unwind 外 panic / 状态不一致 |
| 9 | 鼠标选中时高亮区不在鼠标位置（#5 回归） | 中 | 待确认 | 2026-04 | render 时 selection 高亮查旧 screen_line_map |
| 12 | Ask user tool call 没有询问用户 | 高 | 活动中 | 2026-04 | tool call 未拦截确认直接执行 |
| 13 | Zhipu API 超大请求体返回空响应 | 高 | 待确认 | 2026-04 | body 过大时 API 返回 input_tokens=0 output_tokens=0 |

## 详情

### #1 Resume 时 Markdown 渲染换行丢失（已归档）
**症状**：Session resume 后 assistant 多段落文本连成一块，streaming 路径正常。
**根因**：`render_history_message` 未 split `\n`，`sanitize_for_display` strip 了换行符。
**修复**：`text.lines()` 逐行 push。
**关联**：路径不一致——streaming 做了 split，resume 没做。

### #2 代码块灰色背景导致内容不可读
**症状**：代码块 `bg(DarkGray) + fg(White) + Dim`，深色终端上看不清。
**根因**：之前 fence 扫描器从未成功触发（整块文本 `trim().starts_with("```")` 返回 false），#1 修复后首次触发，暴露了样式问题。
**修复**：改为 `bg(Rgb(40,44,52)) + fg(Rgb(171,178,191))`（One Dark 色系）。
**关联**：依赖于 #1 的修复。

### #3 Tool call 状态栏卡住 + 长时间 tool call 无流式输出
**症状**：
1. 模型完成 tool call 后重新开始 thinking 时，状态栏仍显示 "Calling xxx..."，thinking 文本不显示。
2. 长时间 tool call（如文件搜索、代码分析）执行期间 TUI 无任何输出，执行完毕后才一次性显示所有结果。应改为：先输出 tool call 标题 → 执行过程中流式输出中间结果 → 完成后输出最终结果。

**根因**：
- 症状 1：`UiEvent::Thinking` 在 `tool_call_active == true` 时被跳过，导致 thinking 文本被丢弃、`tool_call_active` 未重置、状态栏未清除。`ToolResult` 虽然设置了 `tool_call_active = false`，但多轮 tool call 场景下状态栏显示不正确。
- 症状 2：tool call 执行期间输出被缓冲，未在 streaming 过程中逐步渲染到 TUI。

**修复**：
- 症状 1：`Thinking` 事件无论 `tool_call_active` 状态都正常显示 thinking 文本，若为 true 则同步重置并更新状态栏。`Text` 事件同理。
- 症状 2：需要将 tool call 输出改为流式——收到 tool call 开始事件时立即渲染标题，执行过程中逐步渲染中间输出，收到 tool result 时渲染最终结果。

**涉及路径**：`aemeath-cli/src/tui/app/event_handler.rs`、`processing.rs`

### #4 Output Area panic 导致进程卡死
**症状**：Output area 渲染时触发 panic，TUI 进程卡死无响应，需 kill。
**根因**：待调查。可能方向：screen_line_map 索引越界、CharIdx 运算溢出、wrap 计算与 screen_line_map 不一致、catch_unwind 捕获后状态不一致导致后续渲染死循环。
**关联**：可能与 #5（screen_line_map 重构）有关。

### #9 鼠标选中时高亮区不在鼠标位置（#5 回归）
**症状**：在 output area 中鼠标选中文字时，高亮区不从鼠标位置开始，有偏移。与已归档 #5 症状相同。
**根因**：#5 的修复（`split_off` 同步裁剪）仍在，但引入了新问题。`render()` 中构建 `new_screen_map` 和渲染行同时进行，渲染 selection 高亮时（`render_line_with_selection` / `render_spans_with_selection`）查的是**旧的** `self.screen_line_map`（第 350-354 行、第 436-439 行），而 `screen_idx` 基于当前帧的 `new_screen_map` 索引。两帧之间若有内容变化（streaming、新消息），索引不对应，导致高亮位置偏移。
**修复方向**：在构建 `new_screen_map` 后、渲染 selection 高亮前，先将 `self.screen_line_map` 更新为 `new_screen_map`，或改为直接使用 `new_screen_map` 查询。
**涉及路径**：`aemeath-cli/src/tui/output_area/mod.rs` render() 方法

### #12 Ask user tool call 没有询问用户
**症状**：模型调用 ask user 类 tool call 时，TUI 未弹出确认对话框或等待用户输入，直接执行并返回结果，用户无机会干预。
**根因**：tool call 执行流程未对 ask user 类请求做拦截确认，直接走普通 tool call 处理路径。
**修复方向**：在 tool call 执行前检测是否为 ask user 类型，若是则暂停执行、弹出确认 UI，等待用户响应后再继续。
**涉及路径**：`aemeath-cli/src/tui/app/processing.rs`、`event_handler.rs`

### #13 Zhipu API 超大请求体返回空响应
**症状**：会话 0000019dc93bab86dfd7032f 中，多轮 tool call 后模型停止输出，TUI 无内容显示。API 返回 `stop_reason=EndTurn` 但 `input_tokens=0 output_tokens=0`，text 为空字符串，无 tool calls。
**根因**：请求体过大（`body_bytes=11659080` 约 11MB），Zhipu GLM-5.1 API 在收到超大请求时静默返回空响应，不报错。compact 后 messages 从 62 降到 23，但 body 仍约 11MB，说明某条 tool result 包含极大内容（可能是文件搜索/读取返回了大量数据），compaction 未能有效压缩。
**修复方向**：
1. 发送前检测 body size，超过阈值时对超大 tool result 做截断或摘要
2. compaction 阶段主动截断过长的 tool result 内容
3. 检测到 `input_tokens=0 output_tokens=0` 的空响应时，视为 API 错误并重试或提示用户
**涉及路径**：`aemeath-core/src/compact/`、stream 发送逻辑

---

# 已归档 Bug

### #5 鼠标选中时位置错位（已确认修复）
**症状**：对话后在 output area 中鼠标选中文字时，选择起点不在点击位置，有偏移。
**根因**：`lines` 滚动裁剪（`skip(offset)`）时，`screen_line_map` 未同步裁剪。鼠标点击用 `rel_row` 索引 `screen_line_map`，但滚动后可见行 0 对应的是 `screen_line_map[offset]` 而非 `screen_line_map[0]`。
**修复**：在 `lines` 裁剪的同时，用 `screen_line_map.split_off(offset)` 同步裁剪前缀。

### #6 /think 命令无法自动补全（已修复）
**症状**：输入 `/t` 时自动补全列表不显示 `/think`。
**根因**：硬编码 handler 未注册到 CommandRegistry。
**修复**：命令系统改用 inventory 自动注册。

### #7 Tool call 行选中不可点击（已修复）
**根因**：screen_line_map 遗漏 tool call 行。

### #8 字符串索引字节/字符混淆（已修复）
**根因**：字符串索引强类型化。

### #10 Markdown 渲染：部分未渲染 + 选中后回退为源码（已修复）
**修复**：TUI output 重构为 TEA 架构，根本性地解决了 screen_line_map 偏移不一致问题。

### #11 Markdown 未渲染 Table（已修复）
**症状**：模型返回的 Markdown 表格（`| col1 | col2 |`）在 TUI 中直接显示为纯文本管道符，无表格渲染。
**根因**：`inline_markdown_spans` 未实现 table 语法解析，`|` 和 `---` 分隔行被当作普通文本处理。
**修复**：在 `markdown.rs` 添加 `is_table_separator`、`is_table_row`、`parse_table_cells`、`render_table_block` 函数。在 `mod.rs` render() 中添加表格块预扫描（类似 code block），检测连续 `| ... |` 行为表格，预渲染为 box-drawing 字符表格（`│`、`┼`、`─`），表头粗体，分隔行用 `DarkGray` 色边框。
**涉及路径**：`aemeath-cli/src/tui/output_area/markdown.rs`、`mod.rs`
