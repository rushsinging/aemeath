# TUI Model/View 架构重设计

## 背景

`apps/cli` 当前 TUI 同时承担了事件消费、业务状态维护、输出行修改、渲染缓存和 ratatui 绘制等职责。此前对 CLI-Agent tool 事件链路的分析表明，tool call 标题颜色偶发不正确的根因不是单点样式问题，而是状态真相、显示模型和渲染缓存边界混杂：

- tool 完成状态由 output line 的内容和样式隐式表达。
- `ToolCall` / `ToolResult` 事件匹配依赖弱绑定和 fallback。
- render cache 与 output line 状态变更缺少统一失效边界。
- 渲染阶段仍可能覆盖完成态符号和颜色。

本文提出一套新的 TUI Model/View 架构，用于逐步重构 `apps/cli/src/tui`。目标不是一次性重写，而是先建立稳定边界，再分阶段迁移 tool 状态、输入编辑、运行状态和副作用执行。

相关背景文档：

- `docs/superpowers/specs/2026-05-27-cli-agent-tool-events.md`

## 设计目标

1. TUI 的核心模型是“用户与 Agent 的交互会话”，不是 `status line` / `input area` / `output area` 三个屏幕区域。
2. 业务真相只存在于 Model State；View State 只能服务显示，不能反向决定业务状态。
3. 保留 TEA 外壳，用 Model Context 重构 TEA Model 内部边界。
4. Agent/SDK/runtime 事件进入 TUI 后必须先被适配为内部意图，不能直接修改输出行。
5. `ToolCall.status` 成为 tool 标题图标和颜色的唯一来源。
6. Render 只消费 ViewModel 和 ViewState，不匹配 tool id、不修改模型、不根据文本反推状态。
7. 架构约束必须能被 stop hook 保护，避免后续改动重新把业务逻辑塞回渲染层或 update 副作用中。

## 术语

本文统一使用以下术语：

| 术语 | 含义 |
|---|---|
| `Msg` | TEA update loop 的统一入口，包装 terminal、Agent、timer、hook 等外部输入。 |
| `Model` | TEA Model 内部的应用模型，保存业务真相和状态转换规则。不是 LLM model。 |
| `Model Context` | Model 内部按业务能力拆分的上下文，如 Conversation、Input、Runtime、Diagnostic。 |
| `Intent` | Application Coordinator 发给某个 Model Context 的处理意图。 |
| `Change` | Model Context 处理 Intent 后产生的状态变化事实。 |
| `ViewAssembler` | 从 Model + ViewState 组装 ViewModel 的模块。 |
| `ViewModel` | Render 的输入数据，描述界面应该显示什么。 |
| `ViewState` | 纯显示交互状态，如 scroll、collapse、selection、animation、render cache。 |
| `Render` | 把 ViewModel + ViewState 画到 terminal 的 ratatui 层。 |
| `Effect` | update 后需要 runtime 执行的副作用描述。 |
| `ViewModelDirty` | 标记某个 ViewModel 需要重新组装。 |

不再使用 `Domain` / `Projection` / `Presenter` / `Cmd` 作为目标架构术语。现有代码中已有的 `Cmd` 可逐步迁移为 `Effect` 语义。

## 总体分层

目标数据流：

```text
External Event
  ↓
Msg
  ↓
Application Coordinator / update
  ↓
Model
  ↓
ViewAssembler
  ↓
ViewModel
  ↓
Render
  ↓
Effect executor
```

更贴近 TEA 的表达：

```text
Msg → update → Model changes → ViewAssembler builds ViewModel → render → Effect
```

### 分层职责

1. **External Event**
   - terminal key/mouse/resize/tick
   - SDK/Agent chat event
   - hook/permission/tool event
   - task status refresh
   - timer/spinner

2. **Msg**
   - 外部事件进入 TUI update loop 的统一消息。
   - 只做包装和路由，不承载业务规则。

