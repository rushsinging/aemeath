# TUI · Model 层设计

> 层级：02-modules / tui（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#796（S2）
> 本文定义 TUI Model 层六个 Context（Conversation / Input / Diagnostic / Session / Config / Workspace）的完整字段、投影状态机、单一真相规则与纯净性约束。Model 是 TEA 管线第④层，纯函数，不执行 IO。

## 1. 定位

Model 是 **TEA 管线中 UI 业务投影的唯一可变状态载体**（第④层）：

- 接收 Coordinator 传递的 Intent，通过 `apply()` 更新内部状态并返回 Change
- **不执行 IO**——不调 `AgentClient`、不发 channel、不 `tokio::spawn`、不访问文件系统
- **不依赖 ratatui**——ViewModel / Render 才引用 ratatui 类型
- 持有全部 UI 状态：对话内容、输入缓冲、诊断信息、session 元数据
- **不持有领域权威态**——domain 态在 Runtime（AgentClient 侧），Model 只做投影

`ViewState` 可以持有 scroll、selection、collapse 与 animation frame 等瞬时交互 / 渲染状态，但 **NEVER** 复制 Run、Interaction、输入内容或其他业务投影事实；Cache 只持可丢弃的派生结果。

> **与 #795 的关系**：#795 §5 只给出六 Context 概要与 Intent / Change 规则；本文是字段、状态机、子模块与纯度约束的唯一战术真相。

## 2. TuiModel 根结构

```rust
struct TuiModel {
    conversation: ConversationModel,    // 对话 + Run 运行态
    input: InputModel,                  // 输入 buffer/cursor/selection/history
    diagnostic: DiagnosticModel,        // 错误/警告/提示/阻塞请求
    session: SessionModel,              // session metadata + resume + task 状态 + save
    config: ConfigProjection,           // provider / model_id（投影自 Config BC）
    workspace: WorkspaceProjection,     // TUI-owned snapshot + 异步 metadata 投影
}
```

| 字段 | Context | 职责 | 投影来源 | Intent 风格 |
|---|---|---|---|---|
| `conversation` | Conversation | run/step 生命周期、tool call、timeline、RunRuntimeState、Interaction | Runtime AgentRun | struct-per-variant + trait dispatch |
| `input` | Input | buffer/cursor/selection/history/completion | 纯 UI | enum match |
| `diagnostic` | Diagnostic | 错误/警告/提示/阻塞请求 | Runtime / Hook 事件 | enum match |
| `session` | Session | session metadata、resume 候选、save 状态、task 状态 | StorageService / Task BC | enum match |
| `config` | Config | provider / model_id | Config BC | enum match |
| `workspace` | Workspace | path_base / workspace_root / context_stack / branch / worktree kind | TUI ACL 对 `WorkingDirectoryChanged` 的转换结果 | enum match |

**设计决策**：

1. Runtime 事件按投影来源进入不同 Context：Run 生命周期字段内聚在 `RunRuntimeState`，Config 投影属于 `ConfigProjection`，Workspace 投影属于 `WorkspaceProjection`，Task 投影属于 `SessionModel`。
2. Interaction **不拆出独立 Context**——同一时刻至多一个 Interaction 块，交互状态内嵌在 OutputTimeline 中；四种 body 共享 request identity 与生命周期，但各自保留 typed draft / reply。
3. 六个 Context 之间 **无直接引用**——跨 Context 通信通过 Coordinator 在 `update()` 中拆分 Intent 并分别 apply。
4. 六个 Context 的核心字段全部私有；ViewAssembler / key translator 只能通过不可变 accessor 或只读 projection view 读取。只有 root reducer 可持有 `&mut TuiModel` 并调用 crate-private mutation facade，架构守卫禁止其他模块调用 `apply` / `reduce_*`。

```rust
impl TuiModel {
    pub(crate) fn conversation(&self) -> &ConversationModel { &self.conversation }
    pub(crate) fn input(&self) -> &InputModel { &self.input }
    pub(crate) fn diagnostic(&self) -> &DiagnosticModel { &self.diagnostic }
    pub(crate) fn session(&self) -> &SessionModel { &self.session }
    pub(crate) fn config(&self) -> &ConfigProjection { &self.config }
    pub(crate) fn workspace(&self) -> &WorkspaceProjection { &self.workspace }
}
```

每个 Context **MUST** 再把内部 collection / value 暴露为不可变 slice、value copy 或专用 read projection；**NEVER** 从 accessor 返回 `&mut`、interior-mutability handle 或可绕过 reducer 的 command object。

## 3. ConversationModel

### 3.1 字段定义与可见性

```rust
struct ConversationModel {
    // ── 对话内容 ──
    runs: Vec<RunProjection>,
    active_run_id: Option<RunId>,
    timeline: OutputTimelineModel,
    queued_submissions: Vec<QueuedSubmission>,
    agent_progress: Vec<AgentProgressEntry>,
    next_run_sequence: usize,
    next_block_sequence: usize,
    revision: u64,
    active_text_block_id: Option<String>,
    active_text_context: Option<(RunId, RunStepId)>,
    active_thinking_block_id: Option<String>,
    active_thinking_context: Option<(RunId, RunStepId)>,
    model_stream_placeholder: Option<ModelStreamWaitingView>,
    // ── 运行态（与 Run 生命周期耦合） ──
    run_runtime: RunRuntimeState,
}
```

| 字段 | 可见性 | 说明 |
|---|---|---|
| `runs` / `active_run_id` | private | Run / RunStep / Tool 的结构化投影；只读 accessor 对 ViewAssembler 开放 |
| `timeline` | private | 有序展示与交互投影；只读 accessor 对 ViewAssembler 开放（见 §3.5） |
| `queued_submissions` / `agent_progress` | private | 排队输入与 sub-agent 进度投影 |
| `next_run_sequence` / `next_block_sequence` | private | ID 序列号 |
| `revision` | private | 内容版本号，供渲染层 memo |
| `active_text_block_id` / `active_text_context` | private | 流式文本块追踪 |
| `active_thinking_block_id` / `active_thinking_context` | private | thinking 块追踪 |
| `model_stream_placeholder` | private | model stream 等待占位 |
| `run_runtime` | private | RunRuntimeState——与 Run 生命周期耦合的运行态（见 §3.6） |

```rust
impl ConversationModel {
    // 只读 projection API；不返回 &mut、不暴露内部 collection 的可变句柄。
    pub(crate) fn runs(&self) -> &[RunProjection] { &self.runs }
    pub(crate) fn active_run(&self) -> Option<&RunProjection> {
        self.active_run_id.as_ref()
            .and_then(|id| self.runs.iter().find(|run| &run.id == id))
    }
    pub(crate) fn timeline(&self) -> &OutputTimelineModel { &self.timeline }
    pub(crate) fn run_runtime(&self) -> &RunRuntimeState { &self.run_runtime }
    pub(crate) fn revision(&self) -> u64 { self.revision }
}
```

