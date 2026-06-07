# 活动中 Bug

> 排序规范：表格行和详情区块均按 ID 升序排列。

| # | 标题 | 优先级 | 状态 | 确认结果 | 发现日期 | 根因类别 |
|---|------|--------|------|----------|----------|----------|
| 111 | LLM 输出长行被截断，TUI 只显示到屏幕宽度即断行消失 | 中 | 待确认 | 待用户确认 | 2026-06 | TUI 长行已自动换行；本轮继续将输出文档宽度额外缩小 2 列，增加正文与 scrollbar 的右侧安全留白 |
| 112 | TUI 输出区更新滞后 | 中 | 待确认 | 待用户确认 | 2026-06 | UiEvent 每个 chunk 同步刷新拖慢主循环；已改为 dirty 标记+按帧批量刷新 |
| 121 | Spinner verb 与 pharse_text 计时显示相同 | 中 | 待确认 | 待用户确认 | 2026-06 | 阶段计时已独立；本轮修正 CallingTool 名称变化不重置 phase_frame 的漏修 |
| 122 | Tool gutter marker 静态且 summary 等 result 才出现 | 高 | 待确认 | 待用户确认 | 2026-06 | running marker 未消费动画帧；header 只看 summary，忽略 ToolArgumentsDelta 已更新的 args_preview |

### #111 LLM 输出长行被截断，TUI 只显示到屏幕宽度即断行消失

**状态**：待确认

**症状**：LLM 输出长行在 TUI 中曾只显示到屏幕宽度，超出部分不可见；首轮修复后长行可换行，但正文右侧尽头仍过于靠近 output area 的 scrollbar。

**根因**：输出文档渲染宽度使用 output area 宽度减去固定预留列。原预留列覆盖边框/scrollbar 后，正文与 scrollbar 之间视觉留白不足。

**修复**：将输出文档宽度统一封装为 `output_document_width()`，在原有预留基础上额外缩小 2 列，使正文和 scrollbar 之间保留更明显的右侧安全留白；同时保持窄终端下最小宽度为 1，避免下溢。

**验证**：
- `cargo test -p cli test_output_document_width_reserves_scrollbar_and_two_padding_columns`
- `cargo test -p cli test_output_document_width_never_underflows`

**涉及路径**：
- `apps/cli/src/tui/app/update.rs`
- `apps/cli/src/tui/app.rs`

### #112 TUI 输出区更新滞后

**状态**：待确认

（详情待补充）

### #121 Spinner verb 与 pharse_text 计时显示相同

**状态**：待确认

**症状**：TUI 中 spinner verb 显示的耗时与 `pharse_text` 中显示/记录的计时相同。用户期望二者表达不同阶段或不同来源的计时，但当前界面上两个计时值一致，容易误导为重复显示或计时逻辑复用错误。

**根因**：首轮修复已将 `LiveStatusAssembler` 改为使用 `SpinnerAnim::phase_elapsed_secs()`，但 `SpinnerAnim::sync_phase()` 的 `same_phase_kind()` 仍只按阶段类型判断：`CallingTool("Read")` → `CallingTool("Edit")` 被视为同一阶段，导致 phase 文案切换但 `phase_frame` 不重置，阶段计时继续累计。

**修复**：在 `SpinnerAnim` 中保留独立 `phase_frame` 与已同步 `phase`；本轮将 phase 等价判定收窄为显示语义级规则：`CallingTool(name)` 只有工具名相同才不重置，`CallingTools { remaining }` 计数变化仍不视为新阶段，普通固定阶段同 variant 不重置，Hook 完整 phase 相同才不重置。

**验证**：
- `cargo test -p cli test_spinner_anim_sync_phase_resets_for_calling_tool_name_change -- --nocapture`
- `cargo test -p cli test_spinner_anim_sync_phase_does_not_reset_for_calling_tools_remaining_change -- --nocapture`
- `cargo test -p cli test_spinner_anim_sync_phase_does_not_reset_for_same_calling_tool_name -- --nocapture`
- `cargo test -p cli spinner_anim -- --nocapture`
- `cargo test -p cli live_status -- --nocapture`
- `cargo fmt --check`
- `cargo clippy -p cli --all-targets -- -D warnings`
- `git diff --check`

