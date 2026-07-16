# TUI · 事件流与 ACL 设计

> 层级：02-modules / tui（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#943 / #944 / #947 / [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 TUI 事件流的唯一链路、AgentEventMapper 防腐层（ACL）、SDK DTO 边界、四类 Interaction reply 资源协议、agent_id 与 sub-agent 事件路由（#612）、转换集中化策略与架构门禁。

> **解耦铁律**（[01-system/05-dependency-rules.md](../../01-system/05-dependency-rules.md)）：
> - **R4**：TUI 只经 `AgentClient`，**NEVER** import 核心内部类型
> - **R7**：领域事件与 TUI Model **NEVER** 跨界直用，**MUST** 经防腐层转换
> - **§5**：Server 化时传输层 NEVER 进核心，`AgentClient` 保持传输透明
>
> UiEvent **NEVER** 持有 `sdk::*` 类型，SDK 类型在第一层 `sdk_event_to_ui_event` 中**彻底**转换为 TUI 自有类型。

## 1. 定位

事件流是 TUI 三条信息流之一（#795 §4.2），承载 Runtime → TUI 的单向数据流：

```
Runtime ChatStream → tokio::spawn task → sdk::ChatEvent
  → sdk_event_to_ui_event（adapter/event_mapping.rs，第一层转换）
  → UiEvent → mpsc channel (cap 256)
  → ui_rx → tokio::select! → TuiMsg::Ui(ui_event)
  → map_agent_event（adapter/agent_event.rs，第二层 ACL）
  → AgentEventMapping { intents }
  → root_reducer → Model Change
  → Coordinator::effects_for(Change) → async Effect runner → result Intent
  → ViewModelDirty → ViewAssembler → Render
```

**两层转换的职责边界**：

| 层 | 位置 | 职责 | 输入 → 输出 |
|---|---|---|---|
| 第一层 | `event_mapping.rs` | **结构转换**——SDK 类型 → TUI 类型，消除 SDK 类型依赖 | `sdk::ChatEvent` → `UiEvent` |
| 第二层 | `agent_event.rs` | **语义翻译**——事件 → Intent 拆分，防腐层核心 | `UiEvent` → `AgentEventMapping` |

> **设计原则**：两层分离是因为结构转换（类型映射）和语义翻译（Intent 拆分）是不同关注点。第一层是机械式 1:1 映射，第二层涉及业务逻辑（sanitize、progress 格式化、hook notice 派生等）。

## 2. 事件流完整链路

### 2.1 链路图

```
┌─ Runtime / SDK Published Language ────────────────────────────┐
│  AgentClient::chat() → ChatStream<Item = sdk::ChatEvent>      │
└──────────┬────────────────────────────────────────────────────┘
           │ sdk::ChatEvent
┌──────────────────────────────────────────────────────────────┐
│  tokio::spawn task（effect/session/processing.rs）            │
│  持 Arc<dyn AgentClient> → chat() → ChatStream                │
│  每条 event → sdk_event_to_ui_event（event_mapping.rs）       │
│    → UiEvent → mpsc::channel(cap 256)                         │
└──────────┬───────────────────────────────────────────────────┘
           │  UiEvent（TUI 内部事件类型）
           ▼
┌──────────────────────────────────────────────────────────────┐
│  App::update（主线程 / TEA Update）                           │
│  tokio::select! → TuiMsg::Ui(ui_event)                        │
│  → update_agent_event()                                       │
│  → map_agent_event（adapter/agent_event.rs）                  │
│    → AgentEventMapping { intents: Vec<AgentIntent> }          │
│  → root_reducer（apply intents → Model Changes）              │
│  → Coordinator::effects_for(changes)                          │
│  → async Effect runner → result Intent                        │
│  → merge_dirty → ViewAssembler → Render                      │
└──────────────────────────────────────────────────────────────┘
```

### 2.2 涉及文件

| 文件 | 职责 |
|---|---|
| Runtime-owned SDK contract | `AgentClient` / `ChatEvent` Published Language 的单一来源 |
| `apps/cli/.../effect/session/processing.rs` | `spawn_processing`：持 `AgentClient` → `ChatStream`；把纯值 SDK event 转换并转发 UiEvent |
| `apps/cli/.../adapter/event_mapping.rs` | `sdk_event_to_ui_event`：第一层结构转换；只产 TUI-owned DTO |
| `apps/cli/.../app/event.rs` | `UiEvent`（`AppEvent`）定义 |
| `apps/cli/.../adapter/agent_event.rs` | `map_agent_event`：第二层 ACL；只产 Intent |
| `apps/cli/.../adapter/agent_event/progress.rs` | sub-agent progress 格式化 |
| `apps/cli/.../adapter/agent_event/sanitize.rs` | tool 输出/参数截断 |
| `apps/cli/.../adapter/hook_notice.rs` | Hook 事件 → TUI notice |

## 3. AgentEventMapper ACL 设计

### 3.1 ACL 职责

`map_agent_event` 是防腐层核心，职责：

1. **Intent 拆分**：一个 UiEvent 可能产生多个 Intent（跨 Context），如 `Error` 同时产生 ConversationIntent + DiagnosticIntent；ACL **NEVER** 直接产生 Effect
2. **sanitize**：tool 输出/参数截断（`sanitize_tool_output` / `sanitize_tool_arguments_delta` / `sanitize_tool_result_content`）
3. **progress 格式化**：sub-agent progress 事件 → 可读字符串（`format_agent_progress`）
4. **hook notice 派生**：Hook 事件 → HookNoticeContent（`hook_event_notice`）
5. **placeholder 清理**：收到实际内容时清 ModelStreamWaiting 占位（`clear_placeholder_then`）
6. **空 payload 守卫**：runtime **MAY** 发送空 payload 事件，ACL **MUST** 在此丢弃，**NEVER** 让空内容进入 Model（见 3.5）

### 3.2 AgentEventMapping 结构