所有 `apply` / `reduce_*` mutation facade **MUST** 为 crate-private，且只允许 `update/root_reducer.rs` 调用；仅靠 Rust 可见性不足以表达 sibling 模块白名单，因此再由 architecture guard 扫描调用点。ViewAssembler **NEVER** 获得 `&mut ConversationModel`。

### 3.2 Run 投影与 RunStatus 状态机

TUI Model 是 Runtime 的投影层，统一使用 `Run` / `RunStep` 领域语言，并用 `*Projection` 后缀表明非权威态。

```rust
// 投影自 Runtime Run 聚合根
struct RunProjection {
    id: RunId,
    user_submission: String,
    status: RunProjectionStatus,
    steps: Vec<RunStepProjection>,
}

// Run 状态机的 TUI 投影（简化自 Runtime RunStatus）
// Runtime 完整态见 runtime/03-loop-and-state-machine.md
enum RunProjectionStatus {
    Created,          // 初始态
    Running,          // 运行中（合并 PreparingContext/InvokingModel/ApplyingResponse/ExecutingTools）
    AwaitingUser,     // 暂停等待用户输入（ask_user / approval）
    Completing,       // 收到 Runtime RunCompleting，正在收尾
    Cancelling,       // Runtime 已接受取消请求，等待终态事件
    Completed,        // 正常完成
    Failed,           // 异常终止
    Cancelled,        // 用户取消
}
```

> **投影简化规则**：Runtime 细粒度执行态在 TUI 合并为 `Running`，但取消的 accepted 与 terminal 两阶段 **MUST** 保留为 `Cancelling` / `Cancelled`。细粒度展示由 `derive_spinner_phase()` 从 RunProjectionStatus + RunStepProjectionStatus 派生（Thinking / Generating / CallingTool 等），SpinnerPhase **NEVER** 自建状态机。

**RunProjectionStatus 状态转换图**：

```
Created ──RunStarted──→ Running ──RunCompleting──→ Completing ──RunCompleted──→ Completed
   │                        │                           │
   │                        ├──RunFailed───────────────→ Failed
   │                        │
   │                        ├──RunAwaitingUser──→ AwaitingUser ──RunResumed──→ Running
   │                        │
   │                        └──RunCancelling──→ Cancelling ──RunCancelled──→ Cancelled
   │
   ├──RunFailed──→ Failed          // 创建即失败（admission 拒绝，单阶段直接终态）
   └──RunCancelling──→ Cancelling  // 创建即取消（admission 期取消，仍须等待独立的 RunCancelled 事件）

ResumeConversation ──→ Completed（恢复已结束会话，不触发 spinner）
```

> **Created 是瞬态**：`Created` 在同一次 `ProjectRunStarted` 中立即推进到 `Running`（或失败/取消路径），不会在 UI 中可见地停留。admission 阶段被拒绝时只有两条权威退出路径：`RunFailed`（单阶段终态）与 `RunCancelling`（两阶段取消，**NEVER** 跳过独立的 `RunCancelled` 事件直接终态化，即不存在 `Created → Cancelled` 的直接边）。`ResumeConversation` **MUST** 是 `ConversationIntent` 枚举中的显式变体，经 `root_reducer.apply()` 唯一入口分发到 `reduce_conversation`；**NEVER** 由 Session resume 流程或任何 helper 在 reducer 之外直接改写 `RunProjectionStatus`。

| 转换 | 触发 Intent | 方法 | 说明 |
|---|---|---|---|
| → Running | `ProjectRunStarted` | `project_run_started()` | Runtime 接受请求并发布 `RunStarted` 后创建 Run + 初始 step |
| Created → Failed | `ProjectRunFailed`（admission 拒绝） | `project_run_failed()` | Run 尚未进入 Running 即被 Runtime admission 拒绝；与 Running → Failed 复用同一方法，单阶段直接终态 |
| → Completed | `ResumeConversation` | `resume_conversation()` | 显式 `ConversationIntent` 变体，经 `root_reducer.apply()` 统一分发；恢复历史会话，run 保持已完成态，不触发 spinner，**NEVER** 存在旁路 mutation 路径 |
| Running → AwaitingUser | `ProjectRunAwaitingUser` | `project_run_awaiting_user()` | 只投影 Runtime `RunAwaitingUser`；`ShowInteraction` 只建立交互块 |
| AwaitingUser → Running | `ProjectRunResumed` | `project_run_resumed()` | 仅 Runtime 真正消费输入并发布 `RunResumed` 后推进；AgentClient command 成功不代表已恢复 |
| Running → Completing | `ProjectRunCompleting` | `project_run_completing()` | 投影 Runtime `RunCompleting` 并清理 active block 追踪 |
| Completing → Running | （Stop Hook Block） | — | 合法回退：`Completing` 是 TUI 对 Runtime 内部状态 `Finishing` 的投影。Runtime PL **不发出** `RunCompleting` 事件——TUI 基于 `RunCompleted` 终态事件推断 Run 已经过了 Finishing 阶段。若 Finishing 因 Stop Hook Block 返回执行态（`PreparingContext`），TUI 会收到后续 `RunStepStarted` 事件，此时 `Completing → Running` 是合法回退 |
| 任一 live 态（含 Created）→ Cancelling | `ProjectRunCancelling` | `project_run_cancelling()` | 投影 Runtime `RunCancelling`；仍非终态；admission 期（Created）取消同样先进入 Cancelling，**NEVER** 直接跳到 Cancelled |
| Cancelling → Cancelled | `ProjectRunCancelled` | `project_run_cancelled()` | 仅 SDK `RunCancelled` 权威终态事件可进入 Cancelled |
| Running → Failed | `ProjectRunFailed` | `project_run_failed()` | 投影 Runtime 异常终态 |

`ResumeConversation` **MUST** 投影为 `Completed`，恢复历史会话不触发 spinner 或新的 Runtime 执行；该 Intent 与其它 Intent 共享同一 `apply()` 入口，**NEVER** 存在绕过 reducer 的旁路调用。

`InteractionReplySent` / `InteractionCancelled` 只结束本地 Interaction 块，**NEVER** 改写 `RunProjectionStatus`。Interaction 取消发送 typed `InteractionCancelReason::UserCancelled`，不等价于取消整个 Run。Run 取消请求的 effect result 也只表示请求 accepted；TUI 必须等待 `RunCancelling`，并且只有 `RunCancelled` 才能进入终态。

### 3.3 RunStep 投影与 RunStepStatus 状态机

```rust
// 投影自 Runtime RunStep 实体
struct RunStepProjection {
    id: RunStepId,
    sequence: usize,
    status: RunStepProjectionStatus,
    assistant_stream: String,
    tool_calls: Vec<ToolCall>,
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
Streaming ──ToolCallStart──→ ToolCalling
    │                              │
    │              ToolCallUpdate(args accumulated)
    │                              ↓
    │                          PendingArgs
    │                              │
    │                  ToolCallUpdate(Ready)
    │                              ↓
    │                           Ready
    │                              │
    │                  bind()/update(Executing)
    │                              ↓
    │                         ToolExecuting
    │                              │
    │              ToolCallComplete(Success/Error/Cancelled)
    │                              ↓
    └──CompleteBlock──→     all tools terminal
                                ↓
                            Completing ──→ Completed
```

