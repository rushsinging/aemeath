# 活动中 Bug

| # | 标题 | 优先级 | 状态 | 确认结果 | 发现日期 | 根因类别 |
|---|------|--------|------|----------|----------|----------|
| 13 | Zhipu API 超大请求体返回空响应 | 高 | 待确认 | 未确认 | 2026-04 | body 过大时 API 返回 input_tokens=0 output_tokens=0 |
| 26 | 几乎每次对话都触发 superpowers skill 调用 | 中 | 活动中 | 未确认 | 2026-05 | SessionStart hook 注入提示过强或 skill 触发条件过宽 |
| 27 | Sub-agent 已执行 tool call 但 task list 状态不更新 | 高 | 修复中 | 未确认 | 2026-05 | AgentTool::call() 未读取 taskId 参数，未在 run_agent 前后管理 task 状态转换；sub-agent TaskStore 与父隔离 |
| 29 | 主 agent tool call 执行后 task list 状态不更新 | 高 | 修复中 | 未确认 | 2026-05 | system prompt 引用不存在的 TodoWrite/TodoRun，缺少 TaskUpdate 强约束 |
| 28 | Output Area 选中/渲染时 panic：slice/split_off 越界 | 高 | 待确认 | 未确认 | 2026-05 | selection 坐标和 screen_line_map 在动态内容变化时未做边界收敛 |

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

### #28 Output Area 选中/渲染时 panic：slice/split_off 越界
**症状**：TUI 中选中并尝试复制文本时崩溃，状态行/spinner 行显示：
```
[PANIC] range start index 4 out of range for slice of length 2 at aemeath-cli/src/tui/output_area/selection.rs:203:32
```
同一 session / turn 中还出现大量相似 panic：
```
[PANIC] `at` split index (is 4) should be <= len (is 0) at aemeath-cli/src/tui/output_area/mod.rs:474:57
```
**根因**：OutputArea 有两类动态状态未在读取/裁剪前做统一边界收敛：
1. `get_selected_text()` 使用 selection 保存的 `(logic_idx, char_col)` 对当前 `lines[logic_idx].content` 切片。动态 streaming、tool progress 替换、行重排后，原来的 `start_col` 可能大于当前 `chars.len()`，导致 `chars[from..to]` 越界。
2. `render()` 中 `lines` 追加 queued/spinner/task 临时行后，`lines.len()` 可能大于 `area.height`，但这些临时行没有对应的 `screen_line_map` 项。用 `lines.len() - area.height` 直接裁剪 `screen_line_map.split_off(offset)` 时，`offset` 可能大于 `screen_line_map.len()`，导致 split 越界。
**复现路径**：
1. 选中一段文本或正在渲染带 spinner/task/status 的 output area
2. 内容因 streaming、tool call progress、spinner/task 临时行、窗口高度变化等发生变化
3. selection 坐标或 screen_line_map 与当前可见行数量不再一致
4. 触发复制或下一帧 render → panic
**修复方向**：
1. `get_selected_text()` 中 `from` 和 `to` 都裁剪到 `chars.len()`，并在 `from > to` 时跳过该行，避免反向/越界切片。
2. `render()` 裁剪 `screen_line_map` 时使用 `offset.min(screen_line_map.len())`，并将最终 map 长度截断到实际可见高度，确保临时行过多时不会越界。
3. 增加回归测试覆盖 selection 行变短、反向 selection clamp、reserved 临时行超过高度三类场景。
**涉及路径**：
- `aemeath-cli/src/tui/output_area/selection.rs`（`get_selected_text` 切片裁剪）
- `aemeath-cli/src/tui/output_area/mod.rs`（`render` 中 `screen_line_map` 与临时渲染行裁剪）

### #27 Sub-agent 已执行 tool call 但 task list 状态不更新
**症状**：父 agent 创建 7 个 task（"#1 拆分 task.rs (509→<400)" ... "#7 ..."），通过 Agent tool 派发 sub-agent 执行其中某个（如 "拆分 state.rs 到400行以下"）。sub-agent 已完成 Read / Bash / Write / Bash / Bash 等多个 tool call（屏幕可见），但临时区域的 task list 仍显示 `Tasks: 0/7`，所有 7 项保持 `☐`（pending）状态。
**复现路径**：
1. LLM 通过 TaskCreate 一次创建多条 task
2. LLM 通过 Agent tool 派发 sub-agent 处理其中一个 task
3. sub-agent 在隔离 context 内调用工具完成工作
4. 父 agent 一侧 task list 未变化，无 in_progress / completed 反馈
**疑似根因**：
1. AgentTool 派发时未读取 / 不接受 `task_id` 参数，无法把"对应哪个父 task"传递下去
2. sub-agent 完成回报后，父 agent 一侧没有自动 `TaskUpdate(completed)` 联动
3. sub-agent 的 TaskStore 与父隔离：即便 sub-agent 自己调用 TaskUpdate，也写到自己的 store，父看不到
4. 父 agent 自身在派发前也没有 `TaskUpdate(in_progress)`（这部分与 #29 共因）
**修复方向**：
1. **AgentTool 自动桥接 taskId**：Agent tool 接受 `task_id` 参数，执行前把对应 task 标 in_progress，执行成功后标 completed，失败时标 cancelled / 留 in_progress
2. **TaskStore 父子共享**：让 sub-agent 通过引用看到父的 TaskStore，TaskUpdate 直接写父
3. **system prompt 增强**：派发 sub-agent 时必须在 prompt 中绑定 task_id，并在 sub-agent 完成后由父 agent 自检状态
4. **UI 兜底**：sub-agent 期间在对应 task 旁显示 `(agent working)` 标记
**涉及路径**：
- `aemeath-tools/src/agent.rs`（AgentTool 接受 `task_id` 并在 run_agent 前后自动联动）
- `aemeath-core/src/task.rs`（TaskStore 父子共享 / 引用语义）
- `aemeath-cli/src/agent_runner.rs`（sub-agent 与父 TaskStore 的关系）
**关联**：
- Bug #29（主 agent 路径同样不更新）—— 共因之一是 LLM 不主动 update，但 sub-agent 路径还多 AgentTool 桥接 + TaskStore 隔离两条独立修复线
- Feature #18（Task list 跨轮次 batch 机制）—— batch 机制本身工作正常

