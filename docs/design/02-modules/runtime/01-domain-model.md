# Agent Runtime · 领域模型

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 Agent Runtime 核心域的领域模型：Run 聚合、RunSpec、RuntimeContext 三元组，及其实体/值对象、不变量、领域事件与 SubAgent 派生规则。**只描述目标态**；与现状的差距记入 `03-engineering/migration-governance`。

## 1. 三元组总览

| 概念 | 回答 | 性质 | 层 |
|---|---|---|---|
| **RunSpec** | 跑**什么**（prompt/tools/model/timeout/资源模式）| 声明式、可序列化、可复用 | 领域 |
| **RuntimeContext** | 用**什么资源**跑（装配好的各 Port + config + event sink）| 活资源、不可序列化、运行时装配 | 基础设施/应用 |
| **Run** | 一次执行实例 | 领域聚合（内存态状态机）| 领域 |

因果链：`RunSpec（声明） ──装配──▶ RuntimeContext（资源） ──注入──▶ Run（执行）`

层级对齐：`Session → Run → Run Step`（≈ OpenAI Thread / Run / Run Step）。

## 2. Run 聚合

```rust
// —— 聚合根 ——
struct Run {
    id: RunId,                 // UUIDv7
    spec: RunSpec,             // 规格（可序列化 / 可复用；active RunSpec 不作 durable checkpoint）
    parent: Option<RunId>,     // Sub Run 指向父 Run（结果/事件回传）
    status: RunStatus,         // 状态机（含 Cancelling 过渡态，见 03-loop-and-state-machine）
    pending_interaction: Option<PendingInteraction>, // 与 AwaitingUser 同步存活，不持久化
    steps: Vec<RunStep>,       // 内部实体序列
    started_at: Instant,
}

/// 每个 Run 独占的协作式取消作用域；属于 RuntimeContext 活资源，不持久化。
/// 子 Run 从父作用域派生，父取消会同步传播到全部子 Run。
struct RunCancellationScope {
    token: CancellationToken,
}

// —— 聚合内实体（有标识+生命周期，归属 Run）——
struct RunStep {
    id: RunStepId,
    status: RunStepStatus,             // Pending/Invoking/Applying/ToolPhase/Done/Failed
    invocation: Option<ModelInvocation>,
    tool_calls: Vec<ToolCall>,
}

struct ToolCall {                      // 实体（ToolCallId + 状态生命周期）
    id: ToolCallId,
    provider_id: String,               // provider 消息边界标识（双 ID）
    name: String,
    args: ToolCallArgs,
    status: ToolCallStatus,            // PendingArgs/Ready/AwaitingApproval/Running/Success/Error/Cancelled
    result: Option<ToolResult>,
}

// —— 值对象（无标识、按值、不可变）——
struct InvocationResponse {
    message: Message,                 // Runtime 将 ProviderCompletion 映射为领域 Message
    stop_reason: StopReason,
    usage: Option<RawUsageSnapshot>,
    effective_reasoning: ReasoningLevel,
}

struct ModelInvocation {               // VO：一次 LLM 调用记录，属于 RunStep
    request: InvocationRequest,
    response: InvocationResponse,
}

struct PendingInteraction {
    request_id: InteractionRequestId,
    continuation: InteractionContinuation,
}

enum InteractionContinuation {
    CompleteToolCall(ToolCallId),
    ContinueToolApproval(ToolCallId),
    ContinuePlanApproval,
    ContinueAfterHardPause,
}
```

**实体 vs VO**：Run（聚合根）/ Run Step / Tool Call = 实体；Model Invocation = VO；`RunId/RunStepId/ToolCallId`、各 `*Status`、`RawUsageSnapshot`、`ToolCallArgs`、`ToolResult`、`ReasoningLevel` = VO。

> **注**：`RuntimeContext` 不进 Run 聚合——它是活资源，由派生逻辑装配后作为参数传入 Loop Engine；崩溃重建即可（呼应"从头开始"，见 `05-recovery-semantics`）。

## 3. 不变量（Run 聚合守护）