**涉及路径**：
- `apps/cli/src/tui/**`
- 可能涉及 runtime stream/status 事件路径

### #122 Tool gutter marker 静态且 summary 等 result 才出现

**状态**：待确认

**症状**：TUI 中 running/pending tool call 的 gutter marker 始终显示静态 `●`，没有执行中闪烁动画；同时 Read/Edit/Bash/Grep/Skill/TaskCreate 等 tool call 的括号 summary 经常和 result 一起出现，用户看不到 ToolArgumentsDelta 到达后的提前更新。

**根因**：gutter marker 在 `apply_gutter()` 阶段按工具状态静态注入，未接收或消费动画帧；tool call header 只使用最终 `summary` 格式化，忽略 conversation model 已在 `ToolArgumentsDelta` 中维护的 `args_preview`，导致 summary 为空时只能显示裸工具名，直到最终 ToolCall 或 result 绑定后才刷新为带括号标题。

**修复**：gutter 层新增 `animated_marker_glyph()` 与 `apply_gutter_with_frame()`，running/pending 工具按动画帧在 `●`/`○` 间切换，成功/失败等终态保持静态；输出文档渲染传入 `spinner_frame`，并在 `SpinnerTick` 标记 output dirty 以刷新缓存外 gutter。tool call header 改为优先使用 `summary`，无 summary 时使用可用的 `args_preview` 格式化，保证 ToolArgumentsDelta 后不等 result 即可显示括号/detail。

**验证**：
- `cargo test -p cli test_animated_marker_glyph_blinks_running_tool_between_filled_and_open_circle -- --nocapture`
- `cargo test -p cli test_tool_call_renders_args_detail_from_args_preview_before_summary -- --nocapture`
- `cargo test -p cli test_output_assembler_tool_arguments_delta_updates_header_before_result -- --nocapture`
- `cargo test -p cli gutter -- --nocapture`
- `cargo test -p cli tool_call -- --nocapture`

**涉及路径**：
- `apps/cli/src/tui/render/output/gutter.rs`
- `apps/cli/src/tui/render/output/document_renderer.rs`
- `apps/cli/src/tui/render/output/blocks/tool_call.rs`
- `apps/cli/src/tui/app/update.rs`

---

### 已修复（待归档）

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

### #4 Output Area panic 导致进程卡死（已修复）
**症状**：Output area 渲染时触发 panic，TUI 进程卡死无响应，需 kill。
**根因**：screen_line_map 索引越界 / CharIdx 运算溢出 / wrap 计算与 screen_line_map 不一致。
**修复**：TUI output 重构为 TEA 架构后根本性解决。

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

### #9 鼠标选中时高亮区不在鼠标位置（已修复）
**根因**：`selection_start/selection_end` 存储的是 `screen_line_map` 的行索引（screen_row），但 `screen_line_map` 在每次 `render()` 时重建。当 streaming 新内容追加后，screen_map 偏移，旧索引指向错误位置，导致高亮偏移。
**修复**：selection 改为存储逻辑行索引（logic_idx）而非屏幕行索引。渲染时通过 screen_map 查找当前 screen_idx 对应的 logic_idx 来匹配选中范围，不再受 screen_map 重建影响。
**涉及路径**：`aemeath-cli/src/tui/output_area/selection.rs`、`mod.rs`

### #10 Markdown 渲染：部分未渲染 + 选中后回退为源码（已修复）
**修复**：TUI output 重构为 TEA 架构，根本性地解决了 screen_line_map 偏移不一致问题。

