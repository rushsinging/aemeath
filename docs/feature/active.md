# 活动中 Feature

| # | 标题 | 优先级 | 状态 | 确认结果 | 目标 |
|---|------|--------|------|----------|------|
| 8 | Memory 系统 | - | ✅ 已完成 | 未确认 | 跨会话持久化记忆，记忆作为一等公民，LLM 自主管理 + Hook 兜底。详见 [spec](specs/008-memory-system.md) |
| 9 | 反思系统 | - | ✅ 已完成 | 未确认 | 关键节点自动反思，发现偏差并提炼经验写入 Memory（依赖 #8）。详见 [spec](specs/009-reflection-system.md) |
| 18 | Task list 跨轮次 batch 机制 | - | ✅ 已完成 | 未确认 | Task 跟随 session 持久化，不再每次用户消息清空；按 batch 分组显示，新 turn 自动切换到新 batch，旧 batch 隐藏；已完成 task 在当前 batch 内继续显示 |
| 21 | TUI 优化 Agent 调用输出展示 | - | ✅ 已完成 | 未确认 | Agent 子任务每个 turn 仅显示工具名列表（如 `Read, Read, Grep`），噪声大、看不出进展。改为按工具+目标/参数摘要分组、合并连续同工具调用、按阶段（探索/编辑/验证）分段，并提供折叠展开 |
| 24 | Spinner 下方 task list 限量显示（最多 7 条） | 中 | ✅ 已完成 | 未确认 | task 多时显示过长挤占主输出。改为窗口化显示：上一条 completed + 所有 in_progress + 后续 pending，总数封顶 7 条；其余以 `… +N more` 折行提示。摘要行 `Tasks: x/y` 仍反映全量进度 |
| 25 | Task list 跨轮次生命周期策略 | 中 | ✅ 已完成 | 未确认 | 同 session 新对话开始时仍显示上次的 task list。补齐三种场景策略：① 全部完成时自动清屏归档；② 中断未完成时提示用户「继续 / 暂存 / 丢弃」；③ 多轮未推进的旧 task 自动提醒确认是否继续 |
| 26 | TUI TEA 架构纯化 | 中 | ✅ 已完成 | 未确认 | 完成 5 项核心改造：① is_processing 移入 App 状态 ② Cmd enum 新增 Batch/RunHookNotification/ReadClipboardImage/ProcessImageFile ③ clipboard/image 3 处 tokio::spawn 改为 Cmd ④ hook notification 3 处 tokio::spawn 改为 Cmd ⑤ 删除废弃 event_handler.rs。update() 去除了所有直接 tokio::spawn，副作用统一收口到 Cmd/runtime |
| 27 | 日志系统职责分层 + 全模块覆盖 | 中 | ✅ 已完成 | 未确认 | ~~filter 缺失~~ 已修复（LoggingConfig::default 四 crate debug）；~~agent.log 双重记录~~ 已迁移到 log::info!；~~LLM request dump 完整 messages~~ 已减量为摘要（消息数 + 最近3条类型）；~~debug.log 直接写入~~ 已切换到 log::info!/log::debug! |
| 28 | MCP 系统完善 | 高 | 待实施 | 未确认 | 当前已有 MCP 骨架（stdio 客户端、Manager、工具注册、/mcp 命令），但存在多处缺口：缺少 SSE/Streamable HTTP 传输、Prompts 支持、Sampling 支持、运行时动态增删 server、server 健康检查与自动重连、tool 变更热刷新、权限审批流程、TUI MCP 状态面板。详见下方详情 |
| 29 | Task reminder 被动注入 | 高 | ✅ 已完成 | 未确认 | TUI 路径已实现：每轮扫描上一条 assistant 消息中的 TaskCreate/TaskUpdate，节流（≥5轮间隔）后注入极简 `<system-reminder>` 摘要。REPL 路径暂未注入 |

### #18 Task list 跨轮次 batch 机制

**目标**：Task list 跨轮次持久化，不再每次用户消息清空。通过 batch 机制区分不同 turn 的 task list，旧 batch 自动隐藏。

**已完成的改动**：

1. **移除自动清空**：`stream.rs` 不再在每次进入时调用 `_task_store.clear()`，task 跟随 session 生命周期。
2. **Batch ID 机制**：`Task` 新增 `batch` 字段，`TaskStore` 新增 `current_batch` 计数器。`create()` 时检测上一 batch 是否全部 completed/deleted，如果是则递增 batch。
3. **当前 batch 显示**：新增 `list_current_batch()` 方法，TUI 只显示最新 batch 的 task（含 Completed）。
4. **Completed 可见**：当前 batch 内 Completed 的 task 继续显示（✓ 图标），摘要行 `━━ Tasks: 3/5 ━━` 反映完成进度。

**涉及路径**：
- `aemeath-core/src/task.rs`（batch 字段、current_batch 计数器、list_current_batch）
- `aemeath-cli/src/tui/app/mod.rs`（update_task_status 使用 list_current_batch）
- `aemeath-cli/src/tui/app/stream.rs`（移除 clear 调用）

---

### #21 TUI 优化 Agent 调用输出展示

**目标**：优化 Agent 子任务每个 turn 的工具调用进度展示，避免只显示 `Read, Read, Grep` 这类无目标列表。

**已完成的改动**：

1. **结构化事件协议**：Agent progress 从 `Sender<String>` 升级为 `Sender<AgentProgressEvent>`，不再依赖 TUI 解析 `[Turn N]` 文本。
2. **工具调用摘要**：Agent runner 根据 tool call input 生成 `AgentToolCallProgress.summary`，例如 `Read ×2: src/lib.rs, src/main.rs | Grep: "AgentProgress" in src`。
3. **同工具分组**：TUI 根据结构化 calls 按工具名合并，并显示调用次数；turn/sequence 仅用于内部定位，默认不展示。
4. **当前进度单行更新**：同一个 Agent tool 的 `ToolCalls` 进度只保留一行，新事件替换旧行，不重复刷屏。
5. **兼容保留**：`AgentProgressKind::Message` 用于普通文本 progress，仍按原逻辑追加和去重。

**涉及路径**：
- `aemeath-cli/src/agent_runner.rs`（Agent tool call progress 摘要生成）
- `aemeath-cli/src/tui/output_area/tool_display.rs`（同 turn progress 替换）

**测试**：新增单元测试覆盖结构化事件构造、目标摘要生成、同 Agent 当前进度替换、不同 Agent 互不覆盖、普通 Message progress 兼容。

---

### #24 Spinner 下方 task list 限量显示（最多 7 条）

**目标**：当 task 数量较多（10+）时，spinner 下方的 task list 占据屏幕大量空间，把主对话/输出挤到看不见。改为按"前后文相关性"窗口化显示，固定上限 7 条左右，让用户能快速看到"刚做完什么、正在做什么、接下来做什么"，而不是被一长串 ☐ pending 淹没。

**当前现状**（`aemeath-cli/src/tui/app/mod.rs:639-672`）：
- `update_task_status()` 把当前 batch 内**所有**非 deleted 的 task 全部 push 到 `task_status_lines`
- 摘要行 `━━ Tasks: x/y ━━` + 每个 task 一行（`✓` / `■` / `□` + 编号 + 标题 + owner）
- 7 条 task → 占 8 行；20 条 task → 占 21 行；输出区域所剩无几

**预期窗口化策略**：

显示顺序（completed → in_progress → pending）：

