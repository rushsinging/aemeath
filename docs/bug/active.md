# 活动中 Bug

| # | 标题 | 优先级 | 状态 | 确认结果 | 发现日期 | 根因类别 |
|---|------|--------|------|----------|----------|----------|
| 32 | Task list 窗口化显示异常：任务接近完成时窗口收缩为 1-2 条 | 高 | 待确认 | 未确认 | 2026-05 | E 轮修复(2026-05-18)：温和扩展和下限保护从未过滤 completed 回退补齐，窗口始终保持 max_lines 行 |
| 42 | TUI 中 Bash 工具输出中文显示为乱码（M- 转义序列） | 中 | 活动中 | 未确认 | 2026-05 | 多条 Bash 命令输出中的中文字符在 TUI 中显示为 `M-eM-^P` 等 cat -v 风格转义序列；Bash tool 使用 `from_utf8_lossy` 不会产生此输出，疑似 TUI 渲染层或 ratatui 文本处理将 UTF-8 多字节字符误转义 |
| 44 | Bash 工具设置 600s timeout 仍被 120s 截断 | 中 | 待确认 | 未确认 | 2026-05 | 已修复：BashTool 覆写 timeout_secs() 返回 600s，匹配 schema 最大允许值；agent.rs 外层超时不再在 Bash 内部 timeout 前截断 (355aca6) |
| 46 | Output area Markdown 表格行选中复制内容错位 | 高 | 待确认 | 未确认 | 2026-05 | 已修复：render 记录 Markdown 表格渲染后的逻辑行文本，selection 统一使用 screen_line_map 对应的数据源，避免 box drawing 表格和原始 Markdown offset 错位 |
| 47 | LLM 声称派发多个 reviewer 但 Agent 实际串行执行 | 高 | 待确认 | 未确认 | 2026-05 | 已修复：execute_non_agent 按并发安全分组并行执行 + Agent tool description 新增并行指引引导 LLM 批量发出 tool calls |

## 专案

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

**关联**：
- Feature #24（task list 窗口化限量显示）—— 本 bug 是 #24 窗口化策略的缺陷
- Feature #18（task batch 机制）—— 同属 task list 显示链路


### #33 Spinner 下方 task list 无法选中和复制

**症状**：spinner 下方的 task list 行（摘要行 `━━ Tasks: 3/5 ━━` 及每条 task 的 `✓ #1 标题`、`■ #2 标题`、`□ #3 标题`）在 TUI 中可见但鼠标无法选中、无法复制。拖拽选中时这些行被跳过，`Ctrl+C` 复制时也拿不到文本。

### #41 执行 /reflect 时 TUI 短暂卡死后才出现 LLM 输出

**状态**：活动中

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
1. **`execute_non_agent` 串行执行所有 non-agent tool calls**：`tools.rs` 中 `execute_non_agent` 使用 `for call in &other_calls` 逐个串行执行，即使工具标记为 `is_concurrency_safe()` 也不并行。每个 call 单独调用 `agent.execute_tools(slice::from_ref(&call))`，完全绕过了 `Agent.execute_tools` 的并发分组逻辑。
2. **LLM 分多轮生成 Agent tool calls**：部分 provider 的 LLM 倾向在不同轮次中逐个生成 Agent tool call，而非在同一轮中批量发出多个 tool_use blocks。Agent tool description 中缺少明确的并行指引。
3. **`execute_agent_calls` 已支持并行**：`agent_calls.rs` 使用 `chunks(batch_size)` + `join_all` 并行执行 Agent calls，此路径无问题。

**修复**：
1. **`execute_non_agent` 并发安全工具并行化**：重构为按 `is_concurrency_safe()` 分组——并发安全工具使用 `Semaphore` + `join_all` 并行执行，非安全工具保持串行。保持原始 tool call 顺序不变。新增 `execute_one_non_agent` 提取单个 tool call 的执行逻辑（hook chain + execute + post hooks + UI result）。
2. **Agent tool description 新增并行指引**：在 tool description 中添加 `IMPORTANT — Parallel execution` 段，明确告知 LLM “同一轮中发出多个 Agent tool calls 会并行执行”、“不要跨多轮逐个发出”。
3. **回归测试**：新增 4 个 `execute_tools` 并发测试——并发安全工具并行执行、非安全工具串行执行、结果顺序保持原始顺序、混合并发/串行场景。

**涉及路径**：
- `aemeath-cli/src/tui/app/stream/tools.rs`（`execute_non_agent` 并行化）
- `aemeath-tools/src/agent_tool.rs`（Agent tool description 并行指引）
- `aemeath-core/src/agent.rs` + `agent_tests.rs`（并发分组测试）
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

**状态**：活动中

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
