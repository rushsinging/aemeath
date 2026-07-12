# TUI · 事件流与 ACL 设计

> 层级：02-modules / tui（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#797（S2）
> 本文定义 TUI 事件流的完整链路、AgentEventMapper 防腐层（ACL）、SDK DTO 边界、agent_id 缺口（R8）、sub-agent 事件路由（#612）、转换集中化策略与架构门禁。

## 1. 定位

事件流是 TUI 三条信息流之一（#795 §4.2），承载 Runtime → TUI 的单向数据流：

```
Runtime ChatStream → tokio::spawn task → sdk::ChatEvent
  → sdk_event_to_ui_event（effect/session/processing/event_mapping.rs，第一层转换）
  → UiEvent → mpsc channel (cap 256)
  → ui_rx → tokio::select! → TuiMsg::Ui(ui_event)
  → map_agent_event_with_tool_header（adapter/agent_event.rs，第二层 ACL）
  → AgentEventMapping { conversation_intents, diagnostic_intents, session_intents, effects }
  → root_reducer → Model change → ViewModelDirty → ViewAssembler → Render
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
┌─ Runtime ─────────────────────────────────────────────────────┐
│  ChatLoop → RuntimeStreamEvent                                │
│  (events.rs — 领域事件，含 RuntimeTurnContext / Message 等)    │
└──────────┬────────────────────────────────────────────────────┘
           │
     ┌─────┴─────┐
     │ convert.rs │  444 行手工 match（⚠️ 已有 5 处漂移）
     │ Runtime → SDK │  RuntimeStreamEvent → sdk::ChatEvent
     └─────┬─────┘
           │
           ▼  sdk::ChatEvent（SDK Published Language）
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
│  → map_agent_event_with_tool_header（adapter/agent_event.rs） │
│    → AgentEventMapping {                                      │
│        conversation: Vec<ConversationIntent>,                 │
│        diagnostic: Vec<DiagnosticIntent>,                     │
│        session: Vec<SessionIntent>,                           │
│        effects: Vec<Effect>,                                  │
│      }                                                        │
│  → root_reducer（apply intents → Model changes）              │
│  → update_ui（处理 side-effects）                             │
│  → merge_dirty → ViewAssembler → Render                      │
└──────────────────────────────────────────────────────────────┘
```

### 2.2 涉及文件

| 文件 | 职责 |
|---|---|
| `agent/.../events.rs` | `RuntimeStreamEvent` 定义（领域事件） |
| `agent/.../convert.rs` | `RuntimeStreamEvent` → `sdk::ChatEvent` 手工转换（444 行） |
| `packages/sdk/src/chat_event.rs` | `ChatEvent` 定义（SDK Published Language） |
| `apps/cli/.../effect/session/processing.rs` | `spawn_processing`：持 `AgentClient` → `ChatStream` → 转发 UiEvent |
| `apps/cli/.../effect/session/processing/event_mapping.rs` | `sdk_event_to_ui_event`：第一层结构转换 |
| `apps/cli/.../app/event.rs` | `UiEvent`（`AppEvent`）定义 |
| `apps/cli/.../adapter/agent_event.rs` | `map_agent_event_with_tool_header`：第二层 ACL |
| `apps/cli/.../adapter/agent_event/progress.rs` | sub-agent progress 格式化 |
| `apps/cli/.../adapter/agent_event/sanitize.rs` | tool 输出/参数截断 |
| `apps/cli/.../adapter/hook_notice.rs` | Hook 事件 → TUI notice |

## 3. AgentEventMapper ACL 设计

### 3.1 ACL 职责

`map_agent_event_with_tool_header` 是防腐层核心，职责：

