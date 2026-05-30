# 活动中 Bug

| # | 标题 | 优先级 | 状态 | 确认结果 | 发现日期 | 根因类别 |
|---|------|--------|------|----------|----------|----------|
| 49 | last turn 时用户提交的内容不会发给 LLM，留在 input queue 区域 | 高 | 修复中 | 用户反馈仍存在 | 2026-05 | 用户反馈该问题仍存在；已定位新增残留窗口：LLM 最终响应前已有 drain，但 Stop hook 执行期间用户输入会发生在最后一次 drain 之后、DoneWithDuration 之前，Stop hook 通过后 runtime 直接 Done，导致输入留在 TUI input_queue。修复：Stop hook 通过后、发送 DoneWithDuration 前再次 drain queue；若 drain 到输入则 append messages 并 continue 主 LLM loop，追加输入处理完成后仍会再次触发 Stop hook |
| 54 | LLM 过度使用 TaskListCreate，简单任务也创建 task list | 中 | 修复中 | 未确认 | 2026-05 | 根因：TaskCreate / TaskListCreate 工具描述只强调多步任务必须使用 task 管理，缺少简单任务禁止创建 task list 的反向约束；模型为避免违反 task workflow，倾向把查看 bug、简单查询、单命令检查也包装成 task list。修复：工具描述改为仅复杂多步任务（≥3 个实质步骤、多依赖变更或并行 sub-agent 协调）使用 task 管理，并明确问答、查看文件/bug 状态、单命令、小范围修改直接执行 |
| 62 | Grep 工具执行中标题文字不可见但复制可见 | 中 | 待确认（随 #58 渲染管线重构修复） | 未确认 | 2026-05 | TUI 中 Grep 工具运行态显示 `● Grep /tui\.log/ in ...` 时，屏幕上看不到 `Grep` 字样，但选中复制能复制出来；疑似工具标题/参数文本颜色与背景色过近或被 running 状态样式覆盖，也可能是 selection/render spans 与 plain text copy 路径不一致 |
| 64 | Agent 未绑定 taskId 仍启动导致 TaskList 无 doing 状态 | 高 | 修复中 | 未确认 | 2026-05 | session `019e4ea6-6f8a-7049-a812-0ab60653770e` 中，LLM 创建 task list 并完成 Task 1 后，启动 Task 2 subagent 时漏传 `taskId`；subagent 实际执行但 TaskStore 未进入 InProgress，TaskList 只显示 done/pending。修复方向：active task batch 存在未完成任务时，Agent 必须传 `taskId`，否则拒绝启动并提示使用绑定 taskId 或显式无跟踪调用。 |
| 65 | 工具结果 fenced code block 后续内容继续显示为 code 颜色 | 中 | 待确认（G2 已接线：工具结果走共享 fence 渲染，fence 结束后普通行恢复正常色） | 未确认 | 2026-05 | Edit/Write 等工具结果中包含 fenced code block 时，例如 `✓ replaced 1 occurrence(s) in ...` 后展示文件路径并以 ``` 收尾，但后续普通内容仍呈现 code 颜色；疑似 Markdown fence 状态未在工具结果块结束后复位，或 tool result 渲染缓存/样式 span 泄漏到后续行 |
| 66 | ExitWorktree 带 path 参数报错"已在 worktree 中" | 中 | 活动中 | 未确认 | 2026-05 | ExitWorktree 传入 `path` 参数时应能退出当前 worktree 再切换到指定路径，但实际报错：`✗ 切换路径失败：已在 worktree 中，请先 ExitWorktree 退出当前 worktree 再进入新的`；疑似 path 路径切换逻辑在判断"当前是否在 worktree 中"时未区分"仅退出"与"退出+切换"两种语义，错误地把 path 参数直接当 EnterWorktree 处理 |
| 69 | worktree 中 LLM 仍尝试搜索主分支路径 | 中 | 修复中 | 待确认 | 2026-05 | 根因：静态 system prompt 中写入具体 `Current workspace root` 会在会话中途 EnterWorktree 后过期；修复调整为静态 prompt 只保留通用路径规则，当前 path_base/working_root 通过 EnterWorktree/ExitWorktree 的 tool result 返回给 LLM，路径越界错误继续提供恢复建议 |
| 71 | TUI 渲染缓存越界 panic + unsafe string guard 覆盖不全 | 高 | 待确认（随 #58 渲染管线重构修复） | 未确认 | 2026-05 | 输出区行数达到 `MAX_LINES=10000` 上限后，`rendered_lines::collect_table_ranges` / `render_range` 收到的 `end` 超过 `lines.len()`，在 `apps/cli/src/tui/output_area/rendered_lines.rs:98` 处 `lines[i]` 越界 panic 导致整个 TUI 崩溃；疑似 `RenderedLineCache::ensure_rendered` 增量分支使用陈旧 `render_start`/`render_end` 未 clamp 到 `total`。同时 `check-unsafe-text-ops.sh` guard 抓不到该类问题：①只扫 `apps/cli/src/tui`，`agent/`、`packages/` 切片不检查；②只在 Stop hook 触发，panic 时跳过；③正则漏掉裸单下标 `slice[i]`（`lines[i]` 正属此类） |
| 72 | agent 双层循环中一轮结束后不自动读取 input queue | 中 | 修复中 | 未确认 | 2026-05 | 根因：P13 SDK 解耦后，CLI TUI 的 `TuiQueueDrainPort` 只在 `spawn_processing` 收到 `Done/DoneWithDuration` 后兜底 drain；`AgentClientImpl::chat` 启动 runtime chat loop 时固定传 `EmptyQueueDrainPort`，导致 runtime 中既有的 `append_queued_input` 检查永远读不到 TUI 排队输入。修复：`ChatRequest` 携带 SDK queue drain 端口，runtime 用 `RuntimeQueueDrainPort` 转接给 `process_chat_loop`，TUI 发起 chat 时注入 `TuiQueueDrainPort`。 |
| 74 | TUI 执行 /reflect 后续文本颜色全部变暗（System 色泄漏） | 中 | 待确认（随 #58 渲染管线重构修复） | 未确认 | 2026-05 | `/reflect` 完成后，`ReflectionDone` 通过 `output_area.push_system(&output.content)` 以 `LineStyle::System`（暗灰蓝）推送整段 reflection 输出（内含 `[User]:`/`[Assistant]:` 会话转录与 markdown），其后续普通/assistant 文本也呈现 System 暗色；疑似与 #65 同族——markdown fence/样式状态或渲染缓存 style 跨 block 泄漏，或 reflection 后未复位为 Assistant 样式 |
| 73 | EnterWorktree 不能创建 worktree 导致 LLM 回退到主工作区 checkout | 高 | 修复中 | 未确认 | 2026-05 | 根因：EnterWorktree 只支持进入已存在 worktree，工具描述未覆盖“开个 wt”的创建语义，LLM 在目标不存在时容易回退到 Bash 执行 `git checkout -b`，把主工作区切到 feature 分支。修复：EnterWorktree 目标路径不存在时默认基于 main 执行 `git worktree add` 创建并进入；path 可选，省略时从 branch 推导 `.worktrees/<安全分支名>`；工具描述明确禁止用 checkout/switch 代替 worktree。 |
| 75 | 中文输入法下 input area 输入顺序错乱（查看 → 看查） | 中 | 待确认 | 用户已验证 | 2026-05 | 已由 feature #53 TUI Model/View 迁移修复：迁移把输入数据流反向为 model→widget，删除了 input_bridge.rs 及 mirror_input_area_to_model 这条 textarea col→字节位置镜像路径，InputDocument（原生按字节维护光标）成为唯一真源，原根因结构性消失。SHOULD 在新路径补 CJK 连续输入回归测试。关联 #48/#33（CJK 字符列处理） |
| 76 | reasoning 模型 think 后 Grep 结果渲染成扁平原始行且滚动条失效 | 中 | 修复中 | 待确认 | 2026-05 | 根因：spinner 上方历史输出同时存在 legacy `OutputArea` 直接写入和新 `ConversationModel -> OutputViewModel -> OutputArea` 全量替换两条路径，用户输入、thinking、tool call 三类块格式/状态来源不一致；真实 reasoning/text block 后 `ToolCallStart.index` 可能不是 0，index 丢失会进一步导致工具块绑定失败。修复：历史输出统一从 ConversationModel 渲染，格式参照 resume（用户 `> ...`、thinking `💭 ...`、tool call 复用 ToolDisplay），runtime/sdk/CLI 透传 ToolCall index；resume 也改为加载模型后通过 ViewModel 渲染，符合新架构。 |
| 78 | input area 粘贴后按空格清空粘贴内容 | 中 | 修复中 | 未确认 | 2026-05 | 同 #77 根因：handle_paste_event 和 processing 模式 paste 均直接调用 input_area.input(ch) 修改 textarea，未走模型。后续空格触发 model.apply(InsertChar) → TextChanged → set_text，用旧文本覆盖 textarea 中的粘贴内容。修复：两处 paste 循环后添加 model.input.document.clear() + insert_text() 同步 |
| 80 | 滚动条不跟随最新内容（全量替换时 scroll_offset 累加） | 中 | 待确认（随 #58 渲染管线重构修复） | 待确认 | 2026-05 | 根因：replace_lines_from_view_model 清空全行后逐行 push_line 重建，push_line 在 auto_scroll=false 时每行 scroll_offset+=1，导致 scroll_offset 被累加到异常值，clamp 后变成 max_offset 而非 0，auto_scroll 无法恢复。修复：全量替换期间临时启用 auto_scroll=true 阻止 push_line 逐行递增 |
| 81 | TUI 输出区中文按单字竖排显示 | 高 | 待确认 | 未确认 | 2026-05 | 根因：#58 后 `refresh_output_widget_from_model` 在首次布局 rect 未就绪时用 `output_area_rect.width.saturating_sub(2).max(1)` 得到 width=1 并立即渲染文档，CJK 宽字符在 markdown wrap 中被逐字符折行。修复：ViewModel 渲染宽度在 layout width 未就绪（<=1）时回退到 OutputArea 已知 `term_width`，并补充 CJK 回归测试 |
| 82 | TUI 渲染 tool call 时丢失 theme 颜色 | 中 | 待确认 | 未确认 | 2026-05 | #58 渲染管线重构后，tool call（如 Bash/Grep/Read 等）的标题、参数、状态指示器在 TUI 中以默认前景色显示，缺少原有的 theme 颜色（如工具名高亮色、运行态动画色、完成态颜色等）；已确认新 `render_tool_call` 只给 icon 使用语义状态色，却把标题 span 固定为 `theme::TEXT`，导致工具名/标题看起来像普通文本；修复后标题与 icon 一起使用状态语义色 |
| 83 | TUI 渲染 tool call 同时输出 summary 和完整内容，重复刷屏 | 中 | 待确认 | 未确认 | 2026-05 | 二次根因：ToolResult 事件可能先于正式 ToolCall 绑定到达，ConversationModel 会先创建 OrphanToolResult；后续 ToolCall 绑定时未提升该 orphan result，导致完整结果作为块外 DiagnosticNotice 泄漏。修复：ToolCall 绑定时按 id 提升 orphan result 为 ToolResult 并完成 ToolCall；assembler 继续跳过已嵌入结果，仅保留短摘要 |
| 84 | TUI 未渲染 TaskListCreate 工具调用 | 中 | 待确认 | 未确认 | 2026-05 | 经验证渲染链完整：task_impls.rs 中 TaskListCreate/TaskCreate/TaskUpdate 等 display 均已注册，lookup_display 返回正确实现，format_tool_call 产出正确 header+details，OutputViewAssembler 正确创建 ToolCallBlockView 并渲染。新增 10 个测试覆盖 display lookup、format_tool_call、端到端 assembler 渲染三条路径。若问题仍存在，可能为事件流层（provider 未发送 ToolCallStart）或 timing 相关问题，需实际运行复现确认。修复 commit: 2de88a1 |
| 85 | Ollama provider 声明但工厂未接线（整模块死代码） | 中 | 待确认 | 未确认 | 2026-05 | provider crate 的 OllamaProvider 是完整 LlmProvider 实现（streaming/重试/非流式回退/empty-response 检测/think 控制），但 `ApiDriverKind` 缺 `Ollama` 变体、`parse("ollama")` 返回 None，client/pool 工厂 match 无 Ollama 分支；config 中 `api:"ollama"` 被 `unwrap_or(OpenAI)` 回退并经 OpenAI 兼容工厂构造，专用 OllamaProvider 永不构造（#61 D3 收窄可见性后暴露为整模块死代码）。修复：补 `ApiDriverKind::Ollama` 变体 + parse/as_str，client/pool 工厂加 Ollama 分支构造 OllamaProvider，`openai_config`/pool 排除 Ollama（防回退 OpenAI 兼容），移除 mod.rs 上的 `#[allow(dead_code)]`。修复 commit: 111393e |
| 86 | TUI tool call 顺序颠倒 | 中 | 修复中 | 未确认 | 2026-05 | 根因：①模型可能先流式输出未完成 assistant text，随后才发送 tool_use；旧逻辑 append ToolCall 导致结论文本在工具调用前。②ToolResult 事件可能早于正式 ToolCall 绑定；提升 orphan result 时旧逻辑 append ToolResult，导致结果在标题前。修复：ToolCall 绑定时插入未完成 assistant text 前；ToolResult 始终插入对应 ToolCall 后；已完成文本块不重排 |
| 87 | TUI tool call 显示完整 tool result 内容且不受 max output 限制，result 渲染格式错误 | 高 | 待确认 | 未确认 | 2026-05 | TUI 中 tool call（如 Read）曾将完整 tool result 内容输出到工具块下方，看起来像 LLM 正文从 tool call result 中刷出。修复：ToolResult 子块只展示 `ToolDisplay::format_result_summary()` 的短摘要（如 `✓ Read completed`），完整 Read 内容不进入渲染文本；assistant 正文保持独立 `AssistantMessage` block；补充回归测试覆盖 Read 完整 active.md 结果不泄漏到 ToolResult。残留（2026-05-30 修复）：嵌入/非嵌入路径已收敛，但 **OrphanToolResult 路径**（结果早于 ToolCall 绑定且未被提升）仍按 `summarize_orphan_result` 截断透传原始带行号 output，并以 `Warning`（橙）色整段刷出——表现为像正文刷屏且颜色不对。修复：`OrphanToolResult` 携带 `tool_name`，assembler 统一走 `summarize_non_embedded_result` 工具摘要，颜色随 Success/Error；删除 `summarize_orphan_result` |
| 88 | TUI Read tool call 头部下重复显示一行 `Read /path` | 中 | 待确认 | 未确认 | 2026-05 | 根因：`ReadDisplay::format_header` 已输出 `Read({path})`，`format_details` 又输出 `Read {path}`，路径在工具块重复成两行。修复：`format_details` 不再重复路径，仅在带 offset/limit 时输出 `offset: N, limit: M`（无则返回空）。回归：`test_format_tool_call_read_details_does_not_duplicate_path` 等 3 个测试 |
| 89 | TUI markdown 表格只渲染表头，分隔行与数据行原样泄漏 | 高 | 待确认 | 未确认 | 2026-05 | 根因：`render_fenced_markdown`（`primitives/fenced.rs`）的表格块收集循环 `while is_table_row(src[end])` 在遇到分隔行 `\|---\|` 时停止——`is_table_row` 对分隔行返回 false（其定义含 `&& !is_table_separator`），故 `block_src` 只含表头一行，`table()` 仅渲染表头，分隔行与全部数据行被 `idx=end` 跳过后当普通文本原样输出（屏幕上表头成表、其余是原始 `\| a \| b \|`）。表现为"模型回复正文表格不渲染"。旧测试断言过弱（仅查 `│`，表头单独即含 `│`）掩盖了该 bug。修复：收集循环改为 `while is_table_row(src[end]) \|\| is_table_separator(src[end])`，整块（表头+分隔+数据行）交给 `table()` 渲染。回归：`test_table_block_renders_separator_and_all_data_rows`（断言无原样 `\|---`、数据行带 `│` 且无原始 ASCII `\|`）|
| 90 | TUI Edit 工具结果不渲染为 diff（只显示 ✓ Edit completed 摘要） | 高 | 待确认 | 未确认 | 2026-05 | 根因：`render_edit_diff`（解析 `---DIFF---` 出加减色 diff）只在 `render_tool_result` 里被调用，但 assembler 的 `tool_result_summary` 对 Edit 走默认 `format_result_summary` 返回 `✓ Edit completed`，使嵌入式 ToolResult 子块的 `result_text` 不含 `---DIFF---`，`render_edit_diff` 解析失败回退普通文本——`render_edit_diff` 在生产里从未拿到原文（单测喂原文故绿、生产坏）。与 #87 不冲突：#87 反对的是 orphan/非嵌入结果以原样文本泄漏到块外（仍走摘要抑制）；本修复仅对**嵌入式**且含 `---DIFF---` 的结果透传原文，由 `render_tool_result` 消费标记渲成 diff。修复：`tool_result_summary` 在 `result.contains(DIFF_MARKER)` 时透传原文（`DIFF_MARKER` 提为 `pub(crate)` 复用，DRY）。回归：`test_output_assembler_late_bound_tool_result_stays_inside_tool_block` 改为端到端断言子块渲染出 `- old`/`+ new` diff 行、无 `---DIFF---` 残留、无 `Edit completed` |