### #29 主 agent tool call 执行后 task list 状态不更新
**症状**：task 列表只有 1 条 `#1 拆分 hook.rs → hook/ 目录`，状态 `☐`（pending）。LLM（主 agent，不是 sub-agent）已通过 Bash tool 执行 `mkdir -p .../aemeath-core/src/hook`（属于该 task 的第一步），并进入 Thinking 阶段（spinner 显示 `Cogitating... 389s (Thinking...)`）。task list 仍显示 `Tasks: 0/1`，#1 仍为 `☐`，没有任何 in_progress / completed 联动。

**复现路径**：
1. LLM 创建 task（TaskCreate）
2. LLM 直接调用 Bash / Edit / Write 等核心 tool 开始执行该 task 的工作
3. 观察 task 状态：始终停在 `☐`，从未变成 `⟳`（in_progress）或 `✓`（completed）

**根因**：当前架构完全依赖 LLM 自觉调用 TaskUpdate，但：
1. **system prompt 缺少强约束**：task 系统的指引可能只是"鼓励"而非"强制"维护状态，LLM 倾向跳过
2. **核心 tool 无自动联动**：Bash / Edit / Write 等核心 tool 执行前后没有任何机制把"当前正在处理哪个 task"标 in_progress
3. 与 sub-agent 路径（#27）的差异：主 agent 路径 **不能** 走 AgentTool 桥接 taskId 的方案——主 agent 一次回复可能跨多个 task，没有显式"哪个 tool call 对应哪个 task"的语义信号

**修复方向**（按介入强度排序）：
1. **system prompt 强约束**（最小改动，主推）：明确规定"开始任何实质性 tool call 前必须先 TaskUpdate(in_progress)；完成后必须 TaskUpdate(completed)"，并在 prompt 中给反例 / 失败示例
2. **任务文本启发式联动**（中等激进）：tool call 执行时若有 in_progress task，不动；若没有 in_progress 但有 pending task，且 LLM 在 thinking 文本或 tool input 中提到该 task 标题关键词，自动标 in_progress
3. **UI 兜底**：tool call 期间在第一个 pending task 旁显示 `(working?)` 提示，让用户知道存在 task/work 错配
4. **Hook 兜底**：PostToolUse hook 检查"是否有 in_progress task"，若无则在日志或 UI 警告

**涉及路径**：
- system prompt 中 task 维护指引（`aemeath-core/src/context.rs` 或专用 task prompt 文件）
- `aemeath-core/src/task.rs`（如做启发式联动需要 query 接口）
- `aemeath-cli/src/tui/output_area/`（UI 兜底显示）

**关联**：
- Bug #27（sub-agent 路径）—— 共因是 LLM 不主动 update task 状态，但修复路径不同
- Feature #18（Task list 跨轮次 batch 机制）—— batch 工作正常，问题在状态推进

### #26 几乎每次对话都触发 superpowers skill 调用
**症状**：几乎每次对话开始时，LLM 都会主动通过 Skill 工具调用 superpowers 系列 skill（如 `superpowers:using-superpowers`、`superpowers:brainstorming` 等），即使用户的请求只是简单提问、查询信息或闲聊，并不需要任何 skill 介入。
**疑似根因**：
1. SessionStart hook（`~/.aemeath/hooks/superpowers-inject.sh` 由 Feature #17 改成短提示）的提示文本可能仍然过强（"if there is even a 1% chance..."），让 LLM 把"调用 skill"当作默认动作
2. `superpowers:using-superpowers` skill 的 description 包含 "Use when starting any conversation"，被 LLM 解读成"每轮都要调用"
3. Skill 列表注入到 system prompt 后，LLM 倾向于用 skill 而非直接回答
**修复方向**：
1. 调整 SessionStart hook 提示语，从"必须检查 skill"改为"如有明确匹配场景再考虑调用"
2. 评估是否对 `using-superpowers` 这类元 skill 做特殊处理（不在每轮提示中暴露）
3. 增加配置开关 `skill.auto_suggest = false`，让用户主动用 `/skill` 命令调用而非 LLM 自主决定
4. 观察并记录哪些用户输入触发了不必要的 skill 调用，形成 ban list
**涉及路径**：`~/.aemeath/hooks/superpowers-inject.sh`、`aemeath-core/src/skill.rs`（skill description 是否参与 system prompt 构建）、`aemeath-core/src/config/`（`skill.auto_suggest` 配置）


# 已归档 Bug

### #25 /clear 命令未清空 status line 数据（已确认修复）
**根因**：`/clear` 仅清空消息历史，未联动复位 status bar 的 task list / active tool calls / spinner 状态等运行态字段。
**修复**：App 暴露统一的运行态复位入口，`CommandAction::Clear` 同时清空 active tool call set、task summary、spinner 与当前 tool call 名；保留 model / provider / cwd / cost 等环境与累计信息。详见 `docs/bug/archived/025-clear-status-line.md`。

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
