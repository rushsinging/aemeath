# TUI · Model 层设计

> 层级：02-modules / tui（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#796（S2）
> 本文定义 TUI Model 层的 3+1 Context 完整字段、投影状态机、单一真相规则与纯净性约束。Model 是 TEA 管线第④层，纯函数，不执行 IO。

## 1. 定位

Model 是 **TEA 管线的唯一可变状态载体**（第④层）：

- 接收 Coordinator 传递的 Intent，通过 `apply()` 更新内部状态并返回 Change
- **不执行 IO**——不调 `AgentClient`、不发 channel、不 `tokio::spawn`、不访问文件系统
- **不依赖 ratatui**——ViewModel / Render 才引用 ratatui 类型
- 持有全部 UI 状态：对话内容、输入缓冲、诊断信息、session 元数据
- **不持有领域权威态**——domain 态在 Runtime（AgentClient 侧），Model 只做投影

> **与 #795 的关系**：#795 §5 给出了 3+1 Context 概要与 Intent/Change 模式。本文深化到每个字段的完整定义、状态机转换图、子模块设计、纯度约束与目标态缺口。

## 2. TuiModel 根结构

```rust
struct TuiModel {
    conversation: ConversationModel,    // 对话 + Run 运行态
    input: InputModel,                  // 输入 buffer/cursor/selection/history
    diagnostic: DiagnosticModel,        // 错误/警告/提示/阻塞请求
    session: SessionModel,              // session metadata + resume + task 状态 + save
    config: ConfigProjection,           // provider / model_id（投影自 Config BC）
    workspace: WorkspaceProjection,     // cwd / worktree / path_base（投影自 WorkspaceService）
}
```

| 字段 | Context | 职责 | 投影来源 | Intent 风格 |
|---|---|---|---|---|
| `conversation` | Conversation | run/step 生命周期、tool call、timeline、RunRuntimeState、AskUser | Runtime AgentRun | struct-per-variant + trait dispatch |
| `input` | Input | buffer/cursor/selection/history/completion | 纯 UI | enum match |
| `diagnostic` | Diagnostic | 错误/警告/提示/阻塞请求 | Runtime / Hook 事件 | enum match |
| `session` | Session | session metadata、resume 候选、save 状态、task 状态 | StorageService / Task BC | enum match |
| `config` | Config | provider / model_id | Config BC | enum match |
| `workspace` | Workspace | cwd / worktree / path_base / branch | WorkspaceService | enum match |

**设计决策**：

1. RuntimeState **按投影来源拆分**——原 13 字段混了 4 个不同 BC 的投影。真正与 Run 生命周期耦合的 9 个字段留在 `RunRuntimeState`（内聚在 ConversationModel），Config 投影独立为 `ConfigProjection`，Workspace 投影独立为 `WorkspaceProjection`，`task_status` 移到 `SessionModel`（投影自 Task BC）。
2. AskUser **不拆出独立 Context**——同一时刻至多一个 AskUserBatch 块，交互状态内嵌在 OutputTimeline 中。
3. 六个 Context 之间 **无直接引用**——跨 Context 通信通过 Coordinator 在 `update()` 中拆分 Intent 并分别 apply。

## 3. ConversationModel

### 3.1 字段定义与可见性

```rust
struct ConversationModel {
    // ── 对话内容 ──
    pub chats: Vec<Chat>,                    // 目标态: runs: Vec<RunProjection>
    pub active_chat_id: Option<ChatId>,      // 目标态: active_run_id: Option<RunId>
    pub timeline: OutputTimelineModel,
    pub queued_submissions: Vec<QueuedSubmission>,
    pub agent_progress: Vec<AgentProgressEntry>,
    next_chat_sequence: usize,              // private
    next_block_sequence: usize,             // private
    revision: u64,                          // private — memo 版本号
    pub(super) active_text_block_id: Option<String>,
    pub(super) active_text_context: Option<(RunId, RunStepId)>,       // 现状: ChatId, ChatTurnId
    pub(super) active_thinking_block_id: Option<String>,
    pub(super) active_thinking_context: Option<(RunId, RunStepId)>,  // 现状: ChatId, ChatTurnId
    pub model_stream_placeholder: Option<ModelStreamWaitingView>,
    // ── 运行态（与 Run 生命周期耦合） ──
    pub run_runtime: RunRuntimeState,
}
```

| 字段 | 可见性 | 说明 |
|---|---|---|
| `chats` | pub | Run 投影列表（现状 `chats: Vec<Chat>`，目标态 `runs: Vec<RunProjection>`） |
| `active_chat_id` | pub | 当前活跃 run（现状 `active_chat_id`，目标态 `active_run_id`） |
| `timeline` | pub | 渲染用扁平时间线（与 chats 双重表示，见 §3.5） |
| `queued_submissions` | pub | 排队中的用户输入 |
| `agent_progress` | pub | sub-agent 进度条目 |
| `next_chat_sequence` / `next_block_sequence` | private | ID 序列号 |
| `revision` | private | 内容版本号，供渲染层 memo |
| `active_text_block_id` / `active_text_context` | pub(super) | 流式文本块追踪 |
| `active_thinking_block_id` / `active_thinking_context` | pub(super) | thinking 块追踪 |
| `model_stream_placeholder` | pub | model stream 等待占位 |
| `run_runtime` | pub | RunRuntimeState——与 Run 生命周期耦合的运行态（见 §3.6） |

### 3.2 Run 投影与 RunStatus 状态机

> **术语对齐**：TUI Model 是 Runtime 的投影层，统一使用 Runtime 领域语言。现状代码中 `Chat` / `ChatTurn` 对应 Runtime 的 `Run` / `RunStep`。本文使用目标态术语，现状代码名在括号内标注。

```rust
// 投影自 Runtime Run 聚合根（现状代码: Chat）
struct RunProjection {
    pub id: RunId,                        // 现状: ChatId
    pub user_submission: String,
    pub status: RunProjectionStatus,     // 现状: ChatStatus
    pub steps: Vec<RunStepProjection>,   // 现状: turns: Vec<ChatTurn>
}

// Run 状态机的 TUI 投影（简化自 Runtime RunStatus）
// Runtime 完整态见 runtime/03-loop-and-state-machine.md
enum RunProjectionStatus {
    Created,          // 初始态
    Running,          // 运行中（合并 PreparingContext/InvokingModel/ApplyingResponse/ExecutingTools）
    AwaitingUser,     // 暂停等待用户输入（ask_user / approval）
    Completing,       // 收到 CompleteChat，正在收尾
    Completed,        // 正常完成
    Failed,           // 异常终止
    Cancelled,        // 用户取消
}
```