| 转换 | 触发 | 方法 | 说明 |
|---|---|---|---|
| → Streaming | `RunStepProjection::new()` | — | 初始态 |
| Streaming → ToolCalling | `ToolCallStart` | `observe_tool_start()` | push placeholder（仅 stream_key，无 ToolCallId） |
| ToolCalling → ToolExecuting | `ToolCallUpdate(Ready)` / `bind_tool()` | `update_tool()` / `bind_tool()` | 参数完整 → 创建 ToolCallId → 开始执行 |
| → Completing | 所有 tool_calls 终态 | `complete_tool()` | 全部终态检查 |
| Completing → Completed | `ProjectRunStepCompleted` | `project_run_step_completed()` | Runtime 确认 step 完成 |

### 3.4 ToolCall 与 ToolCallStatus 状态机

```rust
struct ToolCall {
    id: Option<ToolCallId>,
    stream_key: ToolStreamKey,
    name: String,
    args_preview: String,
    status: ToolCallStatus,
    result: Option<ToolResultPayload>,
    activities: Vec<String>,
    streaming_preview: Option<ToolStreamingPreviewBuffer>,
    agent_meta: Option<AgentMeta>,
}
enum ToolCallStatus {
    PendingArgs,                  // 参数增量收集中（占位已创建，stream_key 已知，ToolCallId 尚未绑定）
    Ready,                        // 参数完整，等待执行（ToolCallId 在此时创建/绑定）
    Executing,                    // 正在执行
    Success,                      // 执行成功（终态）
    Error { message: String },    // 执行失败（终态）
    Cancelled,                    // 已取消（终态）
    Orphaned,                     // 从未被绑定（终态）
}
```

**ToolCallStatus 状态转换图**：

```
ToolCallStart ──→ PendingArgs ──args_complete──→ Ready ──exec──→ Executing
                     │                    │                    │
                     │                    │         ┌─ok──────→ Success
                     │                    │         ├─err─────→ Error { message }
                     │                    │         └─cancel──→ Cancelled
                     │                    │
                     └──orphan()──────────┴──────────→ Orphaned
```

**NEVER** 从 `Executing` 回到 `PendingArgs` / `Ready`。**NEVER** 从终态（`Success` / `Error` / `Cancelled` / `Orphaned`）转出。

| 转换 | 方法 | 说明 |
|---|---|---|
| → PendingArgs | `observe_tool_start(stream_key)` | 创建 placeholder（仅 stream_key 已知，尚未创建 ToolCallId） |
| PendingArgs → Ready | `update_tool(Ready)` | 参数收集完整，此时创建/绑定 ToolCallId |
| Ready → Executing | `bind(id)` / `update(Executing)` | ToolCallId 绑定后开始执行 |
| Executing → Success | `complete(payload, is_error=false)` | 执行成功（终态） |
| Executing → Error | `complete(payload, is_error=true, message)` | 执行失败（终态） |
| Executing → Cancelled | `cancel()` | 用户/系统取消（终态） |
| PendingArgs / Ready → Orphaned | `orphan()` | provider 序号不匹配，从未被绑定（终态） |

> **placeholder 绑定策略**（修正）：placeholder 阶段只有 `stream_key`（provider call ID），尚未创建领域 `ToolCallId`。绑定先按 `stream_key` 查找 placeholder，在 `Ready` 时才创建/关联 `ToolCallId`。绑定按 `stream_key` 直接查找占位，不依赖 provider content-block 序号。跨轮 `index` 重复时不会覆盖已绑定占位。

> **终态保护**：`Success` / `Error` / `Cancelled` / `Orphaned` 均为终态，`update()` 中 **MUST NOT** 覆盖已终态的 ToolCall。

### 3.5 Conversation 的互补投影

ConversationModel 维护两套**互补投影**：

| 表示 | 类型 | 用途 |
|---|---|---|
| 结构化 Conversation 投影 | `runs` + `queued_submissions` + `agent_progress` | Run / RunStep / ToolCall 生命周期、排队输入与 agent 关联结构 |
| 有序交互投影 | `timeline: OutputTimelineModel` | 消息、工具、系统 / Hook / Error、Interaction、progress 与 queued submission 的展示顺序 |

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
| `Interaction` | UserQuestions / ToolApproval / PlanApproval / HardPause 交互块（同一时刻至多一个） |
| `AgentProgress` | sub-agent 进度块 |

**一致性保证**：

- 两个投影都由同一次 reducer Intent 事务更新；同一 Intent 涉及二者时，必须先完成全部校验再原子提交，**NEVER** 留下半更新
- 两者重叠的 User / Assistant / ToolCall / ToolResult、QueuedSubmission 与 AgentProgress 事实使用相同稳定 ID，并保持 Run 内相对顺序、关联关系与终态一致
- `SystemMessage` / `HookNotice` / `Error` / `Interaction` 是 timeline-only 事实；结构化投影 **NEVER** 伪造字段只为让 timeline 可重建
- `revision` 在一次完整 reducer 事务产生 Change 后只 `wrapping_add(1)` 一次
- 渲染层以 `(conversation.revision(), workspace_root, view_state.collapsed_revision())` 三元组为 cache key，不变时跳过全量重建；不变量与完整定义见 [04-view-layer.md §3.3 / §5.1](04-view-layer.md)
- `move_tool_result_after_tool_call` 强制 result 跟在对应 call 之后，处理流式事件乱序
- invariant test 对重叠 ID、Run 内相对顺序、ToolCall / ToolResult 关联、queued / progress 关联和终态做断言

结构化 Conversation 投影与 `timeline` **NEVER** 声称可由对方完整重建：它们由 Runtime Published Language 与本地用户 Intent 经同一 reducer 事务形成互补 UI 投影。跨二者只约束重叠事实，不建立虚假的全量派生关系。

### 3.6 RunRuntimeState（Run 生命周期耦合运行态）

本节只保留与 Run 生命周期耦合的字段；Config、Workspace 与 Task 投影分别由 §7、§8 与 §6 拥有。

```rust
enum ReasoningPhase {
    Idle,
    Explore,
    Plan,
    Execute,
    Verify,
}

struct RunRuntimeState {
    spinner: SpinnerModel,
    thinking: bool,
    graph_phase: Option<ReasoningPhase>, // TUI-owned enum；不泄漏 Workflow 类型
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

```rust
/// Spinner 派生输入（存储在 RuntimeState 中，由 SDK 事件更新）
struct SpinnerModel {
    /// 运行中 tool call 的名称列表（从当前 RunStep 的 tool_calls 中
    /// status == Executing | PendingArgs | Ready 的条目派生）
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