```rust
struct AgentEventMapping {
    intents: Vec<AgentIntent>,
}

enum AgentIntent {
    Conversation(ConversationIntent),
    Input(InputIntent),
    Diagnostic(DiagnosticIntent),
    Session(SessionIntent),
    Config(ConfigIntent),
    Workspace(WorkspaceIntent),
}
```

**设计规则**：

1. **MUST** 一个 UiEvent 映射到一个 `AgentEventMapping`——不允许部分拆分
2. **MUST** 每个 intent 以 `AgentIntent` variant 保留所属 Context，root reducer 只把它分发给对应 Model
3. **MUST** `AgentEventMapping` 是值类型（`#[derive(Debug, Default, PartialEq)]`），便于测试断言
4. **MUST** ACL 函数是纯函数——输入 `&UiEvent`，输出只含 Intent 的 `AgentEventMapping`，**NEVER** 产生 Effect、执行 I/O 或决定副作用
5. **MUST** Effect 只由 Coordinator 从 reducer 返回的 Change 派生；Effect 完成后以新的 TUI-owned Intent 回到同一 reducer

### 3.3 六个 Context 的穷尽映射

`agent_event.rs` **MUST** 对封闭 `UiEvent` 枚举做穷尽 match；禁止 wildcard、默认空 mapping、静默忽略或交给另一条更新路径二次处理。允许一个事件产生多个 Context Intent，但每个 Intent 必须显式出现在下表。

> **唯一例外**：3.5 的空 payload 守卫——空内容事件 **MAY** 返回空 mapping，但 **MUST** 记 `log_debug!` 留痕。事件变体本身仍 **MUST** 显式出现在下表，丢弃的是空 payload 而非事件变体。