> **投影简化规则**：Runtime `RunStatus` 有 11 个状态，TUI 投影合并为 7 个——细粒度展示由 `derive_spinner_phase()` 从 RunProjectionStatus + RunStepProjectionStatus 派生（Thinking / Generating / CallingTool 等），RunProjectionStatus 只需区分"运行中 / 等待用户 / 完成 / 失败 / 取消"。SpinnerPhase **NEVER** 自建状态机。

**RunProjectionStatus 状态转换图**：

```
Created ──StartRun──→ Running ──CompleteRun──→ Completing ──→ Completed
                         │                        │
                         ├──AbortRun──→ Failed     ├──异常──→ Failed
                         │
                         ├──AskUser──→ AwaitingUser ──Resume──→ Running
                         │
                         └──Cancel──→ Cancelled

ResumeConversation ──→ Completed（恢复已结束会话，不触发 spinner）
```

| 转换 | 触发 Intent | 方法 | 说明 |
|---|---|---|---|
| → Running | `StartChat` | `start_chat()` | 创建 Run + 初始 step |
| → Completed | `ResumeConversation` | `ensure_runtime_turn()` | 恢复历史会话，run 保持已完成态，不触发 spinner |
| Running → AwaitingUser | `ShowAskUserBatch` | `show_ask_user_batch()` | Runtime 进入 AwaitingUser 态 |
| AwaitingUser → Running | `ConfirmAskUserBatch` / `DismissAskUserBatch` | — | 用户应答后 resume |
| Running → Completing | `CompleteChat` | `complete_chat()` | 清理 active block 追踪 |
| → Failed | 异常 / AbortChat | — | Runtime 报告异常 |
| → Cancelled | 用户取消 | — | — |

> **已知 bug**：`ensure_runtime_turn()` 当前将 `chat.status` 设为 `Running`，但恢复的历史会话已结束，正确状态应为 `Completed`。目标态修正为 Completed。

> **术语迁移**：代码现状使用 `Chat` / `ChatId` / `ChatStatus` / `ChatTurn` / `ChatTurnId` / `ChatTurnStatus`，目标态统一改为 `Run` / `RunId` / `RunStatus` / `RunStep` / `RunStepId` / `RunStepStatus`（见 §10 缺口 #4）。

### 3.3 RunStep 投影与 RunStepStatus 状态机

```rust
// 投影自 Runtime RunStep 实体（现状代码: ChatTurn）
struct RunStepProjection {
    pub id: RunStepId,                    // 现状: ChatTurnId
    pub sequence: usize,
    pub status: RunStepProjectionStatus,  // 现状: ChatTurnStatus
    pub assistant_stream: String,
    pub tool_calls: Vec<ToolCall>,
}

// RunStep 状态机的 TUI 投影（简化自 Runtime RunStepStatus）
// Runtime 原始态: Pending/Invoking/Applying/ToolPhase/Done/Failed
enum RunStepProjectionStatus {
    Streaming,       // 对应 Runtime Invoking/Applying（assistant 文本流中）
    ToolCalling,     // 对应 Runtime ToolPhase 前期（参数收集中）
    ToolExecuting,   // 对应 Runtime ToolPhase 后期（执行中）
    Completing,      // 对应 Runtime Finishing
    Completed,       // 对应 Runtime Done
    Failed,          // 对应 Runtime Failed
}
```

**RunStepProjectionStatus 状态转换图**：

```
Streaming ──ToolCallStart──→ ToolExecuting
    │                           │
    │              ToolCallUpdate(PendingArgs/Ready)
    │                           ↓
    │                       ToolCalling
    │                           │
    │                  bind()/update(Running)
    │                           ↓
    │                       ToolExecuting
    │                           │
    └──CompleteBlock──→     all tools done
                               ↓
                           Completing ──→ Completed
```

| 转换 | 触发 | 方法 | 说明 |
|---|---|---|---|
| → Streaming | `RunStepProjection::new()`（现状 `ChatTurn::new()`） | — | 初始态 |
| → ToolExecuting | `ToolCallStart` | `observe_tool_start()` | push 占位 ToolCall |
| → ToolCalling | `ToolCallUpdate(PendingArgs/Ready)` | `update_tool()` | 参数就绪 |
| → ToolExecuting | `ToolCallUpdate(Running)` / `bind_tool()` | `update_tool()` / `bind_tool()` | 开始执行 |
| → Completing | 所有 tool_calls 终态 | `complete_tool()` | 全部终态检查 |

### 3.4 ToolCall 与 ToolCallStatus 状态机

```rust
struct ToolCall {
    pub id: Option<ToolCallId>,
    pub stream_key: ToolStreamKey,
    pub name: String,
    pub args_preview: String,
    pub status: ToolCallStatus,
    pub result: Option<ToolResultPayload>,
    pub activities: Vec<String>,
    pub streaming_preview: Option<ToolStreamingPreviewBuffer>,
    pub agent_meta: Option<AgentMeta>,
}
enum ToolCallStatus { PendingArgs, Ready, Running, Success, Error, Cancelled, Orphaned }
```

**ToolCallStatus 状态转换图**：

```
PendingArgs ──bind()──→ Running ──complete(ok)──→ Success
     │                      │
     │                 complete(err)──→ Error
     │
     └──orphan()──→ Orphaned
```

| 转换 | 方法 | 说明 |
|---|---|---|
| → PendingArgs | `pending()` | 创建占位（stream_key 已知，id 待绑） |
| → Running | `bind()` / `update(Running)` | 绑定 ToolCallId，开始执行 |
| → Success | `complete(payload, is_error=false)` | 执行成功 |
| → Error | `complete(payload, is_error=true)` | 执行失败 |
| → Orphaned | `orphan()` | provider 序号不匹配，从未被绑定 |

> **绑定策略**（#87 修复）：`bind_tool(id)` 按 internal ToolCallId 直接查找占位并绑定，不依赖 provider content-block 序号。跨轮 `index` 重复时不会覆盖已绑定占位。

### 3.5 OutputTimeline 双重表示