1. Run 进入 `Completed/Failed/Cancelled` 后**不可再加 Run Step**
2. `Cancelling` 只接受取消收口，不可开始 Model Invocation、Tool Call 或 Compaction
3. 每个 Tool Call **必须归属**某个 Run Step
4. 每个 Run Step **至多一次** Model Invocation
5. Tool Call 状态**单向推进**（不可从 Success 回到 Running）
6. `AwaitingToolApproval` 未决时，不可进入 `ExecutingTools`
7. **timeout > 0 时**，墙钟超时强制迁移到 `Failed`（timeout=0 表示无限，见 §5）
8. 每个 Run **必须独占**一个 cancellation scope；子 Run 从父 scope 派生，NEVER 共享可替换的 Session 级 token 槽
9. `AwaitingUser` 与唯一 `PendingInteraction` **MUST** 同时存活；reply / cancel 必须匹配 `request_id`，每个 continuation 至多完成一次
10. 一个 Run 任一时刻至多一个 PendingInteraction；并发 Tool suspension 必须按原 RunStep 的 ToolCall 顺序串行 resolve，**NEVER** 为同一 Run 同时注册多个 waiter
11. 一个完成的 RunStep 恰好产生一次 `ContextAppend`；assistant 与全部最终 Tool result 按协议顺序一起提交，**NEVER** 逐 suspension 持久化半成品

## 4. 领域事件（→ Event Projection → SDK ChatEvent）

`RunStarted · RunStepStarted · ModelInvocationStarted/Delta/Retrying/Completed · ToolCallRequested/Approved/Executing/Completed/Failed · RunStepCompleted · RunAwaitingUser{request_id}/Resumed{request_id} · CompactionStarted/Completed · StuckDetected · RunCancellationRequested · RunCompleted/Failed/Cancelled`

> **取消事件分两阶段**：`RunCancellationRequested` 在同步取消入口接受请求并将 Run 迁移到 `Cancelling` 时产生；`RunCancelled` 仅在 Provider/Tool/Compact/Hook 等在途工作停止且回滚完成后产生。前者是即时请求事实，后者是异步完成确认。

> **终态事实与业务返回分离**：`RunCompleted { result }`（最后 assistant 文本/结构）/ `RunFailed { error }` / `RunCancelled` 是 Run 聚合产生并经 `EventSink` 投影的权威领域事件；同时 `run_loop` / `derive_sub_run` 直接返回 typed `AgentRunTerminal`。Main 使用事件通知 TUI；Sub 的父 Run **MUST** 消费 typed return 继续业务编排，**NEVER** 反向订阅 EventSink 或遍历 message 提取结果。事件载荷与 typed return 来自同一次终态 mutation，必须一致。

> Event Projection adapter 按 Main/Sub scope 路由与命名：Main terminal/event stream → TUI；Sub event 仅作父级诊断投影，业务 completion 走 typed `AgentRunTerminal` return（详见 #612）。

## 5. RunSpec —— 声明式规格

```rust
/// 一次 Run 的完整规格：声明"要什么"。可序列化、可复用（用户默认 / skill / role / 父 Run 派生）。
struct RunSpec {
    name: String,                     // "main" / sub role 名 / skill 名

    // —— 提示与模型 ——
    model: ModelId,
    system_prompt: SystemPromptSpec,  // 基础 prompt + guidance 选择键

    // —— 能力（交互能力 = Scope/Profile 是否装配并允许 user interaction）——
    tools: ToolAccessSpec,              // Registry Scope + 只能收缩的 Tool Profile

    // —— 执行约束 ——
    timeout: Duration,                // 墙钟上限；**0 = 无限**（Main 默认 0，Sub 可配有限值）
    retry: RetryPolicy,               // Provider 单 invocation retry 上限 / backoff；context exceeded 不计 retry

    // —— 资源模式：驱动 Composition 与 RuntimeContext 装配 ——
    context:   ContextMode,           // SharedSession | Isolated
    workspace: WorkspaceMode,         // Inherit | Snapshot（Composition workspace scope 策略）
    policy:    PolicyMode,            // v0.1.0: AllowAll
    memory:    MemoryMode,            // Enabled | Disabled(不读不写/不 reflection)
    hooks:     HookMode,              // Full | BoundaryOnly | Disabled
    reasoning: ReasoningMode,         // GraphDriven | EffortOnly(level) | Inherit
    task:      TaskMode,              // Shared | Isolated
    finalization: FinalizationSpec,   // deterministic summary / receipt 策略
}

struct FinalizationSpec {
    summary: SummaryMode,
    receipt: ReceiptDetail,
}

enum SummaryMode {
    None,                            // 不生成下一轮 Context 文本投影
    Deterministic {
        per_tool_token_budget: u32,
        total_token_budget: u32,
        value_gate: SummaryValueGate,
    },
}

enum ReceiptDetail {
    Safety,                          // terminal identity / artifact / side effect / unconfirmed
    Full,                            // 再含 completed actions / verified facts / remaining work
}

enum SummaryValueGate { ReusableWorkOnly }

enum ContextMode   { SharedSession, Isolated }
enum WorkspaceMode { Inherit, Snapshot }              // Snapshot: 快照父 frame，改目录不回写
enum PolicyMode    { AllowAll }                         // CLI --yolo 的领域映射；Future 再扩规则模式
enum MemoryMode    { Enabled, Disabled }
enum HookMode      { Full, BoundaryOnly, Disabled }   // BoundaryOnly: 仅 start/stop
enum ReasoningMode {
    GraphDriven,
    EffortOnly(ReasoningLevel), // role / RunSpec 声明的固定 requested level
    Inherit,
} // Main: GraphDriven; Sub: EffortOnly/Inherit
enum TaskMode      { Shared, Isolated }
```