```
━━ Tasks: 3/15 ━━              ← 摘要行始终反映全量
✓ #3 拆分 mod.rs                ← 上一条 completed，仅显示 1 条
■ #4 拆分 hook.rs               ← 所有 in_progress 全显示
■ #5 拆分 task.rs
□ #6 拆分 scheduler.rs           ← 后续 pending，按余量填充
□ #7 拆分 state.rs
□ #8 拆分 guidance.rs
… +7 more pending               ← 折叠提示
```

具体规则：
1. **摘要行保持全量**：`Tasks: x/y` 不受窗口限制
2. **窗口按优先级填充**（默认上限 7 条）：
   - 上一条 completed（最近完成的 1 条）
   - 所有 in_progress（一般 1~3 条）
   - 后续 pending 按 task id 升序填充剩余配额
3. **超出部分**：`… +N more pending` 单行折叠提示
4. **没有 in_progress 时**：第一条 pending 视为"接下来要做"，显示前 6 条 + `… +N more`
5. **全部 completed 时**：显示最后 5~7 条 completed
6. **空 task list**：不显示窗口

**配置项**：
```json
{
  "ui": {
    "task_list": {
      "max_lines": 7,
      "show_last_completed": 1,
      "fold_hint_format": "… +{n} more {status}"
    }
  }
}
```

**实施分解**：
1. `update_task_status()` 增加窗口化逻辑（分桶 → 按规则取窗口 + 折叠提示）
2. 拆出纯函数 `build_task_window(tasks, max_lines, last_completed_count) -> Vec<String>`，单独测试
3. 单元测试覆盖：0 / 1 / max / max+1 / 远超 max 各档；全 pending / 全 in_progress / 全 completed / 混合；in_progress 数量超过 max 时 pending 全部隐藏

**涉及路径**：
- `aemeath-cli/src/tui/app/mod.rs`（`update_task_status` 窗口化）
- 新增：`aemeath-cli/src/tui/app/task_window.rs`（纯函数 + 单元测试）
- `aemeath-core/src/config/`（`ui.task_list.max_lines` 等配置字段）

**关联**：
- Feature #18（task batch 机制）—— 在 batch 之上做窗口化，正交
- Feature #25（task 跨轮次生命周期）—— 限量解决"显示太多"，#25 解决"显示太久"
- Bug #29（主 agent task 不更新）—— 修复后窗口化逻辑会更频繁触发

**开放问题**：
- max 默认 7 是否合适？高分屏 vs 小屏权衡
- 折叠提示是否可点击展开？留作后续 polish
- 全部 completed 时显示 last 5 vs 折叠成 `Tasks: 15/15 ✓ all done`

---

### #25 Task list 跨轮次生命周期策略

**目标**：在同一 session 内，处理"上一轮的 task list 在新对话开始时还会显示"的问题。当前 Feature #18 的 batch 机制只是"新 turn 切到新 batch"，但没规定旧 batch 怎么收尾、怎么提示用户、何时归档。本 feature 补齐三种典型场景的明确策略。

**用户痛点**：「同一个 session 中，新的对话开始时还会显示上次的 task list」

具体场景：
- 上轮 task 全做完了 → 新对话开头还看到一长串 ✓，没价值还占地方
- 上轮 task 没做完用户主动问别的 → 旧 task 状态尴尬，是继续？是放弃？没出路
- 上轮 task 多轮没推进（用户跑题、agent 偏题）→ 沉默积压在 batch 里没人理

---

#### 场景 1：上一轮 task 全部完成

**触发**：上一 batch 内所有 task 都是 `Completed`（或 `Cancelled`），且用户输入新对话。

**策略**：
- 新 turn 开始时检测上一 batch 是否 100% 完成
- 是 → 自动隐藏旧 batch（保留在 TaskStore 历史中，可通过 `/task history` 回看）
- 显示一行 toast（1~2 秒）：`✓ 上一组 task 已完成（5/5）`
- 新 batch 在用户新 task 出现时才创建

#### 场景 2：上一轮 task 中断、用户开新话题

**触发**：上一 batch 内有 `InProgress` / `Pending` task，用户输入了一条**与未完成 task 主题不相关**的新消息。

**判断"主题不相关"**（启发式，不调 LLM）：
- 关键词重叠率低（task 标题与新消息分词后 cosine 相似度 < 0.2）
- 或：用户消息以 `/` 开头（slash 命令通常是控制流）
- 或：消息含明显切换语气（"先放一下"、"换个话题"、"另外"、"对了"等）

**策略**：弹 inline 提示（不阻塞输入）：
```
⚠ 上一组 task 还有 3 项未完成（#4 #5 #6），是否：
  [c] 继续上次任务   [p] 暂存稍后回来   [d] 丢弃这组任务
  （直接回车默认 [p] 暂存）
```

- `[c]` 继续：保留旧 batch 为当前 batch，新消息作为"补充指令"附加
- `[p]` 暂存：旧 batch 标记为 `paused`，从视图隐藏，可 `/task resume <batch_id>` 恢复
- `[d]` 丢弃：旧未完成全部 `Cancelled`，归档

#### 场景 3：旧 task 沉默积压

**触发**：某 batch 内有 `InProgress` / `Pending`，连续 N 轮（默认 3）用户对话没推进它（没 TaskUpdate 涉及它，没 tool call 修改了 task 涉及的文件等）。

**策略**：
```
ℹ 以下 task 已沉默 3 轮：
  ■ #4 拆分 hook.rs
  □ #5 拆分 task.rs
  仍要继续吗？回 /task keep 保留 / /task drop 丢弃 / /task pause 暂存
```

- 不打断当前对话，提示出现一次后不重复（直到再过 N 轮或用户回复）
- 提示文本不入 LLM context（仅 UI 可见，避免污染对话）

---

**配置项**：
```json
{
  "ui": {
    "task_lifecycle": {
      "auto_clear_completed_on_new_turn": true,
      "interrupt_prompt_enabled": true,
      "interrupt_default_action": "pause",
      "stale_remind_after_turns": 3,
      "stale_remind_repeat_interval": 5
    }
  }
}
```

**新增命令 / 状态**：
- `Task.batch_status`: `Active | Paused | Archived`
- `/task pause` —— 当前 batch → Paused
- `/task resume [batch_id]` —— 恢复指定 batch
- `/task keep` —— 沉默提示中确认保留
- `/task drop` —— 当前未完成全部 Cancelled
- `/task history` —— 列出本 session 内所有 batch

**实施分解**：
1. **TaskStore 扩展**：`batch_status` 字段、`Batch` 结构（id / created_at / last_active_turn / status）
2. **场景 1 检测**：`update_task_status()` 调用前 check 上一 batch → 全 completed 隐藏 + toast
3. **场景 2 启发式 + 提示 UI**：新增 `topic_relevance_check(prev_tasks, new_message)`，触发时 push `UiEvent::TaskInterruptPrompt`
4. **场景 3 沉默检测**：turn 结束 hook 中递增每个未完成 task 的 `silence_turns`；达阈值 push `UiEvent::TaskStaleReminder`
5. **命令实现**：`commands/task.rs` 增加 pause / resume / keep / drop / history

**涉及路径**：
- `aemeath-core/src/task.rs`（Batch 结构、batch_status、silence_turns）
- 新增：`aemeath-core/src/task/lifecycle.rs`（场景判定纯逻辑 + 单元测试）
- `aemeath-cli/src/tui/app/mod.rs`（update_task_status 触发场景检测）
- `aemeath-cli/src/tui/app/update.rs`（处理 TaskInterruptPrompt / TaskStaleReminder UI 事件）
- 新增：`aemeath-core/src/command/commands/task.rs`（pause / resume / keep / drop / history）
- `aemeath-core/src/config/`（`ui.task_lifecycle` 配置）