1. **Intent 拆分**：一个 UiEvent 可能产生多个 Intent（跨 Context），如 `Error` 同时产生 ConversationIntent + DiagnosticIntent + Effect
2. **sanitize**：tool 输出/参数截断（`sanitize_tool_output` / `sanitize_tool_arguments_delta` / `sanitize_tool_result_content`）
3. **progress 格式化**：sub-agent progress 事件 → 可读字符串（`format_agent_progress`）
4. **hook notice 派生**：Hook 事件 → HookNoticeContent（`hook_event_notice`）
5. **placeholder 清理**：收到实际内容时清 ModelStreamWaiting 占位（`clear_placeholder_then`）

### 3.2 AgentEventMapping 结构

```rust
struct AgentEventMapping {
    conversation: Vec<ConversationIntent>,
    diagnostic: Vec<DiagnosticIntent>,
    session: Vec<SessionIntent>,
    effects: Vec<Effect>,
}
```

**设计规则**：

1. **MUST** 一个 UiEvent 映射到一个 `AgentEventMapping`——不允许部分拆分
2. **MUST** Intent 拆分按 Context 归类，**NEVER** 把不同 Context 的 Intent 混在同一个 Vec 中
3. **MUST** `AgentEventMapping` 是值类型（`#[derive(Debug, Default, PartialEq)]`），便于测试断言
4. **MUST** ACL 函数是纯函数——输入 `&UiEvent`，输出 `AgentEventMapping`，**NEVER** 产生副作用

### 3.3 事件映射分类

| 分类 | UiEvent 变体 | 目标 Context | 关键逻辑 |
|---|---|---|---|
| 文本流 | Text / Thinking / BlockComplete | Conversation | `clear_placeholder_then` + append |
| 工具调用 | ToolCallStart / ToolCallUpdate / ToolResult | Conversation | sanitize 参数/输出 |
| Agent 进度 | AgentProgress | Conversation | `format_agent_progress` 格式化 |
| 完成事件 | Done / DoneWithDuration / Cancelled | Conversation | `CompleteChat` |
| 用量 | Usage / LiveTps | Conversation | `RecordUsage` + `RecordLiveTps` |
| 错误 | Error | Conversation + Diagnostic + Effect | 三路拆分 |
| 系统消息 | SystemMessage | Conversation | `AppendSystemMessage` |
| ModelStream | ModelStreamWaiting | Conversation | `UpsertModelStreamPlaceholder` |
| 消息同步 | TurnStarted / MicrocompactDone / StopHookBlocked / PostToolExecutionSync / CompactRollback / CompactFinished / ApiError | Session | `MessagesSynced` |
| AskUser | AskUserBatch | （空 mapping，ui_event.rs 单独处理） | — |
| Hook | HookEvent | Conversation | `hook_event_notice` 派生，PostCompact 忽略 |
| 工作区 | WorkingDirectoryChanged | Conversation | `WorkspaceSnapshotReceived` |
| 其余 | SessionReset / UserMessagesWithdrawn / GraphPhaseChanged / CompactProgress / ModelSwitched / ThinkingChanged / ContextEstimated / CommandResultText / SessionResumed / SessionResumeFailed / TaskStatusChanged / UpdateAvailable / SessionSaved / CurrentTurnChanged / UserMessagesAdopted / UserMessagesQueued / ClipboardImage / ReflectionDone / ReflectionApplyDone | 各自 Context | 见 ui_event.rs |

### 3.4 sanitize 策略

| 函数 | 输入 | 输出 | 策略 |
|---|---|---|---|
| `sanitize_tool_output(tool_name, output)` | 原始 output String | 截断后的 String | 按工具名定制截断长度 |
| `sanitize_tool_arguments_delta(name, value)` | 参数 delta 字符串 | 截断后的字符串 | 防止超长参数撑爆 UI |
| `sanitize_tool_result_content(name, content)` | `serde_json::Value` | 处理后的 `Value` | 按工具名过滤敏感/冗余字段 |
| `json_value_kind(content)` | `&Value` | `&str` | 诊断用——返回 JSON 值类型名 |