### #85 Ollama provider 声明但工厂未接线（整模块死代码）

**状态**：待确认

**症状**：`agent/provider/src/providers/ollama/` 是一个完整的 `OllamaProvider` 实现（856 行：带重试/取消的 `stream_message`、非流式回退、空响应检测、`think:false` reasoning 控制、model/max_tokens 管理），但全代码库零构造点。#61 D3 收窄 provider crate 可见性、移除 crate-root `pub use` 后，该模块以 `#[allow(dead_code)]` 暴露为整模块死代码。

**根因（已确认，属接线遗漏）**：
1. `ApiDriverKind` 枚举只有 5 个变体（Anthropic/OpenAI/Zhipu/LiteLLM/Volcengine），**无 `Ollama` 变体**；`ApiDriverKind::parse("ollama")` 返回 `None`。
2. 两个客户端工厂（`client.rs::with_provider`、`pool.rs::create_client`）的 `match` 均无 Ollama 分支。
3. 因此 config 中 `api:"ollama"` 在 `from_args.rs:68` / `pool.rs:120` 处被 `unwrap_or(ApiDriverKind::OpenAI)` 静默回退到 OpenAI；同时 `openai_config()` 仅排除 Anthropic，会给 Ollama 生成 openai_config，使 `from_config` 把它路由到 `OpenAICompatibleProvider`。
4. 结论：专用 `OllamaProvider` 永不构造，Ollama 模型实际走通用 OpenAI 兼容路径，丢失 Ollama 的长超时与空响应处理。CLAUDE.md 架构约定 Provider 支持列表含 Ollama，确认为接线遗漏 bug 而非半成品。

**修复**：
- `api.rs`：补 `ApiDriverKind::Ollama` 变体 + `parse("ollama")` / `as_str()=="ollama"`。
- `client.rs::with_provider`：加 `ApiDriverKind::Ollama => OllamaProvider::new(...)` 分支；`from_api_driver` 的 suffix match 把 Ollama 归入 OpenAI 兜底。
- `pool.rs::create_client`：`openai_config` 与 api_key env match 排除/覆盖 Ollama，使其经 `from_config` 走专用 provider。
- `provider_client.rs::openai_config`：Anthropic | Ollama 均不生成 openai_config；env 名补 `OLLAMA_API_KEY`。
- `providers/openai_compatible/driver.rs::driver_for_api`：Ollama 归入 OpenAI 驱动兜底（防御性，实际不经此路径）。
- `providers/mod.rs`：移除 `#[allow(dead_code)]`，恢复 `pub use ollama::OllamaProvider`。
- 重现测试（修复前失败）：`provider_client.rs` 的 `test_build_llm_client_ollama_constructs_ollama_provider`（config `api:"ollama"` → `client.provider_name()=="ollama"`）、`test_openai_config_skips_ollama`、`test_provider_api_key_env_name_ollama`；`api.rs` 的 `test_from_str_ollama`、`test_as_str_ollama_roundtrip`。

### #86 TUI tool call 顺序颠倒

**状态**：待确认

**症状**：TUI 中 tool call 渲染顺序颠倒，表现包含两类：
1. LLM 响应流中先输出 assistant text block（结论/总结），随后才输出 tool_use block，用户会先看到结论文本，再看到 tool call 执行过程。
2. ToolResult 事件先于正式 ToolCall 绑定到达时，用户会先看到工具执行结果，再看到 `✓ Read(...)` 等 tool call 标题行。

**根因（已确认）**：
1. `ConversationModel::append_or_extend_text_block()` 会立即把流式 assistant 文本追加为 `AssistantText` block，并记录 `active_text_block_id`；后续 `ObserveToolCall` 绑定正式 tool call 时，旧逻辑总是 `blocks.push(ToolCall)`，因此未完成的 assistant 文本会固定排在后到达的工具调用之前。
2. `ToolResult` 事件可能早于正式 `ToolCall` 绑定到达，旧逻辑先创建 `OrphanToolResult`；后续提升 orphan result 时直接 append `ToolResult`，而 `ToolCall` 已按 active text 规则插入到更靠前位置，导致结果块显示在工具标题之前。

**修复**：
1. `ObserveToolCall` 绑定时通过 `insert_tool_call_block_before_active_text()` 插入 block：若当前存在未完成 assistant text block，则把 ToolCall 插入该 active text block 之前；若文本块已通过 `CompleteTextBlock` 完成，保持原有 append 行为。
2. `ToolResult` 统一通过 `insert_tool_result_after_tool_call()` 插入：若对应 ToolCall block 已存在，则结果块紧跟标题之后；否则才 append/orphan，避免结果先于标题显示。

**回归测试**：
1. `test_conversation_places_late_tool_call_before_pending_assistant_text`：先收到 assistant 文本、后收到 ToolCall 时，ToolCall block 应显示在未完成文本之前。
2. `test_conversation_keeps_tool_after_completed_assistant_text`：已完成 assistant 文本后再收到 ToolCall，不应重排到文本之前。
3. `test_conversation_places_tool_result_after_late_bound_tool_call`：ToolResult 先到、ToolCall 后绑定时，结果仍显示在标题之后。
4. `test_conversation_keeps_tool_result_after_existing_tool_call`：正常 ToolCall 后再收到 ToolResult 时，结果紧跟标题之后。

**涉及路径**：
- `apps/cli/src/tui/model/conversation/model.rs`
- `apps/cli/src/tui/model/conversation/tool_flow.rs`
- `apps/cli/src/tui/model/conversation/model_tests.rs`

### #81 TUI 输出区中文按单字竖排显示

**状态**：待确认

**症状**：进入/恢复 TUI 后，上一条 assistant 中文内容被按单字拆成多行显示，例如“理 / 一 / 轮 / ， / 不 / 改 / 代 / 码 / 。”；同屏后续 `system-reminder` 和工具输出仍能正常横向显示。

**根因（已确认）**：#58 输出区渲染管线切到 `ConversationModel -> OutputViewModel -> OutputDocumentRenderer` 后，`refresh_output_widget_from_model` 使用 `layout.output_area_rect.width.saturating_sub(2).max(1)` 作为渲染宽度。首次进入/恢复会话时，frame 尚未 draw，`output_area_rect` 仍是默认 `Rect::default()`，于是渲染宽度变成 1；中文 CJK 字符显示宽度为 2，markdown wrap 在 width=1 下每个字符都会独立成行，形成逐字竖排。

**修复**：`render_document_from_view_model` 在传入 layout width 未就绪（<=1）时，不再直接用 1 渲染，而是回退到 `OutputArea` 已知的 `term_width`。这样 resize 已提供终端宽度但首帧 layout rect 尚未更新时，assistant 中文文本仍按正常宽度渲染。

**回归测试**：
1. `test_assistant_cjk_text_does_not_wrap_per_character_at_normal_width`：正常 80 宽下，`整理一轮，不改代码。` 不应逐字折行。
2. `test_render_document_from_view_model_uses_known_term_width_when_layout_width_unready`：先 `handle_resize(80, ...)`，再模拟 layout width=1 刷新 ViewModel，断言中文 assistant 文档仍只有一行。

**涉及路径**：
- `apps/cli/src/tui/adapter/output_widget.rs`
- `apps/cli/src/tui/render/output/blocks/assistant_message.rs`

### #82 TUI 渲染 tool call 时丢失 theme 颜色

**状态**：待确认

**症状**：#58 渲染管线重构后，TUI 中 tool call（如 Bash/Grep/Read 等）的标题、参数、状态指示器以默认前景色显示，缺少原有的 theme 颜色（如工具名高亮色、运行态动画色、完成态颜色等），所有工具调用看起来像纯文本，无视觉区分。

**根因（已确认）**：新渲染管线中 `render_tool_call` 已按 `ToolCallBlockView.style` 给状态 icon 应用语义状态色（Running/Success/Error 等），但工具标题 span 固定使用 `theme::TEXT`。因此 `●`/`✓` 仍有颜色，`Bash`/`Grep`/`Read(...)` 等工具名和标题看起来像普通文本，造成 tool call theme 颜色丢失。

**修复**：将 tool call header 标题 span 的前景色从 `theme::TEXT` 改为与 icon 一致的 `icon_color`，即由 `semantic_color(view.style)` 派生。这样 running/success/error/cancelled/orphaned 等状态下，状态指示器和工具标题共享对应 theme 颜色。

**回归测试**：
1. `test_tool_call_running_applies_theme_color_to_icon_and_title`：Running 状态下 icon 与标题均使用 `theme::TOOL_RUNNING`。
2. `test_tool_call_success_uses_success_icon_color`：Success 状态下 icon 与标题均使用 `theme::SUCCESS`。

**涉及路径**：
- `apps/cli/src/tui/render/output/blocks/tool_call.rs`

### #83 TUI 渲染 tool call 同时输出 summary 和完整内容，重复刷屏

**状态**：待确认

**最新反馈（2026-05）**：用户确认问题仍存在。当前可见表现为 Read 工具渲染 `✓ Read(...)`、`Read <path>`、`✓ Read completed` 后，仍继续把完整文件内容（例如 `docs/bug/active.md` 表格行）显示在工具块内；另有 Edit 等工具的块内容（如 `replaced ...` 和 `---DIFF---`）出现在工具块外面的情况。已修复 ToolResult 早于正式 ToolCall 绑定时 orphan result 未提升的问题，待用户确认。

**症状**：所有工具（Read/Grep/Bash/Edit 等）的 tool result 在 TUI 工具块结果摘要区展示完整内容，导致长输出重复且刷屏。例如 Read 工具先显示 `✓ Read(...)` + `Read <path>` 详情行，紧接着又在工具块内把整个文件内容原样输出一遍。

**根因（已确认）**：
1. `ToolDisplay` trait 已定义 `format_result_summary()`，但 #58 新管线中的 `OutputViewAssembler::find_tool_view()` 曾未调用它。
2. `find_tool_view()` 直接把完整 tool result 塞入 `ToolCallBlockView.result_summary`，该字段会渲染在工具块结果摘要区。
3. 已绑定 tool call 的独立 `ToolResult` block 会被 `tool_result_is_embedded()` 跳过，不会再输出完整内容；重复刷屏实际来自工具块 summary 字段承载了完整结果。
4. ToolResult 事件可能先于正式 ToolCall 绑定到达；旧逻辑先创建 `OrphanToolResult`，后续 ToolCall 绑定时没有按 id 提升该 orphan result，导致完整结果继续作为块外 `DiagnosticNotice` 渲染。

**修复**：`find_tool_view()` 改用 `ToolDisplay::format_result_summary()` 生成短摘要；未注册 display 时回退为 `✓ <tool> completed` / `✗ <tool> failed`。完整 tool result 仍保存在 conversation/tool call 中供模型上下文使用，但不直接作为 TUI summary 输出。ToolCall 绑定时如果发现同 id 的 orphan result，会先移除 orphan block、完成 active tool，再插入可被 assembler 去重的 `ToolResult` block，避免完整结果泄漏到工具块外。

**回归测试**：
1. `test_output_assembler_summarizes_embedded_tool_result_without_full_output`：Read 多行完整结果不会进入 `result_summary`。
2. `test_output_assembler_uses_error_summary_for_failed_tool_result`：错误结果显示失败短摘要。
3. `test_output_assembler_keeps_tool_result_inside_tool_after_thinking`：thinking 后绑定工具结果不生成独立 DiagnosticNotice，仍嵌入工具块。
4. `test_conversation_late_tool_call_binds_existing_result`：ToolResult 先到达、ToolCall 后绑定时，orphan result 会被提升并绑定回工具调用。
5. `test_output_assembler_late_bound_tool_result_stays_inside_tool_block`：Edit diff 等完整结果不会作为块外诊断文本泄漏。

**涉及路径**：
- `apps/cli/src/tui/view_assembler/output.rs`（`find_tool_view`、result summary 生成）
- `apps/cli/src/tui/model/conversation/model.rs`（ToolCall 绑定时触发 orphan result 提升）
- `apps/cli/src/tui/model/conversation/tool_flow.rs`（ToolResult / orphan result 流转）
- `apps/cli/src/tui/render/output/tool_display/mod.rs`（`ToolDisplay::format_result_summary`）

### #84 TUI 未渲染 TaskListCreate 工具调用

**状态**：活动中

**症状**：LLM 调用 TaskListCreate 时，TUI 输出区无任何可视化反馈（无 spinner、无标题、无结果），用户完全看不到 task list 的创建过程和结果内容。

**根因假设**：
1. `ToolDisplay` registry（`tool_impls.rs`）中可能未注册 TaskListCreate 的 display 实现，`lookup_display("TaskListCreate")` 返回 `None` 后 `find_tool_view` 静默跳过渲染。
2. TaskListCreate/TaskCreate/TaskUpdate 等 task 管理工具可能属于 SDK 层定义的虚拟工具，不经过标准 tool call 渲染路径，需要单独处理。
3. 类似地，其他 task 管理工具（TaskCreate、TaskUpdate、TaskList、TaskGet、TaskStop 等）可能也未被渲染。

**修复方向**：
1. 在 `tool_impls.rs` 中为 TaskListCreate 注册 `ToolDisplayEntry`，实现标题格式和结果摘要（如显示 task list subject、task 数量等）。
2. 检查并补全其他 task 管理工具的 display 注册。
3. 或在 `OutputViewAssembler` 中为未注册工具提供 fallback 渲染（显示工具名 + 简短结果）。

