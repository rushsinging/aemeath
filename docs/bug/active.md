# 活动中 Bug

| # | 标题 | 优先级 | 状态 | 确认结果 | 发现日期 | 根因类别 |
|---|------|--------|------|----------|----------|----------|
| 110 | Stop hook 项目上下文只输出到 stdout，成功时不进入 aemeath.log | 中 | 待确认 | 待用户确认 | 2026-06 | HookRunner 成功时不记录 stdout/stderr 内容；已修复提取 [hook-env] 行写入日志 |
| 112 | TUI 输出区更新滞后 | 中 | 待确认 | 待用户确认 | 2026-06 | UiEvent 每个 chunk 同步刷新拖慢主循环；已改为 dirty 标记+按帧批量刷新 |
| 102 | 长工具调用内容导致 TUI 画面完全不刷新、按键无响应 | 高 | 修复中 | 未确认 | 2026-06 | TUI 保存/渲染大工具参数或 result 触发主线程大量 clone/计算阻塞 event loop |
| 74 | TUI 执行 /reflect 后续文本颜色全部变暗（System 色泄漏） | 中 | 修复中 | 未确认 | 2026-05 | ReflectionDone 以 System(Muted) 暗色推入完整会话转录；修复改为只推摘要 |
| 96 | EnterWorktree 上下文栈与 git 实际状态不一致，导致误报"已在 worktree 中" | 中 | 活动中 | 未确认 | 2026-05 | EnterWorktree 上下文栈与 git 实际状态不同步时误判"已在 worktree 中" |
| 97 | /clear 未清空 task store 和 task list window | 中 | 待确认 | 未确认 | 2026-05 | /clear 未清空 TaskStore 和 task_status lines；已新增 clear_tasks 并清空 task lines |
| 98 | resume 时没有加载 worktree 配置 | 高 | 修复中 | 未确认 | 2026-05 | load_session_impl 丢弃 workspace 上下文，runtime handle 未同步更新 |
| 111 | LLM 输出长行被截断，TUI 只显示到屏幕宽度即断行消失 | 中 | 待确认 | 未确认 | 2026-06 | TUI 长行不自动换行也不可横向滚动，超出屏幕宽度的内容不可见 |
| 113 | AskUserQuestion 回答后 LLM 新输出渲染到 AskUser 块上方 | 中 | 待确认 | 待用户确认 | 2026-06 | AskUser 未清理 active_text_block_id，新输出渲染到 AskUser 块上方；已修复 |
| 115 | check-unit-tests 测试过滤参数误用导致误报失败 | 低 | 待确认 | 待用户确认 | 2026-06 | cargo test 短名+--exact 过滤 0 个测试误报失败；已补充完整路径测试 |
| 116 | TaskListCreate 工具返回未带 task list ID | 中 | 已修复 | 待用户确认 | 2026-06 | TaskListCreate 返回未带 ID；已修复返回格式并增加引导说明 |

### #110 Stop hook 项目上下文只输出到 stdout，成功时不进入 aemeath.log

**状态**：待确认

**症状**：Stop hook 脚本已输出 `[hook-env] AEMEATH_PROJECT_DIR=...`、`CLAUDE_PROJECT_DIR=...`、`ROOT/PWD=...`，但 hook 成功通过时，这些内容只出现在 hook stdout/TUI 验证输出中，不进入 `~/.agents/logs/aemeath.log`；即使将 `logging.level` 调为 `debug`，日志中也只能看到已有 hook start/end 元信息，无法直接检索 `[hook-env]` 行。

**根因**：`HookRunner::execute_hook` 成功等待子进程后只记录 stdout/stderr 字节数，没有记录 stdout/stderr 内容。为避免完整 hook 输出污染日志，需要只提取稳定的 `[hook-env]` 诊断行写入日志。