**关联**：
- Feature #18（task batch 机制）—— 本 feature 在 batch 之上加生命周期状态
- Feature #24（task list 限量显示）—— 限量解决"显示太多"，本 feature 解决"显示太久"
- Bug #29（主 agent task 不更新）—— 修好后场景 1/3 才能准确触发

**开放问题**：
- 主题相关性判断用关键词重叠率够吗？误判率 vs 复杂度（要不要直接调 LLM？太重）
- 场景 2 提示 inline vs ask_user？倾向 inline，但要确认默认 `[p] pause` 不会让用户莫名其妙
- batch 归档保留多久？session 结束时持久化，session resume 时是否复活？
- `/task history` 输出格式：表格 vs 树形？

---

### #26 TUI TEA 架构纯化

**目标**：将当前 TUI 从 TEA-style 进一步收口为更严格的 TEA 架构：事件统一进入 `Msg`，状态变化集中在 `update()`，副作用统一由 `Cmd` 描述并在 runtime 层执行，渲染只读取状态。

**当前判断**：部分符合 TEA，但不是严格 TEA。

已符合：
- `aemeath-cli/src/tui/app/msg.rs` 已有统一 `Msg`，覆盖 terminal event、paste、resize、tick、async `UiEvent`。
- 已有 `Cmd`，包含 `Quit`、`SpawnProcessing`、`SendEvents`、`SaveSession` 等副作用描述。
- `run_loop()` 基本形态为：收集事件 → 转为 `Msg` → `update()` → 执行 `Cmd` → `draw()`。
- 主要 UI 状态集中在 `App`。

主要偏差：
1. `update()` 仍不是纯状态转移，内部存在 `tokio::spawn(...)`、hook notification、clipboard/image 异步读取等副作用。
2. `update()` 参数过重，仍直接依赖 `ui_tx`、`active_cancel`、`SpawnContextRefs`、外部 `is_processing`。
3. 部分异步流程绕过 `Cmd`，直接在 update 层发送事件或启动后台任务。
4. `event_handler.rs` 仍保留 deprecated 旧路径，说明 TEA 迁移尚未彻底清理。
5. task list 刷新通过 `draw` 前异步查询 `TaskStore` 完成，不完全是消息驱动。

**预期设计**：
- `update()` 尽量收口为 `App + Msg -> App + Cmd`。
- 所有副作用都通过 `Cmd` 返回，由 `run_loop()` 的 command executor 统一执行。
- `is_processing` 纳入 `App`，不再作为外部 mutable 状态传入。
- 异步结果通过 `UiEvent` / `Msg::Ui` 回流。
- `draw()` 只根据 `App` 当前状态渲染，不触发异步状态同步。

**建议新增 / 扩展 Cmd**：
- `Cmd::ReadClipboardImage`
- `Cmd::ProcessImageFile(String)`
- `Cmd::RunHookNotification { message, kind }`
- `Cmd::ReplyAskUser { reply_tx, answer }`
- `Cmd::DrainQueuedInput`
- `Cmd::RefreshTaskSnapshot`
- `Cmd::Batch(Vec<Cmd>)`（用于一次 update 返回多个副作用）

**实施分解**：
1. 将 `is_processing` 移入 `App`，消除 update 外部 mutable 参数。
2. 为多副作用场景引入 `Cmd::Batch(Vec<Cmd>)`。
3. 把 update 内 clipboard/image 相关 `tokio::spawn` 改为 `Cmd::ReadClipboardImage` / `Cmd::ProcessImageFile`。
4. 把 update 内 hook notification `tokio::spawn` 改为 `Cmd::RunHookNotification`。
5. 把 AskUserQuestion 回复发送、queued input drain 等 runtime 交互改为 Cmd。
6. task list 从 draw 前轮询改为 `Msg::Tick` / `UiEvent::TaskSnapshotChanged` 驱动。
7. 清理 deprecated `event_handler.rs`，保留唯一事件路径。
8. 为纯逻辑 update 分支补充单元测试：输入提交、处理中排队、Ctrl+C 中断、AskUser 选择、ToolCall/ToolResult 状态推进。

**涉及路径**：
- `aemeath-cli/src/tui/app/msg.rs`（扩展 Cmd）
- `aemeath-cli/src/tui/app/mod.rs`（run_loop command executor、is_processing 收口）
- `aemeath-cli/src/tui/app/update.rs`（移除副作用，返回 Cmd）
- `aemeath-cli/src/tui/app/event_handler.rs`（删除或彻底迁移 deprecated 旧路径）
- `aemeath-cli/src/tui/app/processing.rs`（SpawnProcessing command 执行边界）
- `aemeath-cli/src/tui/app/task_window.rs` / task status 刷新路径

**验收标准**：
- `update.rs` 中不再直接出现 `tokio::spawn`。
- `update.rs` 不直接调用 hook runner 的异步方法。
- `update()` 不再接收 `ui_tx`、`active_cancel`、外部 `is_processing` 可变引用。
- `draw()` 前不再直接异步查询 task store；task 状态通过消息更新。
- TUI 行为保持不变：输入、粘贴图片、AskUserQuestion、工具调用展示、队列输入、Ctrl+C/Esc 中断均正常。

**关联**：
- Feature #7 / #12：input queue 与双层循环机制，可作为消息驱动改造重点。
- Feature #21：Agent progress 已结构化，可继续沿用 `UiEvent` 回流。
- Feature #24 / #25：task list 展示与生命周期逻辑后续可借 TEA 化减少状态错配。
- Bug #30：input queue 不被消费，可能与当前异步流程边界不清有关。

---

### #27 日志系统职责分层 + 全模块覆盖

**目标**：当前日志系统存在两个问题：① `aemeath.log` 与 `agent.log` 职责模糊，大量内容重叠，难以按目的快速定位；② `env_logger` 的 `default_filter_or` 仅显式配置了 `aemeath_llm=debug,aemeath_cli=debug`，其余 crate（`aemeath_core`、`aemeath_tools`）走全局 `warn` 级别，导致 `aemeath-core` 中的 compact、skill、guidance、cost、scheduler 等模块的 `log::debug!` / `log::info!` 调用全部被静默丢弃。

#### 问题 1：日志文件职责不清

当前 4 个日志文件的写入情况：

| 文件 | 写入方 | 内容 |
|------|--------|------|
| `aemeath.log` | `env_logger` 全局 pipe（main.rs） | 所有 `log::*` 宏的输出（info/warn/error/debug），仅包含 filter 通过的模块 |
| `agent.log` | `logging::append_agent_line` / `append_json_line_with_turn` | sub-agent 启动/progress、LLM request payload（messages/system_blocks/tool_schemas）、主 agent 的 LLM request |
| `debug.log` | **无人写入** | `LogFile::Debug` 枚举已定义但无任何调用点 |
| `panic.log` | `init_panic_hook` | panic 信息 + backtrace |

问题：
- `aemeath.log` 和 `agent.log` 都包含 LLM 请求信息，但格式不同（文本 vs JSON），互相重复又不完整
- `debug.log` 定义了但未使用，浪费概念
- 用户排查问题时不知道该看哪个文件

#### 问题 2：全模块日志丢失

`init_logging()` 的 filter 配置：

```rust
// main.rs:81
env_logger::Env::default().default_filter_or("warn,aemeath_llm=debug,aemeath_cli=debug")
```