> **设计原则**：sanitize 是 ACL 的核心职责——Runtime 的 tool 输出可能包含大段文本、二进制数据或敏感信息，TUI 展示前 **MUST** 经过 sanitize。sanitize 逻辑集中在 `adapter/agent_event/sanitize.rs`，**NEVER** 散落在 Model 或 Render 层。

### 3.5 ACL 目标态改进

| # | 现状 | 目标态 |
|---|---|---|
| 1 | `UiEvent` 直接持有 SDK 类型（`sdk::ToolCallStatusView`、`sdk::AgentProgressEventView`、`sdk::HookEventView` 等） | UiEvent 只持有 TUI 自有类型，SDK 类型在第一层 `sdk_event_to_ui_event` 中转换完毕 |
| 2 | `event_mapping.rs` 的 `WorkingDirectoryChanged` 同步调 `git branch` + `worktree kind`（子进程） | 移到 Effect 异步执行或加缓存（#795 §10.5） |
| 3 | `_diagnostic` helper 无调用方（死代码） | 删除 |
| 4 | `map_agent_event_with_tool_header` 接受 `FnMut` 回调格式化 subagent header | 目标态：progress 格式化逻辑内聚在 `progress.rs`，移除外部回调注入 |

## 4. SDK DTO 边界

### 4.1 三类同步方式

| 类别 | 当前同步方式 | 风险 | 目标态 |
|---|---|---|---|
| 30+ tool result 类型 | `pub use` re-export（单一来源） | ✅ 无 | 保持 |
| `ChatEvent` ↔ `RuntimeStreamEvent` | 444 行手工 match（`convert.rs`） | ⚠️ 高——已有 5 处结构漂移 | Runtime 定义 → SDK re-export 或 codegen，删除 convert.rs |
| `ContentBlock` | JSON round-trip（`serde_json::from_value(to_value(...))`） | ⚠️ 脆弱——加变体静默降级 | share 定义 → SDK re-export，删除 JSON round-trip |

### 4.2 已发现的漂移（5 处）

| 字段 | RuntimeStreamEvent | ChatEvent (SDK) | 漂移 |
|---|---|---|---|
| DoneWithDuration | `duration: Duration` | `duration_ms: u64` | 重命名 + 改类型 |
| UserMessagesAdopted | `Vec<(InputId, Message)>` | `Vec<ChatMessage>` | tuple → flat，input_id 丢失 |
| GraphPhaseChanged | `ReasoningNode, ReasoningLevel` | `String, String, String` | 类型擦除 |
| WorkingDirectoryChanged | `PersistedWorkspaceContext` | `WorkspaceContextView` | 不同 struct |
| CompactProgress | `CompactStage, usize` | `String, u32` | 类型擦除 |

> **根因**：Runtime 和 SDK 各自定义事件类型，手工 match 转换。每加一个变体或改一个字段，两边都要改，容易遗漏。类型擦除（enum → String）丢失编译期保障。

### 4.3 目标态：SDK DTO 从 Runtime auto-gen

**原则：Runtime 是类型定义的唯一来源，SDK 不应两边维护。**

| 类别 | 目标 | 迁移动作 |
|---|---|---|
| tool result 类型 | ✅ 保持 `pub use` re-export | 无 |
| `ChatEvent` | Runtime 定义 → SDK re-export | 删除 `convert.rs` 444 行手工 match；runtime 直接暴露 `ChatEvent`，SDK re-export |
| `ContentBlock` | share 定义 → SDK re-export | 删除 JSON round-trip；SDK 直接 `pub use share::message::ContentBlock` |
| 架构守卫 | CI test 验证两侧 JSON shape 一致 | 添加 round-trip 测试 |

### 4.4 UiEvent 类型泄漏现状

当前 `UiEvent`（`AppEvent`）直接持有以下 SDK 类型：

