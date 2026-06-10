# 活动中 Bug

> 排序规范：表格行和详情区块均按 ID 升序排列。

| # | 标题 | 优先级 | 状态 | 确认结果 | 发现日期 | 根因类别 |
|---|------|--------|------|----------|----------|----------|
| 111 | LLM 输出长行被截断，TUI 只显示到屏幕宽度即断行消失 | 中 | 待确认 | 待用户确认 | 2026-06 | thinking/reasoning block 现按渲染宽度预换行，避免长行截断 |
| 112 | TUI tool call spinner 有状态但输出区不显示 tool card | 中 | 待确认 | 待用户确认 | 2026-06 | runtime tool 事件携带 chat/turn context，TUI 按上下文绑定 conversation |
| 121 | Spinner verb 与 pharse_text 计时显示相同 | 中 | 待确认 | 待用户确认 | 2026-06 | 阶段计时已独立；本轮修正 CallingTool 名称变化不重置 phase_frame 的漏修 |
| 122 | Tool gutter marker 闪烁过快且 summary 曾等 result 才出现 | 高 | 待确认 | 待用户确认 | 2026-06 | running marker 已消费动画帧但按每帧奇偶切换过快；header 只看 summary 的问题已修复为回退 args_preview |
| 123 | TUI input queue 不识别换行符 | 中 | 活动中 | 待用户确认 | 2026-06 | input queue 对排队消息中的换行符识别/展示异常，需定位处理路径 |

### #111 LLM 输出长行被截断，TUI 只显示到屏幕宽度即断行消失

**状态**：待确认

**症状**：LLM 输出长行在 TUI 中曾只显示到屏幕宽度，超出部分不可见；首轮修复后普通正文长行可换行，但 reasoning/thinking 文本仍会被截断。例如中文问候触发的 reasoning 内容只显示到 `The user is greeting me in Chinese... a simple g`，后续内容没有自动换行显示。

**根因**：首轮修复只统一了 output document 渲染宽度并预留 scrollbar 右侧安全留白。普通 assistant message 走 markdown/fenced markdown 渲染，会按 `ctx.width` 预换行；但 `thinking.rs` 直接按原始 `text.lines()` 生成 `RenderedLine`，完全未使用 `ctx.width`，长 reasoning 行交给 ratatui `Paragraph` 后被当前可见宽度截断。

**修复**：thinking block 复用 inline markdown 的显示宽度换行逻辑，保留 `theme::THINKING` 样式和 gutter marker 语义；补充窄宽度下长 reasoning 文本会拆成多行且每行不超过渲染宽度的回归测试。

**验证**：
- `cargo fmt --check`
- `git diff --check`
- `cargo test -p cli thinking`
- `cargo test -p cli assistant`

**涉及路径**：
- `apps/cli/src/tui/app/update.rs`
- `apps/cli/src/tui/app.rs`

### #112 TUI tool call spinner 有状态但输出区不显示 tool card

**状态**：待确认

**症状**：运行时已进入 tool call 阶段，spinner phase 会显示类似 `call Bash` 的工具调用状态，但 TUI 输出区没有出现对应的 tool call card，用户只能从状态栏感知到工具正在调用。

**根因**：spinner 走旧状态路径消费 `ToolCallStart` 并更新 `SpinnerPhase::CallingTool(name)`；tool card 渲染则依赖 `ConversationModel.active_chat_id`。当 streaming tool event 到达时本地没有可用 active chat/turn，`ConversationModel` 在处理 `ToolCallStart` / `ToolArgumentsDelta` 等事件时直接返回，导致没有创建 tool block。根因是 TUI 用本地 active chat 作为 streaming runtime 事件归属来源，而不是使用 runtime 会话自身的 chat/turn 上下文。

**修复**：runtime stream 事件新增 `RuntimeTurnContext { chat_id, turn_id }`，并经 SDK `ChatEvent`、TUI `UiEvent` 全链路透传。TUI adapter 在处理 text/thinking/tool event 前先发送 `BindRuntimeTurn`，让 `ConversationModel` 以 runtime context 建立或切换目标 chat/turn，再创建/更新 tool block。历史消息加载继续显式绑定 history turn，保持 resume 渲染路径可用。

**验证**：
- `cargo test -p cli tui::model::conversation::model_extra_tests::test_runtime_tool_event_creates_chat_from_runtime_context_without_active_chat`
- `cargo test -p cli tui::model::conversation`
- `cargo test -p runtime --lib`
- `cargo fmt --check`
- `cargo clippy -p cli --all-targets -- -D warnings`
- `.agents/hooks/check-unit-tests.sh`

**涉及路径**：
- `agent/features/runtime/src/business/chat/looping/**`
- `agent/features/runtime/src/core/client/event.rs`
- `packages/sdk/src/chat.rs`
- `apps/cli/src/chat/no_tui.rs`
- `apps/cli/src/tui/app/**`
- `apps/cli/src/tui/effect/session/processing.rs`
- `apps/cli/src/tui/adapter/agent_event.rs`
- `apps/cli/src/tui/model/conversation/**`
- `apps/cli/src/tui/render/display/render.rs`

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

### #122 Tool gutter marker 闪烁过快且 summary 曾等 result 才出现

**状态**：待确认

**症状**：TUI 中 running/pending tool call 的 gutter marker 首轮修复后可闪烁，但直接按 spinner 90ms 帧奇偶切换，视觉上闪得太快；此前 Read/Edit/Bash/Grep/Skill/TaskCreate 等 tool call 的括号 summary 经常和 result 一起出现，用户看不到 ToolArgumentsDelta 到达后的提前更新。

**根因**：首轮修复让 gutter marker 消费 `spinner_frame`，但直接使用每帧奇偶切换，导致每 90ms 翻转、完整周期 180ms；tool call header 只使用最终 `summary` 格式化，忽略 conversation model 已在 `ToolArgumentsDelta` 中维护的 `args_preview`，导致 summary 为空时只能显示裸工具名，直到最终 ToolCall 或 result 绑定后才刷新为带括号标题。

**修复**：gutter 层新增闪烁分频，running/pending 工具按 `spinner_frame / 4` 在 `●`/`○` 间切换，即约 360ms 翻转、720ms 完整周期；成功/失败等终态保持静态。tool call header 改为优先使用 `summary`，无 summary 时使用可用的 `args_preview` 格式化，保证 ToolArgumentsDelta 后不等 result 即可显示括号/detail。

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

### #123 TUI input queue 不识别换行符

**状态**：活动中

**症状**：TUI 对话处理中继续输入多行内容进入 input queue 后，queue 对消息中的换行符识别/展示异常；多行内容可能被当成单行、丢失换行语义，或后续发送时不能按原多行内容处理。

**根因**：待定位。疑似 input queue 入队、展示或 drain 路径对 queued message 使用单行文本假设，未统一保留并按 `\n` 拆分/传递。

**修复/实现**：待修复。需要定位 input queue 入队、渲染和消费路径，补充覆盖换行符的回归测试，再统一保留多行消息语义。

**验证**：待补充。

**涉及路径**：
- `apps/cli/src/tui/**`
- 可能涉及 runtime/input queue drain 路径

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