结果：
- `aemeath_llm` → debug ✓ 可见
- `aemeath_cli` → debug ✓ 可见
- `aemeath_core` → **warn** ✗ debug/info 被丢弃（compact、skill、guidance、cost、scheduler 等模块的 debug 日志全部丢失）
- `aemeath_tools` → **warn** ✗ debug/info 被丢弃

受影响的日志调用点（部分）：
- `aemeath-core/src/compact/autocompact.rs` — 压缩决策 log::warn
- `aemeath-core/src/skill/mod.rs` — skill 加载 log::warn
- `aemeath-core/src/guidance/resolver.rs` — guidance 匹配 log::debug
- `aemeath-core/src/cost/tracker.rs` — 成本持久化失败 log::warn
- `aemeath-core/src/scheduler/mod.rs` — 调度器任务管理 log::warn
- `aemeath-tools/src/list_mcp_resources.rs` — MCP 资源列举失败 log::warn

#### 预期设计

**1. 日志文件职责重新划分**

| 文件 | 职责 | 内容 |
|------|------|------|
| `aemeath.log` | **应用主日志**：所有模块的结构化运行日志 | env_logger pipe 接收全部 `log::*` 输出，包含所有 crate（core/cli/llm/tools）的 info/warn/error/debug |
| `agent.log` | **Agent 对话审计日志**：LLM 交互的完整记录 | 主 agent 和 sub-agent 的每次 LLM 请求/响应摘要、tool call 触发与结果摘要、token 用量、模型切换。面向"复现对话流程"而非"调试内部状态" |
| `panic.log` | **Panic 崩溃日志**（不变） | panic 信息 + backtrace |
| ~~`debug.log`~~ | **废弃** | 无使用场景，删除 `LogFile::Debug` 枚举 |

**2. 全模块日志 filter 修复**

```rust
// 改前
"warn,aemeath_llm=debug,aemeath_cli=debug"
// 改后
"warn,aemeath_llm=debug,aemeath_cli=debug,aemeath_core=debug,aemeath_tools=debug"
```

或更简洁：
```rust
"warn,aemeath=debug"  // 如果所有 crate 共享 aemeath 前缀
```

**3. agent.log 职责收窄**

当前 `agent.log` 被用于两类不同的信息：
- **运行日志**（sub-agent progress 文本、agent loop event）→ 迁移到 `aemeath.log`（通过 `log::*` 宏）
- **审计日志**（LLM request/response payload、token 统计）→ 保留在 `agent.log`

具体迁移：
- `append_agent_line(LogFile::Agent, ...)` 中的 progress 文本 → 改为 `log::info!(target: "agent", "[role:{} model:{}] {}", role, model, msg)`
- `log_agent_loop_event()` → 改为 `log::info!(target: "agent_loop", ...)`
- `log_tool_result_event()` → 改为 `log::info!(target: "tool_result", ...)`
- LLM request payload（messages/system_blocks/tool_schemas）→ 保留 `append_json_line_with_turn(LogFile::Agent, ...)`
- **LLM request 日志减量**：当前每次请求把完整 `messages` 数组 dump 到 agent.log，数百轮对话后单条目可达数 MB。改为只记录本次请求 **新增的消息**（通常 1-2 条 user/assistant message），cached system blocks 和已发送的历史消息不再重复记录
- 新增：LLM response 摘要（stop_reason、token 用量、耗时）→ `append_json_line_with_turn(LogFile::Agent, ...)`

**4. 配置**

```json
{
  "logging": {
    "default_level": "warn",
    "module_levels": {
      "aemeath_llm": "debug",
      "aemeath_cli": "debug",
      "aemeath_core": "debug",
      "aemeath_tools": "debug"
    },
    "agent_log": {
      "enabled": true,
      "include_request_payload": true,
      "include_response_summary": true,
      "max_payload_bytes": 65536
    },
    "max_bytes": 10485760,
    "max_backups": 5,
    "retention_days": 30
  }
}
```

#### 实施分解

1. **修复 env_logger filter**：`default_filter_or` 加入 `aemeath_core=debug,aemeath_tools=debug`
2. **迁移 agent progress 到 log 宏**：`append_agent_line` 的非审计调用改为 `log::info!`，删除 `LogFile::Debug`
3. **agent.log 收窄为审计日志**：仅保留 LLM request/response 结构化记录，新增 response 摘要写入
4. **配置化 filter**：从 config.json 读取 module_levels，覆盖硬编码 default_filter_or
5. **验证**：全模块 grep `log::*` 调用点，确认 filter 修复后全部可达

#### 实施记录（2026-05）

已完成的改动：

| 文件 | 改动 | 说明 |
|------|------|------|
| `aemeath-core/src/logging.rs` | 删除 `LogFile::Debug` | ~~已事先完成~~ |
| `aemeath-core/src/config/logging.rs` | `LoggingConfig::default()` 已含四 crate debug | ~~已事先完成~~ |
| `aemeath-cli/src/tui/app/stream.rs` | `log_agent_loop_event()` → `log::info!` | 迁移到 aemeath.log |
| `aemeath-cli/src/tui/app/stream.rs` | `log_llm_request_messages()` → 摘要模式 | 只记录消息数 + 最近3条 role/length，不 dump 完整 messages |
| `aemeath-cli/src/agent_runner.rs` | `log_request_messages()` → 摘要模式 | 同上 |
| `aemeath-cli/src/main.rs` | 移除 debug.log 直接写入 | 改用已有的 `log::info!` |
| `aemeath-cli/src/tui/output_area/tool_display.rs` | `debug_log()` → `log::debug!` | 切换调试日志通道 |

#### 涉及路径

- `aemeath-cli/src/main.rs`（init_logging filter 修复 + 配置化）
- `aemeath-core/src/logging.rs`（删除 LogFile::Debug，职责文档化）
- `aemeath-cli/src/agent_runner.rs`（progress 迁移到 log 宏，保留审计写入）
- `aemeath-cli/src/tui/app/stream.rs`（log_agent_loop_event 迁移，保留 LLM request 审计）
- `aemeath-core/src/config/mod.rs`（新增 logging 配置段）

#### 关联

- CLAUDE.md 日志规范节——需同步更新文件职责描述
- Bug #30（input queue 不消费）—— 修复 filter 后能通过 `aemeath.log` 看到更多 agent loop 诊断信息

#### 已完成的改动

1. **日志级别配置化**：新建 `aemeath-core/src/config/logging.rs`，定义 `LoggingConfig` + `SubAgentLogConfig`，支持 `default_level`、`module_levels` (HashMap)、`SubAgentLogConfig`。`Config` 新增 `logging` 字段，`init_logging()` 改为读取配置后调用 `to_filter_string()`，`RUST_LOG` 环境变量仍可覆盖。

2. **配置合并**：`merge.rs` 中实现 logging 配置的深度合并（module_levels 叠加，标量字段优先 overlay）。

3. **筛选器修复**：通过配置默认值 `module_levels: {aemeath_llm=debug, aemeath_cli=debug, aemeath_core=debug, aemeath_tools=debug, sub_agent=info}` 全覆盖所有 crate。不再硬编码逐个模块。

4. **LogFile::Debug 删除**：枚举变体、`file_name()` 分支及测试均已移除，调试日志统一走 `aemeath.log`。

5. **agent.log 收窄**：`agent_runner.rs` 中 sub-agent `progress` 闭包从 `append_agent_line(LogFile::Agent, ...)` 改为 `log::info!(target: "sub_agent", ...)`，运行进度写入 `aemeath.log`。LLM 请求审计 (`append_json_line_with_turn`) 保留在 `agent.log`。