        // Compacting 由 compact_progress 信号直接映射
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
                            return Some(SpinnerPhase::Hook { event: h.event.clone(), detail: h.detail.clone(), outcome: h.outcome.clone() });
                        }
                    }
                    // 默认：等待首 token 或准备上下文
                    Some(SpinnerPhase::Thinking)
                }
            }
        }

        AwaitingUser => None,  // 任一 Interaction 交互期间不显示 spinner

        Completing => None,

        Cancelling => Some(SpinnerPhase::Cancelling),
    }
}
```

**SpinnerPhase 变体与 Runtime 状态的映射**：

| SpinnerPhase | 派生来源（Runtime 状态） | 说明 |
|---|---|---|
| `Thinking` | `RunStatus::PreparingContext` 或 `InvokingModel`（首 token 前） | 等待上下文准备 / 等待首 token |
| `Generating` | `RunStatus::InvokingModel`（收到 delta 后） + `RunStepStatus::Streaming` | 流式生成中 |
| `CallingTool(name)` | `RunStatus::ExecutingTools` + 1 个 tool `Executing` | 单工具执行中 |
| `CallingTools { remaining }` | `RunStatus::ExecutingTools` + N 个 tool `Executing` | 多工具并行执行 |
| `Compacting` | `RunStatus::Compacting` | 上下文压缩中 |
| `Cancelling` | TUI `RunProjectionStatus::Cancelling`（投影自 SDK `RunCancelling`） | 取消已受理，等待 `RunCancelled` 终态 |
| `AgentWorking` | `RunStatus::InvokingModel` + sub-agent progress 事件 | sub-agent 工作中 |
| `Hook { event, detail, outcome }` | Hook 事件（非 RunStatus，由 HookPort 事件驱动） | Hook 执行中 |

> `run_active = run_status ∈ {Running, AwaitingUser, Completing, Cancelling}`，**NEVER** 独立维护 bool 字段。

> SpinnerPhase **NEVER** 定义无 Runtime 事实来源的 `Reflecting` 状态；如需区分 reasoning 模式，必须从 `RunSpec.reasoning_level` 派生。

`RunProjectionStatus` + `RunStepProjectionStatus` 是 spinner 生命周期的唯一事实源；ViewState 仅保存动画帧。

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
| `clear_compact_runtime()` | 清空 compact_progress + running_tool_count 归零（不改 Run 投影；phase / run_active 继续纯派生） |

#### 3.6.5 StatusNotice

| 方法 | 说明 |
|---|---|
| `set_status_notice(notice)` | 设置持久 notice，清空 expiry |
| `set_transient_status_notice(notice, expires_at)` | 设置临时 notice + 过期时间 |
| `set_graph_phase(phase)` | 设置 graph_phase，同步派生 notice（当无临时 notice 时） |
| `expire_transient_notice(now)` | 检查过期，回退到 graph_phase 派生的持久态 |
| `notice_from_phase(phase)` | idle→"Ready"，其他→phase 文案 |

#### 3.6.6 InteractionState

Interaction 块**内嵌在 OutputTimeline** 中（`OutputTimelineItem::Interaction`），同一时刻至多一个。块持有 Runtime run/request identity 的 TUI-owned 无损投影、四种 TUI-owned body、typed draft 与本地 phase，**NEVER** 持有 sender、pending waiter、SDK DTO 或 AgentClient handle。

```rust
struct InteractionState {
    request_id: UiInteractionRequestId,
    run_id: RunId,
    body: UiInteractionBody,
    draft: UiInteractionDraft,
    phase: InteractionPhase,
    error_message: Option<String>,   // ReplyFailed 时存储错误文本
}

enum UiInteractionDraft {
    UserQuestions { slots: Vec<UserAnswerSlot>, current: usize },
    ToolApproval { decision: Option<UiApprovalDecision> },
    PlanApproval { decision: Option<UiApprovalDecision> },
    HardPause { decision: Option<UiHardPauseDecision> },
}

enum InteractionPhase {
    Collecting,
    Confirming,
    ReplyPending,
    CancelPending,
    Replied,
    Cancelled,
    ReplyFailed { message: String },
}
```

**统一状态转换图**：

```
Collecting ──draft 完整──→ Confirming ──ConfirmInteraction──→ ReplyPending
    │                          │                                  ├─InteractionReplySent──→ Replied
    │                          └─修改选择──→ Collecting            └─InteractionReplyFailed（IrrecoverableError）──→ ReplyFailed
    └────────────CancelInteraction────────→ CancelPending ──InteractionCancelled（CancelAccepted）──→ Cancelled

ReplyPending ──InvalidReply（UserQuestions，保留 draft）──────────────────────────→ Collecting
ReplyPending ──InvalidReply（ToolApproval / PlanApproval / HardPause，保留 draft）───→ Confirming
CancelPending ──CancelRejected（UserQuestions，保留 draft）───────────────────────→ Collecting
CancelPending ──CancelRejected（ToolApproval / PlanApproval / HardPause，保留 draft）→ Confirming
```

- UserQuestions 的 Collecting 维护题目索引、选项与自由文本；draft 完整后生成 `UserAnswers`。
- ToolApproval / PlanApproval 的 Collecting 只允许 Approve / Deny，并分别生成对应 reply variant。
- HardPause 展示 stuck diagnostic；Continue 生成 `HardPause(Continue)`，Cancel 走 typed cancel command，**NEVER** 伪造空 reply。
- `InvalidReply` 是 `reply_interaction` 的校验失败结果：**NEVER** 消费 pending request、**NEVER** 进入终态。它把 `ReplyPending` 按原 body variant 退回 `Collecting`（UserQuestions）或 `Confirming`（ToolApproval / PlanApproval / HardPause），`draft` 原样保留，用户可修正后对同一 `request_id` 重试。
- `CancelRejected` 是 `cancel_interaction` 的对称校验失败结果（例如目标 request 已被并发命令抢先终态化但尚未同步到 TUI）：**NEVER** 消费原 pending request、**NEVER** 进入终态。它把 `CancelPending` 按原 body variant 退回 `Collecting`（UserQuestions）或 `Confirming`（ToolApproval / PlanApproval / HardPause），`draft` 原样保留；用户可继续原 draft 或重新发起取消。
- 完整 `InteractionCommandOutcome` 类型化投影表（`ReplySent` / `CancelAccepted` / `InvalidReply` / `CancelRejected` / `NotFound` / `AlreadyCompleted` / `RunCancelling` / `IrrecoverableError`）见 [03-event-flow-and-acl.md §4.6](03-event-flow-and-acl.md#46-interaction-command-outcome-类型化投影)。

| 方法 | 说明 |
|---|---|
| `show_interaction(request_id, run_id, body)` | 仅为已知非终态 Run 显示交互块并按 body 建立同 variant draft；若 Run 不匹配或已有不同 active id，返回协议冲突 Change，**NEVER** 静默覆盖 |
| `update_interaction_draft(request_id, action)` | 只更新匹配 id、与 body variant 相容的 draft；非法 action 产生 Diagnostic Change |
| `confirm_interaction(request_id)` | draft 完整时进入 ReplyPending，返回 typed `InteractionReplyRequested` Change |
| `cancel_interaction(request_id)` | 进入 CancelPending，返回 `InteractionCancelRequested` Change |
| `apply_interaction_result(intent)` | 消费 effect result Intent，按 request id 进入 Replied / Cancelled / ReplyFailed；`InvalidReply` / `CancelRejected` 按 body variant 退回 Collecting / Confirming 并保留 draft，**NEVER** 终态；陈旧 id 为 no-op + Diagnostic Change |
| `interaction_snapshot()` | 读取纯值交互状态快照，供 key → Intent 翻译使用 |

Coordinator **MUST** 把 `InteractionReplyRequested` / `InteractionCancelRequested` Change 分别转换为 `SendInteractionReply` / `CancelInteraction` Effect；effect runner 完成 AgentClient command 后以 result Intent 回到 reducer。Model **NEVER** 直接发送 reply。

`InteractionReplySent` / `InteractionCancelled` 只把匹配 request id 的 Interaction 块推进到 `Replied` / `Cancelled`，证明 Runtime bridge 已接受命令；它们 **NEVER** 推进 Run。Runtime 只有在真正完成 continuation 后才发布 `RunResumed`，TUI 随后才把 Run 从 `AwaitingUser` 投影为 `Running`。并发 Tool suspension 由 Runtime 按稳定 ToolCall 顺序逐个发布；TUI **NEVER** 扩展为多个 active Interaction。

### 3.7 Intent / Change / Update 模式

Conversation 的公开更新入口只接收封闭 Intent，并返回 Change：

```rust
/// 标识一个对话轮次的上下文元数据——由 adapter/event_mapping 从 SDK event 投影生成。
/// 所有字段均为 TUI 自有类型，NEVER 携带 SDK DTO。
struct UiTurnContext {
    run_id: RunId,
    run_step_id: Option<RunStepId>,   // 当前步序号（流式 block 路由用）
    turn_index: usize,                // 当前轮次在 Run 中的序号
    agent_id: Option<AgentId>,        // Main / Sub agent 标识（sub-agent 路由用）
    model_id: Option<String>,         // 当前使用的 model 标识
}