**修复**：
1. 新增 `hook_env_lines`，从 stdout/stderr 中提取以 `[hook-env]` 开头的行。
2. `execute_hook` 在判定 blocked 前，将 stdout/stderr 中的 `[hook-env]` 行写入 `log::info!`，日志包含 event、command、stream 与 line。
3. 不记录完整 hook stdout/stderr，避免单测和构建输出大量进入主日志。

**验证**：
- `cargo test -p hook test_hook_env_lines_extracts_only_hook_env_stdout_lines`

**涉及路径**：
- `agent/features/hook/src/business/hook/runner.rs`
- `agent/features/hook/src/business/hook/tests.rs`

### #102 长工具调用内容导致 TUI 画面完全不刷新、按键无响应

**状态**：修复中

**症状**：执行包含超大参数或结果的工具调用期间，TUI 画面完全不刷新，spinner 不动，键盘输入/快捷键无响应；表现为 event loop 被同步重活堵住，而不是单纯停留在 Generating 状态。高风险工具包括 Write 大 `content`、Edit 大 `old_string/new_string`、Agent 大 `prompt`、Bash 大 `command`，以及 Read/Grep/Glob/Bash/WebFetch/Agent 等大 result。

**初步判断**：工具 I/O 多数走异步路径，不应直接阻塞 TUI 主线程。更可疑的是 TUI 渲染/update 路径保存和处理完整工具参数或完整工具结果，导致每帧发生大字符串 clone、`lines()` 全量收集、宽度计算、block cache version/hash 计算或富文本渲染，从而阻塞 event loop。

**修复方向**：
1. TUI 层展示工具调用时只保留路径、字节数、小预览等摘要，NEVER 将完整大字段放入可反复 clone/render/hash 的 view model。
2. 所有工具结果进入 TUI model 前按字节上限截断；工具结果预览按 `result_max_lines` streaming/take 截断，NEVER 为了显示前 N 行先 `collect()` 完整 result lines。
3. 添加大工具参数/结果回归测试，覆盖格式化/渲染路径不会随正文大小线性处理完整正文。

**涉及路径**：
- `apps/cli/src/tui/adapter/agent_event.rs`
- `apps/cli/src/tui/render/output/tool_display/tool_impls.rs`
- `apps/cli/src/tui/render/output/blocks/tool_result.rs`
- `apps/cli/src/tui/view_assembler/output.rs`

### #97 /clear 未清空 task store 和 task list window

**状态**：待确认

**症状**：执行 `/clear` 后，对话区域被清空，但任务状态窗口仍显示旧 task list；Runtime TaskStore 中旧任务也可能继续存在，后续任务列表与窗口状态不一致。

**根因（已确认）**：`/clear` 只调用 `reset_runtime_state()` 清空 TUI 对话、图片、输出与 session messages；SDK `AgentClient` 没有暴露 TaskStore 清空端口，`RuntimeModel.task_status.lines` 也没有在 clear 路径显式置空，导致下一帧 live status adapter 会继续把旧 task lines 写回 `OutputArea.task_status_lines`。

**修复**：
1. `sdk::AgentClient` 新增 `clear_tasks()` 写端口，默认空实现保持兼容。
2. `AgentClientImpl::clear_tasks()` 委托 runtime `TaskStore.clear()`。
3. `App::reset_runtime_state()` 在同步清空 session messages 后调用 `clear_tasks()`，并通过 `RuntimeIntent::UpdateTaskLines(Vec::new())` 清空 task list window 的 model 真相。
4. 新增回归测试 `test_clear_command_clears_task_store_and_task_window`，覆盖 `/clear` 会调用 clear_tasks 且清空 widget/model task lines。

**验证**：`cargo test -p cli test_clear_command_clears_task_store_and_task_window` 通过。

### #96 EnterWorktree 上下文栈与 git 实际状态不一致，导致误报"已在 worktree 中"

**状态**：修复中