6. **~/.aemeath/config.json 已更新**：添加 `logging` 段，module_levels 囊括所有 crate。

---

### #8 Memory 系统

**目标**：跨会话持久化记忆，让 agent 在不同会话间积累项目知识、用户偏好和决策上下文，避免每次从零开始。

**存储设计**：

```
~/.aemeath/memory/
├── _global.json          # 全局记忆（跨项目）
├── <project-hash>/       # 项目级记忆
│   ├── _index.json       # 记忆索引（id → metadata）
│   ├── <id>.json         # 单条记忆
│   └── _archive/         # 过期/合并后的归档
```

**记忆条目结构**：

```rust
struct MemoryEntry {
    id: String,             // UUIDv7
    category: MemoryCategory,
    content: String,        // 记忆正文
    source: String,         // 来源：session id / reflection / user
    project: Option<String>,// 项目标识（None = 全局）
    relevance_tags: Vec<String>,  // 检索标签
    created_at: u64,
    accessed_at: u64,       // 最后一次被检索注入的时间
    access_count: u32,      // 被检索次数（用于优先级排序）
    expires_at: Option<u64>,// 过期时间（None = 永久）
}
```

**分类**：

```rust
enum MemoryCategory {
    ProjectStructure,  // 项目架构、文件组织
    Decision,          // 重要设计决策及其理由
    Preference,        // 用户偏好（语言、风格、框架选择等）
    Pattern,           // 项目特定模式（命名规范、错误处理方式）
    Pitfall,           // 已知坑点/踩坑记录
    Context,           // 一般上下文知识
}
```

**写入时机**（通过 Hook 触发）：

| 时机 | HookEvent | 写入策略 |
|------|-----------|---------|
| 会话结束时 | `SessionEnd` | LLM 总结本会话关键决策和发现，写入 memory |
| 压缩后 | `PostCompact` | 提取被压缩掉的重要上下文到 memory |
| 用户主动 | `/memory add <content>` 命令 | 直接写入 |
| 反思系统 | `ReflectionGenerated`（新事件） | 反思结果写入 |

**检索注入**（System Prompt 构建阶段）：

1. `build_system_prompt_parts()` 中新增 memory 检索步骤
2. 基于当前 cwd 定位项目 memory 目录
3. 按 `access_count` + `created_at` 加权排序，取 top-N（默认 10 条）
4. 注入到 system prompt 的 dynamic_part 中：
   ```
   # Project Memory
   - [Decision] 使用 tokio channel 而非 mpsc，因为需要跨 async task 通信
   - [Pattern] 错误处理统一用 AemeathError，thiserror derive
   - [Pitfall] bash.rs 中 check_command_safety 不受 allow_all 控制，已修复
   ```
5. 更新被注入条目的 `accessed_at` 和 `access_count`

**新增模块**：

- `aemeath-core/src/memory.rs` — MemoryStore（CRUD + 索引 + 检索 + 淘汰）
- `aemeath-core/src/command/commands/memory.rs` — `/memory` 命令

**新增命令**：

| 命令 | 说明 |
|------|------|
| `/memory` | 显示当前项目的 memory 摘要 |
| `/memory add <content>` | 添加一条记忆 |
| `/memory search <query>` | 搜索记忆 |
| `/memory delete <id>` | 删除一条记忆 |
| `/memory clear` | 清空项目记忆 |

**淘汰策略**：
- 单条记忆超过 90 天未被访问（`accessed_at`）且 `access_count < 3` → 归档
- 单项目记忆超过 100 条 → 触发合并：将相近 tag 的记忆用 LLM 合并为一条摘要
- 归档文件不删除，可通过 `/memory search` 搜索

**配置**（`config.json`）：

```json
{
  "memory": {
    "enabled": true,
    "max_entries_per_project": 100,
    "max_inject_count": 10,
    "auto_summary_on_session_end": true,
    "archive_after_days": 90
  }
}
```

**依赖**：无外部依赖，纯文件系统存储 + JSON 序列化。

---

### #9 反思系统

**目标**：在关键节点自动触发反思，让 agent 从过去的行为中提炼经验，写入 Memory 系统，避免重复犯错。

**反思触发时机**：

| 触发点 | 条件 | 反思内容 |
|--------|------|---------|
| 连续工具失败 | 同一 turn 内 ≥2 次工具调用失败 | 失败原因分析 + 正确做法 |
| 会话结束 | `SessionEnd` hook | 整体会话总结 + 关键决策 |
| 子代理结束 | `SubagentStop` hook | 子代理执行摘要 |
| 用户中断 | 用户按 Escape 取消 | 当前进度快照 + 未完成原因 |
| 重试后成功 | API 错误后重试成功 | 错误类型 + 重试策略有效性 |

**反思流程**：

```
触发条件满足
  → 构造反思 prompt（含近期对话片段）
  → 调用 LLM 生成反思摘要（用轻量模型，如 deepseek-chat）
  → 解析反思结果为结构化 MemoryEntry
  → 写入 MemoryStore
```

**反思 Prompt 模板**：

```
你是一个反思助手。请分析以下对话片段，提炼出对未来会话有价值的信息。

要求：
1. 只记录客观事实和有效经验，不要记录临时状态
2. 每条不超过 200 字
3. 标注分类：Decision / Pattern / Pitfall / Preference

对话片段：
{recent_messages}

请输出 JSON 数组：
[{"category": "...", "content": "...", "tags": ["..."]}]
```

**反思结果结构**：

```rust
struct ReflectionResult {
    entries: Vec<ReflectionEntry>,
}

struct ReflectionEntry {
    category: MemoryCategory,
    content: String,
    tags: Vec<String>,
}
```

**实现策略**：

1. 反思调用使用**独立轻量 LLM 调用**（非主对话），避免干扰上下文
2. 反思在后台异步执行（tokio::spawn），不阻塞主循环
3. 反思结果静默写入 MemoryStore，不显示在对话中
4. 仅在 `memory.enabled = true` 且有有效反思内容时触发

**配置**（`config.json`）：

```json
{
  "reflection": {
    "enabled": true,
    "model": "deepseek/deepseek-chat",
    "max_entries_per_reflection": 3,
    "min_turns_for_session_summary": 5,
    "consecutive_failures_threshold": 2
  }
}
```

**依赖**：
- Feature #8（Memory 系统）— 反思结果写入 MemoryStore
- Hook 系统 — 通过 HookEvent 触发反思

**实施阶段**：
- P0：会话结束反思（最核心，收益最大）
- P1：连续工具失败反思
- P2：子代理反思、用户中断反思

---

### #9 反思系统

**目标**：在关键节点（任务完成、Stop、错误恢复后、用户显式触发）执行反思流程，对最近的行为、决策、失败、用户反馈做结构化总结，将有价值的经验写入 Memory 系统（#8），让 agent 在未来会话中能够基于历史经验做更好的决策。

**依赖**：Feature #8 Memory 系统（反思的输出目标）

**当前实现状态 / 缺口**：

当前 `/reflect` 命令已经接通，但只是轻量占位实现，不等同于完整反思系统：