| UiEvent 变体 | 持有的 SDK 类型 | 应转为 TUI 类型 |
|---|---|---|
| `ToolCallStart` | `sdk::ids::ToolCallId` | `ToolCallId`（TUI 自有别名或 re-export） |
| `ToolCallUpdate` | `sdk::ids::ToolCallId`, `sdk::ToolCallStatusView` | `ToolCallId`, `ToolCallStatus`（TUI 枚举） |
| `ToolResult` | `sdk::ids::ToolCallId`, `sdk::ToolResultImage` | `ToolCallId`, `ToolResultImage`（TUI 类型） |
| `TurnStarted` 等 | `Vec<sdk::ChatMessage>` | `Vec<ChatMessage>`（TUI DTO） |
| `AskUserBatch` | `Vec<sdk::AskUserQuestionItem>`, `oneshot::Sender` | TUI 类型 + channel |
| `AgentProgress` | `sdk::AgentProgressEventView` | `AgentProgressEvent`（TUI DTO） |
| `HookEvent` | `sdk::HookEventView` | `HookEvent`（TUI DTO） |
| `TaskStatusChanged` | `sdk::TaskStatusView` | `TaskStatus`（TUI DTO） |
| `ClipboardImage` | `sdk::ClipboardImageView` | `ClipboardImage`（TUI DTO） |
| `ReflectionDone` | `sdk::ReflectionOutputView` | 删除（死代码） |
| `SessionResumeFailed` | `sdk::SessionResumeFailureKind` | TUI 枚举 |
| `ModelSwitched` | `sdk::ModelSwitchResult` | TUI DTO |
| `ContextEstimated` | `sdk::ContextEstimate` | TUI DTO |
| `WorkingDirectoryChanged` | `sdk::WorkspaceContextView` | TUI 类型 |

> **目标态**：`sdk_event_to_ui_event` 在第一层转换时 **MUST** 把所有 SDK 类型转换为 TUI 自有类型。`UiEvent` **NEVER** 直接持有 `sdk::*` 类型。这需要：
> 1. 在 `app/event.rs` 中定义 TUI 自有 DTO（或从 SDK re-export 但加 TUI 别名）
> 2. `event_mapping.rs` 中完成所有类型转换
> 3. 架构守卫 #6 验证 SDK 类型只在 `adapter/` + `effect/session/processing/` 出现

## 5. 事件缺 agent_id 缺口（R8）

### 5.1 问题

当前 `UiEvent` 和 `sdk::ChatEvent` **没有 `agent_id` 字段**。所有事件默认属于"主 agent"。sub-agent 事件通过 `AgentProgress` 变体传递，但无法区分多个 sub-agent 并行执行时的事件来源。

**影响场景**：
- 两个 sub-agent 并行执行时，TUI 无法区分进度条目属于哪个 sub-agent
- sub-agent 内部的 tool call 事件无法路由到正确的 agent 上下文
- `AgentProgress.tool_id` 是 ToolCallId，不是 AgentId——无法作为 agent 标识

### 5.2 目标态

```rust
/// 事件上下文扩展 agent_id
struct ChatEventContext {
    chat_id: ChatId,       // 现状：Run 投影 ID
    turn_id: ChatTurnId,   // 现状：RunStep 投影 ID
    agent_id: AgentId,     // 目标态新增：区分主 agent 和 sub-agent
}

/// AgentId — 主 agent 用 Default，sub-agent 用 tool call id 派生
type AgentId = String;  // "main" 或 sub-agent 的唯一标识
```

**设计规则**：

1. **MUST** `ChatEventContext` 携带 `agent_id`，标识事件来源的 agent
2. **MUST** 主 agent 的 `agent_id = "main"`（或 `AgentId::default()`）
3. **MUST** sub-agent 的 `agent_id` 由 `AgentTool` 在派发时生成（基于 tool_call_id）
4. **MUST** TUI Model 按 `agent_id` 路由事件到对应的 AgentProgressEntry
5. **SHOULD** `UiTurnContext` 也携带 `agent_id`，保持 ACL 转换一致性