### #11 Markdown 未渲染 Table（已修复）
**症状**：模型返回的 Markdown 表格（`| col1 | col2 |`）在 TUI 中直接显示为纯文本管道符，无表格渲染。
**根因**：`inline_markdown_spans` 未实现 table 语法解析，`|` 和 `---` 分隔行被当作普通文本处理。
**修复**：在 `markdown.rs` 添加 `is_table_separator`、`is_table_row`、`parse_table_cells`、`render_table_block` 函数。在 `mod.rs` render() 中添加表格块预扫描（类似 code block），检测连续 `| ... |` 行为表格，预渲染为 box-drawing 字符表格（`│`、`┼`、`─`），表头粗体，分隔行用 `DarkGray` 色边框。
**涉及路径**：`aemeath-cli/src/tui/output_area/markdown.rs`、`mod.rs`

### #12 Ask user tool call 没有询问用户（已修复）
**症状**：模型调用 ask user 类 tool call 时，TUI 未弹出确认对话框或等待用户输入，直接执行并返回结果。
**根因**：tool call 执行流程未对 ask user 类请求做拦截确认，直接走普通 tool call 处理路径。
**修复**：在 tool call 执行前检测 ask user 类型，拦截后弹出确认 UI 等待用户响应。
**涉及路径**：`aemeath-cli/src/tui/app/processing.rs`、`event_handler.rs`

### #13 Zhipu API 超大请求体返回空响应
**症状**：会话 0000019dc93bab86dfd7032f 中，多轮 tool call 后模型停止输出，TUI 无内容显示。API 返回 `stop_reason=EndTurn` 但 `input_tokens=0 output_tokens=0`，text 为空字符串，无 tool calls。
**根因**：请求体过大（`body_bytes=11659080` 约 11MB），Zhipu GLM-5.1 API 在收到超大请求时静默返回空响应，不报错。compact 后 messages 从 62 降到 23，但 body 仍约 11MB，说明某条 tool result 包含极大内容（可能是文件搜索/读取返回了大量数据），compaction 未能有效压缩。
**修复方向 / 解决进度**：
1. #39 已确认是 #13 的同类根因：超大 tool result 进入请求上下文导致 body 过大；不同 provider 分别表现为 400 `string_above_max_length` 或 `input_tokens=0 output_tokens=0` 空响应。
2. 已在 TUI 主 loop 与子 Agent loop 统一接入 `persist_oversized_results`，超大工具结果进入 LLM 前会落盘并替换为 `<persisted-output>` 引用。
3. 检测空响应并重试/提示仍可作为后续防御增强，但本次已消除已知超大 tool result 直接入上下文的主因。
**涉及路径**：`aemeath-core/src/tool_result_storage.rs`、`aemeath-cli/src/tui/app/stream.rs`、`aemeath-cli/src/agent_runner/loop_helpers.rs`、`aemeath-cli/src/agent_runner/loop_run.rs`

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

### #18 tool call 期间 spinner 偶发消失（已修复）
**根因**：`ToolCallStart` 事件不启动 spinner（只有 `ToolCall` 才启动），但 `Text/Thinking` 事件会 stop_spinner。当 streaming 文本结束后 LLM 发出 tool call，spinner 被 Text stop 后到 ToolCall start 之间有窗口期，期间 spinner 不可见。
**修复**：`ToolCallStart` 也调用 `start_spinner()`，确保 tool call 一识别到就显示 spinner。同时在 `start_spinner/stop_spinner` 添加 debug 日志便于追踪。
**涉及路径**：`aemeath-cli/src/tui/app/update.rs`、`event_handler.rs`、`output_area/spinner.rs`

### #19 AskUserQuestion 等待输入时用户输入被加入 input queue（已修复）
**症状**：AskUserQuestion 无选项（自由输入模式）时，Enter 走了 input queue 路径而非 reply_tx。
**根因**：`update_key` 中 `ask_user_reply_tx` 设置后，Enter 键处理未检查该状态，直接命中 `is_processing` 分支入队。Paste 同理。
**修复**：在 `ask_user_state` 检查之后增加 `ask_user_reply_tx.is_some()` 分支，Enter 时直接 `reply_tx.send()`；Paste 时也跳过入队逻辑。
**涉及路径**：`aemeath-cli/src/tui/app/update.rs`

---