3. **Application Coordinator / update**
   - 把 `Msg` 转成 `Intent`。
   - 协调多个 Model Context。
   - 调用 adapter/mapper 隔离外部协议。
   - 收集 `Change`。
   - 标记 `ViewModelDirty`。
   - 生成 `Effect`。

4. **Model**
   - 保存业务真相。
   - 执行状态转换规则。
   - 不依赖 ratatui、不执行 IO、不调用 AgentClient、不发 channel。

5. **ViewAssembler**
   - 从 Model + ViewState 组装 ViewModel。
   - 可以决定显示文本、semantic status、segment、placeholder、dialog 内容。
   - 不修改 Model，不执行 IO，不写 ratatui buffer。

6. **ViewModel**
   - Render 的结构化输入。
   - 应以 block、segment、semantic style 表达显示意图，而不是暴露业务状态可变引用。

7. **Render**
   - 只负责 layout、wrapping、markdown render、semantic style 到 ratatui style 的转换、scrollbar、cursor、overlay。
   - 不匹配 tool id，不修改 `ToolCall.status`，不根据 output 文本反推状态。

8. **Effect executor**
   - 执行副作用。
   - Effect 结果必须重新进入 `Msg`。

## Model Context 划分

初版 Model 拆成 4 个 context：

```text
Conversation / Input / Runtime / Diagnostic
```

### Conversation Model

负责“用户与 Agent 的交互会话”。它维护对话结构、chat 生命周期、chat turn 生命周期、tool call 状态和 assistant stream。

Conversation 保存的是用户可见的对话语义，不保存 runtime 内部 sub-agent 实体。Runtime-origin event 必须经过 anti-corruption mapper，转换为 Conversation 语言后才能进入 Conversation Model。

### Input Model

负责“用户正在编辑什么，以及用户想表达什么意图”。它维护 buffer、cursor、selection、history、completion、attachment 和 submit 输出。

Input 只产出 `InputSubmission` 或输入状态变化，不直接启动 Agent，不直接改 output，也不决定是否排队。

### Runtime Model

负责外部运行环境和副作用执行状态。它维护 provider/model id、cwd/worktree、session metadata、processing job、task status、usage/cost 等。

Runtime 可以保存 Agent/SDK 事件的运行元数据，但不保存 Conversation 内部 tool 状态真相。

### Diagnostic Model

负责错误、警告、提示和阻塞请求。它统一承接 warning、error、hook blocked、permission prompt、orphan event、late event、debug diagnostic 等。

Diagnostic 让 output/status/dialog 不再各自维护错误状态。

## 信息流设计

TUI 内部信息流分成三条主线。

### User Intent Flow

```text
Terminal Key/Mouse/Paste
→ Msg::Input(...)
→ InputIntent
→ InputModel
→ InputChange
→ Coordinator
→ Conversation/Runtime/Diagnostic Intent
→ Effect
```

例：用户按 Enter。

```text
KeyEvent::Enter
→ Msg::Input(InputMsg::KeyPressed)
→ InputIntent::Submit
→ InputChange::Submitted { submission }
→ Coordinator 判断：
   - conversation idle: ConversationIntent::StartChat + Effect::SpawnAgentChat
   - conversation running: ConversationIntent::QueueSubmission
   - diagnostic prompt active: DiagnosticIntent::AnswerPrompt
```

### Agent Event Flow

```text
SDK ChatEvent
→ Msg::Agent(...)
→ AgentEventMapper
→ ConversationIntent / RuntimeIntent / DiagnosticIntent
→ Model Context
→ Change
→ ViewModelDirty
```

例：tool result。

```text
SdkChatEvent::ToolResult
→ Msg::Agent(AgentMsg::ChatEventReceived)
→ AgentEventMapper
→ ConversationIntent::ObserveToolResult
→ ConversationChange::ToolCallCompleted
→ ViewModelDirty::Output + ViewModelDirty::Status
```

### View Feedback Flow

```text
Scroll / Resize / Collapse / CursorBlink / SpinnerTick
→ Msg::View(...)
→ ViewState reducer
→ ViewModelDirty if needed
→ Render
```