enum ConversationIntent {
    StartRun { text: String }, // 用户意图：只产生 RunStartRequested，不先写 Runtime 投影
    ProjectRunStarted { run_id: RunId, run_step_id: Option<RunStepId>, text: String },
    ProjectRunAwaitingUser { run_id: RunId },
    ProjectRunResumed { run_id: RunId },
    ProjectRunCompleting { run_id: RunId },
    ProjectRunCompleted { context: UiTurnContext },
    ProjectRunFailed { run_id: RunId, message: String },
    ProjectRunCancelling { run_id: RunId },
    ProjectRunCancelled { run_id: RunId },
    RequestRunCancellation { run_id: RunId }, // 只产生 Change，不先改 RunProjectionStatus
    ResumeConversation { run_id: RunId, run_step_id: Option<RunStepId> }, // 显式 Intent，经 root_reducer.apply() 唯一入口分发，NEVER 由 helper 绕过 reducer
    ProjectRunStepStarted { context: UiTurnContext },
    ProjectRunStepCompleted { context: UiTurnContext },
    AppendAssistantText { context: UiTurnContext, text: String },
    ShowInteraction { request_id: UiInteractionRequestId, run_id: RunId, body: UiInteractionBody },
    UpdateInteractionDraft { request_id: UiInteractionRequestId, action: UiInteractionDraftAction },
    ConfirmInteraction { request_id: UiInteractionRequestId },
    CancelInteraction { request_id: UiInteractionRequestId },
    InteractionReplySent { request_id: UiInteractionRequestId },
    InteractionReplyRejected { request_id: UiInteractionRequestId, reason: String },   // outcome=InvalidReply
    InteractionCancelled { request_id: UiInteractionRequestId },
    InteractionCancelRejected { request_id: UiInteractionRequestId, reason: String },  // outcome=CancelRejected
    InteractionReplyFailed { request_id: UiInteractionRequestId, message: String },
    // 其余封闭变体
}

enum ConversationChange {
    RunStartRequested { text: String },
    RunCancellationRequested { run_id: RunId },
    RunStarted,
    ConversationResumed { run_id: RunId },
    ContentAppended,
    InteractionReplyRequested { request_id: UiInteractionRequestId, reply: UiInteractionReply },
    InteractionCancelRequested { request_id: UiInteractionRequestId },
    InteractionStateChanged,
    OutputDirty,
    // 其余封闭变体
}
```

`StartRun` 只记录待提交的用户展示事实并产生 `RunStartRequested`；Coordinator 再生成 `Effect::StartRun`。只有 SDK `RunStarted` 映射出的 `ProjectRunStarted` 才创建 / 推进 Run 投影。`RequestRunCancellation` 同样只产生 Effect 请求，**NEVER** 直接改 Run；后续 `ProjectRunCancelling` / `ProjectRunCancelled` 才投影 accepted / terminal。`ResumeConversation` 是历史会话恢复的显式 Intent（见 §3.2），产生 `ConversationResumed` Change 并投影为 `Completed`；它与其它 Intent 共享同一 `apply()` 入口，**NEVER** 存在绕过 reducer 的旁路调用。

ConversationChange 覆盖：

- 对话生命周期：`RunStartRequested` / `RunCancellationRequested` / `RunStarted` / `ConversationResumed` / `RunStepStarted` / `RunStepCompleted` / `RunCompleting` / `RunCancelling` / `RunCancelled` / `RunCompleted`
- 内容追加：`UserMessageAppended` / `AssistantTextAppended` / `ThinkingTextAppended` / `SystemMessageAppended` / `ErrorAppended`
- 工具追踪：`ToolCallObserved` / `ToolCallBound` / `ToolCallCompleted` / `OrphanToolResultObserved`
- 排队：`QueuedSubmissionAdded` / `QueuedSubmissionsCleared`
- Agent 进度：`AgentProgressRecorded` / `AgentMetaUpdated`
- Interaction：`InteractionShown` / `InteractionUpdated` / `InteractionReplyRequested` / `InteractionCancelRequested` / `InteractionStateChanged` / `InteractionProtocolConflict`
- 运行态：`UsageChanged` / `LiveTpsChanged` / `ProcessingJobChanged` / `CompactProgressChanged` / `StatusNoticeChanged` / `ThinkingChanged` / `GraphPhaseChanged`
- 脏标记：`OutputDirty` / `StyleBoundaryResetRequired`

> `InteractionReplyRejected`（outcome=`InvalidReply`）与 `InteractionCancelRejected`（outcome=`CancelRejected`）都经 `InteractionStateChanged` Change 回退 phase 并保留 draft，**NEVER** 折叠进 `InteractionReplyFailed`；完整 outcome → Intent → Change 映射见 [03-event-flow-and-acl.md §4.6](03-event-flow-and-acl.md#46-interaction-command-outcome-类型化投影)。

### 3.8 revision memo 机制

```rust
impl ConversationModel {
    pub(crate) fn apply(&mut self, intent: ConversationIntent) -> Vec<ConversationChange> {
        let changes = reduce_conversation(self, intent);
        if !changes.is_empty() {
            self.revision = self.revision.wrapping_add(1);
        }
        changes
    }
    pub(crate) fn revision(&self) -> u64 { self.revision }
}
```

- 每次 `apply()` 产生非空 Change 时 `revision += 1`
- no-op apply（空 Change）不增 revision
- 渲染层以 `(revision, workspace_root, collapsed_revision)` 三元组为 cache key，不变时跳过全量 `assemble_from_conversation`；不变量与完整定义见 [04-view-layer.md §3.3 / §5.1](04-view-layer.md)
- architecture guard **MUST** 将 `apply()` 调用点白名单收窄为 `update/root_reducer.rs`；其他模块只能调用只读 accessor

## 4. InputModel

### 4.1 字段定义

```rust
struct InputModel {
    document: InputDocument,
    history: InputHistory,
    completion: InputCompletion,
    mode: InputMode,
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
    buffer: String,
    cursor: usize,
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
    items: Vec<CompletionItem>,
    visible: bool,
    selected_index: Option<usize>,
    query: String,
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
    entries: Vec<String>,
    selected_index: Option<usize>,
    saved_input: String,
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
    AttachClipboardImage(image), SetMode(InputMode),
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
    notices: Vec<DiagnosticNotice>,
    active_prompt: Option<ActivePrompt>,
    next_notice_id: usize,
}
```

### 5.2 DiagnosticNotice

```rust
struct DiagnosticNotice { id: String, severity: DiagnosticSeverity, message: String }
enum DiagnosticSeverity { Error, Warning, Info }
```

### 5.3 ActivePrompt

```rust
struct ActivePrompt { id: String, question: String }
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
    current_session_id: Option<String>,
    dirty: bool,
    message_count: usize,
    resume_candidates: Vec<SessionResumeCandidate>,
    save_status: SessionSaveStatus,
    save_id_counter: u64,              // 单调递增 save ID 生成器
    save_base_revision: Option<u64>,   // SaveStarted 时记录的 conversation.revision() 快照
    pending_save_id: Option<u64>,      // 当前进行中的 save 批次 ID
    task_status: TaskStatusSnapshot,   // 投影自 Task BC
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