| Context | UiEvent 变体 | Intent / 关键规则 |
|---|---|---|
| Conversation | `Text` / `Thinking` / `BlockComplete` / `ToolCallStart` / `ToolCallUpdate` / `ToolResult` / `AgentProgress` / `Done` / `DoneWithDuration` / `Cancelled` / `Usage` / `LiveTps` / `SystemMessage` / `ModelStreamWaiting` / `UserMessagesAdopted` / `UserMessagesQueued` / `GraphPhaseChanged` / `CompactProgress` | 清 placeholder、sanitize、追加 timeline、更新 RunStep / Tool / 互补 timeline 投影与派生输入；turn-level `Cancelled` **NEVER** 代替 Run 终态 |
| Conversation | `RunStarted` / `RunAwaitingUser` / `RunResumed` / `RunCompleting` / `RunCompleted` / `RunFailed` / `RunCancelling` / `RunCancelled` | 按 `run_id` 投影 Runtime 权威生命周期；`RunCancelling` 进入非终态 Cancelling，只有 `RunCancelled` 进入 Cancelled；Interaction command result Intent 不参与此状态机；Created admission 阶段被拒绝时 `RunFailed` 单阶段直转 Failed，`RunCancelling` 仍先进入非终态 Cancelling（**NEVER** 直接跳到 Cancelled），完整 Created → Failed / Cancelling 映射见 [02-model.md §3.2](02-model.md#32-run-投影与-runstatus-状态机) |
| Conversation | `InteractionRequested { request_id, run_id, body }` | 穷尽映射四种 body 为 `ShowInteraction { request_id, run_id, body }`；保留 Runtime run/request identity，只携 TUI DTO，**NEVER** 携 sender |
| Conversation + Diagnostic | `Error` / `ApiError` | Conversation 追加错误块；Diagnostic 记录结构化 notice |
| Conversation + Diagnostic | `HookEvent` | Conversation 追加 sanitize 后的 hook notice；阻断 / 失败同时记录 Diagnostic Intent；PostCompact 也必须显式映射为 no-visual-state Intent，**NEVER** 静默丢弃 |
| Conversation + Config | `ThinkingChanged` | **MUST** 无条件同时产生 `ConversationIntent`（更新可见 thinking 指示器，产生 `ConversationChange::ThinkingChanged`）与 `ConfigIntent::ThinkingChanged { visible }`（更新 reasoning 能力投影，见 [02-model.md §7 ConfigProjection](02-model.md#7-configprojection)）；**NEVER** 用条件判断只产生其中一个 |
| Input | `ClipboardImage` | `InputIntent::AttachClipboardImage`；只携 TUI-owned image DTO |
| Diagnostic | `SessionResumeFailed` / `UpdateAvailable` / `CommandResultText` | 显示可定位 notice 或命令结果；需要改变 Session 的事件同时生成 Session Intent |
| Session | `TurnStarted` / `MicrocompactDone` / `StopHookBlocked` / `PostToolExecutionSync` / `CompactRollback` / `CompactFinished` | `MessagesSynced` 与 session dirty/save 投影 |
| Session + Conversation | `SessionResumed` | **MUST** 同时产生 `SessionIntent::SetCurrentSession` 与 `ConversationIntent::ResumeConversation { run_id, run_step_id }`；后者经同一 `root_reducer.apply()` 把历史 Run 投影为 `Completed`（见 [02-model.md §3.2](02-model.md#32-run-投影与-runstatus-状态机)），**NEVER** 由 helper 内部绕过 reducer 改写 `RunProjectionStatus` |
| Session | `SessionReset` / `UserMessagesWithdrawn` / `SessionResumeFailed` / `TaskStatusChanged` / `SessionSaved` / `CurrentTurnChanged` / `CommandResultText` | 更新 session id、resume/save/task/current-turn 投影；失败 / 文本事件按上表同时进入 Diagnostic |
| Config | `ModelSwitched` / `ContextEstimated` | 更新 provider/model/context-capacity 投影 |
| Workspace | `WorkingDirectoryChanged` | `ApplySnapshot` → `SnapshotApplied { root, revision }`；Coordinator 再产生异步 `ResolveWorkspaceMetadata` Effect |
| Conversation / Session | `ReflectionDone` / `ReflectionApplyDone` | 显式记录 reflection job 终态与 session dirty 状态；若协议不再发布该事件，应从封闭枚举删除而非保留空分支 |

架构测试 **MUST** 构造每个 `UiEvent` 变体并断言至少一个显式 Intent，只有带说明的 no-visual-state Intent 可表达“已消费但不渲染”。

### 3.4 sanitize 策略

| 函数 | 输入 | 输出 | 策略 |
|---|---|---|---|
| `sanitize_tool_output(tool_name, output)` | 原始 output String | 截断后的 String | 按工具名定制截断长度 |
| `sanitize_tool_arguments_delta(name, value)` | 参数 delta 字符串 | 截断后的字符串 | 防止超长参数撑爆 UI |
| `sanitize_tool_result_content(name, content)` | `serde_json::Value` | 处理后的 `Value` | 按工具名过滤敏感/冗余字段 |
| `json_value_kind(content)` | `&Value` | `&str` | 诊断用——返回 JSON 值类型名 |

> **设计原则**：sanitize 是 ACL 的核心职责——Runtime 的 tool 输出可能包含大段文本、二进制数据或敏感信息，TUI 展示前 **MUST** 经过 sanitize。sanitize 逻辑集中在 `adapter/agent_event/sanitize.rs`，**NEVER** 散落在 Model 或 Render 层。

### 3.5 空 payload 守卫

**契约：runtime 允许发空，TUI 负责不渲染。** runtime 侧多处按 `if let Some(x) = ... { send(x) }` 发送——只判 `Option` 是否 `Some`，**不判空字符串**（`looping/tools.rs` 的 `emit_json_hook_context`、`looping/post_batch.rs`、`looping/compact.rs`）。据此：

1. **MUST** 空 payload 的判定发生在 **ACL 层**，**NEVER** 下沉到 Model 或 view_assembler——TUI 自身注入的内容（如 `seed_banner` 的 `BANNER_LINES` 故意用空 System block 产生横幅空行）不经 ACL，下沉会误伤（#1106）
2. **MUST** 判空前先做等价归一化，例如 SystemMessage **MUST** 先剥离 `<system-reminder>` 信封再 `trim().is_empty()`——空信封剥离后才为空
3. **MUST** 丢弃时记 `log_debug!`，保留可观测性（能查到 runtime 发了空事件、发了几条）；这是 3.3「禁止静默忽略」的**唯一例外**：丢弃是显式决策且留痕，不是遗漏
4. **NEVER** 在 runtime 侧逐点补判空——反模式散落十余处，判空责任归展示层，单点收口

> 违反后果（#1106）：空 SystemMessage → `timeline` 空 System item → `SystemNotice` block → `render_diagnostic` 产 1 空行，叠加 `document_renderer` 给 depth0 前插的 1 行 = **每条空事件吃掉 2 行**，在输出区堆出大片空白。

## 4. SDK DTO 边界

### 4.1 类型所有权

| 类型 | 所有者 | 允许出现的位置 |
|---|---|---|
| `AgentClient` / `ChatEvent` / SDK wire DTO | Runtime-owned SDK contract | processing boundary、`adapter/event_mapping.rs`、effect runner 的 AgentClient 调用点 |
| TUI event DTO / `UiEvent` | TUI | `adapter/event_mapping.rs` 之后的 TUI 管线 |
| Intent / Change | 对应 TUI Context | reducer、Coordinator 与测试 |
| `InteractionRequestId` / interaction wire DTO | Runtime-owned SDK contract | processing boundary、event mapping 与 AgentClient effect command；均为纯值、可序列化 |

`ChatEvent` 与 `ContentBlock` 各自只有一个权威定义；SDK 通过 re-export 或同一 schema 生成发布类型。`event_mapping.rs` 对 `ChatEvent` 做封闭枚举穷尽匹配，禁止 JSON round-trip、字符串类型擦除或两份手写 wire schema。

### 4.2 TUI 自有 DTO 完全隔离

**设计原则**：TUI 定义自己的 DTO 类型，`sdk_event_to_ui_event` 在第一层转换时**彻底**消除所有 `sdk::*` 类型。UiEvent 是纯 TUI 类型，model/、update/、view_model/ 永远看不到 SDK 类型。

```rust
// app/event.rs — TUI 自有类型，NEVER import sdk::*

/// TUI 自有的 ToolCall 状态（不依赖 sdk::ToolCallStatusView）。
/// 与 [02-model.md](02-model.md) §3.3 的 ToolCallStatus 7 变体保持一致。
enum ToolCallStatus {
    PendingArgs, Ready, Running,
    Success, Error { message: String },
    Cancelled, Orphaned,
}

/// TUI 自有的 Agent 进度事件（不依赖 sdk::AgentProgressEventView）
struct AgentProgressEvent {
    kind: AgentProgressKind,
}
enum AgentProgressKind {
    Started { role: String, model: String },
    ToolOutput { tool_name: String, output: String },
    Text { text: String },
    ToolCallStart { name: String },
    ToolCallEnd { name: String, success: bool },
    Finished { summary: String },
    Error { message: String },
}

/// TUI 自有的 Hook 事件（不依赖 sdk::HookEventView）
struct HookEvent {
    hook_name: String,
    event_name: String,
    detail: String,
    outcome: HookOutcome,
}

/// TUI 自有的消息（不依赖 sdk::ChatMessage）
struct ChatMessage {
    text: String,
    input_id: Option<InputId>,
    // ...
}

struct UiInteractionRequestId(String); // 对 Runtime InteractionRequestId 的 TUI-owned 无损 newtype
struct UiUserQuestion {
    prompt: String,
    options: Vec<String>,
    default: Option<String>,
}

enum UiInteractionBody {
    UserQuestions(Vec<UiUserQuestion>),
    ToolApproval(UiApprovalPrompt),
    PlanApproval(UiApprovalPrompt),
    HardPause(UiStuckDiagnostic),
}

enum UiInteractionReply {
    UserAnswers(Vec<String>),
    ToolApproval(UiApprovalDecision),
    PlanApproval(UiApprovalDecision),
    HardPause(UiHardPauseDecision),       // v0.1.0 只有 Continue；取消走 typed cancel command
}

enum UiApprovalDecision { Approve, Deny }
enum UiHardPauseDecision { Continue }

struct UiApprovalPrompt { title: String, detail: String }
struct UiStuckDiagnostic { reason: String, recent_actions: Vec<String> }

/// 干净的 UiEvent — 所有字段都是 TUI 自有类型
enum UiEvent {
    Text { context: UiTurnContext, text: String },
    ToolCallStart {
        context: UiTurnContext,
        id: ToolCallId,              // TUI 自有
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    ToolCallUpdate {
        context: UiTurnContext,
        id: ToolCallId,              // TUI 自有
        status: ToolCallStatus,      // TUI 自有
        // ...
    },
    AgentProgress {
        context: UiTurnContext,
        tool_id: ToolCallId,
        event: AgentProgressEvent,   // TUI 自有
    },
    HookEvent(HookEvent),            // TUI 自有
    InteractionRequested {
        request_id: UiInteractionRequestId,
        run_id: RunId,
        body: UiInteractionBody,
    },
    RunResumed { run_id: RunId },
    RunCancelling { run_id: RunId },
    RunCancelled { run_id: RunId },
    // ...
}
```

1. **MUST** 在 `app/event.rs` 中定义所有 TUI 自有 DTO 类型
2. **MUST** `sdk_event_to_ui_event` 中完成所有 `sdk::*` → TUI 类型的转换——这是唯一的转换点
3. **MUST** `UiEvent` 定义中 **NEVER** 出现 `sdk::` 前缀
4. **MUST** `UiEvent` 定义中 **NEVER** 出现 channel、sender、registry handle、AgentClient 或 Project 类型
5. **MUST** 架构守卫 #6 验证：`app/event.rs`、`model/`、`update/`、`view_model/`、`view_assembler/`、`render/` 目录 **NEVER** import `sdk::*` 类型
6. **SHOULD** TUI 自有 DTO 与 SDK DTO 保持字段语义一致，避免 ACL 承担领域决策
7. **MAY** 对简单值类型直接定义 TUI newtype / alias；**NEVER** 以 SDK re-export 让依赖穿过 ACL

### 4.3 Runtime-owned request identity

Runtime 在进入 `AwaitingUser` 前生成 `InteractionRequestId`、注册 pending continuation，再发出不含 channel 的 SDK Published Language。TUI 第一层 ACL 将该 ID 无损转换为 TUI-owned `UiInteractionRequestId`，并穷尽映射四种 body；TUI **NEVER** 生成或重编号协议 identity：

```text
sdk::ChatEvent::InteractionRequested {
    request_id,
    run_id,
    body: UserQuestions(items) | ToolApproval(prompt) |
          PlanApproval(prompt) | HardPause(diagnostic),
}
  → event_mapping: SDK run/id/body → TUI-owned RunId / UiInteractionRequestId / UiInteractionBody
  → UiEvent::InteractionRequested { request_id, run_id, body }
  → AgentEventMapping { ConversationIntent::ShowInteraction { ... } }
  → reducer → InteractionShown Change
```

转换必须可逆地保留 request ID wire value，并保留 `run_id` 供 Model 拒绝旧、未知或未路由 Run 的迟到投影；Composition 已登记 parent-mediated adapter 的 Sub Run 仍可显示，且必须保留 parent/sub correlation。effect runner 仍只以 request ID 调 Runtime-owned `AgentClient` command。processing 只转发纯值事件，**NEVER** 注册 sender、保存 pending reply 或写 Model。

### 4.4 reply 与 cancel

```text
用户确认 → ConversationIntent → reducer
  → InteractionReplyRequested { request_id, reply } Change
  → Coordinator → SendInteractionReply Effect
  → effect runner: AgentClient.reply_interaction(request_id, reply.into_sdk())
  → InteractionReplySent / InteractionReplyFailed result Intent → reducer

用户取消 → ConversationIntent → reducer
  → InteractionCancelRequested { request_id } Change
  → Coordinator → CancelInteraction Effect
  → effect runner: AgentClient.cancel_interaction(request_id, UserCancelled)
  → InteractionCancelled（CancelAccepted）/ InteractionCancelRejected（CancelRejected，回退 Collecting/Confirming 并保留 draft）/ InteractionReplyFailed（IrrecoverableError）result Intent → reducer
```

规则：

1. Runtime-owned bridge **MUST** 校验 request body 与 reply variant，并对未知、重复、已完成或 RunCancelling 返回结构化 `InteractionCommandOutcome`；TUI 只投影该结果，**NEVER** 复制校验真相或假定成功。每个 variant 的类型化投影见 §4.6——`InvalidReply` **NEVER** 映射到终态。
2. UserQuestions 的答案数量 **MUST** 等于 question count，并按原问题顺序把每个 `String` 无损包装为 Runtime `UserAnswer`；不得丢项、重排或附加隐式默认值。ToolApproval / PlanApproval 只接受各自的 Approve / Deny；HardPause 只接受 Continue。`InvalidReply` 不消费 Runtime pending request，用户可修正后重试。
3. cancel 使用 typed `InteractionCancelReason::UserCancelled`，**NEVER** 用等长空字符串或 drop sender 猜测取消。
4. Run cancel / session reset 的 pending continuation 清理由 Runtime cancellation scope 负责；stream failure / processing teardown 只影响 TUI 投影，**NEVER** 冒充 Runtime cancellation 或自行 drain waiter。
5. Model 只建立属于已知非终态 Run 的 Interaction，并要求后续 result Intent 与活跃 `UiInteractionRequestId` 匹配；旧 Run / 未知 Run / 陈旧 request 不改投影，并记录 Diagnostic Intent。
6. TUI 同一时刻只容纳一个 active Interaction。Runtime **MUST** 把并发 Tool suspension 按原始 ToolCall 稳定顺序串行发布；新 request 与未完成 request 冲突时 TUI 记录协议错误，**NEVER** 静默覆盖活跃块或建立第二个 registry。
7. `InteractionReplySent` / `InteractionCancelled` 只更新匹配 Interaction 块的本地阶段，**NEVER** 把 Run 从 `AwaitingUser` 改为 `Running` 或 `Cancelled`；Runtime 完成 continuation 后发布 `RunResumed`，TUI 才恢复 Running。
8. UserQuestions 渲染问题与答案；ToolApproval / PlanApproval 渲染 Approve / Deny；HardPause 渲染 diagnostic 与 Continue / Cancel。所有选择只形成 TUI draft，业务结果仍由 Runtime continuation 决定。

### 4.5 Run 取消的两阶段投影

```text
用户打断 → reducer Change → Coordinator → RequestRunCancellation Effect
  → effect runner 调 Runtime cancel port
  → 请求已投递 / 失败 result Intent（不伪造 Run 终态）

Runtime 接受请求 → SDK RunCancelling { run_id }
  → event_mapping → UiEvent::RunCancelling
  → ConversationIntent::ProjectRunCancelling
  → reducer：live → Cancelling

Runtime 停止在途工作并完成回滚 → SDK RunCancelled { run_id }
  → event_mapping → UiEvent::RunCancelled
  → ConversationIntent::ProjectRunCancelled
  → reducer：Cancelling → Cancelled
```

`RunCancelling` 是取消 accepted 的权威 Published Language 事件；它必须立即让 TUI 展示 Cancelling，但仍是 live 非终态。仅 `RunCancelled` 可进入 Cancelled / Idle。`CancelInteraction` 发送 typed `InteractionCancelReason::UserCancelled`，只取消当前 interaction，**NEVER** 发送空答案，也 **NEVER** 等价为 `RequestRunCancellation`。

> **AgentClient trait 的特殊性**：`AgentClient` 是 Runtime-owned 入站 OHS，由 SDK 发布。TUI 的 processing / effect 边界 **MUST** 依赖此 trait（R4 允许），但 trait 方法返回的 `ChatEvent` / `ChatStream` **MUST** 在 ACL 层转换为 TUI 自有类型后才能进入 UiEvent。

### 4.6 Interaction command outcome 类型化投影

Runtime `AgentClient::reply_interaction` / `cancel_interaction` 返回封闭枚举 `InteractionCommandOutcome`；effect runner **MUST** 按 variant 映射为不同的 result Intent，ACL / reducer **MUST** 按 outcome 区分终态与非终态，**NEVER** 把所有失败折叠为 `ReplyFailed`（[02-model.md](02-model.md) 的 `InteractionPhase` 转换遵循本表）：

| `InteractionCommandOutcome` | TUI result Intent | Model InteractionPhase 目标 | draft 处置 | 交互块终态？ |
|---|---|---|---|---|
| `ReplySent` | `InteractionReplySent { request_id }` | `Collecting`/`Confirming` → `ReplyPending` → `Replied` | 消费 | 是 |
| `CancelAccepted` | `InteractionCancelled { request_id }` | → `Cancelled` | 丢弃 | 是 |
| `InvalidReply { reason }` | `InteractionReplyRejected { request_id, reason }` | **回** `Collecting`（UserQuestions）或 `Confirming`（ToolApproval / PlanApproval / HardPause）；**NEVER** 进入终态 | **保留**，用户修正后重试同一 `request_id` | **否** |
| `CancelRejected { reason }` | `InteractionCancelRejected { request_id, reason }` | **回** `Collecting`（UserQuestions）或 `Confirming`（ToolApproval / PlanApproval / HardPause）；**NEVER** 进入终态 | **保留**，用户可继续原 draft 或重新发起取消 | **否** |
| `NotFound { request_id }` | Diagnostic-only Intent（无 Interaction Change） | 不改当前活跃 Interaction 投影（陈旧 request） | 静默丢弃 | n/a |
| `AlreadyCompleted { request_id }` | Diagnostic-only Intent | 不改投影（协议冲突） | 静默丢弃 | n/a |
| `RunCancelling { run_id }` | `InteractionReplyDeferred { request_id, run_id }`；**NEVER** 推进交互终态 | 保持 `ReplyPending` / `Confirming`，等 Run 终态事件（§4.5） | 保留 | 否 |
| `IrrecoverableError { message }` | `InteractionReplyFailed { request_id, message }` | → `ReplyFailed { message }` | 丢弃 | **是**（唯一进入 `ReplyFailed` 的路径） |

规则：

1. **MUST** `InvalidReply` **NEVER** 映射到 `ReplyFailed` 或 `Cancelled`——它是可恢复的验证失败：Runtime 不消费 pending request，用户修正 draft 后可对同一 `request_id` 重试。
2. **MUST** reducer 收到 `InteractionReplyRejected` 时把 phase 回退到 `Collecting`（UserQuestions）或 `Confirming`（ToolApproval / PlanApproval / HardPause），**保留** draft 与光标位置，并把 `reason` 追加为 Diagnostic notice。
3. **MUST** `NotFound` / `AlreadyCompleted` 只产生 Diagnostic Intent，**NEVER** 改变当前活跃 Interaction 的 phase 或消费 draft；二者表示陈旧 / 协议冲突，effect runner **MUST** 记录结构化诊断日志后静默丢弃。
4. **MUST** `RunCancelling` 不伪造交互终态——交互保持等待，直到 Runtime 发布 `RunCancelled` / `RunCompleted`；`CancelInteraction` 已发 typed `UserCancelled`，Runtime 在 cancellation scope 内清理 pending continuation。
5. **MUST** 只有 `IrrecoverableError` 才进入 `ReplyFailed { message }`；该终态意味着 request 已无法重试（如对应 Run 已死、连接永久断开），且 **MUST** 在 Diagnostic 记录结构化错误。
6. **MUST** effect runner 对封闭 `InteractionCommandOutcome` 做穷尽 match，**NEVER** 把未知 / wildcard outcome 默认到 `ReplyFailed`；新增 variant **MUST** 先扩展此表与对应 result Intent。
7. **MUST** 状态机场景测试覆盖每个 outcome variant：证明 `InvalidReply` 不进入终态且 draft 保留；`NotFound` / `AlreadyCompleted` 不改活跃投影；`RunCancelling` 保持等待；只有 `IrrecoverableError` 进入 `ReplyFailed`。
8. **MUST** `cancel_interaction` 与 `reply_interaction` 共享同一 `InteractionCommandOutcome` 封闭枚举；取消被拒绝时返回 `CancelRejected { reason }`（**NEVER** 复用 `InvalidReply` 语义），reducer 按与规则 2 对称的方式把 phase 回退到 `Collecting` / `Confirming` 并保留 draft，**NEVER** 进入终态。

> **关键**：`ReplyFailed` 是 **Interaction 块** 的终态（用户看到失败块），**NOT** Run 终态。Run 生命周期只由 `Run*` Published Language 事件驱动（§4.5）。

## 5. 事件 identity 与 agent_id（R8）

```rust
/// 事件上下文扩展 agent_id
struct ChatEventContext {
    run_id: RunId,
    run_step_id: RunStepId,
    agent_id: AgentId,
}

/// AgentId — 主 agent 用 Default，sub-agent 用 tool call id 派生
type AgentId = String;  // "main" 或 sub-agent 的唯一标识
```

**设计规则**：

1. **MUST** `ChatEventContext` 携带 `agent_id`，标识事件来源的 agent
2. **MUST** 主 agent 的 `agent_id = "main"`（或 `AgentId::default()`）
3. **MUST** sub-agent 的 `agent_id` 由 `AgentTool` 在派发时生成（基于 tool_call_id）
4. **MUST** TUI Model 按 `agent_id` 路由事件到对应的 AgentProgressEntry
5. **MUST** `event_mapping.rs` 把 SDK context 转为携带同一 `agent_id` 的 TUI-owned `UiTurnContext`
6. **MUST** `agent_event.rs` 与 ConversationModel 按 `agent_id` 路由 `AgentProgress`；ToolCallId **NEVER** 代替 AgentId
7. **MUST** Main/Sub 并行场景测试证明文本、tool call、progress 与终态不会串流

## 6. Sub-agent 事件路由（#612）

### 6.1 嵌套实时展示

**决策：sub-agent 事件实时传递，嵌套展示**

```
TUI OutputTimeline
  ├─ UserMessage
  ├─ AssistantText（父 agent 文本）
  ├─ ToolCall: Agent（sub-agent 派发）
  │   ├─ AgentProgress: "Searching files..."（sub-agent 进度）
  │   ├─ AgentProgress: "Reading config.rs"（sub-agent 进度）
  │   └─ ToolResult: "Found 3 matches..."（sub-agent result）
  ├─ AssistantText（父 agent 继续文本）
  └─ ...
```

| 设计点 | 决策 | 理由 |
|---|---|---|
| 事件实时性 | 实时传递 | 长任务可观测性 |
| 展示方式 | 嵌套在 ToolCall 块下 | 明确归属关系 |
| agent_id 路由 | 按 agent_id 分组 AgentProgressEntry | 支持多 sub-agent 并行 |
| sub-agent result | 完整回传父 LLM + TUI 展示摘要 | 父 LLM 需完整 result，TUI 只需摘要 |
| config 继承 | sub-agent 启动时快照父 agent config | 运行中不受父 agent 切换影响 |

### 6.2 AgentProgressEvent 路由规则

```rust
// SDK 侧
enum AgentProgressKindView {
    Started { role: String, model: String },
    ToolOutput { tool_name: String, output: String },
    Text { text: String },
    ToolCallStart { name: String },
    ToolCallEnd { name: String, success: bool },
    Finished { summary: String },
    Error { message: String },
}
```

| AgentProgressKind | TUI 映射 | 展示 |
|---|---|---|
| `Started` | `UpdateAgentMeta` | sub-agent 元信息（role + model） |
| `ToolOutput` | `RecordAgentProgress` | sanitize 后的 sub-agent tool 输出摘要；**NEVER** 空 mapping |
| `Text` | `RecordAgentProgress` | sub-agent 实时文本流 |
| `ToolCallStart` | `RecordAgentProgress` | sub-agent 内部 tool call 开始 |
| `ToolCallEnd` | `RecordAgentProgress` | sub-agent 内部 tool call 结束 |
| `Finished` | `RecordAgentProgress` | 完成摘要 |
| `Error` | `RecordAgentProgress` + Diagnostic | 错误展示 |

### 6.3 sub-agent config 继承链路

```
父 agent ChatLoop
  → AgentTool::execute(spec)
    → spec 含：model / provider / permission / workspace / context_size / tools / hooks / guidance
    → 子 agent ChatLoop::new(spec)（启动时快照）
    → 子 agent 运行期间不受父 agent config 切换影响
  → 子 agent 完成 → result 回传
```

| config 项 | 继承方式 | 说明 |
|---|---|---|
| model / provider | 父 `RunConfigSnapshot` | sub-agent 可被入参覆盖 |
| permission | 父 `RunConfigSnapshot` | sub-agent 可被入参收紧（NEVER 放宽） |
| workspace | 父 Run 的 workspace snapshot | sub-agent 在同一 workspace 执行 |
| context_size | 父 `RunConfigSnapshot` | sub-agent 独立 context window |
| tools | 父 agent 子集 | sub-agent 可用工具 ⊆ 父 agent |
| hooks | 父 `RunConfigSnapshot` | sub-agent 继承 hook 配置 |
| guidance | 父 `RunConfigSnapshot` | sub-agent 继承 guidance 文件 |

## 7. 转换集中化

### 7.1 两层转换的职责边界

| 层 | 位置 | 输入 → 输出 | 职责 | 禁止 |
|---|---|---|---|---|
| 第一层 | `adapter/event_mapping.rs` | `sdk::ChatEvent` → `UiEvent` | 结构转换、SDK 类型消除 | **NEVER** 产生 Intent / Effect 或执行 I/O |
| 第二层 | `adapter/agent_event.rs` | `&UiEvent` → `AgentEventMapping` | Intent 拆分、sanitize、格式化 | **NEVER** 接触 SDK 类型或产生 Effect |

### 7.2 集中化规则

1. **MUST** 所有 `sdk::ChatEvent` → `UiEvent` 转换 **只在** `event_mapping.rs` 中完成
2. **MUST** 所有 `UiEvent` → `AgentEventMapping` 转换 **只在** `agent_event.rs` 中完成
3. **MUST** `event_mapping.rs` 和 `agent_event.rs` 位于 `adapter/`；结构 / 语义转换 **NEVER** 放进 `effect/`、`model/` 或 `render/`
4. **MUST** Composition 根负责装配——`spawn_processing` 与 EffectRunner 持 `AgentClient`；event_mapping 和 agent_event 保持纯函数，TUI 不装配 pending reply registry
5. **NEVER** 在 `model/` 中 import `sdk::*` 类型（架构门禁 #2 + #6）
6. **MUST** `UiEvent`（`app/event.rs`）**NEVER** 出现 `sdk::` 前缀——TUI 自有 DTO 在此定义，SDK 类型在 `event_mapping.rs` 中彻底转换（R7）
7. **NEVER** 在 `update/`、`view_model/`、`view_assembler/`、`render/` 中 import `sdk::*` 类型——这些层只消费 TUI 自有类型

### 7.3 Composition 根装配

```rust
// effect/session/processing.rs — AgentClient stream 的纯值 SDK event 边界
struct ProcessingSession {
    client: Arc<dyn AgentClient>,
    ui_tx: mpsc::Sender<UiEvent>,
}

impl ProcessingSession {
    async fn spawn(self) {
        let stream = self.client.chat(request).await;
        while let Some(event) = stream.next().await {
            let ui_event = sdk_event_to_ui_event(event); // InteractionRequested 也只含纯值
            self.ui_tx.send(ui_event).await?;
        }
    }
}

// app/update.rs — 主线程 TEA Update
impl App {
    fn update_agent_event(&mut self, event: UiEvent) {
        let mapping = map_agent_event(&event);                  // 第二层，只含 Intent
        let changes = self.root_reducer.apply(mapping.intents); // 唯一 Model 写入
        let effects = self.coordinator.effects_for(changes);    // Change 决定 Effect
        self.effect_queue.extend(effects);                      // update 本身不执行 I/O
    }

    fn update_result_intent(&mut self, intent: AgentIntent) {
        let changes = self.root_reducer.apply([intent]);        // 与事件 Intent 同一 reducer
        self.effect_queue.extend(self.coordinator.effects_for(changes));
    }
}

// effect/runner.rs — 唯一副作用执行点
impl EffectRunner {
    async fn run(&self, effect: Effect) {
        let result_intents = match effect {
            Effect::SendInteractionReply { request_id, reply } =>
                self.agent_client.reply_interaction(request_id.into_sdk(), reply.into_sdk()),
            Effect::CancelInteraction { request_id } =>
                self.agent_client.cancel_interaction(request_id.into_sdk(), UserCancelled),
            effect => self.run_other(effect).await,
        };
        for intent in result_intents {
            self.msg_tx.send(TuiMsg::Intent(intent)).await?;
        }
    }
}
```

`ProcessingSession` 与 EffectRunner 持有 Runtime-owned `AgentClient` 契约；v0.1.0 由 Composition 注入 local adapter。转换层不关心具体实现，远端 transport 明确留给 Server future boundary，本文 **NEVER** 预建 WSS 帧或重连语义。

## 8. 架构门禁

### 8.1 事件流相关门禁

> 本表编号（#2/#6/#7/#8/#9/#10）与 [01-architecture-and-dataflow.md §9](01-architecture-and-dataflow.md) 的全局门禁编号 1–10 是同一体系的子集，名称与证明必须逐条一致；[04-view-layer.md §8](04-view-layer.md) 使用独立的 `V1`–`V8` 视图层门禁编号，不复用本表数字，避免同一数字指向两条不同规则。

| # | 门禁 | Target 证明 |
|---|---|---|
| 2 | Model purity | arch test：`model/` 禁止 import ratatui/tokio/AgentClient/channel/sender |
| 6 | Agent event adapter | arch test：SDK event DTO 只在 `adapter/event_mapping.rs` 与持有 stream 的 processing boundary 出现；`agent_event.rs` 只见 TUI DTO |
| 7 | TEA purity | arch test：`update/` 禁止 `tokio::spawn`/`Command::new`/`.await`，ACL 禁止构造 Effect |
| 8 | Interaction resource isolation | arch test：Runtime request id 只经 SDK DTO → TUI newtype → AgentClient command 无损贯穿；TUI 全树零 sender / pending waiter / 自生成协议 id |
| 9 | Event exhaustiveness | 构造每个 UiEvent 变体，断言第二层 ACL 产生显式 Context Intent；禁止 wildcard 与默认空 mapping |
| 10 | Model write isolation | arch test：六 Context 核心字段私有；`apply` / `reduce_*` 生产调用点只有 `update/root_reducer.rs`，adapter / Coordinator / ViewAssembler 只取得不可变 projection |

状态机场景测试 **MUST** 穷尽四种 Interaction body，并证明：`InteractionReplySent` / `InteractionCancelled` 不改变 Run；`RunResumed` 才把 `AwaitingUser` 投影为 `Running`；`RunCancelling` 只进入非终态 `Cancelling`；`RunCancelled` 才进入终态。该证明必须覆盖 event mapping → Intent、Intent → Change 两层，**NEVER** 只测最终渲染。

### 8.2 门禁 #6 详细规则

**门禁 #6：SDK 类型只在 ACL 边界出现（R4 + R7）**

```
允许 import sdk::ChatEvent 与关联 wire DTO 的精确边界：
  ✅ apps/cli/src/tui/effect/session/processing.rs — 接收 ChatStream，并立即交给 ACL
  ✅ apps/cli/src/tui/adapter/event_mapping.rs     — 唯一 SDK DTO → TUI DTO 转换点

禁止 SDK DTO 的第二层 ACL：
  ❌ apps/cli/src/tui/adapter/agent_event.rs       — 只消费 UiEvent / TUI-owned DTO

允许 import sdk::AgentClient trait 的目录（Runtime-owned OHS 依赖，R4 允许）：
  ✅ apps/cli/src/tui/effect/session/processing/   — 持有 AgentClient
  ✅ apps/cli/src/tui/effect/                      — Effect 执行器调 AgentClient
  ✅ composition 根                                — 依赖注入

禁止 import 任何 sdk::* 类型的目录（R7）：
  ❌ apps/cli/src/tui/app/event.rs                  — UiEvent 只持有 TUI 自有类型
  ❌ apps/cli/src/tui/model/                        — Model 纯净
  ❌ apps/cli/src/tui/app/update/                   — TEA Update 纯净
  ❌ apps/cli/src/tui/view_model/                   — ViewModel 不接触 SDK
  ❌ apps/cli/src/tui/view_assembler/               — ViewAssembler 不接触 SDK
  ❌ apps/cli/src/tui/render/                       — Render 只读 TUI 类型

禁止 sender / pending waiter 的整个 TUI 边界：
  ❌ apps/cli/src/tui/effect/session/processing.rs — 只接收纯值 event
  ❌ apps/cli/src/tui/effect/                     — 只调用 AgentClient typed command
  ❌ apps/cli/src/tui/app/event.rs                 — UiEvent 只携 request id + DTO
  ❌ apps/cli/src/tui/model/                       — Model 只携 request id + 投影状态
  ❌ apps/cli/src/tui/app/update/                  — reducer 只产 Change
  ❌ apps/cli/src/tui/view_model/ 与 render/       — 展示层零运行期 reply 资源
```

> **关键**：`app/event.rs`（UiEvent 定义）也在禁止列表中——UiEvent **NEVER** 持有 `sdk::*` 类型。这是 R7 的直接要求：领域事件与 TUI Model **NEVER** 跨界直用。

### 8.3 门禁实现模式

```rust
// architecture_tests.rs
fn test_sdk_event_types_only_in_adapter() {
    let allowed_files = [
        "tui/adapter/event_mapping.rs",
        "tui/effect/session/processing.rs",
    ];
    let sdk_patterns = ["sdk::ChatEvent", "sdk::ChatEventContext",
                        "sdk::ToolCallStatusView", "sdk::AgentProgressEventView",
                        "sdk::HookEventView", "sdk::WorkspaceContextView"];

    for file in production_source("tui/") {
        let path = file.relative_path();
        if !allowed_files.contains(&path) {
            for pattern in &sdk_patterns {
                assert!(!file.content().contains(pattern),
                    "SDK type {} found in non-adapter file: {}", pattern, path);
            }
        }
    }
}
```

## 9. 迁移治理边界

本文只定义 Target 契约。实现差距、[#943](https://github.com/rushsinging/aemeath/issues/943) / [#944](https://github.com/rushsinging/aemeath/issues/944) / [#947](https://github.com/rushsinging/aemeath/issues/947) 的责任与退出条件只在 [Migration Governance](../../03-engineering/03-migration-governance.md) O6 维护；sub-agent 实时事件与 `agent_id` 的产品范围由 [#612](https://github.com/rushsinging/aemeath/issues/612) 承接。

## 10. 相关文档

- TUI 架构与数据流：[01-architecture-and-dataflow.md](01-architecture-and-dataflow.md)
- TUI Model 层设计：[02-model.md](02-model.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- SDK Published Language：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Server future boundary（本 milestone 不冻结 WSS 协议）：[../server/README.md](../server/README.md)
- sub-agent 调研 issue：#612

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：事件流、AgentEventMapper ACL、SDK DTO 边界、agent_id、sub-agent 路由、转换集中化与架构门禁 | #797 |
| 2026-07-12 | 强化 R4 / R7：TUI 自有 DTO 完全隔离，门禁 #6 覆盖 app/event.rs | #797 |
| 2026-07-12 | DDD/Hexagonal 评审：收敛 AgentEventMapping、event_mapping 与 Effect 边界 | #798 评审 |
| 2026-07-14 | 统一 SDK → TUI DTO → Intent → Change → Coordinator Effect → result Intent；Runtime-owned interaction id 经 AgentClient reply command 闭环，六 Context 穷尽映射，实现差距收口到 Migration Governance O6 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 新增 §4.6 Interaction command outcome 类型化投影：`InvalidReply` 回退 Collecting/Confirming 且保留 draft（**NEVER** 终态）；`NotFound`/`AlreadyCompleted` 静默丢弃 + 诊断；`RunCancelling` 保持等待；仅 `IrrecoverableError` 进入 `ReplyFailed`（外部评审 finding #9，非架构门禁编号） | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | §4.6 补 `CancelRejected` outcome（对称于 `InvalidReply`，回退 Collecting/Confirming 并保留 draft）；§3.3 闭合 `ThinkingChanged` 双 Context（Config + Conversation）无条件同时映射与 `SessionResumed` → `ConversationIntent::ResumeConversation` 显式映射；Created → Failed/Cancelling admission 事件映射交叉引用 02-model.md §3.2；门禁 #8 命名与 01 对齐为『Interaction resource isolation』，并补充与 04 `V1`–`V8` 独立编号体系的交叉引用说明 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