**涉及路径**：
- `apps/cli/src/tui/render/output/tool_display/tool_impls.rs`（TaskListCreate display 注册）
- `apps/cli/src/tui/render/output/tool_display/task_impls.rs`（已有 task 工具 display 实现？）
- `apps/cli/src/tui/view_assembler/output.rs`（fallback 渲染逻辑）

### #76 reasoning 模型 think 后 Grep 结果渲染成扁平原始行且滚动条失效

**状态**：修复中（待确认）

**症状**：使用 reasoning 模型（截图为 DeepSeek-V4-Pro）时，模型输出 thinking 块后紧随的 Grep 工具结果在 TUI 中显示异常：

1. Grep 结果渲染成**扁平的原始文本行**，每行带完整绝对路径前缀（`/Users/.../docs/bug/active.md:N:内容`），没有正常的 `● Grep` 工具调用头和缩进，可读性差。
2. 输出区上方混入上一次 Read 工具输出的 `24` / `25` / `26` 行号碎片，未被正确清理/分隔。
3. **问题出现时滚动条失效**，无法滚动查看输出区。

**复现**：
1. 使用 reasoning 模型（如 DeepSeek-V4-Pro），触发一次包含 thinking 块的回复。
2. thinking 块后让模型执行 Grep（或其他工具）。
3. 观察 Grep 结果是否渲染为扁平原始行、是否混入前序输出碎片、滚动条是否失效。

**根因（已确认）**：
1. spinner 上方历史输出同时存在 legacy `OutputArea` 直接写入和新 `ConversationModel -> OutputViewModel -> OutputArea` 全量替换两条路径，用户输入、thinking、tool call 三类块格式/状态来源不一致；这会造成用户看到 `You:`、thinking 无 `💭`、tool call/result 又被后续模型刷新覆盖或变成扁平行。
2. resume 的视觉格式本身是正确参照（用户 `> ...`、tool call 复用 `ToolDisplay`），但旧 resume 仍直接写 `OutputArea`，不符合新架构，容易与 live path 分裂。
3. 真实 reasoning/text block 后 `ToolCallStart.index` 可能不是 0，但 runtime/sdk/CLI 传递正式 `ToolCall` 事件时丢失 index，CLI mapper 只能硬编码 index=0，导致正式 ToolCall 绑定不到 spinner 上方的 pending 工具块，后续 ToolResult 变成 orphan/扁平诊断行。
4. `ToolResult` 同时嵌入 `ToolCall.result_summary` 并作为独立 block 存在，若 assembler 不去重，会额外生成 `DiagnosticNotice`，进一步放大扁平文本重复。
5. ViewModel 全量替换 `OutputArea` 时若不清理 `screen_line_map` / selection / rendered cache 并 clamp `scroll_offset`，旧渲染窗口会残留，表现为滚动条失效或前序碎片混入。

**修复**：
1. ✅ spinner 上方历史输出统一由 `ConversationModel -> OutputViewModel -> OutputArea` 生成，live path 不再对用户输入、assistant/thinking streaming、tool call/result 同时做 legacy OutputArea 双写。
2. ✅ ViewModel 渲染格式参照 resume：用户输入为 `> ...`，thinking 为 `💭 ...`，tool call/result 复用 `ToolDisplay` 格式。
3. ✅ resume 不再直接写 `OutputArea`，改为把历史消息加载进 `ConversationModel`，再通过 ViewModel 刷新，既保持正确视觉格式，也符合新架构。
4. ✅ runtime `ToolCall.index` 全链路透传到 `RuntimeStreamEvent::ToolCall`、`sdk::ChatEvent::ToolCall`、`UiEvent::ToolCall` 和 `ConversationIntent::ObserveToolCall`，确保非 0 index 的正式 ToolCall 能绑定 pending 工具块。
5. ✅ `OutputViewAssembler` 跳过已嵌入 `ToolCall` 的 `ToolResult`，避免重复 DiagnosticNotice。
6. ✅ `output_adapter` 在替换 lines 时清理 selection、screen map、rendered text cache，并 clamp `scroll_offset` / rendered cache window。
7. ✅ 补充端到端回归：模拟真实 update_enter 后 thinking→Grep 事件，其中 `ToolCallStart.index=1`，验证 OutputArea 实际行包含 `> 用户输入`、`💭 thinking`、工具头、参数、截断结果且无 `You:` 和裸路径扁平行。
8. 待用户用 DeepSeek-V4-Pro 实机确认。

**涉及路径（预计）**：
- `apps/cli/src/tui/output_area/`（tool result 渲染、渲染缓存 block state、滚动/viewport 计算）
- thinking / reasoning 块渲染与状态复位逻辑
- 关联 #65（fenced code block 样式泄漏）、#74（System 色泄漏）、#71（渲染缓存越界）

### #75 中文输入法下 input area 输入顺序错乱（查看 → 看查）

**状态**：待确认（已由 TUI Model/View 迁移修复，用户已验证）

**根因（已确认）**：旧架构下 `mirror_input_area_to_model`（已删除的 `apps/cli/src/tui/core/input_bridge.rs:14`）把数据流方向定为 widget→model：将 tui_textarea 的 `cursor_position()` 返回的光标列号（col，字符索引）直接作为 `InputDocument::move_cursor()` 的字节位置使用。对 CJK 多字节字符（如"看"占 3 字节），字符索引 1 ≠ 字节位置 3，col=1 落在多字节字符中间被 `clamp_to_char_boundary` 修正到 0，导致下一个字符插入到现有字符之前（位置 0），造成顺序颠倒。

**修复（随 feature #53 TUI Model/View 迁移）**：迁移把输入数据流**反向**为 model→widget，删除了 `input_bridge.rs` 及 `mirror_input_area_to_model` 这条 textarea col→字节位置 的镜像路径。现在 InputModel 的 `InputDocument`（原生按字节维护光标）是唯一真源，通过 `input_adapter.rs::apply_input_changes_to_widget` 的 `input_area.set_text(整串文本)` 推给 widget，不再用 textarea 列号反推字节位置，原根因结构性消失。曾在旧文件用 `textarea_cursor_to_byte_pos` 做过临时修复，但随旧文件一并删除，已无关紧要。

**涉及文件**：`apps/cli/src/tui/core/input_adapter.rs`（新 model→widget 适配，替代已删除的 `input_bridge.rs`）。

**遗留**：SHOULD 在新 InputModel 输入路径补一条连续输入多个 CJK 字符（如"查看"）的回归测试，断言 buffer 顺序正确，防止再次回归。

**验证**：`cargo test -p cli` 322 测试全通过，新增 11 个测试覆盖：ASCII 光标、CJK 单字光标、CJK 双字顺序保留、中西混排、多行、边界条件。

**症状**：启用中文输入法（IME）时，在 TUI input area 输入异常。输入“查看”后，input 中实际显示为“看查”，即字符顺序被颠倒；输入查看时看到的是颠倒后的结果。

**复现**：
1. 在 TUI input area 切换到中文输入法。
2. 输入一个由多个汉字组成的词（如“查看”）。
3. 观察 input 中显示的字符顺序与输入顺序相反（显示为“看查”）。

**根因假设**：
1. IME 组合输入（composition / 预编辑串）一次性 commit 多个 CJK 字符时，input buffer 的插入逻辑可能逐字符插入在同一光标位置之前，导致后插入的字符排在前面，整体顺序被颠倒。
2. 终端在中文输入法下可能将一次 commit 拆成多个字符事件按序送达，input area 的字符插入 / 光标推进处理未正确累加偏移，新字符插到了已插入字符之前。
3. 与 CJK 宽字符的字节 / 字符列处理相关（关联 #48 selection-offset-cjk、#33 CJK 拖选高亮），插入位置按列或按字节计算时对多字节字符处理有误。

**修复方向**：
1. 定位 input area 接收键盘 / 字符事件并写入 buffer 的路径，确认一次 commit 多字符时是否按正确顺序、正确光标偏移插入。
2. 添加日志记录每个进入 input buffer 的字符事件（原始字节 / 字符、当前光标位置、插入后 buffer 内容），按调试原则先观测再修复。
3. 补充回归：模拟连续输入多个 CJK 字符（如“查看”），断言 buffer 内容与光标位置与输入顺序一致。

**涉及路径（预计）**：
- `apps/cli/src/tui/` input area 字符输入 / 光标推进逻辑
- `agent/share/src/string_idx`（CharIdx 等统一字符索引）相关插入计算
- 关联 #48、#33（CJK 字符处理）

### #49 last turn 时用户提交的内容不会发给 LLM，留在 input queue 区域

**状态**：修复中（待确认）

**本轮症状**：Stop hook 执行期间，用户提交的新输入能进入 TUI input queue，但 Stop hook 结束后 runtime 直接发送 `DoneWithDuration`，该输入不会被追加进 messages，也不会触发下一轮 LLM。

**根因（已确认）**：最终响应分支在进入 Stop hook 之前已经执行过一次 `append_queued_input`；但 Stop hook 本身可能耗时，用户在 hook 执行期间提交的输入发生在最后一次 drain 之后、`DoneWithDuration` 之前。原实现把 Stop hook 和 Done 发送都封装在 `finalize_main_loop` 内，Stop hook 通过后没有再给主 loop 二次 drain 的机会。

**修复**：Completed 分支改为：Stop hook 通过后先再次调用 `append_queued_input`；如果 drain 到用户输入，则同步 messages 并 `continue` 主 LLM loop；只有二次 drain 为空时才发送 `DoneWithDuration` 并归档已完成 task batch。追加输入被处理完后仍会重新进入最终结束流程并再次触发 Stop hook。

**验证**：新增 `test_process_chat_loop_drains_input_after_stop_hook_before_done`，覆盖 Stop hook 通过后才出现 queued input 时应继续下一轮 LLM；`cargo test -p runtime` 通过。

### #73 EnterWorktree 不能创建 worktree 导致 LLM 回退到主工作区 checkout

**状态**：修复中（待确认）

**症状**：用户要求“开个 wt”时，LLM 知道需要 worktree，但 `EnterWorktree` 只能进入已存在路径，目标不存在时模型回退到 Bash 手动执行 `git checkout -b` / `git worktree add` 组合，容易先把主工作区切到 feature 分支，后续 worktree 创建因分支已被占用失败。

**根因（已确认）**：
1. `EnterWorktree` 工具语义只覆盖“进入已有 worktree”，没有覆盖用户自然语言里的“开 worktree”。
2. 工具描述未明确禁止在主 checkout 中用 `git checkout -b` / `git switch -c` 代替 worktree。
3. 目标不存在时需要 LLM 自己组合 Bash 命令创建，再调用 `EnterWorktree`，增加误操作和 token 成本。

**修复**：
1. `EnterWorktree` 目标路径不存在时默认基于 `main` 创建 worktree 并进入。
2. 移除 `base` 参数，避免 LLM 选择错误基线；`path` 改为可选，省略时从 `branch` 推导 `.worktrees/<安全分支名>`。
3. 推导 path 时仅保留 `A-Z` / `a-z` / `0-9` / `.` / `_` / `-`，路径分隔符和敏感字符压缩替换为 `-`。
4. 工具描述和 schema 明确：开 worktree 必须调用 `EnterWorktree`，禁止在主 checkout 用 checkout/switch 代替。
5. 补充测试覆盖显式 path+branch 创建、branch 推导 path、敏感字符替换、缺少 path/branch 报错、嵌套进入拒绝和 schema。

### #72 agent 双层循环中一轮结束后不自动读取 input queue

**状态**：修复中（待确认）

**症状**：agent 主循环由双层构成：外层 LLM loop（每次 LLM 调用为一次迭代），内层 tool execution loop（并发执行本轮所有 tool_use）。当一轮结束后（LLM 返回最终文本 or 工具全部执行完成、准备下一轮 LLM 调用之前），agent 不会自动 drain 读取 input queue（`AgentInput::UserMessage` / 用户通过 TUI 发送的新消息），导致用户中途发送的输入被忽略或延迟到循环自然结束才被处理。

**复现**：
1. 向 agent 发送一个会触发多轮工具调用的复杂请求（如「分析整个项目结构」）。
2. 在 agent 执行首轮工具期间，通过 TUI 发送一条新消息（如「停，只分析 src 目录」）。
3. 观察 agent 是否在当前工具执行完成后立即处理用户新输入，还是继续原有 LLM loop 直到任务自然结束才响应。

**根因假设**：
1. 外层 LLM loop 的迭代条件只判断「是否收到 LLM 最终响应」和「是否还有 tool_use」，没有在每轮开始前主动 drain input queue。
2. input queue 在 runtime 启动时已建立，但主循环的 tick 入口没有在每轮之间调用 `recv` / `try_recv` 来检查新消息。
3. input queue 的读取被耦合在某个更内层的位置（如 tool execution 完成后），导致只有特定时机才会消费。
4. 双层循环结构（LLM loop + tool loop）使得「一轮结束」的定义不够明确：工具执行完到下一次 LLM 调用之间的窗口没有被用于检查 input queue。

**根因（已确认）**：
1. P13 TUI/Runtime SDK 解耦后，TUI 的排队输入读取端口停留在 CLI 层：`TuiQueueDrainPort` 只在 `spawn_processing` 收到 `Done` / `DoneWithDuration` 后兜底 drain，并通过 `sync_current_messages` 写回 session。
2. `AgentClientImpl::chat` 启动 `process_chat_loop` 时固定传入 `EmptyQueueDrainPort`，导致 runtime chat loop 内既有的 `append_queued_input` 调用（工具轮完成后、最终响应前、取消/API error 等路径）永远只能得到 `None`。
3. 因此 bug 不在 `process_chat_loop` 的轮间检查缺失，而在 SDK 边界没有把 TUI queue drain 端口传给 runtime loop，轮间检查被空实现短路。

**修复**：
1. `sdk::ChatRequest` 新增可选 `queue_drain` 端口，非 TUI 调用保持 `None`。
2. `apps/cli` 发起 chat 时注入 `TuiQueueDrainPort`，并让该端口实现 `sdk::QueueDrainPort`。
3. `agent/runtime` 新增 `RuntimeQueueDrainPort`，把 SDK queue drain 端口适配为 runtime `chat::QueueDrainPort` 后传入 `process_chat_loop`。
4. 补充回归测试覆盖 `RuntimeQueueDrainPort` 能转发 SDK queue 读取，以及无 queue 时安全返回 `None`。

**修复方向**：
1. 在外层 LLM loop 每轮迭代开始前增加 `input_queue.try_recv()` 检查，若有新消息则注入到 messages 列表，重新进入 LLM loop。
2. 明确「一轮结束」的定义：LLM 返回无 tool_use 的最终响应 OR 所有 tool_use 执行完成 → 应在此时 drain input queue 一次。
3. 考虑将 input queue drain 做成一个独立函数（`drain_input_queue()`），在以下时机调用：
   - 每轮 LLM 调用之前
   - 所有 tool 执行完成后、下一轮 LLM 调用之前
   - 外层 loop 的 while 条件判断中
4. 补充回归：agent 执行多轮工具期间用户可随时插入新消息，agent 应在当前轮结束后立即处理；若 input queue 为空则继续原有逻辑。

**涉及路径（预计）**：
- `agent/runtime/src/agent.rs`（主循环 tick / LLM loop / tool loop 入口）
- `agent/runtime/src/chat/`（chat 事件处理与 input queue 建立）
- `agent/runtime/src/chat/looping/`（循环控制与迭代逻辑）

