# 活动中 Bug

| # | 标题 | 优先级 | 状态 | 确认结果 | 发现日期 | 根因类别 |
|---|------|--------|------|----------|----------|----------|
| 13 | Zhipu API 超大请求体返回空响应 | 高 | 待确认 | 未确认 | 2026-04 | body 过大时 API 返回 input_tokens=0 output_tokens=0 |
| 25 | /clear 命令未清空 status line 数据 | 中 | 待确认 | 未确认 | 2026-04 | clear 仅重置消息历史，未联动重置 status bar 任务/成本/spinner 状态 |

## 详情

### #1 Resume 时 Markdown 渲染换行丢失（已修复）
**症状**：Session resume 后 assistant 多段落文本连成一块，streaming 路径正常。
**根因**：`render_history_message` 未 split `\n`，`sanitize_for_display` strip 了换行符。
**修复**：`text.lines()` 逐行 push。
**关联**：路径不一致——streaming 做了 split，resume 没做。

### #2 代码块灰色背景导致内容不可读（已修复）
**症状**：代码块 `bg(DarkGray) + fg(White) + Dim`，深色终端上看不清。
**根因**：之前 fence 扫描器从未成功触发（整块文本 `trim().starts_with("```")` 返回 false），#1 修复后首次触发，暴露了样式问题。
**修复**：改为 `bg(Rgb(40,44,52)) + fg(Rgb(171,178,191))`（One Dark 色系）。
**关联**：依赖于 #1 的修复。

### #13 Zhipu API 超大请求体返回空响应
**症状**：会话 0000019dc93bab86dfd7032f 中，多轮 tool call 后模型停止输出，TUI 无内容显示。API 返回 `stop_reason=EndTurn` 但 `input_tokens=0 output_tokens=0`，text 为空字符串，无 tool calls。
**根因**：请求体过大（`body_bytes=11659080` 约 11MB），Zhipu GLM-5.1 API 在收到超大请求时静默返回空响应，不报错。compact 后 messages 从 62 降到 23，但 body 仍约 11MB，说明某条 tool result 包含极大内容（可能是文件搜索/读取返回了大量数据），compaction 未能有效压缩。
**修复方向**：
1. 发送前检测 body size，超过阈值时对超大 tool result 做截断或摘要
2. compaction 阶段主动截断过长的 tool result 内容
3. 检测到 `input_tokens=0 output_tokens=0` 的空响应时，视为 API 错误并重试或提示用户
**涉及路径**：`aemeath-core/src/compact/`、stream 发送逻辑

### #25 /clear 命令未清空 status line 数据
**症状**：在 TUI 中执行 `/clear` 命令清空对话历史后，output area 的消息已经被清空，但底部 **status line / status bar** 仍然显示上一轮残留信息：
- task 汇总（`✓` / `■` / `□` + subject 行）
- cost / token 用量数字
- spinner 状态或 "Generating..." / "Calling xxx..." 文本
- 当前 tool call 标题
- 等等（具体哪些字段残留待复现确认）

用户预期 `/clear` 不仅清空消息列表，也应该把状态栏复位回初始空白态（保留模型/provider 等环境信息，清掉本会话累计的运行态）。

**根因方向**：待调查。可能方向：
1. `/clear` 命令的 handler（`aemeath-core/src/command/commands/` 或 TUI 层）只调用了 `output_area.clear()` 之类的清空消息接口，未联动调用 `status_bar` / `App` 的状态重置方法
2. status bar 的状态字段（task list、cost、active tool calls、spinner 状态）作为 App / TUI state 的独立字段持有，没有统一的 "reset" 入口
3. cost / token 累计可能是 session 级状态（与 `Session` 结构体绑定），`/clear` 是否应该也重置 session 累计？需决策（一般 `/clear` 只清显示，不动 session 累计；但 task 状态应该清）

**修复方向**：
1. 定位 `/clear` 命令实现：搜 `commands/clear` / `CommandAction::Clear` / `output_area.clear`
2. 列出 status bar 当前展示的所有动态字段，明确哪些应在 `/clear` 时复位：
   - 必须复位：active tool calls、当前 tool call 名、spinner 状态、task summary 行
   - 可选复位：cost / token 累计（建议保留，因为是 session 级数据）
   - 不复位：model、provider、cwd 等环境信息
3. 在 App 中暴露统一的 `reset_runtime_state()`，由 `/clear` 调用
4. 测试场景：执行多轮对话产生 task / tool call / spinner → `/clear` → 状态栏除模型信息外应全部清空

**涉及路径**：
- `aemeath-core/src/command/commands/`（`/clear` 命令实现）
- `aemeath-cli/src/tui/app/update.rs`（`CommandAction::Clear` 分支处理）
- `aemeath-cli/src/tui/app/mod.rs`（App 状态字段）
- `aemeath-cli/src/tui/status_bar.rs`（status bar 渲染来源）

**关联**：Bug #24（spinner 生命周期）— 复位逻辑需考虑 active tool call set 一并清空

**涉及路径**：`aemeath-cli/src/tui/app/mod.rs`、`aemeath-cli/src/tui/app/update.rs`、`aemeath-cli/src/tui/output_area/mod.rs`

---

# 已归档 Bug

### #3 优化 tool call TUI 显示（已确认修复）
**根因**：`UiEvent::Thinking` 在 `tool_call_active == true` 时被跳过；`UiEvent::AgentProgress` 字段错写为 `_tool_id` / `_text` 被忽略；sub-agent tool call 名称延迟发送。
**修复**：Thinking/Text 事件在 tool_call_active 时正常显示并重置；AgentProgress 改用 `tool_id` / `text` 并渲染到对应 tool call 下方；sub-agent 在执行前先发送 `[Turn N] calling: ...`；tool 执行改为顺序发送 `ToolResult`。详见 `docs/bug/archived/003-tool-call-tui-display.md`。