**症状**：
1. 用户在 `main` 分支主工作区（`git branch --show-current` → `main`，`pwd` 不在 `.worktrees/` 下），UI 显示也不在 worktree 中。
2. 调用 `EnterWorktree { branch: "feature/xxx" }`（不给 `path`，走自动创建模式）时报错：`进入 worktree 失败：已在 worktree 中，请先 ExitWorktree 退出当前 worktree 再进入新的`。
3. 直接给 `path` 参数指定已存在的 worktree 路径则成功进入。

**根因**：`enter_worktree()`（`agent/project/src/business/worktree.rs:125-132`）将 `context_stack.is_empty()` 作为"是否在 worktree 中"的**唯一判断依据**，完全不校验 git 实际状态。

`context_stack` 是内存 `Arc<Mutex<Vec<WorkingContext>>>`，通过 `workspace_context_from_tool_context()` → `WorkingDirectoryChanged` 事件持久化到会话存储。会话恢复时从 `WorkspaceContext.context_stack` 还原。

触发链条：
1. Session N：EnterWorktree 成功 → context_stack.push → 会话自动持久化时栈非空
2. Session N 异常结束 / 未调用 ExitWorktree → 残留条目持久化
3. Session N+1：恢复到 main，但 context_stack 从持久化恢复后仍非空 → `enter_worktree()` 误判为"已在 worktree 中"

**修复方向**：`enter_worktree()` 栈非空时，通过 `git rev-parse --git-dir` 校验当前路径是否真实在 `.worktrees/` 下。若栈非空但 git 确认在主工作区，自动清理残留栈并允许进入；仅当 git 也确认在 worktree 中时才拒绝嵌套。

**涉及路径**：`agent/project/src/business/worktree.rs:125-132`（`enter_worktree` 的栈校验逻辑）

### #74 TUI 执行 /reflect 后续文本颜色全部变暗（System 色泄漏）

**状态**：修复中

**症状**：在 TUI 中执行 `/reflect` 后，reflection 输出及其**后续的普通/assistant 文本**全部呈现暗灰蓝色（System 样式）。

**根因**：`ReflectionDone` 在 `ui_event.rs:168` 将 `output.content`（包含完整会话转录 `[User]:`/`[Assistant]:`、markdown 等内容）以 `append_system_notice` → `System(Muted)` 暗色推入输出区。大段暗色文本占据输出区大部分可见区域，视觉上后续 assistant 回复也"看起来暗了"。渲染管线本身无颜色泄漏（每个 block 独立渲染，ASSISTANT 色与 MUTED 色不同），但 reflection 完整内容中的 `[Assistant]:` 转录以 Muted 暗色渲染，混淆了用户对"assistant 回复变暗"的判断。

**修复方向 / 当前状态**：修复中。只推送简短摘要（建议数 + 过时数），不推送完整 reflection 内容。完整内容保留在 `pending_reflection` 中，用户可通过 `/reflect apply` 查看。回归测试覆盖 System block 后 Assistant block 颜色正确性。

**涉及路径**：`apps/cli/src/tui/app/update/ui_event.rs`（`ReflectionDone` 处理）

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
**修复方向 / 解决进度**：
1. #39 已确认是 #13 的同类根因：超大 tool result 进入请求上下文导致 body 过大；不同 provider 分别表现为 400 `string_above_max_length` 或 `input_tokens=0 output_tokens=0` 空响应。
2. 已在 TUI 主 loop 与子 Agent loop 统一接入 `persist_oversized_results`，超大工具结果进入 LLM 前会落盘并替换为 `<persisted-output>` 引用。
3. 检测空响应并重试/提示仍可作为后续防御增强，但本次已消除已知超大 tool result 直接入上下文的主因。
**涉及路径**：`aemeath-core/src/tool_result_storage.rs`、`aemeath-cli/src/tui/app/stream.rs`、`aemeath-cli/src/agent_runner/loop_helpers.rs`、`aemeath-cli/src/agent_runner/loop_run.rs`

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

---