ConversationModel 维护两套对话表示：

| 表示 | 类型 | 用途 |
|---|---|---|
| 结构化 | `chats: Vec<Chat>`（目标态 `runs: Vec<RunProjection>`） | run/step/tool_call 层级结构，业务逻辑操作 |
| 扁平化 | `timeline: OutputTimelineModel` | 渲染用有序块列表，ViewAssembler 消费 |

**OutputTimelineItem 变体**：

| 变体 | 说明 |
|---|---|
| `UserMessage` | 用户消息块 |
| `QueuedUserMessage` | 排队中的用户消息 |
| `AssistantText` | assistant 文本块 |
| `ThinkingText` | thinking 块 |
| `ToolCall` | 工具调用块 |
| `ToolResult` | 工具结果块（强制跟在对应 ToolCall 之后） |
| `SystemMessage` | 系统消息 |
| `HookNotice` | Hook 通知 |
| `Error` | 错误消息 |
| `AskUserBatch` | AskUser 交互块（同一时刻至多一个） |
| `AgentProgress` | sub-agent 进度块 |

**一致性保证**：

- `revision` 版本号在每次产生 Change 的 `apply()` 时 `wrapping_add(1)`
- 渲染层以 `(conversation.revision(), workspace_root)` 为 cache key，不变时跳过全量重建
- `move_tool_result_after_tool_call` 强制 result 跟在对应 call 之后，处理流式事件乱序

> **已知缺口**（#795 §10.8）：chats 与 timeline 无 invariant 测试。目标态：每次 `start_chat` / `append_*` / `complete_chat` 后断言两者同步。

### 3.6 RunRuntimeState（Run 生命周期耦合运行态）

> **拆分原则**：原 RuntimeState 13 字段混了 4 个不同 BC 的投影。本节只保留与 Run 生命周期耦合的 9 个字段。Config 投影移到 §7 ConfigProjection，Workspace 投影移到 §8 WorkspaceProjection，task_status 移到 §6 SessionModel。

```rust
struct RunRuntimeState {
    spinner: SpinnerModel,
    thinking: bool,
    graph_phase: Option<String>,
    compact_progress: Option<CompactProgressModel>,
    processing_jobs: Vec<ProcessingJob>,
    usage: UsageSummary,
    live_tps: Option<f64>,
    status_notice: StatusNotice,          // 从 graph_phase 派生，半耦合
    transient_notice_expiry: Option<Instant>,
}
```

| 字段 | 投影来源 | 与 Run 生命周期耦合 |
|---|---|---|
| `spinner` | RunStatus + RunStepStatus 派生 | ✅ |
| `thinking` | RunSpec.reasoning_level | ✅ |
| `graph_phase` | Runtime workflow phase | ✅ |
| `compact_progress` | RunStatus::Compacting | ✅ |
| `processing_jobs` | Run 执行编排 | ✅ |
| `usage` | Runtime cost tracking | ✅ 每次 Run 累加 |
| `live_tps` | Runtime token 速率 | ✅ |
| `status_notice` | 从 graph_phase 派生 | ✅ 半耦合 |
| `transient_notice_expiry` | 纯 UI 组合态 | ✅ 半耦合 |

#### 3.6.1 SpinnerModel（派生自 Runtime 状态）

> **设计原则**：SpinnerPhase **NEVER** 自建独立状态机，**MUST** 从 `RunProjectionStatus` + `RunStepProjectionStatus` + 运行时上下文（tool_calls / hook 事件）**派生**。SpinnerModel 只存储派生所需的输入数据，phase 是纯函数输出。

**现状问题**：当前 `SpinnerPhase` 是独立状态机，由 `start_chat()` / `generate()` / `think()` / `start_tool_call()` 等方法驱动转换。这导致 spinner 状态与 Run 状态机脱耦——两套状态机需要手动同步，容易不一致。

**目标态**：SpinnerPhase 是派生函数，不再有独立状态转换。

```rust
/// Spinner 派生输入（存储在 RuntimeState 中，由 SDK 事件更新）
struct SpinnerModel {
    /// 运行中 tool call 的名称列表（从当前 RunStep 的 tool_calls 中
    /// status == Running | PendingArgs | Ready 的条目派生）
    active_tools: Vec<String>,
    /// 最近一次 hook 事件（由 HookExecuted intent 更新）
    last_hook: Option<HookSnapshot>,
    /// 最近一次 sub-agent 进度（由 AgentProgress intent 更新）
    last_agent_progress: Option<AgentProgressEntry>,
}

/// SpinnerPhase 是纯派生函数，不存储在 Model 中
fn derive_spinner_phase(
    run_status: RunProjectionStatus,
    step_status: Option<RunStepProjectionStatus>,
    spinner: &SpinnerModel,
) -> Option<SpinnerPhase> {
    match run_status {
        // 终态：无 spinner
        Completed | Failed | Cancelled | Created => None,

        // Compacting：直接映射
        // 注：RunProjectionStatus 当前合并了 Compacting 到 Running，
        // 需要补充 compact_progress 信号或从 Runtime 事件推导
        _ if spinner.compact_active() => Some(SpinnerPhase::Compacting),

        // Running 态：根据 step_status 和上下文细分
        Running => {
            match step_status {
                Some(ToolExecuting) | Some(ToolCalling) => {
                    let tools = &spinner.active_tools;
                    if tools.len() == 1 {
                        Some(SpinnerPhase::CallingTool(tools[0].clone()))
                    } else if tools.len() > 1 {
                        Some(SpinnerPhase::CallingTools { remaining: tools.len() })
                    } else {
                        Some(SpinnerPhase::CallingTools { remaining: 0 })
                    }
                }
                Some(Streaming) => {
                    // 有 agent progress 时显示 AgentWorking
                    if spinner.last_agent_progress.is_some() {
                        Some(SpinnerPhase::AgentWorking)
                    } else {
                        Some(SpinnerPhase::Generating)
                    }
                }
                _ => {
                    // Hook 执行中
                    if let Some(h) = &spinner.last_hook {
                        if h.is_running() {
                            return Some(SpinnerPhase::Hook { .. });
                        }
                    }
                    // 默认：等待首 token 或准备上下文
                    Some(SpinnerPhase::Thinking)
                }
            }
        }

        AwaitingUser => None,  // AskUser 交互期间不显示 spinner

        Completing => None,
    }
}
```