`SummaryMode` 只控制是否为下一轮 Context 生成 deterministic 文本投影，所有模式都 **NEVER** 调用 LLM summary。`ReceiptDetail::Safety` 是不可降低的安全下限：必须保留 child/run/tool identity、terminal status、artifact refs、可能副作用与 `CancellationUnconfirmed`；`None` 只表示没有 Context summary，**NEVER** 表示丢弃终态 receipt。Main 默认 `Deterministic + Full`；Sub 默认 `None + Safety`。只有明确需要自身 continuation 的特殊 Sub 才可声明 `Deterministic + Full`，且仍受父级预算收缩。

**去掉 max_turns**：无限循环由 `timeout` + 防 stuck（`04-stuck-prevention`）双重兜底，不再用轮次上限。

## 6. RuntimeContext —— 装配的活资源

```rust
/// 按 RunSpec 装配出的执行资源容器：运行时构造，注入 Loop Engine。不可序列化，不进 Run 聚合。
struct RuntimeContext {
    context:   Arc<dyn ContextPort>,    // Sub: 独立 context manager
    provider:  Arc<dyn ProviderPort>,   // Main/Sub 可共享不可变 transport；调用期 Invocation Scope 独立
    tool_catalog:   Arc<dyn ToolCatalogPort>,   // 按 Registry Scope/Profile 投影 schemas
    tool_execution: Arc<dyn ToolExecutionPort>, // 不暴露 Tool/Registry，实现单次函数调用
    policy:    Arc<dyn PolicyPort>,     // v0.1.0: AllowAllPolicy
    interaction: Arc<dyn InteractionPort>, // Runtime-owned 等待 / reply seam
    memory:    Arc<dyn MemoryPort>,     // Sub(Disabled): NoOpMemory
    reflection: Arc<dyn ReflectionPromptPort>, // 纯 prompt / parse / format；apply 仍走同一 memory Arc
    task:      Arc<dyn TaskAccess>,     // Sub: 独立实例；NEVER 暴露 TaskPersist
    hooks:     Arc<dyn HookPort>,       // Sub: BoundaryOnly
    reasoning: Arc<dyn ReasoningPort>,  // 发布 requested level；Sub: EffortOnly/Inherit
    usage:     Arc<dyn UsageSink>,      // 非阻塞；Audit MVP 只记录 metadata
    config:    ConfigSnapshot,          // Main/Sub 共享
    clock:     Arc<dyn Clock>,          // request builder 冻结 CalendarDate；Prompt 不读全局时钟
    input:     Arc<dyn InputBuffer>,    // 入站：Main=TUI通道+忙期buffer; Sub=固定初始队列
    events:    Arc<dyn EventSink>,      // 出站纯投影：Main→TUI；Sub→父级诊断（业务结果走 typed return）
    cancel:    RunCancellationScope,    // per-Run；Provider/Tool/Compact/Hook 共享或派生
}
```

`RunSpec` 可序列化是为了模板、默认值与 Sub 派生，不表示 active Run 可恢复。Session 只可在自身 metadata / preference 字段保存 model 等下次 Run 默认偏好，**NEVER** 持久化 active RunSpec、RunStatus、RunStep 或 cancellation scope；resume 后的新输入始终创建全新 Run。