### #24 Tool call 执行时 spinner 偶尔消失（已确认修复）
**根因**：`tool_call_active` 单 bool 无法表达批量并发；output area 临时行裁剪优先保留底部 task 行挤出 spinner。
**修复**：改用 `active_tool_call_ids: HashSet<String>` 跟踪未完成 tool；渲染顺序改为 queued → task status → spinner；`ToolResult` 当 remaining=0 时调用 `start_spinner()`。详见 `docs/bug/archived/024-spinner-disappears-during-tool-call.md`。

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

### #4 Output Area panic 导致进程卡死（已修复）
**症状**：Output area 渲染时触发 panic，TUI 进程卡死无响应，需 kill。
**根因**：screen_line_map 索引越界 / CharIdx 运算溢出 / wrap 计算与 screen_line_map 不一致。
**修复**：TUI output 重构为 TEA 架构后根本性解决。

### #14 Tool call 标题可选中但无法复制（已修复）
**症状**：选中 tool call 标题行可高亮但复制时拿不到文本。
**根因**：`copy_selection` 实现未处理 tool call 行（LineKind 非 Text 分支被跳过）。
**修复**：所有可见+高亮内容均纳入剪贴板路径。

### #15 resume 和 session 命令在 TUI 中表现不对（已修复）
**症状**：`--resume` 和 `sessions` 子命令行为异常。
**根因**：CLI 重构为 subcommand 架构后，resume/session 路径未正确接入 TUI 启动流程。
**修复**：`--resume` 参数正确传递到 `run_chat()` → TUI 启动，sessions 子命令输出格式修正。

### #16 /resume 会话列表行字符被吞（已修复）
**症状**：`/resume` 弹出的自动补全建议列表中，CJK 字符被吞（如"分析"→"分"，"feature"→"eature"）。
**根因**：`render_suggestions_in_area` 用屏幕列号当字符索引（`chars().nth(x_usize)`）逐字符写入 buf，CJK 宽字符占 2 列导致字符跳过；截断用 `text.len()`（字节长度）而非 unicode 显示宽度。
**修复**：按显示宽度遍历字符写入 buf，CJK 字符占多列时正确填充后续 cell；截断改用 `truncate_unicode_width`。
**涉及路径**：`aemeath-cli/src/tui/input_area.rs` `render_suggestions_in_area()`

### #12 Ask user tool call 没有询问用户（已修复）
**症状**：模型调用 ask user 类 tool call 时，TUI 未弹出确认对话框或等待用户输入，直接执行并返回结果。
**根因**：tool call 执行流程未对 ask user 类请求做拦截确认，直接走普通 tool call 处理路径。
**修复**：在 tool call 执行前检测 ask user 类型，拦截后弹出确认 UI 等待用户响应。
**涉及路径**：`aemeath-cli/src/tui/app/processing.rs`、`event_handler.rs`

### #17 对话进行中 input area 无法 Ctrl/Cmd+V 粘贴
**状态**：✅ 已修复
**根因**：`update.rs` 中 `Msg::Paste` 事件在 `is_processing == true` 时直接丢弃
**修复**：processing 态下 Paste 分支：文本粘贴插入 input area + 入 queued_messages queue；空粘贴尝试剪贴板图片；图片路径粘贴加载图片
**涉及路径**：`aemeath-cli/src/tui/app/update.rs`

### #19 AskUserQuestion 等待输入时用户输入被加入 input queue（已修复）
**症状**：AskUserQuestion 无选项（自由输入模式）时，Enter 走了 input queue 路径而非 reply_tx。
**根因**：`update_key` 中 `ask_user_reply_tx` 设置后，Enter 键处理未检查该状态，直接命中 `is_processing` 分支入队。Paste 同理。
**修复**：在 `ask_user_state` 检查之后增加 `ask_user_reply_tx.is_some()` 分支，Enter 时直接 `reply_tx.send()`；Paste 时也跳过入队逻辑。
**涉及路径**：`aemeath-cli/src/tui/app/update.rs`

### #18 tool call 期间 spinner 偶发消失（已修复）
**根因**：`ToolCallStart` 事件不启动 spinner（只有 `ToolCall` 才启动），但 `Text/Thinking` 事件会 stop_spinner。当 streaming 文本结束后 LLM 发出 tool call，spinner 被 Text stop 后到 ToolCall start 之间有窗口期，期间 spinner 不可见。
**修复**：`ToolCallStart` 也调用 `start_spinner()`，确保 tool call 一识别到就显示 spinner。同时在 `start_spinner/stop_spinner` 添加 debug 日志便于追踪。
**涉及路径**：`aemeath-cli/src/tui/app/update.rs`、`event_handler.rs`、`output_area/spinner.rs`

### #9 鼠标选中时高亮区不在鼠标位置（已修复）
**根因**：`selection_start/selection_end` 存储的是 `screen_line_map` 的行索引（screen_row），但 `screen_line_map` 在每次 `render()` 时重建。当 streaming 新内容追加后，screen_map 偏移，旧索引指向错误位置，导致高亮偏移。
**修复**：selection 改为存储逻辑行索引（logic_idx）而非屏幕行索引。渲染时通过 screen_map 查找当前 screen_idx 对应的 logic_idx 来匹配选中范围，不再受 screen_map 重建影响。
**涉及路径**：`aemeath-cli/src/tui/output_area/selection.rs`、`mod.rs`