View Feedback 默认只改 ViewState，不改 Model。只有当用户显式触发业务动作时，才转换为 Intent。

## Intent / Change / Effect

### Intent

Intent 表示发给某个 Model Context 的处理意图。

示例：

```text
ConversationIntent::StartChat
ConversationIntent::ObserveAssistantText
ConversationIntent::ObserveToolCallStart
ConversationIntent::ObserveToolArguments
ConversationIntent::ObserveToolCall
ConversationIntent::ObserveToolResult
ConversationIntent::CompleteChat
ConversationIntent::QueueSubmission
```

```text
InputIntent::InsertText
InputIntent::MoveCursor
InputIntent::Submit
InputIntent::AcceptCompletion
InputIntent::Clear
```

```text
RuntimeIntent::UpdateWorkspace
RuntimeIntent::RefreshTaskStatus
RuntimeIntent::RecordUsage
RuntimeIntent::StartProcessingJob
RuntimeIntent::FinishProcessingJob
```

```text
DiagnosticIntent::RecordNotice
DiagnosticIntent::OpenPrompt
DiagnosticIntent::AnswerPrompt
DiagnosticIntent::DismissNotice
```

### Change

Change 表示 context 处理 Intent 后产生的状态变化事实。

示例：

```text
ConversationChange::ChatStarted
ConversationChange::ChatTurnStarted
ConversationChange::AssistantTextAppended
ConversationChange::ToolCallObserved
ConversationChange::ToolCallBound
ConversationChange::ToolCallCompleted
ConversationChange::ChatCompleting
ConversationChange::ChatCompleted
ConversationChange::SubmissionQueued
```

```text
InputChange::TextChanged
InputChange::CursorMoved
InputChange::Submitted
InputChange::Cleared
```

```text
RuntimeChange::WorkspaceChanged
RuntimeChange::TaskStatusChanged
RuntimeChange::ProcessingStarted
RuntimeChange::ProcessingFinished
```

```text
DiagnosticChange::NoticeRecorded
DiagnosticChange::PromptOpened
DiagnosticChange::PromptAnswered
DiagnosticChange::NoticeDismissed
```

### Effect

Effect 表示 update 后交给 runtime 执行的副作用。

示例：

```text
Effect::SpawnAgentChat
Effect::SaveSession
Effect::FetchTaskStatus
Effect::CopyToClipboard
Effect::RequestRender
Effect::StartTimer
Effect::StopTimer
Effect::RunHook
```

规则：

- Model Context 不直接执行 Effect。
- Context 可以通过 Change 表达事实。
- Coordinator 根据 Change 生成 Effect。
- Effect 执行结果重新进入 `Msg`。

## Conversation 模型

Conversation 是核心 Model Context。它的结构如下：

```text
Conversation
├── chats: Vec<Chat>
├── active_chat_id
├── messages_snapshot
└── sync_version
```

### Chat

`Chat` 表示一次用户提交触发的完整 Agent 响应。

```text
Chat
├── id
├── user_submission
├── status
├── turns: Vec<ChatTurn>
├── final_response
├── started_at
├── completed_at
└── failure
```

`ChatStatus`：

```text
Created / Running / Completing / Completed / Failed / Cancelled
```

`Done` 不直接等于 `Completed`。收到 done 后 Chat 先进入 `Completing`，完成 final sync/save/drain 后才进入 `Completed`。

### ChatTurn

`ChatTurn` 表示 Chat 内部一次 model response / tool execution cycle。

```text
ChatTurn
├── id
├── sequence
├── status
├── assistant_stream
├── tool_calls
├── started_at
└── completed_at
```

`ChatTurnStatus`：

```text
Streaming / ToolCalling / ToolExecuting / Completing / Completed / Failed
```

典型流程：

```text
Streaming → ToolCalling → ToolExecuting → Completing → Completed
```

无 tool 的最终回答流程：

```text
Streaming → Completing → Completed
```

### ToolCall

`ToolCall` 是 `ChatTurn` 内部 entity，是 tool 标题状态的唯一业务来源。

