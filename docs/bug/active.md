# 活动中 Bug

| # | 标题 | 优先级 | 状态 | 确认结果 | 发现日期 | 根因类别 |
|---|------|--------|------|----------|----------|----------|
| 119 | TUI tool call 空 summary 覆盖流式参数导致 Skill(?) 与 TaskCreate 缺失 | 高 | 待确认 | 待用户确认 | 2026-06 | ToolCall 绑定时空 summary 覆盖 ToolArgumentsDelta 收集的参数预览 |
| 118 | Hook env 中项目目录仍指向主工作区而非当前 worktree | 高 | 活动中 | 未确认 | 2026-06 | HookRunner 注入给 hook 子进程的 AEMEATH_PROJECT_DIR/CLAUDE_PROJECT_DIR 与匹配阶段 project_dir 不一致 |
| 112 | TUI 输出区更新滞后 | 中 | 待确认 | 待用户确认 | 2026-06 | UiEvent 每个 chunk 同步刷新拖慢主循环；已改为 dirty 标记+按帧批量刷新 |
| 74 | TUI 执行 /reflect 后续文本颜色全部变暗（System 色泄漏） | 中 | 修复中 | 未确认 | 2026-05 | ReflectionDone 以 System(Muted) 暗色推入完整会话转录；修复改为只推摘要 |
| 96 | EnterWorktree 上下文栈与 git 实际状态不一致，导致误报"已在 worktree 中" | 中 | 活动中 | 未确认 | 2026-05 | EnterWorktree 上下文栈与 git 实际状态不同步时误判"已在 worktree 中" |
| 98 | resume 时没有加载 worktree 配置 | 高 | 修复中 | 未确认 | 2026-05 | load_session_impl 丢弃 workspace 上下文，runtime handle 未同步更新 |
| 111 | LLM 输出长行被截断，TUI 只显示到屏幕宽度即断行消失 | 中 | 待确认 | 待用户确认 | 2026-06 | TUI 长行已自动换行；本轮继续将输出文档宽度额外缩小 2 列，增加正文与 scrollbar 的右侧安全留白 |

### #119 TUI tool call 空 summary 覆盖流式参数导致 Skill(?) 与 TaskCreate 缺失

**状态**：待确认

**修复 commits**：待提交

**症状**：会话 `019e93a2-950d-715d-807b-f98a880902be` 中 TUI 开始显示 `Skill(superpowers:...)` 正确，但工具完成后变成 `Skill(?)`；随后创建 task list 和 task 时，TUI 显示了 task list 的 tool call，但没有显示对应的 `TaskCreate` tool call header。

**根因**：运行时已经向 TUI 发送 `ToolCallStart`、`ToolArgumentsDelta`、`ToolCall` 与 `ToolResult` 事件；问题出在 TUI conversation model 绑定阶段。`ToolArgumentsDelta` 已经把真实入参写入 `args_preview`，但后续 `ToolCall` 到达时若 `summary` 为空，会把已收集参数覆盖为空，渲染层再用空 JSON/Null 调用工具显示 formatter，导致 `Skill` 取不到 `skill` 字段回退为 `?`，任务类工具也失去 subject/description 摘要。

**修复**：`ToolCall::bind` 在收到空 summary 时不再覆盖已有摘要；若已经存在 `args_preview`，则使用流式参数作为 summary fallback。`ConversationModel::observe_tool_call` 同步使用最终 summary 更新 block，并避免用空 summary 清空已有 block 摘要。补充回归测试覆盖 `Skill` 空 summary 保留流式参数，以及 `TaskListCreate` 后紧跟 `TaskCreate` 时两个 tool call block 都保留且摘要正确。

**验证**：
- `cargo test -p cli test_conversation_preserves_streamed_args_when_tool_call_summary_is_empty -- --nocapture` 先失败、修复后通过
- `cargo test -p cli test_conversation_keeps_distinct_task_tool_blocks_after_empty_summary_bind -- --nocapture` 先失败、修复后通过
- `cargo fmt --check && cargo test -p cli tui::model::conversation -- --nocapture && cargo clippy -p cli --all-targets -- -D warnings`

**涉及路径**：
- `apps/cli/src/tui/model/conversation/model.rs`
- `apps/cli/src/tui/model/conversation/tool_call.rs`
- `apps/cli/src/tui/model/conversation/model_tests.rs`

### #118 Hook env 中项目目录仍指向主工作区而非当前 worktree

**状态**：已修复

**修复 commits**：待提交

**症状**：`~/.agents/logs/aemeath.log` 中 hook 匹配阶段已经记录当前 worktree 的 `project_dir`，例如 `.../aemeath/.worktrees/fix-111-tui-column-scroll-padding`；但 hook 脚本 stdout 中提取出的 `[hook-env] AEMEATH_PROJECT_DIR=...` 与 `[hook-env] CLAUDE_PROJECT_DIR=...` 仍是主工作区 `/Users/guoyuqi/Nextcloud/work/claudecode/aemeath`。这会导致 Stop hook / 项目 hook 在 worktree 会话中按主工作区执行检查或输出错误上下文。

**根因**：待定位。初步判断 HookRunner 匹配阶段使用的 `project_dir` 与构造 hook 子进程环境变量时使用的项目根来源不同步，环境变量注入仍取主 checkout 的 project dir。

**修复方向**：检查 hook 执行环境变量注入路径，确保 `AEMEATH_PROJECT_DIR` 与 `CLAUDE_PROJECT_DIR` 使用当前会话/工具上下文的 effective project dir，并与 `hook match` 日志中的 `project_dir` 一致；补充覆盖 worktree 场景的回归测试。

**验证**：`cargo fmt -p runtime`、`cargo test -p runtime test_process_chat_loop_uses_workspace_working_root_for_stop_hook_env -- --nocapture`、`cargo test -p runtime business::chat::looping::loop_runner::tests -- --nocapture`、`cargo clippy -p runtime --all-targets -- -D warnings`。

**涉及路径**：
- `agent/features/hook/src/business/hook/runner.rs`
- `agent/features/policy/src/` 或 runtime 调用 HookRunner 的上下文传递路径
- `specs/policy-hook-audit.md`

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