**SpinnerPhase 变体与 Runtime 状态的映射**：

| SpinnerPhase | 派生来源（Runtime 状态） | 说明 |
|---|---|---|
| `Thinking` | `RunStatus::PreparingContext` 或 `InvokingModel`（首 token 前） | 等待上下文准备 / 等待首 token |
| `Generating` | `RunStatus::InvokingModel`（收到 delta 后） + `RunStepStatus::Streaming` | 流式生成中 |
| `CallingTool(name)` | `RunStatus::ExecutingTools` + 1 个 tool `Running` | 单工具执行中 |
| `CallingTools { remaining }` | `RunStatus::ExecutingTools` + N 个 tool `Running` | 多工具并行执行 |
| `Compacting` | `RunStatus::Compacting` | 上下文压缩中 |
| `AgentWorking` | `RunStatus::InvokingModel` + sub-agent progress 事件 | sub-agent 工作中 |
| `Hook { event, detail, outcome }` | Hook 事件（非 RunStatus，由 HookPort 事件驱动） | Hook 执行中 |

> **chat_active 也可派生**：`chat_active = run_status ∈ {Running, AwaitingUser}`，**NEVER** 独立维护 bool 字段。当前代码中 `chat_active` 与 `RunProjectionStatus` 重复维护，是双重真相。

> **Reflecting 已移除**：当前代码中 `Reflecting` 变体没有对应的 Runtime 状态，是自己造的。目标态移除——如果需要区分 reasoning 模式的 spinner，应从 `RunSpec.reasoning_level` 派生，而非自建状态。

> **已知问题**（#795 §10.4）：spinner 状态三处同步。根因就是 SpinnerPhase 是独立状态机。改为派生函数后，根因消除——只有 `RunProjectionStatus` + `RunStepProjectionStatus` 一个真相源，spinner phase 自动跟随。

#### 3.6.2 UsageSummary

```rust
struct UsageSummary {
    input_tokens: u64, output_tokens: u64, last_input_tokens: u64,
    api_calls: u64, context_size: u64, cost_usd: f64,
}
```

| 方法 | 说明 |
|---|---|
| `record_usage(input, output, last_input, cost)` | 累加 token 与成本，返回 (input, output, cost) 元组 |
| `set_context_size(size)` | 设置 context window 大小 |
| `update_last_input_tokens(tokens)` | 更新最近一次 input token 数 |

#### 3.6.3 ProcessingJobTracker

```rust
struct ProcessingJob { id: String, chat_id: Option<String>, status: ProcessingStatus }
enum ProcessingStatus { Running, Finished, Failed }
```

| 方法 | 说明 |
|---|---|
| `start_processing_job(id, chat_id)` | 添加 Running job |
| `finish_processing_job(id, success)` | 标记 Finished / Failed |

#### 3.6.4 CompactProgressModel

```rust
struct CompactProgressModel { stage: String, current: Option<u32>, total: Option<u32> }
```

| 方法 | 说明 |
|---|---|
| `set_compact_progress(stage, current, total)` | 设置进度 + 调用 `start_compact()` 激活 spinner |
| `clear_compact_runtime()` | 清空 compact_progress + running_tool_count 归零（不碰 phase/chat_active） |

#### 3.6.5 StatusNotice

| 方法 | 说明 |
|---|---|
| `set_status_notice(notice)` | 设置持久 notice，清空 expiry |
| `set_transient_status_notice(notice, expires_at)` | 设置临时 notice + 过期时间 |
| `set_graph_phase(phase)` | 设置 graph_phase，同步派生 notice（当无临时 notice 时） |
| `expire_transient_notice(now)` | 检查过期，回退到 graph_phase 派生的持久态 |
| `notice_from_phase(phase)` | idle→"Ready"，其他→phase 文案 |

#### 3.6.6 AskUserState

AskUser 交互块**内嵌在 OutputTimeline** 中（`OutputTimelineItem::AskUserBatch`），同一时刻至多一个，固定 id `"ask-user"`。

```
AskUserPhase: Answering → Confirming → Confirmed
```

**AskUser 状态转换图**：

```
                    ┌─ 多题末题答完 ─→ Confirming ──ConfirmAskUserBatch──→ Confirmed
Answering (idx 0..N)│                    │
    │                │              NavigateAskUserTo(i)
    │                └─── 跳回某题 ──→ Answering(idx=i)
    └─ 单题直接答完 ─→ Confirmed（跳过 Confirming）
```

| 方法 | 说明 |
|---|---|
| `show_ask_user_batch(slots)` | 显示交互块（替换已存在的） |
| `answer_current_ask_user(answer)` | 回答当前问题，自动前进或进入确认页 |
| `navigate_ask_user_to(index)` | 确认页跳回某题重新作答 |
| `set_ask_user_cursor(cursor)` | 选项光标（越界夹取） |
| `toggle_ask_user_selected(index)` | 切换选项勾选（仅 LLM 选项可勾） |
| `set_ask_user_chat_input(active)` | 切换 Type something 子态 |
| `append/delete/move_ask_user_chat_*` | Type something 输入框编辑 |
| `set_ask_user_confirm_cursor(cursor)` | 确认页导航光标（0..=N+1） |
| `confirm_ask_user_batch()` | 确认提交，进入终态 |
| `dismiss_ask_user_batch()` | 移除交互块 |
| `ask_user_snapshot()` | 读取当前交互状态快照（供控制器读取） |

### 3.7 Intent / Change / Update 模式

Conversation 采用 **struct-per-variant + trait dispatch**：

```rust
trait ConversationUpdate {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange>;
}
```

每个 Intent 是独立 struct，`impl ConversationUpdate` 逻辑在 `intent_impls.rs`。`ConversationIntent` enum 仅做传输容器，含 48 个 variant（27 conversation + 14 runtime + 7 ask_user）。