```text
ToolCall
├── id: Option<ToolCallId>
├── stream_key: ToolStreamKey
├── name
├── args_preview
├── summary
├── status
├── result
├── activities
└── timing
```

`ToolStreamKey`：

```text
{ chat_id, chat_turn_id, name, index }
```

`ToolCallStatus`：

```text
PendingArgs / Ready / Running / Success / Error / Cancelled / Orphaned
```

状态流转：

```text
PendingArgs → Ready → Running → Success
PendingArgs → Ready → Running → Error
Running → Cancelled
PendingArgs/Ready/Running → Orphaned
```

匹配规则：

1. `ToolCallStart(name, index)` 创建 `PendingArgs`。
2. `ToolArgumentsDelta(name, index)` 通过 `ToolStreamKey` 更新 args preview。
3. `ToolCall(id, name, summary, index)` 通过 `ToolStreamKey` 绑定真实 id，并进入 `Ready/Running`。
4. `ToolResult(id)` 通过真实 `tool_call_id` 标记 `Success/Error`。
5. 找不到匹配的 `ToolResult` 时进入 Diagnostic，不再 fallback 到最后一个 running output line。
6. 如果完整 `ToolCall` 先于 `ToolCallStart`，允许直接创建 `Ready/Running`。

## ViewAssembler / ViewModel / Render

### ViewAssembler

ViewAssembler 从 Model + ViewState 组装 ViewModel：

```text
OutputViewAssembler → OutputViewModel
StatusViewAssembler → StatusLineViewModel
InputViewAssembler → InputAreaViewModel
DialogViewAssembler → DialogViewModel
```

### OutputViewModel

Output 以 block 为核心，不以 line 为业务核心。

```text
OutputViewModel
├── blocks: Vec<OutputBlockView>
├── version
└── follow_tail_hint
```

`OutputBlockView`：

```text
UserMessage
AssistantMessage
ToolCall
ToolResult
ToolActivity
DiagnosticNotice
SystemNotice
Separator
```

`ToolCallBlockView`：

```text
ToolCallBlockView {
  key,
  chat_id,
  turn_id,
  tool_call_id,
  title,
  icon,
  semantic_status,
  args_preview,
  summary,
  activity_summary,
  result_summary,
  collapsible,
  collapsed,
}
```

`semantic_status`：

```text
Pending / Running / Success / Error / Cancelled / Orphaned
```

图标由 ViewAssembler 根据 status 生成：

```text
Pending/Running -> ●
Success -> ✓
Error -> ✗
Cancelled -> –
Orphaned -> ?
```

Render 不再覆盖这些字符。

### StatusLineViewModel

Status line 是多个 Model Context 的摘要，不是业务状态仓库。

```text
StatusLineViewModel {
  left: Vec<StatusSegment>,
  center: Vec<StatusSegment>,
  right: Vec<StatusSegment>,
  severity,
}
```

Status line 从 Conversation/Input/Runtime/Diagnostic 汇总：

- active chat / running tools / queued submissions
- input mode / selection / completion
- provider / model_id / cwd / worktree / task status / usage
- warning / error / active prompt

### InputAreaViewModel

InputModel 保存编辑真相，InputAreaViewModel 描述显示内容。

```text
InputAreaViewModel {
  text,
  cursor,
  selection_ranges,
  placeholder,
  mode_label,
  queued_hint,
  completion_popup,
  attachment_chips,
  disabled_reason,
}
```

### DialogViewModel

Diagnostic/permission/hook stop 通过 DialogViewModel 统一展示。

```text
DialogViewModel {
  kind,
  title,
  body,
  actions,
  default_action,
  severity,
}
```

### ViewState

ViewState 保存纯显示交互状态：

```text
ViewState {
  output_scroll_offset,
  output_follow_tail,
  collapsed_blocks,
  selected_text_range,
  terminal_size,
  spinner_frame,
  cursor_blink,
  render_cache,
}
```