`task_status` 属于 SessionModel，因为 Task 与 Session 关联，且不参与 Run 生命周期。

### 6.3 SessionSaveStatus 状态机

```rust
enum SessionSaveStatus { Idle, Saving { save_id: u64, base_revision: u64 }, Saved, Failed { message: String } }
```

```
Idle ──SaveStarted──→ Saving ──SaveFinished──→ Saved
                          │
                          └──SaveFailed──→ Failed { message }

SaveFinished 清 dirty 逻辑：
  if save_id == current_save_id && conversation.revision() == base_revision:
      dirty = false   // 保存期间无新修改
  else:
      dirty = true    // 保存期间有新修改，仍需落盘
```

> **Dirty 时序安全（revision/generation 协议）**：`SaveFinished` 不能无脑 `dirty=false`。保存过程中若产生新修改，那些修改的 dirty 标记 **MUST** 保留。实现方式：
> 
> 1. `SaveStarted` 时记录 `base_revision: u64`（当前 conversation revision）和 `save_id: u64`（单调递增）
> 2. `SaveFinished { save_id }` 只有 `save_id` 匹配当前保存批次时才生效
> 3. 生效时：如果 `conversation.revision() == base_revision`，则 `dirty = false`（保存期间无新修改）；否则保留 `dirty = true`（保存期间的修改仍未落盘）
> 
> **NEVER** 使用 XOR 公式 `dirty = dirty_at_save_start ^ changes_since_save_start`——该公式在并发修改场景下有竞态窗口，且语义不清晰。

### 6.4 SessionResumeCandidate

```rust
struct SessionResumeCandidate { id: String, /* display fields */ }
```

### 6.5 Intent / Change

```rust
enum SessionIntent {
    SetCurrentSession { id },
    MarkDirty,
    MessagesSynced { message_count },
    SaveStarted { save_id: u64, base_revision: u64 },
    SaveFinished { save_id: u64 },
    SaveFailed { message },
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
| `SaveStarted` | 记录 base_revision + save_id → Saving |
| `SaveFinished` | 匹配 save_id 且 revision == base_revision 时设 Saved + 清 dirty；否则保留 dirty |
| `SaveFailed` | 设 Failed { message } |

## 7. ConfigProjection

> **投影来源**：AgentClient / SDK 发布的 config snapshot event 经 TUI ACL 转换后的 TUI-owned DTO。TUI **NEVER** 直连 ConfigReader / ConfigWriter、watch receiver 或 `ConfigSnapshot` 实现类型；provider / model_id 有独立投影更新路径，切换 model / provider 不触发 Conversation revision。

```rust
struct ConfigProjection {
    provider: Option<String>,
    model_id: Option<String>,
    thinking_visible: bool,            // 当前 model/provider 是否暴露 extended thinking 能力
}
```

| 方法 | 说明 |
|---|---|
| `set_provider_model(provider, model_id)` | 设置 provider 和 model_id |
| `set_thinking_visible(visible)` | 更新 reasoning 能力可见性投影 |

### 7.1 Intent / Change

```rust
enum ConfigIntent {
    ProviderModelChanged { provider, model_id },
    ThinkingChanged { visible: bool },  // 与 ConversationIntent 侧 ThinkingChanged 同一 UiEvent 拆分产生，见 03 §3.3
}
enum ConfigChange {
    ProviderModelChanged,
    ThinkingChanged,
}
```

`ThinkingChanged` 是**双 Context 事件**：`agent_event.rs` 对同一个 `UiEvent::ThinkingChanged` **MUST** 无条件拆分出 `ConfigIntent::ThinkingChanged { visible }`（更新 §7 的 reasoning 能力投影）与 `ConversationIntent`（更新 §3 的可见 thinking 指示器，产生 `ConversationChange::ThinkingChanged`），两个 Intent 必须同时产生，**NEVER** 用条件判断只产生其中一个；完整映射见 [03-event-flow-and-acl.md §3.3](03-event-flow-and-acl.md#33-六个-context-的穷尽映射)。

## 8. WorkspaceProjection

> **上游事实**：SDK Published Language 中的 `ChatEvent::WorkingDirectoryChanged { workspace: WorkspaceContextView, .. }`。TUI ACL **MUST** 在边界将 SDK DTO 完整转换为 TUI-owned `WorkspaceSnapshot`；UiEvent、Intent 与 Model **NEVER** 持有 `sdk::*` 类型。

```text
SDK ChatEvent::WorkingDirectoryChanged { workspace: WorkspaceContextView, .. }
  → sdk_event_to_ui_event（TUI ACL 第一层：SDK DTO → TUI WorkspaceSnapshot）
  → UiEvent::WorkingDirectoryChanged(WorkspaceSnapshot)
  → AgentEventMapper（TUI ACL 第二层）
  → WorkspaceIntent::ApplySnapshot(snapshot)
  → WorkspaceProjection / WorkspaceChange::SnapshotApplied { root, revision }
  → Coordinator → ResolveWorkspaceMetadata Effect
  → WorkspaceIntent::ApplyMetadata { root, revision, branch, kind }
```

```rust
// 以下类型均由 TUI 拥有，NEVER import sdk::*。
struct WorkspaceStackEntry {
    path_base: String,
    workspace_root: String,
}