- `aemeath-core/src/command/commands/reflect.rs` 会读取当前 Project Memory，并调用轻量输出逻辑。
- 当项目 Memory 为空时，会输出类似「当前项目没有长期记忆，建议在关键决策后写入 Memory」。
- 当前不会调用 LLM 分析最近对话，也不会基于 `aemeath-core/src/reflection/prompt.rs` 的 prompt 生成结构化建议。
- `suggested_memories` 当前不会自动产生有效条目，通常显示「暂无建议」。
- `/reflect apply` 目前也是占位行为，不会自动写入 Memory。
- 因此当前状态应理解为：Reflection 命令/框架已存在，但智能反思、自动建议、自动落库尚未真正生效。

**补齐目标**：
1. `/reflect` 能读取最近 N 轮对话摘要，而不仅是读取已有 Memory。
2. 调用独立轻量 LLM，根据 `build_reflection_prompt()` 生成 JSON 反思结果。
3. 解析 `deviations`、`suggested_memories`、`outdated_memories`、`user_alert`。
4. 支持 `/reflect apply` 将确认后的 `suggested_memories` 写入 Project Memory。
5. 根据配置决定是否允许自动应用建议；默认建议先人工确认，避免污染长期记忆。

**设计草案**：

#### 触发时机
- **任务完成后**：TaskUpdate 将 task 置为 `completed` 时，对该 task 的执行过程做总结
- **Stop 事件**：会话结束 / agent 主动停止时，对整段会话做反思
- **错误恢复后**：tool call 失败 → 修复 → 成功 的链路上，提炼"哪种修复有效"
- **用户显式触发**：`/reflect` slash 命令，对最近 N 轮做即时反思
- **PostCompact 钩子**：上下文压缩前抢救关键经验

#### 反思维度
- **成功模式**：哪些工具组合 / 推理路径达成了目标
- **失败教训**：哪些假设错了、哪些 tool call 走了弯路
- **用户偏好**：用户在本次会话中的纠正、拒绝、确认（参考 superpowers `feedback` 类型）
- **未解决问题**：本次会话中悬而未决的事项（提示下次继续）

#### 输出格式
- 结构化条目（type / title / body / scope），写入 Memory 系统
- 每条反思 must 标注来源会话 ID + 时间戳，便于追溯
- 避免重复：写入前检索 Memory，相似条目优先 update 而非 insert

#### 实施阶段
1. **Phase 1**：实现 `/reflect` 命令 + 基础反思 prompt 模板（依赖 #8 已落地的 Memory 接口）
   - 当前仅完成命令骨架与轻量占位输出，尚未接入 LLM 反思与自动写入。
2. **Phase 2**：接入 Stop / TaskUpdate(completed) 自动触发
3. **Phase 3**：错误恢复链路反思 + PostCompact 钩子

**涉及路径**（待实施）：
- `aemeath-core/src/reflection/` — 反思引擎、prompt 模板、写入策略
- `aemeath-core/src/command/commands/reflect.rs` — `/reflect` 命令
- `aemeath-cli/src/tui/app/update.rs` — Stop 事件触发钩子
- `aemeath-cli/src/tui/app/stream.rs` — TaskUpdate / 错误恢复触发钩子

**开放问题**：
- 反思是否消耗当前 session 的 model 调用，还是用独立的轻量 model（成本权衡）
- 反思失败（如 LLM 返回空）时是否静默丢弃 vs 提示用户
- Memory 容量上限策略：何时压缩 / 淘汰旧反思

---

### #12 Input Queue 双层循环优化

**目标**：让 LLM 在一个 user turn 内部（API call → tool calls → 下一次 API call → tool calls ...）的细粒度节点上**主动检查 input queue**，把用户排队的反馈尽早注入对话流，而不是等整个 agent loop 跑完才"看到"用户的新输入。让用户感受到"agent 听得见我"，而不是"agent 必须把这一摊事干完才理我"。

**背景**：
- Feature #7 已实现多消息 input queue（VecDeque），processing 期间用户可连续排队多条输入
- 当前消费时机是**外层 user-turn 循环**末尾——agent 完成所有 tool call、模型给出最终 stop_reason=EndTurn 后才 pop 一条 queue 进入下一轮
- 痛点：当 agent 进入长链路（连续 N 个 tool call、长 thinking、子 agent 嵌套）时，用户中途看到方向跑偏想纠正，目前必须等整轮结束才能让 agent 看到——体验上像"AI 自顾自跑"，用户反馈延迟极高
- Bug #21（粘贴入队语义）和 Feature #11（reasoning_effort）都是输入控制相关，本 feature 解决"何时让 agent 看到输入"

**设计**：

#### 1. 双层循环模型

```
outer loop: per user turn (现状)
  └─ inner loop: per agent step（API call + tool exec）
     ├─ 每次 inner 迭代开始前：检查 input queue
     ├─ 若 queue 非空：把队列内容作为 user message 注入 messages，跳过本轮原计划，继续 inner loop
     └─ 若 queue 为空：照常发起下一次 API call / 工具执行
```

inner loop 退出条件（沿用现状）：模型返回 `stop_reason = EndTurn` 且无 tool call。

#### 2. 检查点（粗到细）

按介入成本递增分级：

| 检查点 | 介入成本 | 说明 |
|--------|----------|------|
| **A. 每次 API call 前** | 低（必做） | 下一轮请求构造前 pop 全部 queue，作为 user message 拼到 messages 末尾。模型在下次回复时就能看到 |
| **B. tool call 批次完成后** | 低（必做） | 一批并行/顺序 tool call 跑完、准备发回 LLM 前，先 pop queue。最自然的"让 LLM 看到用户新指令"时机 |
| **C. tool call 之间（顺序）** | 中（可选） | 如果 tool call 改顺序执行（Bug #3 的修复方向），可在两个 tool call 之间检查；带"用户已发声"信号意味着后续 tool call 可能被取消 |
| **D. streaming 期间** | 高（不做） | 中断正在进行的 API call。语义复杂、provider 兼容性差，**不在本期范围** |

本期落地 **A + B**。C 留作后续扩展，需要 Bug #3 完成顺序执行后再做。

#### 3. 注入语义

用户排队消息进入 messages 时怎么标记？两种方案：

- **方案 1（普通 user message）**：直接 `Message::user(content)` 拼到末尾，模型自然继续对话
- **方案 2（带元数据的 system note）**：包成 `<user_interrupt>...</user_interrupt>` 或类似标签，提示模型"这是用户中途追加的反馈，请优先采纳"

推荐**方案 1 默认 + 方案 2 配置开关**。普通方案足够大部分场景；标签包裹在 agent 自主决策长链路被纠偏时有用。

#### 4. 取消进行中工作的策略

用户中途插话时，已经 in-flight 的 tool call 怎么办？

- **本期**：让进行中的 tool call **跑完**（不取消），跑完后注入用户消息，下一轮 API call 前模型自己决定要不要采纳
- **后期**（依赖 CancellationToken 基础设施）：选项化的"温柔取消"——给 in-flight tool 发取消信号，taken-effect 后注入用户消息

#### 5. 队列读取并发安全

- 当前 input queue 是 `VecDeque<String>` 包在 App 状态里，UI 线程 push、agent loop 主线程 pop
- 已有共享访问机制（具体待 grep 确认 `Arc<Mutex<...>>` / `tokio::sync::Mutex` / channel）
- 双层循环本期只是**多次调用同一个 pop 接口**，不改并发模型

#### 6. UI 反馈

- 用户在 processing 中输入并 Enter 后：input queue 区显示新条目（已有）
- inner loop 在 A/B 检查点 pop 到消息时：在 output area 注入一条 system 提示行 `[Injected from queue: "..."]`，让用户**看到**"我的反馈被吃进去了"，而不是默默并入下一轮 prompt
- 状态栏可临时高亮 1s 表示"queue 已消费"