**ConversationChange** 含 30+ variant，覆盖：
- 对话生命周期：`ChatStarted` / `ChatTurnStarted` / `ChatCompleting` / `ChatCompleted`（目标态：`RunStarted` / `RunStepStarted` / `RunCompleting` / `RunCompleted`）
- 内容追加：`UserMessageAppended` / `AssistantTextAppended` / `ThinkingTextAppended` / `SystemMessageAppended` / `ErrorAppended`
- 工具追踪：`ToolCallObserved` / `ToolCallBound` / `ToolCallCompleted` / `OrphanToolResultObserved`
- 排队：`QueuedSubmissionAdded` / `QueuedSubmissionsCleared`
- Agent 进度：`AgentProgressRecorded` / `AgentMetaUpdated`
- AskUser：`AskUserShown` / `AskUserUpdated` / `AskUserDismissed`
- 运行态：`UsageChanged` / `LiveTpsChanged` / `ProcessingJobChanged` / `CompactProgressChanged` / `SpinnerPhaseChanged` / `SpinnerStopped` / `StatusNoticeChanged` / `ThinkingChanged` / `GraphPhaseChanged`
- 配置态：`ProviderModelChanged`（→ ConfigProjection）/ `WorkspaceChanged`（→ WorkspaceProjection）/ `TaskStatusChanged`（→ SessionModel）——目标态移出 Conversation
- 脏标记：`OutputDirty` / `StyleBoundaryResetRequired`

> **已知不一致**（#795 §5.4）：Conversation 用 struct-per-variant + trait dispatch，其他三个用 enum match。后续统一（见 §10）。

### 3.8 revision memo 机制

```rust
impl ConversationModel {
    pub fn apply<U: ConversationUpdate>(&mut self, update: U) -> Vec<ConversationChange> {
        let changes = update.update(self);
        if !changes.is_empty() {
            self.revision = self.revision.wrapping_add(1);
        }
        changes
    }
    pub fn revision(&self) -> u64 { self.revision }
}
```

- 每次 `apply()` 产生非空 Change 时 `revision += 1`
- no-op apply（空 Change）不增 revision
- 渲染层以 `(revision, workspace_root)` 为 cache key，不变时跳过全量 `assemble_from_conversation`

## 4. InputModel

### 4.1 字段定义

```rust
struct InputModel {
    pub document: InputDocument,
    pub history: InputHistory,
    pub completion: InputCompletion,
    pub mode: InputMode,
}
```

| 字段 | 类型 | 职责 |
|---|---|---|
| `document` | InputDocument | buffer / cursor / image_spans / copied_text_spans |
| `history` | InputHistory | 输入历史 / selected_index / saved_input |
| `completion` | InputCompletion | 补全 items / visible / selected_index / query |
| `mode` | InputMode | Normal / Completion |

### 4.2 InputDocument

```rust
struct InputDocument {
    pub buffer: String,
    pub cursor: usize,
    // image_spans: Vec<ImageSpan>      — 图片占位 span
    // copied_text_spans: Vec<Span>     — 复制标记 span
}
```

| 方法 | 说明 |
|---|---|
| `insert_text(text)` / `insert_pasted_text(text)` | 插入文本（清补全、清历史选中） |
| `replace_text(text)` | 全量替换 |
| `delete_backward()` / `delete_forward()` / `delete_word_before_cursor()` | 删除 |
| `move_cursor(cursor)` / `move_left/right/home/end/up/down()` | 光标移动 |
| `insert_image(image)` | 插入图片 span |
| `submit_text()` / `display_text()` / `drain_images()` | 提交时调用 |
| `clear()` | 清空 |

> **span 模型**：`copied_text_spans` + `image_spans` 在 buffer 中维护位置区间，编辑/删除时统一位移。提交时 `submit_text()` 按原始 buffer 位置还原 placeholder（#507 修复：image placeholder 保留不剔除）。

### 4.3 InputCompletion

```rust
struct InputCompletion {
    pub items: Vec<CompletionItem>,
    pub visible: bool,
    pub selected_index: Option<usize>,
    pub query: String,
}
```

补全触发类型：

| TriggerType | 说明 |
|---|---|
| `AtSymbol` | `@` 触发（model 补全） |
| `SlashCommand` | `/` 触发（命令补全） |
| `ModelArg` | model 参数补全 |
| `ModelSubCommand` | model 子命令补全 |
| `ResumeArg` | `/resume` 参数补全 |

`SuggestionType::Session` 有特殊替换逻辑：从 replacement 中提取 id，拼接 `/resume {id}`。

### 4.4 InputHistory

```rust
struct InputHistory {
    pub entries: Vec<String>,
    pub selected_index: Option<usize>,
    pub saved_input: String,
}
```

- `MoveCursorUp` 在第一行时触发 `history_previous()`
- `MoveCursorDown` 在最后一行时触发 `history_next()`
- 选中历史时 `saved_input` 保存当前输入，退出历史时恢复

### 4.5 InputMode

```rust
enum InputMode { Normal, Completion }
```

- `SetCompletions` 根据补全可见性自动切换 mode
- `AcceptCompletion` / `Submit` / `Clear` 回到 Normal

### 4.6 Intent / Change

```rust
enum InputIntent {
    InsertChar(char), InsertText(String), InsertPastedText(String), ReplaceText(String),
    MoveCursor(/*cursor*/), MoveCursorLeft/Right/Home/End/Up/Down,
    InsertNewline, DeleteBackward, DeleteWordBeforeCursor, DeleteForward,
    MoveHistoryPrevious, MoveHistoryNext, ReplaceHistory(Vec<String>),
    SetCompletions { query, items }, SelectCompletionNext, SelectCompletionPrevious,
    AcceptCompletion, AcceptCompletionValue(String),
    InsertImage(image), SetMode(InputMode),
    Submit, Clear,
}
enum InputChange {
    TextChanged { text, cursor },
    CursorMoved { cursor },
    CompletionChanged { visible, selected_index, items },
    ModeChanged { mode },
    HistorySelected { text, cursor },
    Submitted { submission },
    Cleared,
}
```

## 5. DiagnosticModel

### 5.1 字段定义

```rust
struct DiagnosticModel {
    pub notices: Vec<DiagnosticNotice>,
    pub active_prompt: Option<ActivePrompt>,
    next_notice_id: usize,  // private
}
```

### 5.2 DiagnosticNotice

```rust
struct DiagnosticNotice { pub id: String, pub severity: DiagnosticSeverity, pub message: String }
enum DiagnosticSeverity { Error, Warning, Info }
```

### 5.3 ActivePrompt

```rust
struct ActivePrompt { pub id: String, pub question: String }
```

同一时刻至多一个 `active_prompt`。`AnswerPrompt` 清空 `active_prompt` 并返回答案。

### 5.4 Intent / Change