### #69 worktree 中 LLM 仍尝试搜索主分支路径

**状态**：修复中（待确认）

**症状**：进入 worktree 后，LLM 调用 `Glob` / `Grep` / `Read` 等工具时，仍然传入 main 工作区的绝对路径作为搜索/读取目标，触发 workspace 边界保护错误：

```text
✗ Glob(docs/bug/active.md)
  ✗ Search path '/Users/guoyuqi/Nextcloud/work/claudecode/aemeath' is outside the workspace '/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/bug-67-resume-tui'.
```

工具被正确拦截（安全机制工作正常），但 LLM 反复重试同样的越界路径，迫使用户/agent 自行纠正路径，影响 worktree 工作流效率。

**复现**：
1. 在 `.worktrees/<branch>` 目录中启动 TUI/runtime。
2. 触发任意需要文件搜索的工具调用（Glob/Grep/Read 相对或绝对路径）。
3. 观察 LLM 是否会传入 main 工作区根绝对路径（而不是当前 worktree 路径）。
4. 若传入主分支路径，工具返回 workspace 越界错误；LLM 通常需要多轮才意识到。

**根因假设**：
1. 系统提示/上下文中显示的「Working directory」仍是 main 工作区根，而 workspace 边界实际指向 worktree 路径，两者不一致导致 LLM 选错路径基准。
2. 项目记忆/历史会话中保留了 main 路径作为常用根，LLM 偏向复用而非以当前 cwd 为准。
3. 工具描述未明示「优先使用相对路径或当前 workspace 根」，LLM 倾向用绝对路径，且绝对路径模板取自项目根。
4. EnterWorktree/cwd 切换后未同步更新 system reminder 中的 cwd 字段，导致 LLM 看到的 cwd 仍为外层。

**根因（已确认）**：
1. 仅在静态 system prompt 中写入具体 `Current workspace root` 会在会话中途 `EnterWorktree` / `ExitWorktree` 后变成旧值，反而可能误导 LLM。
2. 当前 workspace 的实时状态源是执行中的 workspace context（`path_base` / `working_root` / context stack），它会被 Enter/ExitWorktree 修改，并被文件/搜索工具用于相对路径解析和安全边界。
3. 因此 LLM 需要通过 Enter/ExitWorktree 的 tool result 获取最新 `path_base` / `working_root`，而不是依赖 system prompt 或额外 reminder 动态注入。

**修复**：
1. 静态 system prompt 去掉具体 `Current workspace root`，只保留通用规则：工具路径优先使用相对路径；绝对路径必须位于当前 workspace；不要复用其他 checkout/main/worktree/历史会话中的绝对路径。
2. `EnterWorktree` / `ExitWorktree` 成功结果统一输出当前 `path_base`、`working_root`、分支和后续路径使用规则，直接在 tool result 中告诉 LLM 最新 workspace context。
3. `validate_search_path_from_base` 与文件路径越界错误继续补充恢复建议：优先使用相对路径或当前 workspace，下次不要重试同一个外部绝对路径。
4. 新增回归测试覆盖静态 prompt 不再包含固定 workspace root、以及 worktree tool result 包含 `path_base` / `working_root` 与路径提示。

**修复方向**：
1. 在系统提示/会话上下文中明确标注当前 workspace 根与 cwd，并与工具 workspace 边界保持一致；EnterWorktree 后必须同步刷新这两个字段。
2. 工具描述/指南补充：在 worktree 中应优先使用相对路径，或使用工具提供的 workspace 根变量，不要硬编码项目根绝对路径。
3. 工具边界报错信息中明确给出当前 workspace 根与建议替换路径，方便 LLM 一次纠正。
4. 评估在 LLM system context 中加入「禁止跨 workspace 越界搜索」的硬性指令，并补充正反样例。

**涉及路径（预计）**：
- worktree/cwd 切换后 system reminder/context 中 cwd 字段刷新
- Glob/Grep/Read 等工具的 workspace 边界判定与报错文案
- 工具描述/项目指南中关于 worktree 工作路径的提示
- EnterWorktree/ExitWorktree 上下文栈与 cwd 同步逻辑


### #71 TUI 渲染缓存越界 panic（len 10000 / index 10000）

**状态**：待确认（随 #58 渲染管线重构结构性修复）。新管线消除了 `render_range`/`rendered_lines` 行下标越界路径，输出区只剩 `document: RenderedDocument`，滚动夹取由 `adapter/output_widget.rs::clamp_scroll_state` 用 `total_lines().saturating_sub(visible_height)` 完成；全仓 `render_range`/`OutputLine`/`LineStyle` 零残留。unsafe guard 覆盖问题另行评估。

**症状**：长会话中 TUI 直接崩溃，panic 信息：

```text
[PANIC] index out of bounds: the len is 10000 but the index is 10000 at apps/cli/src/tui/output_area/rendered_lines.rs:98:21
```

`rendered_lines.rs:98` 是 `collect_table_ranges` 外层循环 `while i < end { let line = &lines[i]; ... }`。`lines.len() == 10000`（等于 `MAX_LINES`），而 `i == 10000`，说明传入的 `end` 大于 `lines.len()`。

**影响**：输出区累计行数达到上限（`output_area/types.rs: MAX_LINES = 10000`）后，任意触发渲染（滚动、流式追加、resize）都可能 panic，整个 TUI 进程崩溃退出。崩溃发生在正常 agent loop 收尾之前，**Stop hook（架构守卫 / 单测 / build）也因此来不及执行**，表现为"stop hook 没有生效"。

**根因（假设）**：
1. 输出区内容是上限 10000 行的 `VecDeque`（`content.rs`：超过 `MAX_LINES` 时从头部 pop）。
2. `RenderedLineCache`（`rendered_cache.rs`）以行下标缓存渲染结果，并维护 `render_start` / `render_end` 渲染区间。
3. `ensure_rendered` 的 dirty 分支用 `block_start` / `block_end`（均 ≤ `total`）调用 `render_range`，是安全的；但**增量分支**直接用 `self.render_start` / `self.render_end` 作为 `render_range` 的区间端点。当 `lines` 长度因到达上限或裁剪发生变化、而 `render_start` / `render_end` 仍是旧值（> 当前 `total`）时，`render_range` 收到 `end > lines.len()`，在 `collect_table_ranges` 处越界。
4. `content_changed` 只 `truncate` 了 `cache`，没有同步 clamp `render_start` / `render_end`。

**复现方向**：
1. 制造超过 10000 行的输出（长会话或大量工具结果），使 `VecDeque` 持续从头部裁剪。
2. 在裁剪发生后触发滚动 / resize / 流式追加，观察是否 panic。

**修复方向**：
1. `ensure_rendered` 在使用 `render_start` / `render_end` 作为渲染端点前，统一 clamp 到当前 `lines.len()`（`total`）。
2. `content_changed` / `truncate` 时同步收缩 `render_start` / `render_end`，避免端点滞留旧值。
3. 在 `render_range` / `collect_table_ranges` 入口对 `end` 做 `end.min(lines.len())` 防御，杜绝越界 panic。
4. 补充回归：构造 `lines.len()` 缩小但 `render_start`/`render_end` 仍为旧值的缓存状态，断言 `ensure_rendered` 不 panic。

**涉及路径**：
- `apps/cli/src/tui/output_area/rendered_lines.rs`（`render_range` / `collect_table_ranges` 越界点）
- `apps/cli/src/tui/output_area/rendered_cache.rs`（`ensure_rendered` 增量分支、`content_changed` 端点同步）
- `apps/cli/src/tui/output_area/content.rs`、`types.rs`（`MAX_LINES` 裁剪逻辑）

#### 关联问题：unsafe string guard 覆盖不全，抓不到本 panic

**背景**：项目用 Stop hook `check-unsafe-text-ops.sh` 强制 TUI 代码使用统一安全字符串/索引（`crate::tui::display::safe_text`），避免按字节切片 UTF-8、`chars().nth()`、区间切片等导致的 panic。目标是"每次代码修改都检查不合适的字符串用法"，但本次 panic 暴露了 guard 的覆盖缺口——它根本抓不到 `lines[i]` 这类裸下标。

**现状验证（2026-05-27）**：
- TUI 范围内脚本运行通过（0 violations），`safe_text.rs` 在 `apps/cli/src/tui/display/safe_text.rs`，TARGET 与豁免路径有效。
- 无滥用 `allow unsafe_text_op` 逃逸注释（TUI 内 0 处）。
- 即脚本本身在 TUI 范围内没有被绕过，问题在于"检查范围/触发时机/匹配规则"不足。

**缺口**：
1. **扫描范围只限 `apps/cli/src/tui`**：DDD 重构把大量逻辑搬到 `agent/`、`packages/`，那里的原始字节/字符切片完全不被检查，例如：
   - `agent/runtime/src/compact/summary.rs:174` `response_text[start..end]`
   - `agent/runtime/src/compact/summary.rs:87` `&result[head_protect..split_point]`
   - `agent/runtime/src/chat/reflection.rs:152` `&text[start + 7..]`
   项目已有统一安全字符串模块 `agent/share/src/string_idx`（`CharIdx`），但 guard 未在 TUI 之外强制使用。
2. **只在 Stop hook 触发，非每次编辑触发**：无 PreToolUse/PostToolUse hook，仅在 agent loop 收尾跑；若本轮异常结束（如本 panic）则整体跳过。
3. **正则漏掉裸单下标 `slice[i]`**：当前只匹配 `.chars().nth(`、`&x[a..b]`、`x[a..b]`（区间切片），不匹配 `v[i]`。本 panic 的 `lines[i]` 越界正属此类，guard 永远抓不到。

**guard 修复方向**：
1. 扩展扫描范围到核心 crate（`agent/`、`packages/`），并引导改用 `agent/share/src/string_idx`；保留 `allow unsafe_text_op` 逃逸应对已知安全的按字节边界切片，避免误报阻塞。
2. 正则补充裸单下标 `ident[ident]` 检测（区分常量/已知边界场景，必要时配合 allow 注释）。
3. 评估增加 PreToolUse/PostToolUse 或 pre-commit 级检查，使字符串用法在每次修改时即时校验，而非仅依赖 Stop 收尾。

**guard 涉及路径**：
- `.agents/hooks/check-unsafe-text-ops.sh`（TARGET 范围、perl 正则、豁免列表）
- `.agents/aemeath.json`（hook 注册：是否新增 PreToolUse/PostToolUse）
- `agent/share/src/string_idx/`（统一安全字符串/索引模块）

### #74 TUI 执行 /reflect 后续文本颜色全部变暗（System 色泄漏）

**状态**：待确认（随 #58 渲染管线重构结构性修复）。每个 block 独立从自身 kind/style 派生颜色，不存在跨 block 共享可变样式状态；System(Muted) block 后 Assistant block 仍用 ASSISTANT 色。回归测试 `blocks/mod.rs::test_render_block_assistant_after_system_does_not_inherit_dark`。

**症状**：在 TUI 中执行 `/reflect` 后，reflection 输出及其**后续的普通/assistant 文本**全部呈现暗灰蓝色（System 样式），而非正常的 assistant 前景色。截图中 `[User]:` / `[Assistant]:` 会话转录、`## 第一版 DDD 边界建议`、`### 1. TUI App Shell`、`负责组合所有上下文...` 等内容均为统一暗色。

**根因（假设）**：
1. `ReflectionDone`（`apps/cli/src/tui/core/update/ui_event.rs:210`）通过 `self.output_area.push_system(&output.content)` 把整段 reflection 输出以 `LineStyle::System` 推入输出区；`output.content` 内含会话转录和多段 markdown，全部按 System（暗色）渲染。
2. System 也属于 `is_markdown_style`（`rendered_lines.rs:393`），其内若含 fenced code block / 特殊 markdown，可能令渲染缓存的 fence/style 状态跨 block 泄漏到后续行（与 #65、#2 同族）。
3. reflection 结束后，后续 assistant 文本可能未显式复位样式，继续沿用 System 暗色；或渲染缓存未在 reflection block 后正确 invalidate/分隔。

**复现**：
1. 在 TUI 会话中执行 `/reflect`。
2. 等待 reflection 输出完成。
3. 观察 reflection 输出本身及其后续文本是否都变为暗灰蓝色。

**修复方向**：
1. 明确 reflection 输出的样式语义：若仅 reflection 摘要应为 System，则其后续 assistant/普通文本必须复位为对应样式，不得继承 System。
2. 排查 `push_system` 推送多行 markdown 时渲染缓存 block state 是否泄漏（复用 #65 的 fence/style 复位修复）。
3. 考虑 reflection 输出与普通对话内容用不同 block 边界分隔，渲染时各自独立初始化样式状态。
4. 补充回归：push 一段 System 样式 markdown 后，下一条 Assistant 文本不应使用 System 前景色。

**涉及路径**：
- `apps/cli/src/tui/core/update/ui_event.rs`（`ReflectionDone` → `push_system`）
- `apps/cli/src/tui/output_area/`（`push_system` 行样式、markdown 渲染缓存 block state）
- 关联 #65（工具结果 fenced code block 样式泄漏）、#41（/reflect 异步化）

### #66 ExitWorktree 带 path 参数报错"已在 worktree 中"

**状态**：活动中

**症状**：ExitWorktree 工具传入 `path` 参数时，预期行为是退出当前 worktree 后切换到指定路径，但实际报错：

```text
✗ ExitWorktree
  {"path":"/Users/guoyuqi/Nextcloud/work/claudecode/aemeath"}
  ✗ 切换路径失败：已在 worktree 中，请先 ExitWorktree 退出当前 worktree 再进入新的
```

用户正在 worktree 中，想通过 `ExitWorktree(path="/some/target")` 一步切换回目标工作区，却被告知"已在 worktree 中，请先 ExitWorktree"。先执行无参 `ExitWorktree` 退出 worktree，再用 `EnterWorktree` 或直接 cd 目标路径才能到达，操作需拆为两步。

**复现**：
1. 先通过 `EnterWorktree` 进入任意 worktree。
2. 调用 `ExitWorktree(path="/Users/guoyuqi/Nextcloud/work/claudecode/aemeath")`（或其他绝对/相对路径）。
3. 观察错误返回：`✗ 切换路径失败：已在 worktree 中，请先 ExitWorktree 退出当前 worktree 再进入新的`。
4. 先执行无参 `ExitWorktree` 成功退出 worktree，再执行同样带 path 的 ExitWorktree 或手动 cd 才能到达目标路径。

**根因假设**：
1. ExitWorktree 带 `path` 参数时内部逻辑等价于 `先 EnterWorktree(path)`，导致检查到当前已在 worktree 中时直接拒绝，没有先执行 ExitWorktree。
2. `ExitWorktree(path)` 的处理分支未区分"仅退出"和"退出+切换"：前者应直接恢复上下文栈并 `cd`，后者应在退出后跳过"已在 worktree 中"检查再执行路径切换。
3. 路径存在性/合法性的校验也可能触发 worktree 嵌套检查，混淆了两个操作的执行边界。

**修复方向**：
1. 梳理 `ExitWorktree(path)` 的执行分支：先执行无参 ExitWorktree 的退出逻辑（pop 上下文栈、恢复工作目录），再以恢复后的工作目录为基础执行 path 切换（等价于无条件 cd 目标路径，不再嵌套检查 worktree）。
2. 若 `ExitWorktree(path)` 在设计上不允许从 worktree 直接切换到另一个 worktree，则必须在文档/Bash 提示中明确该限制，并提供分步指引；但从用户视角来看，提供 `path` 参数本身就意味着"我要退出并切换"。
3. 补充回归：从任意 worktree 调用 ExitWorktree(path) 应能切换到目标路径；目标路径不存在时给出明确路径错误而非 worktree 嵌套错误。