#### 7. 配置

`config.json` 新增：
```json
{
  "input_queue": {
    "interrupt_mode": "between_calls",  // off | between_calls | between_tools
    "wrap_with_metadata": false          // 是否用 <user_interrupt> 标签包裹
  }
}
```

CLI 不暴露（属于体验设置，slash 命令 `/queue mode <...>` 切换）。

#### 8. 实施阶段

1. **Phase 1**（本期）：在 `agent_runner.rs` / `processing.rs` 的 inner loop A/B 检查点加 `pop_all_queued()` 调用 + UI 注入提示
2. **Phase 2**：增加 `<user_interrupt>` 包裹选项 + `/queue` slash 命令
3. **Phase 3**（依赖 Bug #3 顺序执行 + cancel 基础设施）：tool call 之间检查（C 检查点）、温柔取消进行中的 tool

**测试场景**：
- 用户 send 消息 → agent 进入 5 个 tool call 链 → 用户在第 2 个 tool 执行时排队 "stop, focus on X" → 期望：第 2 个 tool 跑完后，下一次 API call 前模型立即看到 "stop, focus on X" 并改变方向
- 用户连续排队 3 条 → 一次 pop 全部 → 拼成 3 条 user message 一起注入
- 队列在 inner loop 跑完都没消费过 → 退到 outer loop 时按原逻辑 pop（保持兼容）
- agent 在 ask_user 等待中（Bug #19 已修复）→ queue 不消费，等 ask_user 走完
- subagent 嵌套时：父 agent 的 queue 不应被子 agent 消费；子 agent 自己有独立 inbox（待决策，建议本期父子 agent 都不互通）

**涉及路径**：
- `aemeath-cli/src/agent_runner.rs`（agent 主循环 inner step）
- `aemeath-cli/src/tui/app/processing.rs`（user turn 顶层循环）
- `aemeath-cli/src/tui/app/mod.rs`（input queue 数据结构 + pop 接口）
- `aemeath-cli/src/tui/app/update.rs`（UI 注入提示）
- `aemeath-core/src/config/mod.rs`（`input_queue` 配置）
- 新增（Phase 2）：`aemeath-core/src/command/commands/queue.rs`

**关联**：
- Feature #7（input queue 基础实现，已完成）
- Bug #21（粘贴入队语义）— 必须先确保入队来源干净
- Bug #3（tool call 流式 + 顺序执行）— Phase 3 的 C 检查点依赖
- Bug #19（ask_user 等待态独占 input，已修复）— queue 消费时需绕开 ask_user 状态

**开放问题**：
- 子 agent 是否共享父 agent 的 input queue？默认不共享，但 deeply nested agent 时父用户反馈如何透传？
- 标签包裹 `<user_interrupt>` 是否 model-agnostic？某些模型可能把它当 XML 字面量解析
- 用户排队"取消当前 tool call"语义如何表达？需要一个特殊关键字 / 命令前缀（例如 `/cancel`）还是 LLM 自行从语义判断？

---

### #28 MCP 系统完善

**目标**：将当前 MCP 骨架（stdio 客户端 + 基础 Manager + 工具注册 + 占位 `/mcp` 命令）升级为完整的 MCP 系统，支持多传输协议、运行时生命周期管理、安全审批、TUI 可视化，对齐 Claude Code 的 MCP 能力。

#### 当前已有实现

| 模块 | 路径 | 状态 |
|------|------|------|
| MCP Server Config | `aemeath-core/src/mcp.rs` | ✅ `McpServerConfig`（command + args + env） |
| Stdio Client | `aemeath-core/src/mcp.rs` | ✅ JSON-RPC over stdin/stdout，initialize → list_tools → call_tool |
| 连接管理器 | `aemeath-core/src/mcp_manager.rs` | ⚠️ 已实现 `McpConnectionManager`（connect_all / reconnect / register_tools），但未被主流程使用 |
| 启动加载 | `aemeath-cli/src/mcp_loader.rs` | ✅ 从 `.mcp.json` / `~/.aemeath/mcp.json` 读取并注册 |
| MCP Tool | `aemeath-tools/src/mcp_tool.rs` | ✅ 动态 Tool trait 实现 |
| ListMcpResources | `aemeath-tools/src/list_mcp_resources.rs` | ✅ 列出 server 资源 |
| ReadMcpResource | `aemeath-tools/src/read_mcp_resource.rs` | ✅ 读取指定资源 |
| `/mcp` 命令 | `aemeath-core/src/command/commands/tools.rs` | ⚠️ 占位实现，add/remove/tools 均返回静态文本提示 |
| 安全校验 | `aemeath-core/src/mcp.rs` | ✅ 命令绝对路径 + shell 元字符拦截 + 危险 env 过滤 |

#### 缺口与目标

##### 1. 传输协议扩展

当前仅支持 **stdio** 传输。需要新增：

- **SSE (Server-Sent Events)**：`url` 字段配置，通过 HTTP + SSE 双向通信
- **Streamable HTTP**：MCP 2025-03-26 新传输，单 HTTP 端点 + 可选 SSE 升级
- 配置格式对齐 Claude Code：
  ```json
  {
    "mcpServers": {
      "my-server": {
        "command": "/path/to/server",   // stdio
        "args": ["--port", "3000"],
        "env": { "KEY": "VALUE" }
      },
      "remote-server": {
        "url": "https://example.com/mcp",  // SSE / Streamable HTTP
        "headers": { "Authorization": "Bearer ..." }
      }
    }
  }
  ```

**涉及路径**：
- `aemeath-core/src/mcp.rs`（新增 `SseTransport` / `StreamableHttpTransport`，抽象 `Transport` trait）
- `McpServerConfig` 新增 `url` / `headers` 字段

##### 2. McpConnectionManager 接入主流程

当前 `mcp_loader.rs` 绕过 Manager 直接操作 `ToolRegistry`。需要：

- `main.rs` 改用 `McpConnectionManager` 管理全部 MCP 生命周期
- Manager 持有 `ToolRegistry` 引用，动态注册/注销工具
- 支持运行时增删 server（不重启）

**涉及路径**：
- `aemeath-cli/src/main.rs`（替换 `load_mcp_tools` 为 Manager）
- `aemeath-cli/src/mcp_loader.rs`（改为 Manager 初始化入口）
- `aemeath-core/src/mcp_manager.rs`（补齐 register/unregister 动态逻辑）

##### 3. Server 健康检查与自动重连

- 定时心跳（`ping` JSON-RPC 方法），检测 server 存活
- 连接断开时自动重连（已有 `auto_reconnect` 配置，需实际实现）
- 重连后重新 discover tools 并更新 registry
- 重连失败达到上限时标记 `Failed`，通知 UI

**涉及路径**：
- `aemeath-core/src/mcp_manager.rs`（心跳任务 + 重连逻辑）
- `aemeath-core/src/mcp.rs`（`McpClient::ping`）

##### 4. Tool 变更热刷新

- Server 通过 `notifications/tools/list_changed` 通知工具列表变更
- Client 收到后重新 `tools/list`，diff 增删并更新 `ToolRegistry`
- 新工具自动出现在 LLM tool schema 中，删除的工具从 schema 移除

**涉及路径**：
- `aemeath-core/src/mcp.rs`（notification 监听循环）
- `aemeath-core/src/mcp_manager.rs`（tool diff + registry 更新）

##### 5. Prompts 支持

MCP 协议的 `prompts/list` + `prompts/get` 能力：

