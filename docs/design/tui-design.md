# TUI 设计

## 定位

TUI 是**入站适配器**——负责把用户终端输入转换为核心域调用，把核心域事件转换为屏幕渲染。它不承载业务逻辑，不定义领域模型，不决定会话状态。

## 六边形边界

TUI 通过 `AgentClient` trait（`packages/sdk`）与 Runtime 通信——这是核心域暴露的入站端口：

```
  Terminal Input                   Terminal Output
       │                                ▲
       ▼                                │
  ┌─────────────┐               ┌──────┴──────┐
  │  TUI        │               │  TUI        │
  │  Input      │               │  Render     │
  │  Adapter    │               │  Adapter    │
  └──────┬──────┘               └──────▲──────┘
         │ Msg                         │ ViewModel
         ▼                             │
  ┌─────────────┐               ┌──────┴──────┐
  │  Model      │──ViewAssembler──▶ ViewModel  │
  │  (业务真相)  │               │  (显示状态)  │
  └──────┬──────┘               └─────────────┘
         │ AgentClient trait
         ▼
  ┌─────────────┐
  │  Runtime    │
  │  (核心域)    │
  └─────────────┘
```

**六边形合规**：TUI 不直接调用 Runtime 内部类型，只依赖 `packages/sdk` 的 `AgentClient` trait 和 DTO。

## 分层数据流

```
Terminal Event → Msg → Coordinator / update → Model → ViewAssembler → ViewModel → Render → Effect
```

| 层 | 职责 | 六边形角色 |
|---|---|---|
| Msg | 统一入口，包装 terminal / Agent / timer / hook 输入 | 入站适配器转换 |
| Model | 按业务能力拆分的 Context，保存业务真相和状态转换规则 | 领域投影 |
| Intent | Coordinator 发给 Model Context 的处理意图 | 应用命令 |
| Change | Model Context 处理 Intent 后产生的状态变化事实 | 领域事件 |
| ViewAssembler | 从 Model + ViewState 组装 ViewModel | 投影组装 |
| ViewState | 纯显示交互状态（scroll / collapse / selection / animation） | 视图状态 |
| Render | 把 ViewModel + ViewState 画到 ratatui | 出站渲染 |
| Effect | update 后需要 Runtime 执行的副作用描述 | 出站副作用 |

### 三条信息流主线

**用户意图流**：
```
Terminal Key/Mouse/Paste
→ Msg::Input(...)
→ InputIntent
→ InputModel
→ InputChange
→ Coordinator
→ Conversation/Runtime/Diagnostic Intent
→ Effect
```

**Agent 事件流**：
```
SDK ChatEvent
→ Msg::Agent(...)
→ AgentEventMapper（anti-corruption）
→ ConversationIntent / RuntimeIntent / DiagnosticIntent
→ Model Context
→ Change
→ ViewModelDirty
```

**视图反馈流**：
```
Scroll / Resize / Collapse / CursorBlink / SpinnerTick
→ Msg::View(...)
→ ViewState reducer
→ ViewModelDirty if needed
→ Render
```

视图反馈默认只改 ViewState，不改 Model。只有当用户显式触发业务动作时，才转换为 Intent。

## Model Context

Model 按业务能力拆分为四个 Context，对应核心域的不同投影。Model 保存业务真相、执行状态转换规则，**不依赖 ratatui、不执行 IO、不调用 AgentClient、不发 channel**。

### Conversation Model

负责"用户与 Agent 的交互会话"。维护对话结构、chat 生命周期、turn 生命周期、tool call 状态和 assistant stream。

```
Conversation
├── chats: Vec<Chat>
├── active_chat_id
├── messages_snapshot
└── sync_version
```

**Chat**：一次用户提交触发的完整 Agent 响应。
- `ChatStatus`：Created / Running / Completing / Completed / Failed / Cancelled
- 收到 done 后先 `Completing`，完成 final sync/save/drain 后才 `Completed`