**涉及路径（预计）**：
- `ExitWorktree` 工具实现（path 参数处理与 worktree 嵌套检查）
- `EnterWorktree` / context stack 生命周期管理

### #65 工具结果 fenced code block 后续内容继续显示为 code 颜色

**状态**：待确认（G2 已接线）。把 assistant 与工具结果共用的 fence/markdown/table 状态机提取为共享原语 `primitives/fenced.rs::render_fenced_markdown`，`blocks/assistant_message.rs` 与 `blocks/tool_call.rs::format_result_lines` 均调用它（DRY，fence 渲染单一实现）。每个 block 独立渲染、状态机随调用销毁，fence 结束后普通行恢复 base 色，结构上隔离泄漏。回归：`primitives/fenced.rs::test_fenced_does_not_leak_code_color_after_close` 与 `blocks/tool_call.rs::test_tool_call_result_fence_does_not_leak_code_color_after_close`（断言 fence 内代码行为 CODE 色、fence 结束后普通行 ≠ CODE 色）。Edit 工具结果的 `---DIFF---` diff 渲染路径（G1）保持优先：`render_tool_call` 先判 Edit diff，否则才走 fence/markdown 渲染。

**症状**：TUI 输出区展示工具结果时，如果结果内容包含 fenced code block，代码块结束后后续普通内容仍显示为 code 颜色。用户观察到如下片段后，后面的内容都变成 code 颜色：

```text
✓ replaced 1 occurrence(s) in ␠
/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/docs/superpowers/plans/2026-05-24-task-window-refactor.md
```

**影响**：工具结果后的普通 assistant 文本、后续 tool call 展示或文档内容被错误套用 code block 样式，降低可读性，也可能影响用户判断哪些内容属于代码块。

**根因假设**：
1. Markdown fence 状态机在处理 tool result 渲染片段时没有在片段结束后复位，导致 `in_code_block` 状态泄漏到后续输出。
2. 工具结果的 styled spans/cache 跨 block 复用，code block foreground/background 样式没有在 fence 结束行后清空。
3. fenced code block 结束标记被工具结果格式化、缩进或换行拆分影响，导致渲染器没有识别 closing fence。
4. streaming/append 路径和历史渲染路径对 tool result 的 Markdown block state 初始化/收尾不一致。

**修复方向**：
1. 检查 output area Markdown fenced code block 状态机，确保每个消息/tool result block 渲染结束时不会把 code style 泄漏到下一个 block。
2. 对 closing fence 的识别兼容工具结果中带缩进、语言标记、空白字符的情况。
3. 明确 tool result 渲染缓存的 block state 生命周期：同一消息内可延续，跨消息/tool result 必须按设计复位或正确继承。
4. 补充回归覆盖：工具结果包含完整 fenced code block 后，下一行普通文本不应使用 code foreground/background。

**涉及路径**：
- `apps/cli/src/tui/output_area/markdown/` fenced code block 渲染逻辑
- tool result display / output area styled span cache
- streaming append 与历史消息 render 路径

### #64 Agent 未绑定 taskId 仍启动导致 TaskList 无 doing 状态

**状态**：修复中

**症状**：session `019e4ea6-6f8a-7049-a812-0ab60653770e` 中，主 LLM 已创建 task list 并填充多个任务。Task 1 完成后，系统提示 Task 2 已解除阻塞；随后主 LLM 启动 Task 2 subagent，subagent 实际运行并修改代码，但 task list 中 Task 2 仍保持 `pending`，界面上只看到 `done` 与 `pending`，没有 `doing / in_progress`。

**日志证据**：
1. `TaskUpdate taskId=1 status=completed` 返回 `Unblocked tasks now ready: → #2`。
2. 后续 `Agent(description="Implement Task 2", ...)` 调用缺少结构化 `taskId` 字段。
3. subagent 返回 `DONE_WITH_CONCERNS` 并产生文件修改，证明任务实际执行。
4. 因 AgentTool 未绑定 task，TaskStore 未自动执行 `Pending → InProgress → Completed/Pending` 生命周期。

**根因**：AgentTool 目前只在输入包含 `taskId` 时桥接 task 生命周期；当 active task batch 存在未完成任务时，未传 `taskId` 的 Agent 仍会启动。工具描述要求模型传 `taskId`，但没有工具层强制约束，LLM 遗漏参数时系统不会阻止，导致 subagent 执行和 task 状态投影脱钩。

**修复方向**：
1. 当 TaskStore 存在 active list 且有 pending/in_progress 任务时，AgentTool 缺少 `taskId` 应直接返回错误，提示传入 `taskId` 或先完成/关闭 task list。
2. 保留无 active task list 时的自由 Agent 调用能力，避免破坏普通并行调研/review 场景。
3. 保留已有 `taskId` 生命周期管理：绑定 task 成功时标记 InProgress，成功后 Completed，失败后 Pending。
4. 增加回归测试覆盖 active task list + missing taskId 拒绝启动，且验证 subagent runner 未被调用。

**涉及路径**：
- `packages/tools/src/agent_tool.rs`
- `packages/tools/src/agent_tool_tests.rs`
- `packages/core/src/task/` TaskStore active list / batch 状态查询

### #62 Grep 工具执行中标题文字不可见但复制可见

**状态**：待确认（随 #58 渲染管线重构结构性修复）。ToolCall 标题在 `blocks/tool_call.rs` 用 `theme::TEXT` 前景渲染、图标用 `semantic_color`，与背景独立；回归测试 `blocks/tool_call.rs::test_tool_call_title_visible_not_background_color`（fg ≠ bg 且 ≠ SURFACE）。

**症状**：TUI 中 Grep 工具运行态显示类似：

```text
● Grep /tui\.log/
  in /Users/guoyuqi/Nextcloud/work/claudecode/aemeath
```

但屏幕上看不到 `Grep` 字样；选中复制时文本又能正常复制出来，说明逻辑文本存在，只是渲染视觉不可见或颜色不可辨。

**复现**：
1. 触发 Grep 工具执行，例如搜索 `/tui\.log/`
2. 在 TUI output area 观察 tool call running 行
3. `●` spinner 和路径/参数可能可见，但 `Grep` 字样不可见
4. 选中复制该区域，粘贴后可以看到 `Grep` 文本

**根因假设**：
1. Grep 工具标题 span 的前景色与背景色过近，或被 theme 中 running/tool_name 颜色设置为透明/背景色。
2. tool running 状态样式覆盖了 tool name span，导致 `Grep` 文本渲染为不可见颜色。
3. 复制路径读取的是 plain text/原始文本，渲染路径使用 styled spans，因此复制可见而屏幕不可见。
4. 仅 Grep 受影响可能与工具名/参数分段样式、regex 参数高亮或 `in ...` 第二行缩进样式有关。

**修复方向**：
1. 检查 tool display 中 tool name、running spinner、argument/path span 的 style 合并逻辑。
2. 检查 theme 中 tool running/tool name/secondary text 的前景色是否与当前背景冲突。
3. 确保所有 tool call running 行的 tool name 都使用可见的主文本或工具强调色。
4. 补充视觉/快照或样式单元测试：Grep/Glob/Read/Bash 等工具运行态 tool name span 颜色不能等于背景色。

**涉及路径**：
- `apps/cli/src/tui/output_area/tool_display/`
- TUI theme/status/tool running style 定义
- output area selection/copy 与 styled render 分离逻辑

### #61 Diff 渲染行号顶到最左破坏缩进，且选中后高亮丧失

**状态**：待确认（diff 渲染原语已端到端接通，G1 完成）。#58 渲染管线下：diff 行保留左缩进由 `primitives/diff.rs::test_diff_line_keeps_left_indent_not_flush_left` 覆盖，选中后保留原 fg 由 `selection_overlay.rs::test_overlay_sets_bg_keeps_fg`（唯一上色路径，只设 bg）覆盖。接线：Edit 工具结果以 `---DIFF---` 标记携带 old/new 文本，新增 `blocks/edit_diff.rs` 解析并经 `primitives::diff::diff(old,new,ext,width)` 渲染（行号 + 加减语义色 + 语法高亮 + 两空格缩进），在 `blocks/tool_call.rs::render_tool_call` 中检测 result_summary 的 diff 标记后路由；diff 行作为普通 RenderedLine 流经统一 selection overlay，自动获得「选中保留 fg」。端到端回归：`tool_call.rs::test_tool_call_edit_result_renders_diff_with_numbers_signs_indent_color`、`edit_diff.rs` 8 个单测。Write 工具结果不含 diff（仅 "wrote N bytes"），无需接线；assistant ```diff fence 来源为 unified diff 文本而非 old/new 对，本期未处理（见 concerns）。

**症状**：
1. TUI output area 渲染 unified diff 时，新增的 old/new 行号区域顶到了最左边，没有保留输出区原有的左侧缩进/边距，导致 diff 块视觉上“贴边”，破坏整体缩进层级。
2. diff 部分可以被选中并复制，但选中后语法高亮/选区高亮丧失，表现为选中状态下高亮样式没有正确叠加或被覆盖。
3. 疑似之前已经统一过的“选中高亮 + 复制”逻辑没有覆盖新的 diff 行号/语法高亮渲染路径，导致 diff 使用了旁路渲染或直接 Span 输出。

**复现**：
1. 让 LLM 输出包含 unified diff 的 Markdown/code block
2. 在 TUI output area 中观察 diff 行号区域是否贴到最左边、缺少与普通内容一致的缩进
3. 鼠标/键盘选中 diff 区域
4. 观察选中时 diff 原有语法高亮或选区高亮是否消失；复制内容虽然可用，但视觉反馈不一致

**根因假设**：
1. diff 行号渲染时没有继承 output line 的 content inset/padding，或直接从 area.x 开始绘制，绕过了统一缩进计算。
2. diff 语法高亮行使用了独立 `Span`/`Line` 构造，未经过统一的 selection overlay 样式合并逻辑。
3. selection 高亮可能以“整行覆盖 style”的方式应用，覆盖了 diff 内部语法高亮，而不是做 foreground/background 的组合叠加。
4. 复制路径和高亮路径分离：复制已读取原始/逻辑文本，但选中渲染没有走统一 selection renderer。

**修复方向**：
1. diff 行号区域应遵循 output area 统一左边距/缩进规则，行号作为内容的一部分在缩进之后绘制。
2. 将 diff 渲染接入统一的 selection-aware render pipeline，避免绕过已有“选中高亮 + 复制”逻辑。
3. selection 样式应只叠加背景/反色，不应清空 diff 语法高亮的前景色；必要时定义统一 style merge 策略。
4. 补充回归覆盖：diff 行号不贴边、选中 diff 时仍有可见选区背景、语法高亮不被完全抹掉、复制内容保留 diff 原文与换行。

**涉及路径**：
- `apps/cli/src/tui/output_area/markdown/` diff 渲染与语法高亮逻辑
- output area selection 渲染/复制统一逻辑
- Feature #35（Diff 渲染中 add 行语法高亮 + 行号显示）相关实现

### #49 last turn 时用户提交的内容不会发给 LLM，留在 input queue 区域

**状态**：修复中（2026-05-24 补齐三条缺失 drain 路径）

**症状**：用户在 LLM 处理期间提交的消息（last turn）没有继续发送给 LLM，而是留在 input queue 区域。表现为当前轮 LLM 结束后，排队输入仍显示在队列里，没有自动进入下一轮请求。

**历史修复**：此前曾抽取 `append_queued_input`，在 EndTurn/无工具调用和工具轮结果同步后统一 drain queued input；有消息则同步 messages 并 `continue` 进入下一轮，并补充正常/空队列/通道关闭单元测试。

**当前反馈**：用户确认问题仍存在，因此原先“已确认修复”结论撤销，重新标记为 active。需要重点检查是否仍有 last turn 路径绕过 `append_queued_input`，或队列被 drain 后没有触发下一轮 LLM 请求。

**根因假设**：
1. 某些结束路径（stop/cancel/hook/stream error/无 tool call/EndTurn）未调用统一的 queued input drain。
2. 队列内容被同步到 messages 后，状态机没有 `continue` 或没有重新启动后台处理。
3. TUI input queue 区域与实际 input_queue 数据源不同步，导致已消费但 UI 未清除，或 UI 清除但消息未发送。
4. last turn 提交时机处于 streaming 收尾与 idle 切换之间，触发了竞态，队列未被下一轮消费。

**本轮修复（2026-05-24）**：审计发现 `append_queued_input` 仅覆盖了 EndTurn/无工具调用和工具轮结果同步两条路径，以下三条路径完全缺失 drain：
1. **interrupted（用户按 Escape 取消）**：直接 truncate messages + break，队列内容被丢弃。修复：drain 优先，有内容则 continue 恢复而非取消。
2. **stall_detector（重复输出检测）**：直接 break 退出循环。修复：drain 后有内容则 continue。
3. **API Error**：直接进入 finalize_main_loop。修复：drain 优先，有内容则 continue 跳过错误处理。

修复后所有 `break` 出口前均有 `append_queued_input` drain 检查。`finalize_main_loop`（含 stop hook）仅在队列为空时才执行。

**修复方向**：
1. 为所有 LLM 结束路径统一添加 input_queue drain 检查，尤其是 EndTurn、无工具调用、hook stop、错误返回、用户 stop 后的状态切换。
2. drain 到消息后必须显式触发下一轮处理，避免仅更新 messages 但没有继续请求 LLM。
3. input queue UI 状态应与实际队列消费原子同步，防止残留显示。
4. 添加日志定位：记录 last turn 入队、drain、messages append、下一轮启动、UI queue 清除等关键节点。2026-05-23 已先补充 TUI 收到 Done/DoneWithDuration 时的 `[bug49_input_queue_at_done]` 日志，记录 input queue 与 UI queued_messages 状态。
5. 补充/更新回归测试覆盖用户反馈路径，而不仅是已有 EndTurn/无工具调用路径。

**涉及路径**：
- `apps/cli/src/tui/app/stream/` 或 LLM background loop 相关逻辑
- input queue 状态管理与 TUI 展示逻辑
- `append_queued_input` 及其调用点

### 专案 A：Task 系统生命周期管理（Bug #27 + #29 + #32 + #33 + #34 + #36 + #37；Feature #18 + #24 + #25 + #29 + #30 + #33）

**统一描述**：Task 系统在状态流转、batch 隔离、跨轮次清理、窗口化显示、选中复制、reminder 注入、agent loop 收尾、工具调用展示等维度存在关联缺陷或改进项，统一作为专案 A 管理。