`WorkspaceMode` 保留在 `RunSpec` 中作为声明式装配策略，但 RuntimeContext **NEVER** 持有 Workspace 端口、Project trait 或 wiring。Composition 为 active Main session slot 保留一个跨多 Run / resume 复用的 `CompositionWorkspaceScope`，只在 Main agent 启动时选择 Project production wiring；Sub Run 对父 wiring 执行 Project-owned isolated derivation 并持有独立 child scope。scope **NEVER** 进入 Runtime、Tool 或 Context 类型。

### RunSpec 模式 → RuntimeContext / CompositionWorkspaceScope 装配映射

| RunSpec 字段 | Main | Sub | 装配 |
|---|---|---|---|
| `context` | SharedSession | Isolated | `ContextPort` 实例 |
| `model` | 可共享不可变 provider transport | 可共享不可变 provider transport | 按 model 选择 adapter；每次调用创建独立 Invocation Scope，隔离 model/reasoning/max tokens |
| `workspace` | Inherit | Snapshot | Composition：Main 复用 active-session-slot production scope；Sub 对 parent scope 执行 isolated derivation，并从同一 child wiring 向 Context / Tool backing implementation 分发窄 view；RuntimeContext 无 workspace 字段 |
| `policy` | AllowAll | AllowAll | v0.1.0 同一 PolicyPort；Future 模式另行设计 |
| `memory` | Enabled | Disabled | Main 使用 active session slot 的同一 Arc；Sub 为 `NoOpMemory` 或显式共享父 Arc |
| `hooks` | Full | BoundaryOnly | per-tool / 仅 start-stop |
| `reasoning` | GraphDriven | EffortOnly(level)/Inherit | 全 graph / 固定 requested effort / 继承父 requested effort；model clamp 由 Provider resolver 完成 |
| `task` | Shared | Isolated | 独立 `TaskStore` |
| `tools` | Main Scope + 完整基线 Profile | Sub Scope + 收缩 Profile | Scope 决定装配资源，Profile 只按 capability 收缩 |
| `finalization.summary` | Deterministic（默认 per-tool 512、总计 `min(4096, context_window×2%)`） | None（默认不生成自身 Context 投影） | 固定模板消费 typed receipts；NEVER 调用 LLM |
| `finalization.receipt` | Full | Safety（不可降低） | StepFinalizer 收集；父 Agent Tool 消费 Sub terminal receipt |
| `cancel` | 新建 Run root + per-Step child scope | 从父 tool scope 派生 child Run root + per-Step child scope | 父 Step cancel 对 Agent Tool 传播 `TerminateRun(ParentStepCancelled)`；共享父绝对 deadline |

## 7. SubAgent 派生：控制权矩阵 + 安全铁律

SubAgent 派生 = 父 Run 给出**子 RunSpec** → 注入 dispatch Tool 的 composition-provided AgentDispatch 捕获或按 RunId 索引父 `CompositionWorkspaceScope` → 派生子 scope 并装配**子 RuntimeContext** → 启动子 Run，跑同一套 Loop（引擎零分支）。Runtime 只编排 Tool 调用并声明 `WorkspaceMode::Snapshot`，**NEVER** 接触 scope 或 Project 能力。

### 安全铁律
> **Sub 的权限/能力 NEVER 超过 Main 授予的范围。Main 只能"削弱或平移"，NEVER 让 Sub 越权。**

作为 RunSpec 派生不变量：Registry Scope 只能移除工具/资源，Tool Profile 的 allowed capabilities 只能收缩；policy 不可放宽；Sub workspace **MUST** 为 Snapshot，并由 Composition 对父 Run scope 执行隔离派生。

### 控制权矩阵

| RunSpec 字段 | 控制权 | 说明 |
|---|---|---|
| **prompt（任务）** | 🟢 main 可控 | 就是任务本身 |
| **model** | 🟢 main 可控 | 选模型，无安全风险 |
| **timeout** | 🟢 main 可控（有 cap）| 任务时长；0=无限 |
| **memory** | 🟢 main 可控（默认 off）| `share_memory` 参数，main 决定是否给 sub 注入 |
| **system_suffix** | 🟡 role 预设 | 角色人设 |
| **reasoning effort** | 🟡 role 预设 + 继承父 | role>model>继承父进程 |
| **tools** | 🔴 Scope/Profile 固定受限，**不可扩大** | 安全：Scope 只移除资源，Profile capability 集只收缩 |
| **workspace** | 🔴 固定独立 | 安全：隔离，防改父目录 |
| **policy** | 🔴 固定继承，**不可放宽** | 安全铁律 |
| **hooks** | 🔴 固定 BoundaryOnly | 一致性/安全 |
| **task** | 🔴 固定独立 | 防污染父任务 |