### 5.3 迁移动作

1. SDK `ChatEventContext` 加 `agent_id` 字段
2. Runtime `RuntimeTurnContext` 加 `agent_id`，ChatLoop 在 emit 事件时填充
3. `convert.rs` 传递 `agent_id`
4. `event_mapping.rs` 传递到 `UiTurnContext`
5. `agent_event.rs` 按 `agent_id` 路由 `AgentProgress` 事件
6. TUI Model 的 `agent_progress: Vec<AgentProgressEntry>` 按 `agent_id` 分组

## 6. Sub-agent 事件路由（#612）

### 6.1 现状链路

```
父 agent ChatLoop
  → AgentTool::execute()
    → 子 agent ChatLoop（独立 agent loop）
      → 子 agent 事件 → AgentProgressEvent
    → 子 agent final result → ToolResult（回传父 LLM）
  → 父 agent 收到 ToolResult
```

**当前 sub-agent 事件传递到 TUI 的方式**：

1. 子 agent loop 内部事件 → `AgentProgressEvent` → 父 agent `RuntimeStreamEvent::AgentProgress`
2. 父 agent emit `AgentProgress` 到 ChatStream
3. TUI 收到 `UiEvent::AgentProgress`，`map_agent_event_with_tool_header` 格式化为字符串
4. 字符串追加到 `agent_progress: Vec<AgentProgressEntry>`，渲染为进度块

### 6.2 问题

| 问题 | 现状 | 影响 |
|---|---|---|
| sub-agent 中间事件被聚合 | 子 agent 的文本流/tool call 被压缩为 `AgentProgressEvent` 字符串 | TUI 无法展示 sub-agent 的实时细节 |
| 无 agent_id 区分 | 多个 sub-agent 并行时无法区分 | 进度条目混在一起（R8） |
| sub-agent result 截断 | result 作为 ToolResult 回传父 LLM，但 TUI 看到的 ToolResult 是 sanitize 后的 | TUI 无法展示完整 result |
| config 继承不透明 | sub-agent 继承父 agent 的 provider/model/permission/workspace，但 TUI 无感知 | 切换 model 时不影响进行中的 sub-agent |

### 6.3 目标态设计

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

### 6.4 AgentProgressEvent 路由规则

```rust
// SDK 侧
enum AgentProgressKindView {
    Started { role: String, model: String },
    ToolOutput { tool_name: String, output: String },
    Text { text: String },         // 目标态新增：sub-agent 实时文本
    ToolCallStart { name: String }, // 目标态新增：sub-agent 内部 tool call
    ToolCallEnd { name: String, success: bool }, // 目标态新增
    Finished { summary: String },
    Error { message: String },
}
```

| AgentProgressKind | TUI 映射 | 展示 |
|---|---|---|
| `Started` | `UpdateAgentMeta` | sub-agent 元信息（role + model） |
| `ToolOutput` | 当前忽略（`AgentEventMapping::default()`） | 目标态：展示 sub-agent tool 输出摘要 |
| `Text`（新增） | `RecordAgentProgress` | sub-agent 实时文本流 |
| `ToolCallStart`（新增） | `RecordAgentProgress` | sub-agent 内部 tool call 开始 |
| `ToolCallEnd`（新增） | `RecordAgentProgress` | sub-agent 内部 tool call 结束 |
| `Finished` | `RecordAgentProgress` | 完成摘要 |
| `Error` | `RecordAgentProgress` + Diagnostic | 错误展示 |

### 6.5 sub-agent config 继承链路

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
| model / provider | 父 agent 当前值快照 | sub-agent 可被入参覆盖 |
| permission | 父 agent 当前值 | sub-agent 可被入参收紧（NEVER 放宽） |
| workspace | 父 agent 当前值 | sub-agent 在同一 workspace 执行 |
| context_size | 父 agent 当前值 | sub-agent 独立 context window |
| tools | 父 agent 子集 | sub-agent 可用工具 ⊆ 父 agent |
| hooks | 父 agent 当前值 | sub-agent 继承 hook 配置 |
| guidance | 父 agent 当前值 | sub-agent 继承 guidance 文件 |