- LLM 可通过 `ListMcpPrompts` / `GetMcpPrompt` 工具发现和使用 MCP prompt
- Prompt 可包含参数化模板，返回格式化的 prompt 文本
- 注入到对话上下文作为 system/user message

**涉及路径**：
- 新增 `aemeath-tools/src/list_mcp_prompts.rs`
- 新增 `aemeath-tools/src/get_mcp_prompt.rs`

##### 6. Sampling 支持（可选，P2）

MCP 协议的 `sampling/createMessage` 能力：MCP server 反向请求 LLM 生成。

- 需要安全审批：server 发起 sampling 请求时，弹出用户确认
- 用当前 session 的 model 执行，结果回传给 server
- 成本计入当前 session

##### 7. 运行时动态增删 Server

完善 `/mcp` 命令为真正的操作命令：

| 命令 | 说明 |
|------|------|
| `/mcp` | 列出所有 server 及其状态、工具数量 |
| `/mcp add <name>` | 交互式添加 server（输入 command/url、args、env） |
| `/mcp remove <name>` | 断开连接并移除 server 配置 |
| `/mcp tools [server]` | 列出指定 server 或全部 server 的工具 |
| `/mcp restart <name>` | 重启指定 server |
| `/mcp logs <name>` | 查看 server 最近 stderr 输出 |

**涉及路径**：
- `aemeath-core/src/command/commands/mcp.rs`（从 tools.rs 拆出独立命令文件）

##### 8. 安全审批流程

当前安全仅做启动时命令校验。需要增强：

- **首次连接审批**：新 server 首次连接时弹出确认，展示 command / url / 工具列表，用户批准后才注册
- **Tool 调用审批**：高敏感度工具（写文件、执行命令类）调用前需用户确认
- **审批配置**：
  ```json
  {
    "mcp": {
      "auto_approve_servers": ["trusted-server"],
      "require_approval_for_tools": ["*"],
      "never_approve_tools": ["dangerous_tool"]
    }
  }
  ```
- 审批状态持久化，同项目同 server 只需首次审批

**涉及路径**：
- `aemeath-core/src/mcp_manager.rs`（审批状态管理）
- `aemeath-cli/src/tui/app/update.rs`（审批 UI 弹窗）

##### 9. TUI MCP 状态面板

在 TUI 中展示 MCP 连接状态：

- `/mcp` 命令输出格式化为表格（server 名称、状态图标、工具数、延迟）
- tool call 执行时，output area 标注 `[MCP:server_name]` 前缀
- server 断开/重连时在 output area 显示通知行

**涉及路径**：
- `aemeath-cli/src/tui/output_area/`（MCP tool call 标注）
- `aemeath-cli/src/tui/app/mod.rs`（MCP 状态通知）

#### 配置

```json
{
  "mcp": {
    "servers": {
      "my-server": {
        "command": "/path/to/server",
        "args": [],
        "env": {}
      }
    },
    "auto_connect": true,
    "auto_reconnect": true,
    "reconnect_delay_seconds": 5,
    "max_reconnect_attempts": 3,
    "health_check_interval_seconds": 30,
    "require_approval": true,
    "auto_approve_servers": [],
    "max_tool_response_bytes": 1048576
  }
}
```

配置来源优先级（对齐项目约定）：
1. 项目级 `.mcp.json`
2. 全局 `~/.aemeath/config.json` 中的 `mcp` 段
3. 全局 `~/.aemeath/mcp.json`

#### 实施阶段

- **P0（必须）**：McpConnectionManager 接入主流程 + 运行时增删 server + `/mcp` 命令完善 + 安全审批流程
- **P1（重要）**：SSE/Streamable HTTP 传输 + 健康检查与自动重连 + tool 变更热刷新
- **P2（可选）**：Prompts 支持 + Sampling 支持 + TUI MCP 状态面板

#### 涉及路径

- `aemeath-core/src/mcp.rs`（传输抽象、新 transport、ping）
- `aemeath-core/src/mcp_manager.rs`（接入主流程、生命周期、审批）
- `aemeath-cli/src/mcp_loader.rs`（改为 Manager 初始化）
- `aemeath-cli/src/main.rs`（Manager 集成）
- `aemeath-core/src/command/commands/mcp.rs`（从 tools.rs 拆出，完整实现）
- `aemeath-tools/src/mcp_tool.rs`（配合 Manager 重构）
- `aemeath-tools/src/list_mcp_prompts.rs`（新增）
- `aemeath-tools/src/get_mcp_prompt.rs`（新增）
- `aemeath-cli/src/tui/app/update.rs`（审批 UI）

#### 关联

- Feature #17（Skill 延迟加载）—— MCP tool 注册与 Skill 注册共享 ToolRegistry，动态注册机制可复用
- Bug #27 / #29（Task 状态不更新）—— Manager 重构不应影响 tool call 执行链路

#### 开放问题

- SSE 传输是否需要WebSocket fallback？
- Sampling 支持的成本归因：算 session 成本还是独立计费？
- 项目级 `.mcp.json` 中的 server 审批是否应更严格（防供应链攻击）？
- tool 响应大小限制默认值多少合适？1MB？


### #29 Task reminder 被动注入

**目标**：每 N 轮自动在 user message 流里注入精简 task 状态提醒，LLM 不用主动调 `TaskList` 也能感知当前进度。参考 Claude Code 的 `getTaskReminderAttachments`。

**设计决策**：

1. **不进 system prompt**：避免破坏 prompt cache
2. **极简内容**：只一行摘要，控制在 ~30 tokens。不需要 TaskList 的完整展开
3. **节流**：避免每轮都喂

**注入格式**：

```text
<system-reminder>
Tasks: 5/10 done, 2 in_progress, 3 pending. Use TaskList to see details.
</system-reminder>
```

**节流策略**：

| 参数 | 值 | 说明 |
|------|-----|------|
| `TURNS_SINCE_WRITE` | 5 | 距上次 TaskCreate/TaskUpdate 的 assistant turn 数 ≥ 5 才注入 |
| `TURNS_BETWEEN_REMINDERS` | 5 | 距上次注入 reminder ≥ 5 轮才再注入 |
| 熔断 | — | 最后一条 assistant message 中已有 TaskCreate/TaskUpdate → 不注；task 列表为空 → 不注 |

**注入时机**：

1. 每轮请求前，在 attachment pipeline 中（与 CLAUDE.md 注入同位置）
2. 倒扫 recent assistant messages，统计 tool_use
3. 满足条件时构建 `<system-reminder>` message，作为 `isMeta: true` 的 user message 插入
4. 若有多个 system-reminder，合并到一条 message 中

**涉及路径**：

- `aemeath-cli/src/tui/app/stream.rs`：请求前的 attachment 注入点
- `aemeath-cli/src/tui/app/mod.rs`：`TaskStore` 引用传递
- `aemeath-cli/src/repl/mod.rs`：REPL 模式同样需要注入

**Token 预算**：

注入内容控制在 ~30 tokens（约 100 字符）。对比完整 TaskList 输出（每个 task 2~3 行 ≈ 20-30 tokens），注入 10 个 task 的完整清单需要 200-300 tokens——只用摘要可减少 90% 开销。

**开放问题**：

- 摘要中是否包含 pending task 的 ID 列表？有利 LLM 直接 TaskUpdate，但增加 token
- 子 agent 是否也需要注入？当前 TaskStore 无隔离，子 agent 能看到主 agent 的所有 task——建议先只对主 agent 注入