ViewState 可以影响 ViewAssembler 的显示结果，例如 collapsed blocks；但不能改变 Model 状态。

### Render cache

Render cache 只属于 render 层，cache key 必须至少包含：

```text
ViewModel.version
terminal_width
theme_version
view_state_version
```

禁止只按 line content 或 block title 猜测缓存有效性。

## 目标模块结构

目标目录：

```text
apps/cli/src/tui/
  app/
    mod.rs
    state.rs
    run_loop.rs

  update/
    mod.rs
    msg.rs
    coordinator.rs
    agent_mapper.rs
    input_mapper.rs
    change_router.rs

  model/
    mod.rs
    conversation/
      mod.rs
      model.rs
      intent.rs
      change.rs
      chat.rs
      chat_turn.rs
      tool_call.rs
      message_snapshot.rs
      ids.rs
    input/
      mod.rs
      model.rs
      intent.rs
      change.rs
      document.rs
      history.rs
      completion.rs
      attachment.rs
    runtime/
      mod.rs
      model.rs
      intent.rs
      change.rs
      processing_job.rs
      workspace.rs
      usage.rs
      task_status.rs
    diagnostic/
      mod.rs
      model.rs
      intent.rs
      change.rs
      notice.rs
      prompt.rs

  view_assembler/
    mod.rs
    output.rs
    status.rs
    input.rs
    dialog.rs

  view_model/
    mod.rs
    output.rs
    status.rs
    input.rs
    dialog.rs
    style.rs

  view_state/
    mod.rs
    output.rs
    input.rs
    layout.rs
    animation.rs
    cache.rs

  render/
    mod.rs
    layout.rs
    output/
      mod.rs
      block.rs
      markdown.rs
      cache.rs
      line.rs
    status.rs
    input.rs
    dialog.rs
    theme.rs

  effect/
    mod.rs
    effect.rs
    executor.rs
    session.rs       # session 生命周期副作用：spawn agent chat / save / load / resume
    completion.rs    # 补全候选 IO 生成（扫描文件系统等），结果回灌 InputModel

  adapter/
    mod.rs
    agent_event.rs
    terminal_event.rs
    task_event.rs
    hook_event.rs
```

## completion 与 session 的归属

`completion/` 与 `session/` 不是独立 layer，不应作为顶层目录长期存在。它们按职责拆入已有的层：

### session → Effect 执行 + Runtime 状态

session 相关代码（`spawn_processing`、`session_lifecycle.run`、`resume`）全是副作用编排：`tokio::spawn` 启动 chat 处理循环、`agent_client.load_session().await`、save/restore tasks。

- **执行**归 Effect 层：对应 `Effect::SpawnAgentChat` / `Effect::SaveSession` / load/resume，由 `effect/executor.rs`（或 `effect/session.rs`）执行。
- **状态**归 Runtime Model：processing job、session metadata 等存于 `model/runtime/`（`processing_job.rs` 等）。
- 会话内的对话语义（resume 后的 messages）经 anti-corruption mapper 进入 Conversation Model，不在 session 副作用模块里维护业务真相。

### completion → Input Model（纯）+ Effect（IO 候选）

补全本就是 Input Model 的一部分（Input 维护 buffer、cursor、selection、history、**completion**、attachment）。candidate 生成需拆开：

- **纯逻辑/状态**归 Input Model：补全上下文解析（`extract_completion_token` 等 parser）与补全弹窗状态（active/selected）存于 `model/input/`（`completion.rs`）。
- **IO 候选生成**归 Effect：扫描文件系统（`std::fs::read_dir`）、枚举命令/模型/历史会话等，做成 `Effect::FetchCompletions`（类比 `Effect::FetchTaskStatus`），结果以 `Msg` 回灌 InputModel 的补全状态。
- Model 不直接做 IO（见“分层职责”第 4 条），因此 `files.rs` 这类扫盘逻辑必须经 Effect，而非留在 Input Model 内。

> 迁移由 feature #57 收口：删除/并入顶层 `completion/`、`session/`，并由顶层目录白名单 guard 锁定结构。