**ChatTurn**：Chat 内部一次 model response / tool execution cycle。
- `ChatTurnStatus`：Streaming / ToolCalling / ToolExecuting / Completing / Completed / Failed

**ToolCall**：tool 标题状态的**唯一业务来源**。
- `ToolCallStatus`：PendingArgs / Ready / Running / Success / Error / Cancelled / Orphaned
- 匹配规则：`ToolCallStart(name, index)` 创建 `PendingArgs` → `ToolArgumentsDelta` 更新 args → `ToolCall(id)` 绑定真实 id → `ToolResult(id)` 标记完成
- 找不到匹配的 `ToolResult` 时进入 Diagnostic，不再 fallback

Runtime-origin event 必须经过 anti-corruption mapper，转换为 Conversation 语言后才能进入 Conversation Model。

### Input Model

负责"用户正在编辑什么，以及用户想表达什么意图"。维护 buffer、cursor、selection、history、completion、attachment 和 submit 输出。

- 只产出 `InputSubmission` 或输入状态变化
- 不直接启动 Agent，不直接改 output，也不决定是否排队
- 补全上下文解析与弹窗状态归 Input Model；IO 候选生成归 Effect

### Runtime Model

负责外部运行环境和副作用执行状态。维护 provider/model id、cwd/worktree、session metadata、processing job、task status、usage/cost 等。

- 可以保存 Agent/SDK 事件的运行元数据
- 不保存 Conversation 内部 tool 状态真相

### Diagnostic Model

负责错误、警告、提示和阻塞请求。统一承接 warning、error、hook blocked、permission prompt、orphan event、late event、debug diagnostic。

- 让 output/status/dialog 不再各自维护错误状态

## Intent / Change / Effect

### Intent（发给 Model Context 的处理意图）

```
ConversationIntent::StartChat / ObserveAssistantText / ObserveToolCallStart /
    ObserveToolArguments / ObserveToolCall / ObserveToolResult / CompleteChat / QueueSubmission

InputIntent::InsertText / MoveCursor / Submit / AcceptCompletion / Clear

RuntimeIntent::UpdateWorkspace / RefreshTaskStatus / RecordUsage /
    StartProcessingJob / FinishProcessingJob

DiagnosticIntent::RecordNotice / OpenPrompt / AnswerPrompt / DismissNotice
```

### Change（Model Context 处理 Intent 后产生的状态变化事实）

```
ConversationChange::ChatStarted / ChatTurnStarted / AssistantTextAppended /
    ToolCallObserved / ToolCallBound / ToolCallCompleted / ChatCompleting / ChatCompleted

InputChange::TextChanged / CursorMoved / Submitted / Cleared

RuntimeChange::WorkspaceChanged / TaskStatusChanged / ProcessingStarted / ProcessingFinished

DiagnosticChange::NoticeRecorded / PromptOpened / PromptAnswered / NoticeDismissed
```

### Effect（update 后交给 runtime 执行的副作用）

```
Effect::SpawnAgentChat / SaveSession / FetchTaskStatus /
    CopyToClipboard / RequestRender / StartTimer / StopTimer / RunHook
```

规则：
- Model Context 不直接执行 Effect
- Context 通过 Change 表达事实，Coordinator 根据 Change 生成 Effect
- Effect 执行结果重新进入 `Msg`

## ViewAssembler / ViewModel / ViewState

### ViewAssembler

从 Model + ViewState 组装 ViewModel，可决定显示文本、semantic status、segment，**不修改 Model，不执行 IO，不写 ratatui buffer**。

```
OutputViewAssembler → OutputViewModel
StatusViewAssembler → StatusLineViewModel
InputViewAssembler  → InputAreaViewModel
DialogViewAssembler → DialogViewModel
```

### OutputViewModel

以 block 为核心，不以 line 为业务核心：

```
OutputBlockView: UserMessage | AssistantMessage | ToolCall | ToolResult |
    ToolActivity | DiagnosticNotice | SystemNotice | Separator
```