| 类型 | 原始条目 | 角度 | 状态 |
|------|----------|------|------|
| Bug | #27 Sub-agent 已执行 tool call 但 task list 状态不更新 | 状态流转：sub-agent 路径 | 已有修复（2026-05-11）：AgentTool 新增 taskId 参数 + 自动桥接 |
| Bug | #29 主 agent tool call 执行后 task list 状态不更新 | 状态流转：主 agent 路径 | 已有修复（2026-05-11）：system prompt 强约束 + TaskCreate 描述增强 |
| Bug | #32 Task list 窗口化：始终只显示 1 条 task | 窗口化显示：限量显示策略缺陷 | 已修复（2026-05-11）：TTL 只优先 recent completed，补齐窗口时回退使用旧 completed |
| Bug | #33 Spinner 下方 task list 无法选中、复制和高亮 | 交互：task/input/status selection/copy 映射与高亮渲染 | 待确认；2026-05-18 补充修复 input area CJK 拖选高亮按字符列绘制导致宽字符后半段漏高亮 |
| Bug | #34 Task reminder 干扰新用户请求 | batch 隔离：提醒不隔离 | ✅ 已归档（2026-05-17）：用户确认修复；详见 `docs/bug/archived/034-task-reminder-interference.md` |
| Bug | #36 TaskListCreate 后新任务编号未从 1 开始 | batch 内编号：session 第二次 TaskListCreate 时 TaskCreate 编号沿用全局递增而非从 1 开始 | 已修复（2026-05-11）：TUI 使用 batch 内局部显示编号 |
| Bug | #37 Task list 全部完成后切换对话仍显示旧 task | 跨轮次清理：已完成 batch 挂留 | 已修复（2026-05-11）：当前列表只读取 Active/Paused batch，归档 batch 自动隐藏 |
| Feature | #18 Task list 跨轮次 batch 机制 | 基础机制：Task 跟随 session 持久化并按 batch 分组显示 | ✅ 已完成，未确认 |
| Feature | #24 Spinner 下方 task list 限量显示（最多 7 条） | 窗口化显示：限制 task list 占用空间 | ✅ 已完成，未确认；关联 Bug #32 |
| Feature | #25 Task list 跨轮次生命周期策略 | 生命周期：完成归档、中断提示、旧任务提醒 | ✅ 已完成，未确认 |
| Feature | #29 Task reminder 被动注入 | reminder：按轮次扫描并注入极简摘要 | ✅ 已完成，未确认 |
| Feature | #30 Agent loop 收尾工作 | 收尾一致性：统一 finalize、记录停止原因、task/list 收尾检查 | ✅ 已完成，未确认 |
| Feature | #33 优化 TaskListCreate / TaskListComplete 工具调用显示 | 展示优化：隐藏噪声，改为简洁摘要 | ✅ 已完成，未确认；已实现简洁 header、summary 详情和成功结果静默 |

**专案 A 相关 Feature 来源**：见 `docs/feature/active.md` 的 #18、#24、#25、#29、#30、#33。

**本次修复（#32 + #36 + #37）**：

**#32 — 窗口化始终只显示 1 条 task**：
- 串行执行且大量 completed task 的 `updated_at` 超过 TTL 后，窗口只保留当前 in_progress，旧 completed 在下限补齐前已被过滤掉
- 根因：`build_task_window()` 把 TTL 过滤后的 completed 列表作为唯一补齐来源，导致没有 pending 时无法回退填满窗口
- 修复：保留 unfiltered completed 作为 fallback；TTL 仍优先显示 recent completed，但窗口有剩余容量时从旧 completed 回退补齐，避免退缩成 1 条
- 回归测试：`test_build_task_window_serial_execution_keeps_context_when_recent_completed_expire`

**#36 — 新 batch 任务编号不从 1 开始**：
- 同一 session 中第二次 TaskListCreate 后，TaskCreate 分配的 task id 仍沿用全局递增（如 #6、#7...），而非新 batch 从 #1 重新编号
- 根因：TaskStore 的 task_id 是全局自增计数器；此前只在部分展示路径宣称使用 batch 内相对编号，但 `build_task_window()` 实际仍直接格式化 `Task.id`
- 修复：`build_task_window()` 先按当前 batch 内 task id 升序生成显示编号映射，再渲染为 batch 内相对编号（1, 2, 3...），不改变底层全局 task id
- 回归测试：`test_build_task_window_displays_batch_local_numbers`

**#37 — 已完成 batch 挂留**：
- 当前 batch 所有 task 已 completed，但下一轮新用户消息开始时，旧 task list 仍显示
- 根因：TaskListComplete 后 batch 标记为归档，但当前列表查询仍从所有 task 的最大 batch 推断当前 batch
- 修复：`TaskStore::list_current_batch()` 只选择 Active/Paused batch；没有活动 batch 时返回空列表，已归档 batch 不再回流显示

**涉及路径**：
- `aemeath-tools/src/agent.rs`（#27：AgentTool taskId 桥接）
- `aemeath-core/src/task.rs`（TaskStore batch 管理、completed batch 归档）
- `aemeath-tools/src/task_list_create.rs`、`task_list_complete.rs`（#34：batch 生命周期）
- `aemeath-cli/src/tui/app/task_window.rs`（#32：TTL fallback 补齐窗口；#36：batch 内局部编号）
- system prompt 中 task 维护指引

## 详情

### #56 Stop hook 返回 exit 2 后 LLM 仍结束
**症状**：Stop hook 中的检查脚本（如 `check-unsafe-text-ops.sh`）返回 exit 2 后，日志记录 `blocked=true`，但 TUI agent loop 仍发送 Done 并结束，LLM 没有机会根据 hook 输出继续修复。
**根因**：TUI 收尾路径对 Stop 事件调用 `run_plain`，只展示 hook 执行过程，不读取 JSON/blocked 结果；`finalize_main_loop` 返回 `()`，无法把阻止停止的反馈传回主 loop。
**修复方向 / 当前状态**：修复中。Stop hook 改为 `run_json`，检测 `blocked` 或 JSON `decision=block` 后返回反馈；主 loop 将反馈作为 system-reminder 追加给 LLM 并继续下一轮，不发送 Done。
**涉及路径**：`apps/cli/src/tui/app/stream.rs`、`apps/cli/src/tui/app/stream/finalize.rs`

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
**修复（2026-05-11）**：
1. `Agent` tool 新增 `taskId/task_id` 参数，执行前自动将父 task 标记为 `in_progress`，成功后标记为 `completed`，失败后恢复为 `pending`。
2. 子代理上下文继续复用父 `TaskStore`，父侧 task list 可实时观察到 AgentTool 的状态桥接结果。
3. 子代理注册工具时排除 TaskCreate/TaskUpdate/TaskList 等协调类工具，避免子代理绕过父级状态管理。
4. 新增 `agent_tool_tests` 覆盖 taskId 缺失、成功完成、失败回滚三条路径。

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

**修复（2026-05-11）**：
1. 静态 system prompt 新增强制流程：新多步请求先 `TaskListCreate`，直接执行 Read/Grep/Glob/Bash/Edit/Write 等工具前先 `TaskUpdate(in_progress)`，完成后 `TaskUpdate(completed)`，全部完成后 `TaskListComplete`。
2. `TaskCreate` 工具描述同步加入 TaskListCreate 前置要求，降低模型跳过状态维护的概率。
3. 新增 prompt 单元测试覆盖直接工具前后必须 TaskUpdate、Agent taskId 委派、Task reminder 可能无关等约束。

### #26 几乎每次对话都触发 superpowers skill 调用（已归档：不作为 Bug）
**归档原因**：用户确认该项不算 Bug，不再作为活动 bug 跟踪。
**原现象**：几乎每次对话开始时，LLM 都会主动通过 Skill 工具调用 superpowers 系列 skill（如 `superpowers:using-superpowers`、`superpowers:brainstorming` 等），即使用户的请求只是简单提问、查询信息或闲聊，并不需要任何 skill 介入。
**原疑似根因**：SessionStart hook 提示语偏强、`using-superpowers` description 触发面过宽、Skill 列表注入后模型倾向调用 skill。
**后续处理**：如需调整，应作为体验优化 / feature 另行登记，而非 bug 修复。

### #34 Task reminder 干扰新用户请求（已归档 2026-05-17）
用户确认修复。修复内容：新增 batch summary 与 TaskListCreate/TaskListComplete，task reminder 按 batch 输出并明确提示旧 batch 可能与最新用户消息无关。详见 `docs/bug/archived/034-task-reminder-interference.md`。

### #31 WebSearch 工具返回空结果（已归档 2026-05-14）
用户确认修复。修复内容：结果块匹配改为 `<div class="result "`，title/snippet 不再依赖属性顺序，检测 `anomaly.js` 后 fallback 到 Bing 搜索。详见 `docs/bug/archived/031-web-search-empty-results.md`。

### #32 Task list 窗口化：始终只显示 1 条 task

**症状**：task list 显示行为不一致，表现出两种症状：

**症状 A — 窗口退缩至 1 条**：
Session `019e0665-0efc-7e7e-ad54-e895c2ae8a3a` 实例：
- Task 1~10 陆续创建，总数 > 7
- LLM 持续完成任务（TaskUpdate completed），不断增加新 task
- task list 窗口始终只显示 1 行（如正在执行 #9，则只显示 #9）
- #9 完成后跳到 #10，#9 随之消失，窗口仍只显示 1 行

**症状 B — completed 挂留 + 窗口截断**：
- Task 2 已完成（completed），但仍滞留在 task list 中不消失
- 同时在显示 task 4（执行中）和 task 5（待执行）
- 即 task list 显示：2（completed）、4（in_progress）、5（pending）
- 未达 7 条上限，但 task 3 等中间 task 未显示，completed 未自动清理

**症状 C — completed 不是"最近"、pending 跳号（2026-05-12 截图）**：
- `Tasks: 5/11`，共 5 条 completed，窗口只显示 #1（通常是最早完成的，而非"最近完成"）
- 显示顺序：✓ #1、■ #3、■ #9、□ #4、□ #5、□ #10、□ #11
- pending 列表从 #5 跳到 #10，#6/#7/#8 既未出现在 completed 也未出现在 pending 段，疑似被静默截断
- 期望：「最近完成（按 updated_at desc 取 1~N 条）+ 所有 in_progress + 后续 pending 升序连续填充」

**症状 D — 仅剩最后 in_progress 时窗口未填满（2026-05-17 截图）**：
- `Tasks: 9/10`，只显示 `✓ #9 Important 7: can_create_agents 硬校验` 和 `■ #10 Minor 1-6...`
- 实际还有 #1~#8 completed，可用于填满 7 条窗口，但被 TTL 过滤提前丢弃
- 期望：无 pending 且存在 in_progress 时，窗口显示最近 completed 补足剩余容量 + 当前 in_progress，例如 #4~#9 completed + #10 in_progress

**复现路径**：
1. LLM 创建 ≥ 2 条 task
2. LLM 完成部分 task，新建更多 task（总数持续波动）
3. 观察 task list 显示 —— 始终只有 1 行

**根因**：`build_task_window()` 窗口化策略在两处逻辑缺陷：

1. **症状 A 根因**：窗口填充规则"上一条 completed + 所有 in_progress + 后续 pending"在串行执行场景（1 条 in_progress）下，completed 最多只取 1 条，结果窗口极易退缩至 1 条
2. **症状 B 根因**：
   - completed 未设置自动清理（TTL），过期 completed 不会自动从窗口排除
   - 窗口填充时对 pending 的截断位置不正确，跳过了 task 3（pending）而直接到了 task 5
3. **症状 C 根因（疑似）**：
   - "最近完成"未按 `updated_at` 降序取最新，而是按 id 升序取第一条 completed → 永远显示 #1
   - pending 段在 in_progress 之后接着取"id > 最大 in_progress id"的 pending，导致 #6/#7/#8 若状态是 completed 但被 TTL 排除，pending 仍从 #4 起，但配额耗尽前出现跳号说明排序/截断逻辑存在 off-by-one

另外需确认 `task_status_lines` 是批量替换还是增量追加 —— 如果是增量方式，旧行不会被移除，会导致 completed 长期滞留。