## 迁移里程碑

### Milestone 1：建立 ViewModel 边界

目标：让 output/status/input render 逐步消费 ViewModel，而不是直接消费散乱 AppState。

范围：

- 新增最小 `view_model/`、`view_assembler/`、`view_state/`。
- 先通过 adapter 包装现有状态。
- 不改变 Agent 协议。
- 不改变 tool matching。

验收：

- Output/Status/Input render 有明确 ViewModel 输入。
- `ViewModelDirty` 能触发重建。
- 现有行为不变。

### Milestone 2：引入 ConversationModel，迁移 ToolCall 状态

目标：`ToolCall.status` 成为 tool 标题颜色唯一来源。

范围：

- 新增 `model/conversation/`。
- 实现 `ConversationModel`、`Chat`、`ChatTurn`、`ToolCall`、`ConversationIntent`、`ConversationChange`。
- 接入 `ToolCallStart`、`ToolArgumentsDelta`、`ToolCall`、`ToolResult`、`AssistantTextDelta`、`Done`。
- OutputViewAssembler 从 ConversationModel 生成 tool block。
- 删除或旁路 `mark_tool_header_done()`、最后 running fallback、渲染阶段 dot 覆盖。

验收：

- tool 完成后标题颜色由 `ToolCall.status` 稳定推导。
- `ToolResult` 找不到匹配时进入 Diagnostic。
- 不再通过 output line 修改完成态。

### Milestone 3：引入 InputModel

目标：输入编辑、提交、排队、prompt answer 语义统一。

范围：

- 新增 `model/input/`。
- key event 先转为 `InputIntent`。
- submit 输出 `InputSubmission`。
- Coordinator 决定 start chat / queue / prompt answer。

验收：

- input area 不直接掺杂 Agent 状态。
- queued input 语义由 Conversation/Input 协调表达。

### Milestone 4：引入 RuntimeModel + DiagnosticModel

目标：status line、error、warning、prompt、hook notice 有统一来源。

范围：

- 新增 `model/runtime/`、`model/diagnostic/`。
- status line 只从 `StatusViewAssembler` 读取。
- permission/hook/error/orphan/late event 进入 DiagnosticModel。

验收：

- status line 不再是状态仓库。
- orphan/late event 可在 output/status/dialog 中一致显示。

### Milestone 5：Effect 收敛

目标：副作用执行集中到 EffectExecutor。

范围：

- 将现有 `Cmd` 语义迁移为 `Effect`。
- update/coordinator 返回 `Vec<Effect>`。
- Effect executor 执行 SpawnAgentChat、SaveSession、FetchTaskStatus、CopyToClipboard、RunHook、RequestRender。
- effect 结果重新进入 `Msg`。

验收：

- update 不直接执行副作用。
- EffectExecutor 成为副作用统一出口。

## 架构门禁

TUI 架构重设计必须由 stop hook 保护，防止后续改动绕过边界。项目根目录 `.agents/aemeath.json` 已配置 `Stop` hook：

```json
{
  "hooks": {
    "Stop": [
      {
        "matcher": "",
        "command": "\"{AEMEATH_PROJECT_DIR}/.agents/hooks/check-architecture-guards.sh\"",
        "timeout": 120
      }
    ]
  }
}
```

其中 `check-architecture-guards.sh` 聚合执行架构检查，包括当前已有的：

```text
check-cargo-dependency-graph.sh
check-cli-thin-entry.sh
check-share-no-upstream-deps.sh
check-cola-layer-purity.sh
check-forbidden-imports.sh
check-rust-file-lines.sh
check-tui-tea-purity.sh
check-unsafe-text-ops.sh
```

### 当前已存在的 TUI TEA 门禁

`check-tui-tea-purity.sh` 当前检查 `apps/cli/src/tui/core` 中非豁免文件，禁止在 update/state 等纯逻辑模块中出现：