struct WorkspaceSnapshot {
    path_base: String,
    workspace_root: String,
    context_stack: Vec<WorkspaceStackEntry>,
}

struct WorkspaceMetadata {
    workspace_root: String,
    snapshot_revision: u64,
    branch: Option<String>,
    kind: WorktreeKind,
}

struct WorkspaceProjection {
    path_base: Option<String>,
    workspace_root: Option<String>,
    context_stack: Vec<WorkspaceStackEntry>,
    branch: Option<String>,
    kind: WorktreeKind,
    snapshot_revision: u64,
}
enum WorktreeKind { Unknown, MainCheckout, LinkedWorktree }
```

| 方法 | 说明 |
|---|---|
| `apply_snapshot(snapshot)` | 原子替换 path/root/stack，递增 `snapshot_revision`，并清空旧 branch/kind |
| `apply_metadata(metadata)` | 仅当 `(workspace_root, snapshot_revision)` 与当前投影一致时回填 branch/kind；过期结果为 no-op |

### 8.1 Intent / Change

```rust
enum WorkspaceIntent {
    ApplySnapshot(WorkspaceSnapshot),
    ApplyMetadata(WorkspaceMetadata),
}
enum WorkspaceChange {
    SnapshotApplied { workspace_root: String, revision: u64 },
    MetadataApplied,
}
```

`WorkspaceContextView` 只提供 `path_base`、`workspace_root`、`context_stack`；branch 与 worktree kind **MUST** 由异步 `ResolveWorkspaceMetadata` Effect 解析，**NEVER** 在事件 mapper、reducer 或 Model 中同步执行 git。Coordinator **MUST** 从 `SnapshotApplied` Change 创建携带 root/revision 的 Effect；结果回填时 Model **MUST** 校验同一 tuple，**NEVER** 让旧 workspace 的异步结果覆盖新 snapshot。Effect **MAY** 按 workspace root 缓存 metadata。

`WorkspaceProjection` **MUST** 只依赖 TUI-owned DTO，**NEVER** import SDK、Project 端口、Project 实现类型或 composition-only handle。`WorkingDirectoryChanged` 是 workspace core snapshot 的权威事实；Model **NEVER** 从零散 UI 操作自行推断状态。

## 9. 投影状态机规则

### 9.1 非领域权威声明

Model 中的所有状态机都是**投影状态机**，不是领域权威态：

| 状态机 | 权威位置 | Model 中的角色 |
|---|---|---|
| RunProjectionStatus | Runtime `RunStatus` | 投影——从 SDK 事件推导，简化合并细粒度运行态 |
| RunStepProjectionStatus | Runtime `RunStepStatus` | 投影——从 SDK 事件推导 |
| ToolCallStatus | Runtime ToolCallStatus | 投影——从 SDK 事件推导 |
| SpinnerPhase | 派生——从 run/tool 生命周期 | Model 内部推导 |
| InteractionPhase | Runtime request identity + TUI 用户交互 | 四类 body 的本地交互投影；AgentClient command result 只终结交互，不代表 Run 转换 |
| SessionSaveStatus | Runtime StorageService | 投影——从 SDK 事件推导 |
| WorkspaceProjection | `WorkingDirectoryChanged` 的 core snapshot + TUI Effect metadata | ACL 转换后的投影；metadata 仅在 root/revision 匹配时回填 |

**规则**：

1. **MUST** Model 状态机的转换**只能**由 Intent 触发，**NEVER** 由 Model 自行轮询或定时器驱动。
2. **MUST** 状态机转换产生 Change，Coordinator 消费 Change 决定是否生成 Effect。
3. **MUST** 当 SDK 事件与 Model 状态不一致时，**以 SDK 事件为准**——Model 是投影，不是权威。
4. **NEVER** 在 Model 中维护 Runtime 不存在的状态——避免幻觉态。
5. **MUST** 区分“Effect 已交付”与“Runtime 已转换”：Interaction command result 只更新 InteractionPhase；Run 只由 `RunAwaitingUser` / `RunResumed` / `RunCancelling` / `RunCancelled` 等 SDK 权威事件推进。

### 9.2 状态终态保护

- `ToolCallStatus::Success` / `Error` / `Cancelled` / `Orphaned` 为终态，`update()` 中 **MUST NOT** 覆盖已终态的 ToolCall
- `Error` 变体 **MUST** 包含 `{ message: String }`，与 ACL DTO 一致
- `RunProjectionStatus::Completed` / `Failed` / `Cancelled` 为终态；`Cancelling` 不是终态（等待 Runtime `RunCancelled`）
- `InteractionPhase::Replied` / `Cancelled` / `ReplyFailed` 为终态；只有匹配 request id 的 result Intent 可进入终态

## 10. 单一真相规则

### 10.1 domain 态属 AgentClient

以下状态**MUST**只在 Runtime（AgentClient 侧）维护，Model 只做只读投影：

- AgentRun 生命周期（Running / AwaitingUser / Cancelling / Completed / Failed / Cancelled）
- Message 列表（权威对话历史）
- Tool 执行结果（权威 payload）
- Context window token 计数
- Permission 决策

### 10.2 UI 态只在 Model

以下状态**MUST**只在 Model 维护，**NEVER**在 ViewAssembler / ViewState / Render 中独立持有：

- SpinnerPhase 派生输入（`active_tools` / `last_hook` / `last_agent_progress`）——phase 本身是纯函数派生，不存储
- InputMode（Normal / Completion）
- InteractionPhase（Collecting / Confirming / ReplyPending / CancelPending / Replied / Cancelled / ReplyFailed）
- 四类 Interaction 的 typed draft 快照（UserQuestions cursor / selected / chat_input，approval decision，HardPause decision）
- OutputTimeline 块顺序
- DiagnosticNotice 列表
- SessionResumeCandidate 列表

### 10.3 禁止双重真相

| 状态 | 真相源 | 禁止 |
|---|---|---|
| spinner 可见性 | `derive_spinner_phase(run_status, step_status, spinner)`（`Cancelling` 可见，其余按纯函数返回值） | **NEVER** 在 spinner model 或 view_state 独立维护 `run_active` bool |
| spinner phase | `derive_spinner_phase(run_status, step_status, spinner)` 纯函数派生 | **NEVER** 在 model 存储独立 phase 状态机或 view_state 维护业务 phase |
| input buffer | `model.input().document().buffer()` | **NEVER** 在 render 层维护独立缓冲 |
| active prompt | `model.diagnostic().active_prompt()` | **NEVER** 在 view_state 维护 prompt 副本 |
| Run 取消终态 | SDK `RunCancelling` / `RunCancelled` 两阶段事件 | **NEVER** 由 cancel request accepted、Interaction cancel 或 command result 直接写 `Cancelled` |

`model.conversation().run_runtime().spinner()` 是 spinner 派生输入的唯一来源；`view_state` 只存 `spinner_frame`（动画帧）。

## 11. Model 纯净性约束

### 11.1 禁止依赖

Model 层 `MUST NOT` import 以下 crate：

| 禁止依赖 | 理由 |
|---|---|
| `ratatui` | 渲染框架——Model 不产出渲染类型 |
| `tokio` | 异步运行时——Model 不执行 async 操作 |
| `std::process::Command` | 子进程——Model 不执行 IO |
| `crate::tui::render::*` | 渲染层——方向反了 |
| 任意 `sdk::*` 类型（含 `AgentClient` 与 DTO） | SDK 类型 **MUST** 在 TUI ACL 边界转换；Model **MUST** 只消费 TUI-owned 类型，Controller / Effect Handler 才持出站端口（见 §12） |
| Project 端口、实现类型或 composition-only handle | TUI **MUST** 只接收 SDK 事件经 ACL 形成的投影，**NEVER** 直连 Project |

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
| TUI-owned DTO | ACL 输出的 WorkspaceSnapshot / ChatMessage / ToolCallStatus 等纯值类型 |
| `share`（共享内核） | ContentBlock / InputId 等基础类型 |
| `std` | 基础类型 |

### 11.4 可执行证明

1. Model architecture test **MUST** 拦截 ratatui、tokio、process、channel、AgentClient、SDK DTO 与 Project 类型的越界 import。
2. Model encapsulation guard **MUST** 证明六 Context 核心字段私有，`apply` / `reduce_*` 的生产调用点只有 root reducer；ViewAssembler 只取得不可变 accessor / projection view。
3. reducer 单元测试 **MUST** 逐 Intent 断言 Model 与 Change，不启动异步 runtime。
4. Interaction 场景测试 **MUST** 穷尽 UserQuestions / ToolApproval / PlanApproval / HardPause，并分层覆盖 Runtime request-id DTO → Intent、Intent → Change、Change → AgentClient Effect、Effect result → Intent；sender / pending waiter 不得出现在 TUI fixture；`InteractionReplySent` / `InteractionCancelled` **MUST NOT** 改 Run 状态，只有 `RunResumed` / `RunCancelling` / `RunCancelled` 推进对应投影。另有场景证明两个并发 Tool suspension 被 Runtime 串行发布，第二个 request 不覆盖第一个。
5. structured Conversation / timeline invariant test **MUST** 覆盖 append、tool result 乱序、queued、progress、complete 与 resume，并只校验重叠稳定 ID、相对顺序、关联与终态；**NEVER** 伪造“二者可全量互相重建”的测试。
6. Workspace metadata test **MUST** 证明陈旧 `(root, revision)` 结果不会覆盖新 snapshot。

实现差距与退役责任只在 [Migration Governance](../../03-engineering/03-migration-governance.md) O6 维护。

## 12. AgentClient 消费边界

`AgentClient` 是 Runtime-owned 入站 OHS，由 SDK 发布并以 [Runtime 端口文档](../runtime/06-ports-and-adapters.md) 为唯一签名真相。TUI **NEVER** 复制第二份 trait、把 `RunId` 改名为 `ChatId`，或自行发明 start / cancel / interaction 结果语义。

v0.1.0 只要求 Composition 注入当前 local adapter。Controller / Effect runner 只持 `Arc<dyn AgentClient>`，把 start、`cancel_run`、`reply_interaction` 与 `cancel_interaction` 的 typed outcome 转回 TUI-owned result Intent；Model / reducer / View **NEVER** 持该 handle或直接调用它。request id 由 Runtime 生成，TUI 只做无损 ACL 转换。

Server / WSS、Call/Resp frame、重连、缓冲和远端错误映射明确属于 [Server Future boundary](../server/README.md) / #794。它们 **NEVER** 在本 milestone 预建、冻结或被描述为已经与 local adapter 共用同一 transport DTO；未来设计仍必须保持 Runtime core 对传输透明。

## 14. 相关文档

- TUI 架构与数据流：[01-architecture-and-dataflow.md](01-architecture-and-dataflow.md)
- TUI 事件流与 ACL：[03-event-flow-and-acl.md](03-event-flow-and-acl.md)
- 原始 TUI 设计（历史归档）：[../../../snapshot/design/04-tui-design.md](../../../snapshot/design/04-tui-design.md)
- Runtime-owned AgentClient OHS：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Server future boundary（本 milestone 不冻结 WSS 协议）：[../server/README.md](../server/README.md)
- SDK Published Language：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Project Workspace 端口边界（TUI 不直接消费）：[../project/02-ports-and-adapters.md](../project/02-ports-and-adapters.md)
- 代码组织规范：[../../01-system/06-code-organization.md](../../01-system/06-code-organization.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)
- 统一语言（TUI/TEA/Context）：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Context 字段、Run / RunStep / ToolCall / Spinner / AskUser 投影状态机、单一真相规则与 Model 纯净性约束 | #796 |
| 2026-07-12 | 术语对齐 Runtime 统一语言，新增 AwaitingUser 投影态 | #796 |
| 2026-07-12 | SpinnerPhase 改为 `derive_spinner_phase(run_status, step_status, spinner)` 纯派生；移除无 Runtime 事实来源的状态 | #796 |
| 2026-07-12 | RuntimeState 按投影来源拆分：RunRuntimeState（9 字段，Run 耦合）+ ConfigProjection（provider/model_id，Config BC）+ WorkspaceProjection（cwd/worktree，Project BC），task_status 移到 SessionModel（Task BC）；3+1→3+3 Context | #796 |
| 2026-07-12 | 新增 §12 AgentClient 适配器：LocalAgentClient（直连）+ WssAgentClient（远程，#794）；Controller 只依赖 trait，组合根注入 | #796 |
| 2026-07-14 | WorkspaceProjection 使用 TUI-owned snapshot 与带 root/revision 的异步 metadata Effect；Interaction 收敛为四种 body 共用 request id → Change → Effect → result Intent，实现差距记录收口到 Migration Governance O6 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | Run 恢复 / 取消只投影 Runtime 权威事件；六 Context 核心字段私有；runs / timeline 定义为原子维护的互补投影 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | AgentClient 签名收敛到 Runtime 唯一真相；TUI 只消费 local adapter，WSS / frame / reconnect 退回 Server future boundary | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | InteractionPhase 统一状态转换图补 `InvalidReply`：ReplyPending 按 body variant 回退 Collecting / Confirming 并保留 draft，NEVER 终态 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 状态图补 `CancelRejected`：CancelPending 按 body variant 回退 Collecting / Confirming 并保留 draft；`RunProjectionStatus` 补齐 Created → Failed / Cancelling admission 事件的显式 Intent/方法映射，移除 Created → Cancelled 的直接终态旁路（须经 Cancelling） | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 修正 `ResumeConversation`：定义为显式 `ConversationIntent` 变体，经 `root_reducer.apply()` 唯一入口分发，删除"由 `ensure_runtime_turn()` 内部绕过 reducer"的错误描述 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | ConfigProjection 补 `thinking_visible` 字段与 `ConfigIntent::ThinkingChanged`，闭合 `ThinkingChanged` 双 Context（Config + Conversation）无条件同时映射；cache key 不变量统一为三元组 `(revision, workspace_root, collapsed_revision)`，与 01 / 04 对齐 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