`ToolCallBlockView` 包含：key、chat_id、turn_id、tool_call_id、title、icon、semantic_status、args_preview、summary、activity_summary、result_summary、collapsible、collapsed。

`semantic_status`：Pending / Running / Success / Error / Cancelled / Orphaned。

图标由 ViewAssembler 根据 status 生成（Pending/Running → ●、Success → ✓、Error → ✗、Cancelled → –、Orphaned → ?），Render 不再覆盖。

### StatusLineViewModel

多个 Model Context 的摘要，不是业务状态仓库。从 Conversation / Input / Runtime / Diagnostic 汇总：active chat / running tools、input mode / selection、provider / model / cwd / task status、warning / error / active prompt。

### InputAreaViewModel

InputModel 保存编辑真相，InputAreaViewModel 描述显示内容：text、cursor、selection_ranges、placeholder、mode_label、completion_popup、attachment_chips、disabled_reason。

### ViewState

纯显示交互状态：output_scroll_offset、output_follow_tail、collapsed_blocks、selected_text_range、terminal_size、spinner_frame、cursor_blink、render_cache。

ViewState 可以影响 ViewAssembler 的显示结果（如 collapsed blocks），但不能改变 Model 状态。

Render cache key 必须至少包含：`ViewModel.version` + `terminal_width` + `theme_version` + `view_state_version`。

## SDK DTO 边界

TUI 与 Runtime 的类型边界彻底消解——这是六边形架构的直接要求：

- `apps/cli/src/tui/**` **MUST NOT** 出现 `runtime::api` 或 `::runtime` 类型依赖
- `sdk::ChatEvent` 使用强类型 DTO：`ToolResultImage`、`AgentProgressEventView`、`WorkspaceContextView` 等
- TUI 内部事件和渲染状态只使用 SDK DTO 或 TUI 私有 view model
- Runtime 类型与 SDK DTO 的转换集中在 `agent/runtime` 的 `AgentClientImpl` 及 composition root

### SDK DTO 类型

| DTO | 字段 |
|---|---|
| `ToolResultImage` | `base64`, `media_type` |
| `AgentProgressEventView` | `sequence`, `kind: AgentProgressKindView` |
| `WorkspaceContextView` | `path_base`, `working_root`, `context_stack` |
| `ReflectionOutputView` | `content`, `input_tokens`, `output_tokens` |
| `SkillView` | `name`, `description`, `source` |

## 收敛终态：单一真相守卫

TUI 的收敛目标是让状态**结构性唯一**，而非逐块禁止：

1. **domain 态不是 TUI 的真相，是 AgentClient 的**——conversation / cost / tasks / project 由 `AgentClient` 的只读快照提供，经变更通道刷新；TUI 只读投影
2. **UI 局部态只在 Model**——widget 是纯投影，不使用 `StatefulWidget` 承载 app 态

收敛后守卫合并为 1 条结构规则：「app 态只在 `model/`；domain 态只读投影自 `AgentClient`；`render/` 与 widget 不得持有或写入任何 app/domain 态」。

## 架构门禁

| 门禁 | 保护目标 |
|---|---|
| Model purity guard | `model/` 禁止依赖 ratatui、IO、tokio spawn、AgentClient、channel |
| Render isolation guard | `render/` 只能消费 `view_model` 和 `view_state`，禁止状态变更逻辑 |
| ViewAssembler boundary guard | 允许读 model/view_state、输出 view_model，禁止 IO/副作用/ratatui |
| ViewModel dependency guard | 禁止依赖 model 的可变内部类型和 ratatui |
| Agent event adapter guard | SDK/runtime event 类型只能在 `adapter/` 或 `update/agent_mapper.rs` |
| TEA purity guard | update/state 等纯逻辑模块禁止 tokio::spawn、Command::new、.await |

## 参考文档

- [TUI Model/View 架构](superpowers/specs/2026-05-27-tui-model-view-architecture.md)
- [TUI SDK DTO 边界](snapshot/specs/047-tui-sdk-dto-boundary-design.md)