- `tokio::spawn(`
- `std::thread::spawn(`
- `Command::new(`
- hook runner 调用
- clipboard/image 处理
- `Handle::block_on(` / `Runtime::block_on(`
- `block_in_place`
- `.await`

该检查应继续保留，并随目录迁移扩展到新的 `apps/cli/src/tui/update` 与 `apps/cli/src/tui/model`。

### 新增架构门禁建议

随着本设计落地，应逐步新增以下 guard，并纳入 `check-architecture-guards.sh`：

1. **Model purity guard**
   - 目标目录：`apps/cli/src/tui/model`。
   - 禁止依赖 `ratatui`、terminal backend、clipboard、filesystem IO、tokio spawn、AgentClient、channel send。
   - 允许纯数据类型、时间戳 value object、错误类型和状态转换。

2. **Render isolation guard**
   - 目标目录：`apps/cli/src/tui/render`。
   - 禁止引用 `ConversationIntent`、`ConversationChange`、`ToolCallStatus` 的可变模型实现细节。
   - Render 只能消费 `view_model` 和 `view_state` 类型。
   - 禁止出现类似 `mark_tool_header_done`、`find_last_running_tool`、`ObserveToolResult` 的状态变更逻辑。

3. **ViewAssembler boundary guard**
   - 目标目录：`apps/cli/src/tui/view_assembler`。
   - 允许读取 `model`、`view_state`、输出 `view_model`。
   - 禁止执行 IO、副作用、spawn、channel send。
   - 禁止直接依赖 ratatui。

4. **ViewModel dependency guard**
   - 目标目录：`apps/cli/src/tui/view_model`。
   - 禁止依赖 `model` 的可变内部类型。
   - 禁止依赖 ratatui。
   - 只允许语义样式，例如 `SemanticStyle`，ratatui style 映射留在 render/theme。

5. **Agent event adapter guard**
   - Agent/SDK event 类型只能在 `adapter/` 或 `update/agent_mapper.rs` 出现。
   - `model/conversation` 不直接依赖 SDK `ChatEvent` 或 runtime `RuntimeStreamEvent`。
   - 进入 ConversationModel 的必须是 `ConversationIntent`。

6. **Output line legacy guard**
   - 在 Milestone 2 后启用。
   - 禁止新增通过 output line 直接表达 tool 完成状态的 API。
   - 禁止 fallback 到“最后一个 running header”。
   - 禁止 render 阶段覆盖 tool status icon。

7. **Effect boundary guard**
   - 在 Milestone 5 后启用。
   - 禁止 `update/` 和 `model/` 直接执行副作用。
   - 副作用只能通过 `Effect` 返回，并由 `effect/executor.rs` 执行。

### Stop hook 执行策略

1. 每次停止会话前，`.agents/aemeath.json` 的 Stop hook 必须执行 `check-architecture-guards.sh`。
2. `check-architecture-guards.sh` 作为聚合入口，新增 guard 应追加到该脚本中，而不是分散配置到多个 stop hook。
3. 架构 guard 应尽量使用静态 grep/perl 检查，失败信息必须指向具体文件和规则。
4. 对历史遗留代码允许设置明确豁免列表，但豁免必须集中、带注释、可逐步删除。
5. 每完成一个 milestone，应同步收紧对应 guard，避免架构倒退。

### 门禁与迁移节奏

- Milestone 1 完成后：启用 ViewModel/ViewAssembler 基础依赖方向检查。
- Milestone 2 完成后：启用 output line legacy guard 和 render icon overwrite guard。
- Milestone 3 完成后：启用 InputModel purity guard。
- Milestone 4 完成后：启用 Runtime/Diagnostic 到 StatusViewAssembler 的边界检查。
- Milestone 5 完成后：启用 Effect boundary guard。

架构门禁不是一次性全部打开，而是跟随迁移里程碑逐步加严。这样既能保护目标架构，也不会因为历史代码尚未迁移而阻塞正常开发。

## 收敛终态：结构性单一真相 → 守卫瘦身