## 7. 转换集中化

### 7.1 两层转换的职责边界

| 层 | 位置 | 输入 → 输出 | 职责 | 禁止 |
|---|---|---|---|---|
| 第一层 | `event_mapping.rs` | `sdk::ChatEvent` → `UiEvent` | 结构转换、SDK 类型消除 | **NEVER** 产生 Intent / Effect |
| 第二层 | `agent_event.rs` | `&UiEvent` → `AgentEventMapping` | Intent 拆分、sanitize、格式化 | **NEVER** 接触 SDK 类型 |

### 7.2 集中化规则

1. **MUST** 所有 `sdk::ChatEvent` → `UiEvent` 转换 **只在** `event_mapping.rs` 中完成
2. **MUST** 所有 `UiEvent` → `AgentEventMapping` 转换 **只在** `agent_event.rs` 中完成
3. **MUST** `event_mapping.rs` 和 `agent_event.rs` 在 `adapter/` 或 `effect/session/processing/` 目录下——**NEVER** 在 `model/` 或 `render/` 中
4. **MUST** composition 根负责装配——`spawn_processing` 持 `AgentClient`，event_mapping 和 agent_event 是静态函数
5. **NEVER** 在 `model/` 中 import `sdk::*` 类型（架构门禁 #2 + #6）

### 7.3 Composition 根装配

```rust
// effect/session/processing.rs — 唯一的 AgentClient 持有方
struct ProcessingSession {
    client: Arc<dyn AgentClient>,  // 端口接口，组合根注入
    ui_tx: mpsc::Sender<UiEvent>,  // 256 cap channel
}

impl ProcessingSession {
    async fn spawn(self) {
        let stream = self.client.chat(request).await;
        while let Some(event) = stream.next().await {
            let ui_event = sdk_event_to_ui_event(event);  // 第一层
            let _ = self.ui_tx.send(ui_event).await;       // channel
        }
    }
}

// app/update.rs — 主线程 TEA Update
impl App {
    fn update_agent_event(&mut self, event: UiEvent) {
        let mapping = map_agent_event_with_tool_header(&event, default_header);  // 第二层
        self.root_reducer.apply(mapping);
    }
}
```

> **与 #796 §13 端口适配器的关系**：`ProcessingSession` 持有的 `AgentClient` trait 是端口接口，可以是 `LocalAgentClient`（直连）或 `WssAgentClient`（远程）。转换层不关心适配器实现——`ChatStream` 产出的 `ChatEvent` 类型一致。

## 8. 架构门禁

### 8.1 事件流相关门禁

| # | 门禁 | 实现方式 | 现状 |
|---|---|---|---|
| 2 | Model purity | arch test：`model/` 禁止 import ratatui/tokio/AgentClient | ❌ 缺失 |
| 6 | Agent event adapter | arch test：SDK event 类型只在 `adapter/` + `effect/session/processing/` 出现 | ❌ 缺失 |
| 7 | TEA purity | arch test：`update/` 禁止 `tokio::spawn`/`Command::new`/`.await` | ❌ 缺失 |

### 8.2 门禁 #6 详细规则

**门禁 #6：SDK event 类型只在 adapter 层出现**

```
允许 import sdk::ChatEvent / sdk::ChatEventContext / sdk::*View 的目录：
  ✅ apps/cli/src/tui/adapter/
  ✅ apps/cli/src/tui/effect/session/processing/

禁止 import sdk::ChatEvent / sdk::ChatEventContext / sdk::*View 的目录：
  ❌ apps/cli/src/tui/model/
  ❌ apps/cli/src/tui/app/update/
  ❌ apps/cli/src/tui/view_model/
  ❌ apps/cli/src/tui/view_assembler/
  ❌ apps/cli/src/tui/render/
```