**修复方向**：
1. `build_task_window()` 添加**下限保护**：窗口结果 < `min(3, total_tasks)` 时扩大填充（补入更多 pending / completed）
2. `build_task_window()` 修复 **pending 截断顺序**：按 task id 升序取 pending，不跳跃
3. Completed task **自动清理**：窗口化时排除太旧的 completed（如已完成超过 3 秒），或每次重建窗口时只保留最近 N 条 completed
4. 确认 `update_task_status` 每次推送的是 **完整窗口行列表**（批量替换），而非增量追加
5. 单元测试覆盖：
   - 10 tasks、1 in_progress / 9 pending → ≥ 3 条显示
   - 5 tasks、1 completed(#2) / 1 in_progress(#4) / 3 pending(#3,#5,#6) → 按序显示 #2,#3,#4,#5

**涉及路径**：
- `aemeath-cli/src/tui/app/task_window.rs`（`build_task_window` 窗口化逻辑）
- `aemeath-cli/src/tui/app/mod.rs`（`update_task_status` 调用侧）

**修复（2026-05-09）**：
1. **Completed TTL 过滤**：按 `updated_at` 降序排列，排除更新超过 30s 的旧 completed
2. **温和扩展**：填充完核心任务后，有余量时自动补充更多 completed 和 pending
3. **下限保护**：扩展后不足 `min(3, total)` 时进一步从 completed 头部补充
4. **pending 顺序**：`pending.sort_by_key(|t| t.id.parse::<u64>().unwrap_or(u64::MAX))` 确保升序
5. **单元测试**：新增 4 个测试覆盖下限保护、TTL 过滤、pending 顺序、温和扩展场景
6. **门禁脚本补漏**：`scripts/check-unsafe-text-ops.sh` 新增不带 `&` 的切片检测模式

**修复（2026-05-11）**：
7. **TTL 阈值调整**：30s → 300s（5 分钟），且仅当 completed 数量超过 `max_lines` 时才触发 TTL 过滤；窗口有空位时所有 completed 都显示
8. **摘要行全量 completed 计数**：`Tasks: x/y` 中的 x 改为使用全量 completed 数（`all_completed_count`），而非 TTL 过滤后的数量，修复"Tasks: 1/5 但实际已完成 3 条"的问题
9. **Completed 显示顺序修正**：completed 行改为按 task id 升序显示，保持 task list 视觉顺序稳定；TTL 判断仍使用最大 `updated_at` 作为最新完成时间。
10. **回归测试补充**：新增用户示例对应的 completed 乱序测试；`task_window` 16 个单元测试、`cargo test -p aemeath-cli`、`cargo check -p aemeath-cli` 通过。

**修复验证（2026-05-18）**：
11. **症状 D 验证**：当前 `build_task_window()` 已在 `pending_count == 0 && in_progress_count > 0` 时跳过 completed TTL 过滤，并按 `remaining - in_progress_count` 选取最近 completed 补满窗口。
12. **回归测试确认**：`test_bug32_user_snapshot_keeps_full_window_when_only_recent_completed_and_in_progress` 覆盖 9/10 完成场景，期望显示 #4~#9 completed + #10 in_progress；`cargo test -p aemeath-cli task_window -- --nocapture` 通过 19 个 task_window 测试。

**修复（2026-05-18 E 轮）**：用户复现：13 条 task，一开始显示 7 条，pending 减少后窗口逐渐收缩到 6/5/4/1 条。
13. **根因**：温和扩展和下限保护阶段只在 TTL 过滤后的 `completed_for_display` 中选取，当大量 completed 超过 TTL（5 分钟）后，TTL 过滤移除了大部分 completed，剩余 completed 不够补齐窗口。
14. **修复**：新增 `shown_ids: HashSet<&str>` 跟踪已显示的 task id 避免重复；温和扩展先从 TTL 过滤后 completed 补充，仍有余量时从 `all_completed_sorted`（未过滤）回退补齐；下限保护也从 `all_completed_sorted` 选取。所有选取阶段先 `collect()` 再遍历避免借用冲突。
15. **回归测试**：新增 `test_bug32_window_stays_full_with_ttl_pressure`（8 completed + 1 in_progress，窗口保持 7 条）和 `test_bug32_window_never_shrinks_during_progression`（4 阶段渐进完成，窗口始终 7 条）。`cargo test -p aemeath-cli` 135 通过。

**修复（2026-05-19 F 轮）**：用户复现 session `019e359e-4a50-77a7-a752-56f6ac115240`：窗口显示 `✓ #8 修复 architecture.md` 后接 `✓ #1/#2/#3/#4`，completed 区块排序错乱。
16. **根因**：`build_task_window()` 先按 `updated_at` 选出最近 completed（如 #8），再把温和扩展补齐的旧 completed 插入到 `1 + comp_show` 之后，导致 completed 区块变成「最近完成 + 旧 completed 升序」，而不是视觉上的显示编号升序。
17. **修复**：新增 `merge_completed_lines()`，每次 completed 扩展补齐后重建 completed 区块并按显示编号排序；窗口仍按 `updated_at` 选择最近 completed 作为候选，只修正最终显示顺序。
18. **回归测试**：新增 `test_bug32_completed_expansion_keeps_display_order_for_user_snapshot`，覆盖用户截图中的 #8/#1/#2/#3/#4/#9/#10 顺序；同步调整 `test_completed_lines_keep_task_id_order_when_expanded` 和 mix 场景期望。`cargo test -p aemeath-cli task_window` 22 通过。

**关联**：
- Feature #24（task list 窗口化限量显示）—— 本 bug 是 #24 窗口化策略的缺陷
- Feature #18（task batch 机制）—— 同属 task list 显示链路


### #33 Spinner 下方 task list 无法选中和复制

**症状**：spinner 下方的 task list 行（摘要行 `━━ Tasks: 3/5 ━━` 及每条 task 的 `✓ #1 标题`、`■ #2 标题`、`□ #3 标题`）在 TUI 中可见但鼠标无法选中、无法复制。拖拽选中时这些行被跳过，`Ctrl+C` 复制时也拿不到文本。

### #41 执行 /reflect 时 TUI 短暂卡死后才出现 LLM 输出

**状态**：待确认（已修复渲染缓存未 invalidate 问题）

**症状**：
- 在 TUI 中执行 `/reflect` 后界面像卡死一样无即时反馈
- 等待一段时间后才开始出现 LLM 输出
- 用户感知为命令执行期间 UI 事件循环被阻塞，而不是正常的流式/异步反馈

**复现路径**：
1. 在 TUI 会话中输入 `/reflect`
2. 观察命令提交后的界面响应
3. 界面短时间无更新，过一会儿才显示 LLM 相关输出

**疑似根因**：
1. `/reflect` 命令路径可能在 TUI update/命令处理阶段同步等待 LLM 调用，阻塞事件循环
2. reflection 调用未通过 `Cmd`/runtime 异步副作用模型执行，或虽然异步执行但没有先更新状态/进度
3. reflection LLM 请求未接入和主对话一致的 streaming/progress 反馈，导致首个输出前没有任何 UI 心跳

**修复方向**：
1. 排查 `/reflect` 命令入口、TUI update 路径和 reflection runner 的调用关系
2. 确保 LLM 请求类副作用不在 `update()` 同步等待，必须通过 `Cmd` 或 runtime 异步执行
3. 提交 `/reflect` 后立即显示状态（如"正在反思..."），并保持 spinner/UI 可刷新
4. 如 reflection 输出不支持 token 级流式，至少在请求开始、收到响应、解析建议、写入 pending/auto-apply 阶段推送进度
5. 添加回归测试或结构性测试，覆盖 `/reflect` 不阻塞 update 主路径

**涉及路径**：
- `aemeath-core/src/command/commands/reflect.rs`
- `aemeath-core/src/reflection.rs`
- `aemeath-cli/src/tui/app/update/`
- `aemeath-cli/src/tui/app/stream/`

### #41 执行 /reflect 时 TUI 短暂卡死后才出现 LLM 输出

**状态**：待确认

**症状**：
- 在 TUI 中执行 `/reflect` 后界面像卡死一样无即时反馈
- 等待一段时间后才开始出现 LLM 输出
- 用户感知为命令执行期间 UI 事件循环被阻塞，而不是正常的异步反馈

**根因**：
- TUI `run_loop` 处理 `pending_slash` 时直接 `await handle_slash_command`
- `/reflect` 分支继续 `await run_llm_reflection()`，LLM 请求在主事件循环内同步等待
- 等待期间 tick/key/mouse/UI event 都无法被处理，所以界面看起来卡住

**修复**：
1. `/reflect` 默认 LLM 调用改为后台 `tokio::spawn` 执行
2. 提交命令后立即显示 `[reflection: calling LLM...]`、启动 spinner，并设置 `is_processing=true`
3. 后台任务通过 `UiEvent::ReflectionStarted`、`ReflectionUsage`、`ReflectionDone` 回传进度、token 用量和解析后的 ReflectionOutput
4. `UiEvent::ReflectionDone` 在主线程统一格式化输出、auto apply 或保存 pending suggestions，并停止 spinner
5. 新增回归测试 `test_spawn_llm_reflection_returns_before_llm_finishes`，使用阻塞型测试 provider 验证 `/reflect` 不等待 LLM 完成即可返回

**涉及路径**：
- `aemeath-cli/src/tui/app/run_loop.rs`
- `aemeath-cli/src/tui/app/slash.rs`
- `aemeath-cli/src/tui/app/slash/reflection.rs`
- `aemeath-cli/src/tui/app/update/ui_event.rs`
- `aemeath-cli/src/tui/app/event.rs`
- `aemeath-cli/src/tui/app/slash_tests.rs`

### #47 LLM 声称派发多个 reviewer 但 Agent 实际串行执行
**状态**：待确认

**症状**：
- LLM 在回复中说“派发 6 个 reviewer”或类似表述，用户预期多个 reviewer/Agent 会并行执行
- 实际观察到 reviewer/Agent 调用按一个接一个串行运行，整体耗时接近所有 reviewer 时间相加
- 表述和实际执行模型不一致，容易误导用户对并行能力与进度的判断

**复现路径**：
1. 让 LLM 对同一问题或多个独立文件派发多个 reviewer/Agent
2. 观察 TUI/tool call 执行顺序和 task list 状态
3. LLM 文案声称“派发多个 reviewer”，但实际只有前一个 Agent 完成后才启动下一个

**根因**：
**核心根因**：请求体缺少 `parallel_tool_calls: true` 参数。DeepSeek/OpenAI 等模型默认每轮只返回 1 个 tool call，导致"并行派发 6 个 reviewer"实际变成 6 轮串行。日志确认：每轮 LLM RESPONSE `tool_calls=1`。

1. **`execute_non_agent` 串行执行所有 non-agent tool calls**：`tools.rs` 中 `execute_non_agent` 使用 `for call in &other_calls` 逐个串行执行，即使工具标记为 `is_concurrency_safe()` 也不并行。每个 call 单独调用 `agent.execute_tools(slice::from_ref(&call))`，完全绕过了 `Agent.execute_tools` 的并发分组逻辑。
2. **LLM 分多轮生成 Agent tool calls**：部分 provider 的 LLM 倾向在不同轮次中逐个生成 Agent tool call，而非在同一轮中批量发出多个 tool_use blocks。Agent tool description 中缺少明确的并行指引。
3. **`execute_agent_calls` 已支持并行**：`agent_calls.rs` 使用 `chunks(batch_size)` + `join_all` 并行执行 Agent calls，此路径无问题。

**修复**：
1. **`execute_non_agent` 并发安全工具并行化**：重构为按 `is_concurrency_safe()` 分组——并发安全工具使用 `Semaphore` + `join_all` 并行执行，非安全工具保持串行。保持原始 tool call 顺序不变。新增 `execute_one_non_agent` 提取单个 tool call 的执行逻辑（hook chain + execute + post hooks + UI result）。
2. **Agent tool description 新增并行指引**：在 tool description 中添加 `IMPORTANT — Parallel execution` 段，明确告知 LLM “同一轮中发出多个 Agent tool calls 会并行执行”、“不要跨多轮逐个发出”。
3. **回归测试**：新增 4 个 `execute_tools` 并发测试
4. **v2 修复——请求体添加 `parallel_tool_calls: true`**：在 OpenAI Compatible provider 的 stream 和 non-stream 路径中，当有 tools 时设置 `parallel_tool_calls: true`，让 API 允许模型在同一轮返回多个 tool_use blocks——并发安全工具并行执行、非安全工具串行执行、结果顺序保持原始顺序、混合并发/串行场景。

**涉及路径**：
- `aemeath-cli/src/tui/app/stream/tools.rs`（`execute_non_agent` 并行化）
- `aemeath-tools/src/agent_tool.rs`（Agent tool description 并行指引）
- `aemeath-core/src/agent.rs` + `agent_tests.rs`（并发分组测试）
- `aemeath-llm/src/providers/openai_compatible/request_body.rs`（添加 parallel_tool_calls）
- `aemeath-llm/src/providers/openai_compatible/non_stream.rs`（添加 parallel_tool_calls）
### #36 TaskListCreate 后新任务编号未从 1 开始（已归档 2026-05-14）

用户确认修复。修复内容：TUI 渲染改用 batch 内局部显示编号，list_current_batch() 过滤已归档 batch。详见 docs/bug/archived/036-task-list-numbering.md。

### #39 超大工具结果触发 API 400 string_above_max_length

**症状**：会话 `019e17da-d39f-700f-ae2d-b68a41e12f70` turn 74 返回：
```
API error [400 Bad Request]: string too long. Expected a string with maximum length 
10485760, but got a string with length 27850677
```
`input[143].output[0].text` 超过 10MB 限制，导致会话中断。

**根因**：turn 73 中 Grep 工具搜索冲突标记，匹配到 `~/.aemeath/logs/input.log` 大型会话日志文件，返回了 27MB 结果。`persist_oversized_results`（已实现于 `tool_result_storage.rs`）仅在 REPL 路径集成，TUI 主 loop 和子 Agent loop 均未调用，导致超大工具结果直接塞入 LLM 上下文。

**修复**：
1. 统一 `MAX_TOOL_RESULT_CHARS` 常量定义，`tool_result_storage.rs` 引用 `crate::compact::MAX_TOOL_RESULT_CHARS`
2. TUI 主 loop（`stream.rs`）在 `all_results` 合并后、`Message::tool_results_rich` 前调用 `persist_oversized_results`
3. 子 Agent loop（`agent_runner.rs`）在 `truncate_tool_results` 前调用 `persist_oversized_results`
4. 超过 50KB 的工具结果写入 `~/.aemeath/tool-results/{session_id}/{tool_use_id}.txt`，上下文仅保留 `<persisted-output>` 引用标签

**修复（2026-05-15）**：
1. TUI 主 loop 新增 `tool_results_for_api()`，在 `all_results` 合并后、构造 `Message::tool_results_rich` 前调用 `persist_oversized_results`。
2. 子 Agent loop 的 `append_tool_results()` 接受 `session_id`，在构造 tool result message 前调用 `persist_oversized_results`；保持 UI/progress/json logger 仍记录原始 tool 输出摘要，只有进入 LLM 上下文的内容替换为引用。
3. 新增回归测试覆盖 TUI 主 loop 与子 Agent loop 两条路径，验证超过 `MAX_TOOL_RESULT_CHARS` 的结果被替换为 `<persisted-output>`，且引用中包含 session id。
4. #13 与 #39 确认为同类问题；#13 的超大 body 空响应由本修复消除主因，空响应检测可另作防御增强。

**涉及路径**：
- `aemeath-core/src/tool_result_storage.rs`
- `aemeath-cli/src/tui/app/stream.rs`
- `aemeath-cli/src/tui/app/stream/tools.rs`
- `aemeath-cli/src/agent_runner/loop_helpers.rs`
- `aemeath-cli/src/agent_runner/loop_run.rs`

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

---

### #42 TUI 中 Bash 工具输出中文显示为乱码（M- 转义序列）

**症状**：在 TUI 中，多条 Bash 命令的输出中文字符显示为 `M-eM-^P`、`M-gM-^W` 等 `cat -v` 风格的转义序列。不仅限于 `cat -A` 管道命令，普通 Bash 命令输出也会出现。

**复现路径**：在 TUI 中执行任何包含中文输出的 Bash 命令，观察输出区域。

**排查**：
1. Bash 工具（`aemeath-tools/src/bash.rs`）使用 `String::from_utf8_lossy` 将 `Vec<u8>` 转为 `String`，`from_utf8_lossy` 对合法 UTF-8 不做转义，只替换非法字节为 `\u{fffd}`，不会产生 `M-` 转义序列。
2. 应用日志（`~/.aemeath/aemeath.log`）中未找到包含 `M-` 转义内容的 Bash 输出记录。
3. 疑似问题在 TUI 渲染层或 ratatui 文本处理环节。

**疑似根因**：
- TUI 渲染路径中某处将 UTF-8 多字节字符拆分或按字节处理，导致高字节被 `cat -v` 式转义显示
- 或终端环境 locale 配置问题（但 Bash 工具未显式设置 `LANG`/`LC_ALL`）

**修复方向**：
1. 确认 Bash 工具 spawn 子进程时是否设置了 `LANG=en_US.UTF-8` 或 `LC_ALL=C` 导致中文输出被转义
2. 检查 TUI markdown/代码块渲染路径是否有按字节处理字符串的地方
3. 添加日志：Bash 工具执行前后打印 stdout 原始字节和转为 String 后的内容，确认乱码发生在哪个环节

**涉及路径**：
- `aemeath-tools/src/bash.rs`（Bash 工具输出收集）
- `aemeath-cli/src/tui/`（TUI 渲染层）
- 可能涉及 markdown 渲染中代码块/工具输出区域的文本处理

---

### #44 Bash 工具设置 600s timeout 仍被 120s 截断

**症状**：Bash 工具调用传入 `timeout: 600000` 时，UI/日志显示命令 timeout 为 600s，例如：

```text
$ docker build -f "apps/studio/docker/Dockerfile.dev" "apps/studio" --target builder --progress=plain  (timeout: 600s)
```

但实际执行约 120s 后失败：

```text
Tool Bash timed out after 120s
```

**影响**：长时间命令（如 Docker build、大型依赖安装、长测试）无法通过单次 Bash 调用完成；用户会被 `timeout: 600s` 展示误导，以为命令本应允许运行 10 分钟。

**根因假设**：Bash 工具自身参数/schema 允许传入最高 600s timeout，但工具执行外层还存在 tool call/API 网关/宿主 runtime 的 120s 硬超时。当前展示的是内层 Bash timeout，实际生效的是更短的外层 timeout。

**修复方向**：
1. 明确区分并记录两层 timeout：Bash 命令 timeout 与外层 tool call timeout。
2. 若外层硬限制可从配置读取，应在 Bash 工具展示和错误消息中显示有效 timeout（取两者较小值），避免 UI 显示 600s 但 120s 失败。
3. 若外层硬限制不可控，应在文档/工具描述中说明长任务需后台执行并轮询日志，例如 `cmd > /tmp/build.log 2>&1 &` 后分次查看。

**临时规避**：将长命令拆分为多个短步骤，或用后台进程执行并通过后续 Bash 调用轮询日志和退出状态。

**涉及路径**：
- `aemeath-tools/src/bash.rs`（Bash 工具 timeout 参数、命令展示、错误消息）
- tool call 调度/runtime 层（外层执行超时来源）

### #43 TaskUpdate 使用全局 id 但 TUI task list 显示局部编号，agent 引用编号不一致

**症状**：
- TUI task list 显示 batch 内局部编号（如 `#2 定位现有显示逻辑`、`#3 按 TDD 修改显示行为`、`#4 验证提交合并`）。
- Agent 调用 `TaskUpdate(9) -> completed`，tool 输出 `Updated task #9: 定位现有显示逻辑`，使用的是全局 task id `9`。
- 用户无法将 agent 输出中的 `#9` 与 TUI 显示的 `#2` 对应起来；agent 也可能因为编号不一致而引用错误的 task。

**复现**：
1. 创建多个 task list batch（如第一个 batch 创建 #1~#7，第二个 batch 创建 #8~#10）
2. TUI 中第二个 batch 显示为 `#1/#2/#3`（局部编号）
3. Agent 执行 `TaskUpdate` 时使用全局 id（如 `#9`），输出 `Updated task #9`
4. TUI 显示的局部编号是 `#2`，但 agent 报告的是 `#9`

**根因**：Bug #36 修复引入了 TUI 局部显示编号，但 TaskUpdate / TaskCreate 等 tool 的输入输出仍使用全局 task id。两套编号体系未对齐。

**修复方向**：
1. **方案 A**：TaskUpdate / TaskCreate / TaskList 等工具的输出统一使用与 TUI 一致的 batch 局部编号，内部映射回全局 id 执行操作。
2. **方案 B**：TaskUpdate 接口同时接受全局 id 和局部编号，tool schema 中注明优先使用局部编号。
3. **方案 C**：TUI 也显示全局 id，去掉局部编号（回退 #36 修复）。

推荐方案 A：agent 看到的编号应与 TUI 一致，降低用户困惑。TaskStore 需提供 batch 内局部编号到全局 id 的双向映射。

**涉及路径**：
- `aemeath-core/src/task.rs`（TaskStore 编号映射）
- `aemeath-tools/src/task_update.rs`（输入解析 + 输出格式）
- `aemeath-tools/src/task_create.rs`（输出格式）
- `aemeath-tools/src/task_list.rs`（输出格式）
- `aemeath-cli/src/tui/`（TUI task list 渲染，已用局部编号）

### #46 Output area Markdown 表格行选中复制内容错位

**状态**：待确认（已修复渲染缓存未 invalidate 问题）

**症状**：在 output area 中选中 markdown 表格行并复制时，复制出的文本与屏幕上看到的渲染内容不匹配。选中范围偏移、复制内容错位或缺失。

**根因**：

1. **渲染文本与原始 content 不一致**：表格行原始 `OutputLine.content` 为 markdown 格式（如 `| hello | world |`），但屏幕上渲染的是 `render_table_block()` 转换后的样式文本（如 ` ┼───┼───┼` 或 ` │ hello │ world │`），两者字符内容和显示宽度完全不同。
2. **screen_line_map offset 基于渲染文本，get_selected_text 读取原始 content**：
   - 有 selection 时，`render_table_rows()` 从 styled spans 拼回纯文本 `line_text`，再做 `push_wrapped_offsets` 记录 screen_line_map。offset 对应的是渲染后文本。
   - `get_selected_text()` 通过 `get_line_content(logic_idx)` 读取的是 `OutputLine.content`（原始 markdown 文本）。
   - 两套文本的字符数和宽度不同，导致 char offset 映射错位。
3. **无 selection 时**直接用原始 `row_spans` 渲染，不经过 `push_wrapped_offsets`，screen_line_map 中该行可能缺失或 offset 不正确。

**修复方向**：
1. **方案 A（推荐）**：表格行渲染后的文本作为 `get_line_content()` 的数据源，让 screen_line_map 和选中复制使用同一份渲染文本。可在 render 阶段将表格行的渲染文本暂存，或为表格行建立独立的 content 缓存。
2. **方案 B**：表格行不参与 selection，选中时跳过表格区域（简单但功能缺失）。
3. **方案 C**：`push_wrapped_offsets` 对表格行使用原始 markdown content 而非渲染文本，让 offset 与 `get_line_content()` 一致。但这样选中高亮位置会与屏幕显示错位。

推荐方案 A，保持选中高亮与复制内容一致。

**涉及路径**：
- `aemeath-cli/src/tui/output_area/render.rs`（`render_table_rows()`）
- `aemeath-cli/src/tui/output_area/selection.rs`（`get_line_content()`、`get_selected_text()`）
- `aemeath-cli/src/tui/output_area/markdown/table.rs`（`render_table_block()`）
- `aemeath-cli/src/tui/output_area/render_blocks.rs`（`render_table_cache()`）

**关联**：
- Feature #32（TUI 选中和复制逻辑统一）
- Bug #33（spinner 下方 task list 无法选中复制——同类问题已修复，修复模式可参考）

### #80 滚动条不跟随最新内容

**状态**：待确认（随 #58 渲染管线重构结构性修复）。全量替换逐行 push_line 累加 scroll_offset 的路径已删除，`render_document_from_view_model` 只 `set_document` + clamp；回归测试 `adapter/output_widget.rs::test_render_document_from_view_model_clamps_stale_scroll_offset`/`_preserves_valid_scroll_offset`。

**症状**：LLM streaming 输出时，滚动条不自动滚到最底部跟随最新内容。用户按 Shift+Home/PageUp 后即使按 Shift+End 回到最底，后续新内容仍然不跟随。

**根因**：`refresh_output_widget_from_model` 在每次 agent event（包括每字符 streaming text）触发 `replace_lines_from_view_model`，后者清空所有行后逐行 `push_line` 重建。`push_line` 在 `auto_scroll=false` 时每行 `scroll_offset += 1`。streaming 过程中若用户曾向上滚动（即使已用 Shift+End 回到底部），全量重建导致 `scroll_offset` 被累加到异常大值（N 行 × 原有偏移 → N + offset），`clamp_scroll_state` 将其 cap 到 `max_offset` 而非 0，`auto_scroll` 因此永远无法恢复为 `true`。

**修复**：`replace_lines_from_view_model` 全量重建期间临时设置 `auto_scroll = true`，防止 `push_line` 逐行递增 `scroll_offset`。重建完成后恢复原 `auto_scroll` 值，`clamp_scroll_state` 正确 clamp。

**涉及文件**：
- `apps/cli/src/tui/adapter/output_widget.rs`（`replace_lines_from_view_model`、`clamp_scroll_state`）
- `apps/cli/src/tui/output_area/scroll.rs`（`scroll_up`/`scroll_down`）
- `apps/cli/src/tui/app/update.rs`（`refresh_output_widget_from_model`）

### #86 TUI 中先展示结论再展示 tool call，顺序颠倒

**状态**：修复中

**症状**：LLM 响应流中先输出 text block（结论/总结文本），随后再输出 tool_use block；TUI 按流式到达顺序渲染，导致用户先看到结论文本，再看到 tool call 执行过程，视觉上不符合"先执行工具、再给结论"的因果顺序。

**根因（待确认）**：ConversationModel / OutputViewAssembler 按流式事件到达顺序追加 block，未对 text 与 tool_use 做因果排序。LLM（尤其 reasoning 模型）有时先输出一段总结/思考文本，再决定调用工具。

**修复方向**：
1. 延迟渲染 text block：当 text block 后续紧接 tool_use 时，暂缓显示 text，等 tool call 完成后再统一渲染
2. 流结束后重排：在 assistant turn 完成时，将 tool_use block 移到 text block 之前重新排列
3. 仅调整视觉顺序，不改变消息实际存储顺序（避免影响 conversation history）

**精确根因 + bind 修复（2026-05-30，实机 IDTRACE 日志定位）**：实机排查 #87 泄漏时定位到 id 绑定链路的两个数据层根因，与本 bug「前置 text/thinking」同源：
- **根因 A（index 语义错位）**：`ToolCallStart` 用「工具序号」(0,1)，而 `ObserveToolCall` 用「content-block 序号」。当 assistant 回复前面有 thinking/text block 时，content 序号整体 +1，两者错位（实测 `start 0,1` vs `bind 1,2`）→ `bind_tool` 的 `(name,index)` 精确匹配失败 → 该工具成 orphan、且首个占位永不绑定。
- **根因 B（跨轮占位覆盖）**：整个 Chat 只有一个 `turn-1`（`chat.rs` 无任何 `turns.push`），agent loop 每轮都往同一 turn 追加占位，`index` 跨轮重复（0,1,0,1）→ `bind_tool` 的 find-first 命中**前一轮已绑定**的占位并覆盖它 → 前轮 tool_call 的 id 被冲掉 → 其结果在 assemble 时 `find_tool_name_by_id`=None → 直接导致 #87 第二处泄漏。

**修复**：`bind_tool` 改为**只绑未绑定占位**——优先 `(name,index)` 精确匹配的未绑定占位，否则回退同名首个未绑定占位，**绝不覆盖已绑定占位**。一处修复同时根治 A（index 错位不再 orphan）与 B（跨轮不再覆盖丢 id），并消除 #87 第二处泄漏的数据层成因。

**回归测试**（`chat_turn::tests`）：`test_bind_tool_exact_match_binds_correct_placeholder`、`test_bind_tool_falls_back_to_unbound_when_index_mismatched`、`test_bind_tool_never_overwrites_already_bound_placeholder`。

**说明**：本次根治的是 id 绑定/泄漏；本条目原描述的「视觉上 text 先于 tool 的排序」若仍存在，属独立的渲染排序问题，仍需下方「修复方向」的重排，待实机确认。

**涉及文件**：
- `apps/cli/src/tui/model/conversation/chat_turn.rs`（`bind_tool` 只绑未绑定占位）
- `apps/cli/src/tui/model/conversation/model.rs` / `tool_flow.rs`（id 绑定链路）
- `apps/cli/src/tui/render/output/`（渲染管线）

### #87 TUI tool call 显示完整 tool result 内容且不受 max output 限制，result 渲染格式错误

**状态**：待确认

**症状**：tool call result 在 TUI 中渲染时曾存在两个问题：
1. **格式错误**：直接展示工具返回的原始 diff 内容或 Read 完整输出，而非格式化的摘要视图。
2. **不受最大行数限制**：大文件操作的完整内容会全部刷屏，看起来像 LLM 正文从 tool call result 中刷出。

**根因（已确认）**：嵌入 ToolResult 路径需要只展示 `ToolDisplay::format_result_summary()` 生成的短摘要；非嵌入/Orphan ToolResult 路径若直接透传 `output`，会被 `DiagnosticNotice` 逐行渲染完整内容。

**修复**：
1. ToolResult 子块只展示短摘要（例如 `✓ Read completed`），完整 Read 内容不进入渲染文本。
2. assistant 正文保持独立 `AssistantMessage` block，不混入 ToolResult。
3. 非嵌入与 Orphan ToolResult 均使用工具 display 摘要（不再透传/截断原始 output）。

**残留修复（2026-05-30）**：上一轮仅收敛了嵌入/非嵌入路径，OrphanToolResult 路径（结果早于 ToolCall 绑定且未提升）仍走 `summarize_orphan_result` 截断透传原始带行号 `output`，并以 `Warning`（橙）色整段刷出——表现为"正文刷屏 + 颜色不对"。本次：
- `ConversationBlock::OrphanToolResult` 新增 `tool_name` 字段，`observe_tool_result` push 时写入（此前 `_tool_name` 被丢弃）。
- assembler 的 orphan 臂改走 `summarize_non_embedded_result(Some(tool_name), ..)`，与非嵌入路径统一（DRY）；颜色随 Success/Error 而非 Warning。
- 删除冗余的 `summarize_orphan_result`。

**回归测试**：
1. `test_output_assembler_summarizes_embedded_tool_result_without_full_output`：Read 完整输出不会进入 ToolResult 子块。
2. `test_output_assembler_keeps_assistant_text_outside_read_result`：Read 完整 active.md 内容不泄漏到 ToolResult，后续 LLM 正文保持独立 AssistantMessage。
3. `test_non_embedded_tool_result_uses_summary`：非嵌入路径走摘要不刷屏。
4. `test_orphan_read_result_shows_summary_not_full_content` / `test_orphan_tool_result_shows_summary_not_raw_output`：orphan 路径只显示工具摘要、颜色为 Success，不刷原始内容。

**涉及文件**：
- `apps/cli/src/tui/view_assembler/output.rs`
- `apps/cli/src/tui/view_assembler/output_tests.rs`
- `apps/cli/src/tui/view_assembler/output_unit_tests.rs`
- `apps/cli/src/tui/render/output/blocks/tool_result.rs`
- `apps/cli/src/tui/model/conversation/block.rs`
- `apps/cli/src/tui/model/conversation/tool_flow.rs`

**第二处泄漏 + 治本（2026-05-30，实机 IDTRACE 日志定位）**：上述残留修复后实机仍复现刷正文。加 `LEAK-TRACE`/`IDTRACE` 日志定位到第二条泄漏路径与数据层根因：
- **表层第二处泄漏**：`ConversationBlock::ToolResult` 臂在 `find_tool_name_by_id`=None（工具名未知）时，`summarize_non_embedded_result(None, ..)` 走 `truncate_output_lines` 把完整带行号 `output` 当摘要刷出（5 行 + `N lines omitted`）。修复：`summarize_non_embedded_result` 永不截断原始 output——`tool_name=None` 也走通用完成摘要（占位名 `Tool`）；删除 `truncate_output_lines`。
- **数据层根因（详见 #86）**：`find_tool_name_by_id`=None 的成因是 `bind_tool` 跨轮覆盖/错位导致 tool_call 的 id 丢失/未绑定，已由 #86 的 `bind_tool`「只绑未绑定占位」根治。

新增回归：`test_summarize_non_embedded_unknown_tool_uses_generic_summary`、`test_non_embedded_tool_result_with_unknown_id_does_not_leak_raw_output`。

### #88 TUI Read tool call 头部下重复显示一行 `Read /path`

**状态**：待确认

**症状**：Read 工具调用渲染时，头部 `✓ Read(/path)` 下方多出一行冗余的 `Read /path`，路径重复显示。

**根因（已确认）**：`ReadDisplay::format_header` 输出 `Read({path})`（含路径），`format_details` 又输出 `Read {path}`（再次含路径）；`render_tool_call` 把 header 作首行、每条 detail 作后续行，于是路径出现两次。对照 `Glob`/`WebFetch` 的 `format_details` 已是空 `vec![]`，Read 属漏改。

**修复**：`ReadDisplay::format_details` 不再重复路径——无 offset/limit 时返回空 `vec![]`，仅在带 offset 时输出 `offset: N`（含 limit 时追加 `, limit: M`）。

**回归测试**（`tool_display::tests`）：
1. `test_format_tool_call_read_details_does_not_duplicate_path`：details 不含 header 已有的路径。
2. `test_format_tool_call_read_no_offset_yields_no_detail_line`：无 offset/limit 时 details 为空。
3. `test_format_tool_call_read_offset_limit_shown_without_path`：有 offset/limit 时展示该信息且不重复路径。

**涉及文件**：
- `apps/cli/src/tui/render/output/tool_display/tool_impls.rs`
- `apps/cli/src/tui/render/output/tool_display/mod.rs`