## 8. Main / Sub 差异矩阵（最终）

| 项 | Main | Sub |
|---|---|---|
| context | 共享 Session | 独立 |
| tools | Main Scope + 基线 Profile | Sub Scope + capability 收缩 Profile |
| workspace | Inherit | 独立快照（改目录不回写父）|
| task | 共享 | 独立 |
| memory | 读写 + reflection | **不读不写**（可由 main 开启注入）|
| hooks | Full | BoundaryOnly（start/stop）|
| policy | AllowAll | AllowAll |
| reasoning | GraphDriven | EffortOnly(level) / Inherit（无 graph，固定或继承 requested level）|
| provider | 共享不可变 transport；独立 Invocation Scope | 共享不可变 transport；独立 Invocation Scope |
| timeout | 默认 0（无限）| 可配有限值 |
| 事件出口 | → TUI | → 父 Run（#612）|
| interaction | SDK/TUI adapter | 显式 parent-mediated adapter；未装配则 unavailable |
| 输入 | 常驻多轮 | 单次输入 |
| Finalization summary | Deterministic + Full receipt | 默认 None + Safety receipt；特殊 continuation Sub 可显式提高 |
| 父 Step 取消 | 取消 Main 当前 Step | 对关联 Sub 递归执行 TerminateRun，不允许 Sub 回 Drain 续跑 |

> **差异 100% 由 RunSpec + Composition 装配 + RuntimeContext + Event adapter 表达，Loop Engine 零分支。**

## 9. 相关文档

- 模块边界：[02-module-boundaries.md](02-module-boundaries.md)
- 状态机与 Loop：[03-loop-and-state-machine.md](03-loop-and-state-machine.md)
- 防 stuck：[04-stuck-prevention.md](04-stuck-prevention.md)
- 恢复语义：[05-recovery-semantics.md](05-recovery-semantics.md)
- 端口与装配：[06-ports-and-adapters.md](06-ports-and-adapters.md)
- Session 聚合：[../context-management/01-session.md](../context-management/01-session.md)
- Project Workspace 端口：[../project/02-ports-and-adapters.md](../project/02-ports-and-adapters.md)
- 代码组织规范：[../../01-system/06-code-organization.md](../../01-system/06-code-organization.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)
- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：Run 聚合 + RunSpec + RuntimeContext 三元组、不变量、领域事件、控制权矩阵、安全铁律、差异矩阵 | #761 |
| 2026-07-11 | RuntimeContext 补入站端口 input（InputBuffer）；澄清 result 不进 RuntimeContext | #761 |
| 2026-07-11 | output/result 定案：统一经 EventSink，result 为 RunCompleted 载荷（无独立 RunResult），靠终态事件识别 | #761 |
| 2026-07-11 | 领域事件补终态族对称载荷（RunFailed{error} / RunCancelled）+ ModelInvocationRetrying | #761 |
| 2026-07-12 | 取消语义收敛：per-Run cancellation scope、Cancelling 不变量、取消请求/完成双事件 | #700 |
| 2026-07-12 | RuntimeContext 的 ToolPort 拆为 Catalog/Execution；RunSpec tools 改为 Registry Scope + capability Profile | #787 |
| 2026-07-12 | Provider 隔离语义收敛为共享不可变 transport + 每次调用独立 Invocation Scope | #788 |
| 2026-07-14 | 移除 Runtime Workspace 端口；WorkspaceMode 仅驱动 composition-internal workspace scope，Main 在 Session 内复用，Sub 从父 Project wiring 派生同一隔离实例供 Context / Tool 装配；补齐 pending interaction identity / continuation | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 增加 PlanApproval continuation；冻结单 PendingInteraction、Tool suspension 串行化与每 RunStep 单次 ContextAppend 不变量 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-15 | RunSpec 增加 FinalizationSpec：Main 默认 deterministic summary + Full receipt，Sub 默认无 summary + Safety receipt；父 CancelRunStep 对 Agent Tool Sub 传播共享绝对 deadline 的 TerminateRun | [#700](https://github.com/rushsinging/aemeath/issues/700) |