```rust
enum DiagnosticIntent {
    RecordNotice { severity, message },
    OpenPrompt { id, question },
    AnswerPrompt { answer },
    DismissNotice { id },
}
enum DiagnosticChange {
    NoticeRecorded { id, severity },
    PromptOpened { id },
    PromptAnswered { answer },
    NoticeDismissed { id },
}
```

| 方法 | 说明 |
|---|---|
| `highest_severity()` | 返回当前最高级别（Error > Warning > Info），空列表返回 None |

## 6. SessionModel

### 6.1 字段定义

```rust
struct SessionModel {
    pub current_session_id: Option<String>,
    pub dirty: bool,
    pub message_count: usize,
    pub resume_candidates: Vec<SessionResumeCandidate>,
    pub save_status: SessionSaveStatus,
    pub task_status: TaskStatusSnapshot,   // 从 RunRuntimeState 迁入——投影自 Task BC
}
```

### 6.2 TaskStatusSnapshot

```rust
struct TaskStatusSnapshot {
    total: usize, completed: usize, in_progress: usize, lines: Vec<String>,
}
```

| 方法 | 说明 |
|---|---|
| `set_task_status(total, completed, in_progress)` | 更新计数（保留 lines） |
| `set_task_lines(lines)` | 更新展示行 |

> **迁移说明**：`task_status` 原在 `RuntimeState` 中，但它的投影来源是 Task BC，与 Run 生命周期无耦合。移到 SessionModel 因为 Task 与 Session 关联（每个 session 有独立的 task 列表）。

### 6.3 SessionSaveStatus 状态机

```rust
enum SessionSaveStatus { Idle, Saving, Saved, Failed { message: String } }
```

```
Idle ──SaveStarted──→ Saving ──SaveFinished──→ Saved（dirty=false）
                          │
                          └──SaveFailed──→ Failed { message }
```

### 6.4 SessionResumeCandidate

```rust
struct SessionResumeCandidate { pub id: String, /* display fields */ }
```

### 6.5 Intent / Change

```rust
enum SessionIntent {
    SetCurrentSession { id },
    MarkDirty,
    MessagesSynced { message_count },
    SaveStarted, SaveFinished, SaveFailed { message },
    ResumeCandidatesLoaded { candidates },
    SetTaskStatus { total, completed, in_progress },
    SetTaskLines { lines },
}
enum SessionChange {
    CurrentSessionChanged { id },
    DirtyChanged { dirty },
    MessagesSynced { message_count },
    SaveStatusChanged { status },
    ResumeCandidatesChanged { candidates },
    TaskStatusChanged,
}
```

| 转换 | 说明 |
|---|---|
| `MessagesSynced` | 设置 message_count + 清 dirty |
| `SaveFinished` | 设 Saved + 清 dirty |
| `SaveFailed` | 设 Failed { message } |

## 7. ConfigProjection

> **投影来源**：Config BC（`ConfigSnapshot`）。provider / model_id 有独立更新路径——用户切 model / 切 provider 时不经过 Run 生命周期，原 RuntimeState 中这两个字段与 Run 无耦合。

```rust
struct ConfigProjection {
    pub provider: Option<String>,
    pub model_id: Option<String>,
}
```

| 方法 | 说明 |
|---|---|
| `set_provider_model(provider, model_id)` | 设置 provider 和 model_id |

### 7.1 Intent / Change

```rust
enum ConfigIntent {
    ProviderModelChanged { provider, model_id },
}
enum ConfigChange {
    ProviderModelChanged,
}
```

> **迁移说明**：`provider` / `model_id` 原在 `RuntimeState` 中。移到独立 `ConfigProjection` 后，更新路径清晰——用户切 model 只更新 ConfigProjection，不触发 Conversation 的 revision 增长。

## 8. WorkspaceProjection

> **投影来源**：WorkspaceService（Project BC）。cwd / worktree / path_base 有独立更新路径——进/出 worktree 时不经过 Run 生命周期。

```rust
struct WorkspaceProjection {
    pub cwd: Option<String>,
    pub worktree: Option<String>,
    pub path_base: Option<String>,
    pub workspace_root: Option<String>,
    pub branch: Option<String>,
    pub kind: WorktreeKind,
}
enum WorktreeKind { Unknown, MainCheckout, LinkedWorktree }
```

| 方法 | 说明 |
|---|---|
| `update_workspace(cwd, worktree)` | 设置 cwd 和 worktree |
| `set_workspace_snapshot(path_base, root, branch, kind)` | 设置完整工作区快照 |

### 8.1 Intent / Change

```rust
enum WorkspaceIntent {
    UpdateWorkspace { cwd, worktree },
    SetWorkspaceSnapshot { path_base, root, branch, kind },
}
enum WorkspaceChange {
    WorkspaceChanged,
}
```

> **迁移说明**：`workspace` 原在 `RuntimeState` 中。移到独立 `WorkspaceProjection` 后，进/出 worktree 只更新 WorkspaceProjection，不影响 Conversation revision。

## 9. 投影状态机规则

### 9.1 非领域权威声明

Model 中的所有状态机都是**投影状态机**，不是领域权威态：

| 状态机 | 权威位置 | Model 中的角色 |
|---|---|---|
| RunProjectionStatus（现状 `ChatStatus`） | Runtime `RunStatus` | 投影——从 SDK 事件推导，简化合并 11 态为 7 态 |
| RunStepProjectionStatus（现状 `ChatTurnStatus`） | Runtime `RunStepStatus` | 投影——从 SDK 事件推导 |
| ToolCallStatus | Runtime ToolCallStatus | 投影——从 SDK 事件推导 |
| SpinnerPhase | 派生——从 run/tool 生命周期 | Model 内部推导 |
| AskUserPhase | Runtime（AskUserQuestion tool） | 投影——从 SDK 事件 + 用户输入推导 |
| SessionSaveStatus | Runtime StorageService | 投影——从 SDK 事件推导 |

**规则**：

1. **MUST** Model 状态机的转换**只能**由 Intent 触发，**NEVER** 由 Model 自行轮询或定时器驱动。
2. **MUST** 状态机转换产生 Change，Coordinator 消费 Change 决定是否生成 Effect。
3. **MUST** 当 SDK 事件与 Model 状态不一致时，**以 SDK 事件为准**——Model 是投影，不是权威。
4. **NEVER** 在 Model 中维护 Runtime 不存在的状态——避免幻觉态。