> 本节是对上文「架构门禁」的**收敛目标补充**。上文列的是**边界**守卫（Model purity / Render isolation / ViewAssembler / ViewModel / Effect boundary 等），这些是本质、保留。本节针对的是另一类——**single-source-of-truth 守卫**（#55–#59 系列），它们偏多，根因是"单一真相"被做成了**逐块禁止**而非**结构上不可能**。收敛终态应让这批守卫**有资格退役/合并**。

### 为什么这类守卫多

`.agents/hooks/` 现存约 5 个 single-source 守卫——`check-tui-input-single-source`、`check-tui-status-single-source`、`check-tui-spinner-task-single-source`、`check-tui-output-scroll-selection-single-source`、`check-tui-selection-single-source`。每个**钉死一块状态**，禁止它在 **Model** 与 **ratatui 有状态 widget**（InputArea / OutputArea / StatusBar 自带的内部 buffer）之间**重复**。

根因：TEA 要求"状态只在 Model"，但 ratatui 的 `StatefulWidget` 允许 widget 自存状态 → 同一块状态出现**两份真相**（如 `model.input.document` vs InputArea 内部缓冲）。**守卫数 ≈ 状态能泄漏的地方数**；逐块禁止，所以多。这是迁移过程中**逐块钉**的脚手架，不是稳定态的本质需要。

### 两条结构事实（让重复不可能，而非逐块禁止）

1. **domain 态不是 TUI 的真相，是 AgentClient 的**——conversation / cost / tasks / project 由 `AgentClient` 的只读快照（`session_snapshot()` / `cost()` / `task_list()` / `project()`）提供，经其**变更通道 `changes()`** 刷新；TUI **只读投影、NEVER 在 TUI 内再造一份**。→ status / spinner-task 这几个守卫的源头消失（TUI 根本不持 domain 态）。
2. **UI 局部态只在 Model**（`model/input`、`model/runtime` 等），**widget 是纯投影**——不使用 `StatefulWidget` 承载 app 态，渲染只读 `ViewModel` / `ViewState`。→ input / scroll / selection 这几个守卫的源头消失（不存在第二份）。

### 守卫合并（终态）

| 现状（逐块禁止） | 收敛后 |
|---|---|
| input / status / spinner-task / scroll-selection / selection 五个 single-source | **合并为 1 条结构规则**：「app 态只在 `model/`；domain 态只读投影自 `AgentClient`；`render/` 与 widget 不得持有或写入任何 app/domain 态」 |
| Model purity / Render isolation / ViewAssembler boundary / ViewModel dependency / Effect boundary / TEA purity | **保留**（本质，与状态归属正交） |
| unsafe-text-ops | 保留（Unicode 安全，与架构无关） |

### 退役判据

当某块状态满足以下结构条件，其对应 single-source 守卫即可从 `check-architecture-guards.sh` 退役（或并入上面那条结构规则）：

1. 该块状态**唯一定义在 `model/`**（domain 态则唯一来源是 `AgentClient` 投影），全仓**无第二处可写定义**。
2. 对应 widget 改为**无状态 / 纯投影**，渲染只读 `ViewModel` / `ViewState`。
3. 写入该块状态的路径**唯一**（单一 assembler / reducer），且已被现有边界守卫（ViewAssembler boundary / Model purity）覆盖。

### 与 AgentClient 的关系（双模式通用）

TUI 是**入站 adapter**，只依赖 `dyn AgentClient`（其只读快照 + 变更通道）。domain 真相在 runtime / AgentClient，TUI 投影——**结构上不可能有第二份**。这套收敛对**本地直连**与**远程 server 模式**同样成立（TUI 不区分 `AgentClientImpl` 与远程客户端实现）。

### 节奏：终点是更少、更结构化的守卫

延续本文"门禁随迁移里程碑加严"的原则，但**终点不是更多守卫，而是更少**：每完成一块状态的**结构性收敛**（状态唯一归位 + widget 纯投影）→ **退役 / 合并**它的 single-source 守卫，而不是新增。判断"该不该单一真相"是设计决定；守卫只锁"已决定之后的结构后果"。