**目标态**：`UiEvent`（`app/event.rs`）也不持有 SDK 类型——当前是现状缺口，需逐步迁移（见 §4.4）。

### 8.3 门禁实现模式

```rust
// architecture_tests.rs
fn test_sdk_event_types_only_in_adapter() {
    let allowed_dirs = ["tui/adapter/", "tui/effect/session/processing/"];
    let sdk_patterns = ["sdk::ChatEvent", "sdk::ChatEventContext",
                        "sdk::ToolCallStatusView", "sdk::AgentProgressEventView",
                        "sdk::HookEventView", "sdk::WorkspaceContextView"];

    for file in production_source("tui/") {
        let path = file.relative_path();
        if !allowed_dirs.iter().any(|d| path.starts_with(d)) {
            for pattern in &sdk_patterns {
                assert!(!file.content().contains(pattern),
                    "SDK type {} found in non-adapter file: {}", pattern, path);
            }
        }
    }
}
```

## 9. 现状缺口与目标态

| # | 缺口 | 现状 | 目标态 | 关联 |
|---|---|---|---|---|
| 1 | UiEvent 持有 SDK 类型 | 14+ 个 UiEvent 变体直接持有 `sdk::*` 类型 | UiEvent 只持有 TUI 自有类型，第一层转换完成全部类型消除 | #797 |
| 2 | convert.rs 444 行手工 match | RuntimeStreamEvent → ChatEvent 手工转换，已有 5 处漂移 | Runtime 定义 → SDK re-export，删除 convert.rs | #795 §8 |
| 3 | 事件缺 agent_id | ChatEventContext 无 agent_id，sub-agent 事件无法区分来源 | ChatEventContext 加 agent_id，TUI 按 agent_id 路由 | #797 R8 |
| 4 | sub-agent 事件被聚合 | 子 agent 事件压缩为字符串，无实时细节 | AgentProgressKind 扩展 Text/ToolCallStart/ToolCallEnd，实时传递 | #612 |
| 5 | WorkingDirectoryChanged sync I/O | event_mapping.rs 同步调 git branch + worktree kind 子进程 | 移到 Effect 异步执行或加缓存 | #795 §10.5 |
| 6 | `_diagnostic` helper 死代码 | 无调用方 | 删除 | #795 §10.2 |
| 7 | subagent header 回调注入 | `map_agent_event_with_tool_header` 接受 `FnMut` 回调 | progress 格式化内聚在 progress.rs | #797 |
| 8 | 架构门禁 #6 缺失 | SDK 类型在 model/ 等非 adapter 目录可见 | 补齐门禁，SDK 类型只在 adapter + processing 出现 | #795 §9 |
| 9 | UiEvent::ReflectionDone / ReflectionApplyDone 死代码 | `#[allow(dead_code)]`，映射时静默丢弃 | 删除 | #795 §10.2 |
| 10 | ToolOutput progress 被忽略 | `AgentProgressKindView::ToolOutput` 返回 `AgentEventMapping::default()` | 目标态展示 sub-agent tool 输出摘要 | #612 |
| 11 | ContentBlock JSON round-trip | `serde_json::from_value(to_value(...))` | share 定义 → SDK re-export | #795 §8 |

## 10. 相关文档

- TUI 架构与数据流：[01-architecture-and-dataflow.md](01-architecture-and-dataflow.md)
- TUI Model 层设计：[02-model.md](02-model.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- SDK Published Language：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Server 模块（WssAgentClient 传输基础）：[../server/README.md](../server/README.md)
- sub-agent 调研 issue：#612

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：事件流完整链路、AgentEventMapper ACL、SDK DTO 边界、agent_id 缺口 R8、sub 事件路由 #612、转换集中化、架构门禁、现状缺口 11 项 | #797 |