### 9.2 状态终态保护

- `ToolCallStatus::Success` / `Error` 为终态，`update()` 中 **MUST NOT** 覆盖已终态的 ToolCall
- `RunProjectionStatus::Completed` / `Failed` / `Cancelled`（现状 `ChatStatus`）为终态
- `AskUserPhase::Confirmed` 为终态（block 等待 dismiss）

## 10. 单一真相规则

### 10.1 domain 态属 AgentClient

以下状态**MUST**只在 Runtime（AgentClient 侧）维护，Model 只做只读投影：

- AgentRun 生命周期（Running / Paused / Aborted / Completed）
- Message 列表（权威对话历史）
- Tool 执行结果（权威 payload）
- Context window token 计数
- Permission 决策

### 10.2 UI 态只在 Model

以下状态**MUST**只在 Model 维护，**NEVER**在 ViewAssembler / ViewState / Render 中独立持有：

- SpinnerPhase 派生输入（`active_tools` / `last_hook` / `last_agent_progress`）——phase 本身是纯函数派生，不存储
- InputMode（Normal / Completion）
- AskUserPhase（Answering / Confirming / Confirmed）
- AskUser 交互快照（cursor / selected / chat_input）
- OutputTimeline 块顺序
- DiagnosticNotice 列表
- SessionResumeCandidate 列表

### 10.3 禁止双重真相

| 状态 | 真相源 | 禁止 |
|---|---|---|
| spinner 可见性 | `RunProjectionStatus ∈ {Running, AwaitingUser}`（派生） | **NEVER** 在 spinner model 或 view_state 独立维护 `chat_active` bool |
| spinner phase | `derive_spinner_phase(run_status, step_status, spinner)` 纯函数派生 | **NEVER** 在 model 存储独立 phase 状态机或 view_state 维护业务 phase |
| input buffer | `model.input.document.buffer` | **NEVER** 在 render 层维护独立缓冲 |
| active prompt | `model.diagnostic.active_prompt` | **NEVER** 在 view_state 维护 prompt 副本 |

> **已知违规**（#795 §10.4）：spinner 状态三处同步。目标态：`model.conversation.runtime.spinner` 为唯一来源，`view_state` 只存 `spinner_frame`（动画帧）。

## 11. Model 纯净性约束

### 11.1 禁止依赖

Model 层 `MUST NOT` import 以下 crate：

| 禁止依赖 | 理由 |
|---|---|
| `ratatui` | 渲染框架——Model 不产出渲染类型 |
| `tokio` | 异步运行时——Model 不执行 async 操作 |
| `std::process::Command` | 子进程——Model 不执行 IO |
| `crate::tui::render::*` | 渲染层——方向反了 |
| `sdk::AgentClient` trait | 出站端口接口——Model 不直接调 Runtime，Controller 通过依赖注入选择适配器（见 §13） |

### 11.2 禁止操作

| 禁止操作 | 理由 |
|---|---|
| `tokio::spawn` | 副作用——通过 Effect 描述 |
| `.await` | async——Model 是同步纯函数 |
| `Command::new` | 子进程——通过 Effect |
| channel send/recv | 通信——通过 Effect |
| `AgentClient::*` 调用 | Runtime 调用——通过 Effect |

### 11.3 允许依赖

| 允许依赖 | 用途 |
|---|---|
| `sdk`（DTO 类型） | ChatEvent / ChatMessage / ContentBlock 等只读类型 |
| `share`（共享内核） | ContentBlock / InputId 等基础类型 |
| `std` | 基础类型 |

### 11.4 目标态 vs 现状

| 约束 | 现状 | 目标态 | 关联 |
|---|---|---|---|
| Model purity arch test | ❌ 缺失 | 补齐（#795 §9 门禁 #2） | #795 |
| reducer 纯化 | `root_reducer` 直接调 `runtime.start_chat()` 等副作用 | reducer 只产出 Change，Coordinator 消费 Change 生成 Effect | #795 §10.1 |
| RuntimeState 字段私有化 | 全 `pub` | 只经业务方法操作 | #795 §10.1 |
| Intent 风格统一 | Conversation 用 struct-per-variant，其他用 enum | 统一（后续 issue 决策） | #795 §10.9 |

## 12. 现状缺口与目标态

| # | 缺口 | 现状 | 目标态 | 关联 |
|---|---|---|---|---|
| 1 | reducer 副作用 | `root_reducer` 直接调 runtime 方法 | Change → Coordinator → Effect | #795 §10.1 |
| 2 | spinner 独立状态机 | SpinnerPhase 有 `start_chat()` / `generate()` 等自驱动转换，与 RunStatus 脱耦 | 改为 `derive_spinner_phase()` 纯函数，从 RunProjectionStatus + RunStepProjectionStatus 派生 | #795 §10.4 |
| 3 | chats/timeline 无 invariant 测试 | 无 | 每次 append/complete 后断言同步 | #795 §10.8 |
| 4 | Intent 风格不一致 | Conversation struct-per-variant，其他 enum | 统一 | #795 §10.9 |
| 5 | Model purity arch test | 缺失 | 补齐门禁 #2 | #795 §9 |
| 6 | RuntimeState 字段全 pub | TODO 标注 | 逐步私有化 | #795 §10.1 |
| 7 | `model.rs` / `update.rs` / `effect.rs` / `view_state.rs` 的 `#![allow(dead_code)]` | 遮蔽真实死代码 | 移除 allow，逐个清理 | #795 §10.2 |
| 8 | `ensure_runtime_turn()` 将 chat.status 设为 Running | 恢复历史会话后应为 Completed | 修正为 Completed | #796 |
| 9 | 术语未对齐 Runtime 统一语言 | 代码用 `Chat`/`ChatTurn`/`ChatStatus`/`ChatTurnStatus` | 改为 `Run`/`RunStep`/`RunStatus`/`RunStepStatus`，TUI 投影类型加 `*Projection` 后缀 | #796 |
| 10 | RuntimeState 杂物箱混 4 个 BC 投影 | provider/model_id（Config）、workspace（Project）、task_status（Task）混在 RuntimeState 中 | 拆为 RunRuntimeState（9 字段）+ ConfigProjection + WorkspaceProjection，task_status 移到 SessionModel | #796 |
| 11 | TUI→Runtime 绑死 LocalAgentClient | Controller 直接依赖 `AgentClientImpl`，无端口适配器抽象 | 抽 `AgentClient` trait 为端口接口，实现 `LocalAgentClient`（现状）+ `WssAgentClient`（#794），组合根注入 | #796 |

## 13. 出站端口适配器

> **设计原则**：TUI → Runtime 的通信 **MUST NOT** 绑死在单一 trait 实现上。`AgentClient` 是端口接口（trait），**MUST** 支持多个适配器实现——本地直连和 WSS 远程连接。

### 13.1 端口定义

```rust
/// TUI 出站端口（SDK 定义）
/// TUI 通过此端口与 Runtime 通信，不关心 Runtime 在本地还是远端
trait AgentClient: Send + Sync {
    async fn start_chat(&self, req: ChatRequest) -> Result<ChatStream>;
    async fn cancel_chat(&self, chat_id: ChatId) -> Result<()>;
    async fn submit_tool_approval(&self, ...) -> Result<()>;
    async fn submit_ask_user_answer(&self, ...) -> Result<()>;
    async fn execute_slash_command(&self, ...) -> Result<...>;
    // ...
}
```

### 13.2 适配器实现

| 适配器 | 场景 | 传输方式 | 状态 |
|---|---|---|---|
| `LocalAgentClient` | TUI 与 Runtime 同进程（现状） | 直接函数调用 + channel | ✅ 已实现 |
| `WssAgentClient` | TUI 通过 Server 远程连接 Runtime（#794） | WebSocket Secure 帧 | ❌ 待实现（#794 启动后） |

```
┌─ TUI ─────────────────────────────────┐
│  Controller / Effect Handler          │
│  依赖 trait AgentClient（端口接口）     │
└──────────┬────────────────────────────┘
           │
     ┌─────┴─────┐
     │ 依赖注入   │
     └─────┬─────┘
           │
    ┌──────┴──────┐
    │             │
    ▼             ▼
┌──────────┐  ┌──────────────┐
│ Local    │  │ Wss          │
│ Agent    │  │ Agent        │
│ Client   │  │ Client       │
│ (直连)   │  │ (WSS 远程)   │
└────┬─────┘  └──────┬───────┘
     │               │ WebSocket Frame
     │ channel       │ (Call/Resp 协议)
     ▼               ▼
┌──────────┐  ┌──────────────┐
│ Runtime  │  │ Server       │
│ (同进程) │  │ (控制面+worker)│
└──────────┘  └──────────────┘
```

### 13.3 协议契约

无论本地还是远程，TUI 与 Runtime 之间的通信 **MUST** 遵循同一套协议契约（SDK Published Language）：

- **请求**：`ChatRequest` / `CancelRequest` / `ApprovalRequest` / `AskUserAnswer` / `SlashCommand`
- **响应流**：`ChatStream`（`Stream<Item = ChatEvent>`）
- **事件类型**：`ChatEvent` 枚举（TextDelta / ToolCallStart / ToolCallUpdate / CompleteChat 等）

`LocalAgentClient` 直接传递内存对象；`WssAgentClient` 序列化为 `Call`/`Resp` 帧传输（协议定义见 [07-server-design.md](../../07-server-design.md)），但 **MUST** 保证两端看到的 DTO 类型一致。

### 13.4 设计约束

1. **MUST** Controller / Effect Handler 只依赖 `AgentClient` trait，**NEVER** 直接依赖 `LocalAgentClient` 或 `WssAgentClient` 具体类型。
2. **MUST** 适配器选择在组合根（composition root）通过依赖注入完成，**NEVER** 在 TUI 业务代码中硬编码。
3. **MUST** `WssAgentClient` 的 `ChatStream` 实现与 `LocalAgentClient` 行为一致——同样的事件顺序、同样的错误语义。远程断连 **MUST** 映射为 `ChatEvent::Error`，**NEVER** panic。
4. **SHOULD** `WssAgentClient` 支持自动重连 + 断线期间事件缓冲，避免网络抖动导致 UI 状态丢失。
5. **MUST** Model 层 **NEVER** 知道当前使用哪个适配器——端口透明性是投影层的前提。

> **与 #794 的关系**：Server 模块（#794）的 `WsProxy` 双向透传 + `Call`/`Resp` 帧协议是 `WssAgentClient` 的传输基础。#794 启动后同步实现 `WssAgentClient`。

## 14. 相关文档

- TUI 架构与数据流：[01-architecture-and-dataflow.md](01-architecture-and-dataflow.md)
- 原始 TUI 设计（历史归档）：[../../04-tui-design.md](../../04-tui-design.md)
- Runtime 端口（AgentClient = TUI 出站端口）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Server 模块（WssAgentClient 传输基础）：[../server/README.md](../server/README.md)
- Server 设计草案（Call/Resp 帧协议）：[../../07-server-design.md](../../07-server-design.md)
- SDK Published Language：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 统一语言（TUI/TEA/Context）：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：3+1 Context 完整字段、ChatStatus/ChatTurnStatus/ToolCallStatus/SpinnerPhase/AskUserPhase 状态机、RuntimeState 8 子模块、单一真相规则、Model 纯净性约束、现状缺口 | #796 |
| 2026-07-12 | 术语对齐 Runtime 统一语言：Chat→Run、ChatTurn→RunStep、ChatStatus→RunProjectionStatus、ChatTurnStatus→RunStepProjectionStatus；新增 AwaitingUser 投影态；补充术语迁移缺口 #9 | #796 |
| 2026-07-12 | SpinnerPhase 从独立状态机改为派生函数：`derive_spinner_phase(run_status, step_status, spinner)`，映射表标注每个变体的 Runtime 派生来源；移除 Reflecting（无对应 Runtime 状态）；chat_active 改为派生 | #796 |
| 2026-07-12 | RuntimeState 按投影来源拆分：RunRuntimeState（9 字段，Run 耦合）+ ConfigProjection（provider/model_id，Config BC）+ WorkspaceProjection（cwd/worktree，Project BC），task_status 移到 SessionModel（Task BC）；3+1→3+3 Context | #796 |
| 2026-07-12 | 新增 §13 出站端口适配器：AgentClient trait 为端口接口，LocalAgentClient（直连）+ WssAgentClient（远程，#794）；Controller 依赖 trait 不绑死实现，组合根注入 | #796 |
