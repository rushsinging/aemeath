# Agent Runtime · 领域模型

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
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
    spec: RunSpec,             // 规格（可序列化，随 session 快照）
    parent: Option<RunId>,     // Sub Run 指向父 Run（结果/事件回传）
    status: RunStatus,         // 状态机（见 03-loop-and-state-machine）
    steps: Vec<RunStep>,       // 内部实体序列
    started_at: Instant,
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
struct ModelInvocation {               // VO：一次 LLM 调用记录，属于 RunStep
    request: InvocationRequest,
    response: InvocationResponse,
    usage: Usage,
    effort: ReasoningLevel,            // 来自 Workflow
}
```

**实体 vs VO**：Run（聚合根）/ Run Step / Tool Call = 实体；Model Invocation = VO；`RunId/RunStepId/ToolCallId`、各 `*Status`、`Usage`、`ToolCallArgs`、`ToolResult`、`ReasoningLevel` = VO。

> **注**：`RuntimeContext` 不进 Run 聚合——它是活资源，由派生逻辑装配后作为参数传入 Loop Engine；崩溃重建即可（呼应"从头开始"，见 `05-recovery-semantics`）。

## 3. 不变量（Run 聚合守护）

1. Run 进入 `Completed/Failed/Cancelled` 后**不可再加 Run Step**
2. 每个 Tool Call **必须归属**某个 Run Step
3. 每个 Run Step **至多一次** Model Invocation
4. Tool Call 状态**单向推进**（不可从 Success 回到 Running）
5. `AwaitingToolApproval` 未决时，不可进入 `ExecutingTools`
6. **timeout > 0 时**，墙钟超时强制迁移到 `Failed`（timeout=0 表示无限，见 §5）

## 4. 领域事件（→ Event Projection → SDK ChatEvent）

`RunStarted · RunStepStarted · ModelInvocationStarted/Delta/Completed · ToolCallRequested/Approved/Executing/Completed/Failed · RunStepCompleted · RunAwaitingUser/Resumed · CompactionStarted/Completed · StuckDetected · RunCompleted/Failed/Cancelled`

> Event Projection adapter 按 Main/Sub 路由与命名（Main→TUI，Sub→父 Run，详见 #612）。

## 5. RunSpec —— 声明式规格

```rust
/// 一次 Run 的完整规格：声明"要什么"。可序列化、可复用（用户默认 / skill / role / 父 Run 派生）。
struct RunSpec {
    name: String,                     // "main" / sub role 名 / skill 名

    // —— 提示与模型 ——
    model: ModelId,
    system_prompt: SystemPromptSpec,  // 基础 prompt + guidance 选择键

    // —— 能力（交互能力 = 是否含 ask_user 工具）——
    tools: ToolSelection,             // 允许的工具集（全集 / 白名单）

    // —— 执行约束 ——
    timeout: Duration,                // 墙钟上限；**0 = 无限**（Main 默认 0，Sub 可配有限值）

    // —— 资源模式：驱动 RuntimeContext 装配 ——
    context:   ContextMode,           // SharedSession | Isolated
    workspace: WorkspaceMode,         // Inherit | Snapshot
    policy:    PolicyMode,            // Direct | DelegatedApproval
    memory:    MemoryMode,            // Enabled | Disabled(不读不写/不 reflection)
    hooks:     HookMode,              // Full | BoundaryOnly | Disabled
    reasoning: ReasoningMode,         // GraphDriven | EffortOnly | Inherit
    task:      TaskMode,              // Shared | Isolated
}

enum ContextMode   { SharedSession, Isolated }
enum WorkspaceMode { Inherit, Snapshot }              // Snapshot: 快照父 frame，改目录不回写
enum PolicyMode    { Direct, DelegatedApproval }      // Delegated: 需确认时转发父（S2 设计，暂不实现）
enum MemoryMode    { Enabled, Disabled }
enum HookMode      { Full, BoundaryOnly, Disabled }   // BoundaryOnly: 仅 start/stop
enum ReasoningMode { GraphDriven, EffortOnly, Inherit } // Main: GraphDriven; Sub: EffortOnly/Inherit
enum TaskMode      { Shared, Isolated }
```

**去掉 max_turns**：无限循环由 `timeout` + 防 stuck（`04-stuck-prevention`）双重兜底，不再用轮次上限。

## 6. RuntimeContext —— 装配的活资源

```rust
/// 按 RunSpec 装配出的执行资源容器：运行时构造，注入 Loop Engine。不可序列化，不进 Run 聚合。
struct RuntimeContext {
    context:   Arc<dyn ContextPort>,    // Sub: 独立 context manager
    provider:  Arc<dyn ProviderPort>,   // 按 spec.model 选定；**Sub 持独立 client 副本**(避免共享踩踏)
    tools:     Arc<dyn ToolPort>,       // 按 spec.tools 装配的受限 registry
    policy:    Arc<dyn PolicyPort>,     // Sub: DelegatedApproval 装饰器(设计)
    memory:    Arc<dyn MemoryPort>,     // Sub(Disabled): NoOpMemory
    task:      Arc<dyn TaskPort>,       // Sub: 独立实例
    workspace: Arc<dyn WorkspacePort>,  // Sub: 独立快照 frame
    hooks:     Arc<dyn HookPort>,       // Sub: BoundaryOnly
    reasoning: Arc<dyn ReasoningPort>,  // Sub: EffortOnly/Inherit
    audit:     Arc<dyn AuditSink>,
    config:    ConfigSnapshot,          // Main/Sub 共享
    input:     Arc<dyn InputSource>,    // 入站：Main=TUI通道+忙期buffer; Sub=固定初始队列
    events:    Arc<dyn EventSink>,      // 出站：Main→TUI ; Sub→父 Run
}
```

### RunSpec 模式 → RuntimeContext 装配映射

| RunSpec 字段 | Main | Sub | 装配 |
|---|---|---|---|
| `context` | SharedSession | Isolated | `ContextPort` 实例 |
| `provider` | 共享 client | **独立副本** | 避免并发 sub 踩踏 reasoning/max_tokens |
| `workspace` | Inherit | Snapshot | 快照父 frame，改目录不回写 |
| `policy` | Direct | DelegatedApproval | 转发装饰器（设计态）|
| `memory` | Enabled | Disabled | 真实 / `NoOpMemory` |
| `hooks` | Full | BoundaryOnly | per-tool / 仅 start-stop |
| `reasoning` | GraphDriven | EffortOnly/Inherit | 全 graph / 仅 effort（无设置继承父）|
| `task` | Shared | Isolated | 独立 `TaskStore` |
| `tools` | 全集 | 白名单 | 受限 registry |

## 7. SubAgent 派生：控制权矩阵 + 安全铁律

SubAgent 派生 = 父 Run 给出**子 RunSpec** → 装配**子 RuntimeContext** → 启动子 Run，跑同一套 Loop（引擎零分支）。

### 安全铁律
> **Sub 的权限/能力 NEVER 超过 Main 授予的范围。Main 只能"削弱或平移"，NEVER 让 Sub 越权。**

作为 RunSpec 派生不变量：派生时 tools 只能收缩不能扩张；policy 不可放宽；workspace 强制隔离。

### 控制权矩阵

| RunSpec 字段 | 控制权 | 说明 |
|---|---|---|
| **prompt（任务）** | 🟢 main 可控 | 就是任务本身 |
| **model** | 🟢 main 可控 | 选模型，无安全风险 |
| **timeout** | 🟢 main 可控（有 cap）| 任务时长；0=无限 |
| **memory** | 🟢 main 可控（默认 off）| `share_memory` 参数，main 决定是否给 sub 注入 |
| **system_suffix** | 🟡 role 预设 | 角色人设 |
| **reasoning effort** | 🟡 role 预设 + 继承父 | role>model>继承父进程 |
| **tools** | 🔴 固定受限，**不可扩大** | 安全：防 sub 获得 main 没给的能力 |
| **workspace** | 🔴 固定独立 | 安全：隔离，防改父目录 |
| **policy** | 🔴 固定继承，**不可放宽** | 安全铁律 |
| **hooks** | 🔴 固定 BoundaryOnly | 一致性/安全 |
| **task** | 🔴 固定独立 | 防污染父任务 |

## 8. Main / Sub 差异矩阵（最终）

| 项 | Main | Sub |
|---|---|---|
| context | 共享 Session | 独立 |
| tools | 全集 | 受限白名单 |
| workspace | Inherit | 独立快照（改目录不回写父）|
| task | 共享 | 独立 |
| memory | 读写 + reflection | **不读不写**（可由 main 开启注入）|
| hooks | Full | BoundaryOnly（start/stop）|
| policy | Direct | DelegatedApproval（设计态；当前仍 allow_all）|
| reasoning | GraphDriven | EffortOnly（无 graph，无设置继承父）|
| provider client | 共享 | **独立副本** |
| timeout | 默认 0（无限）| 可配有限值 |
| 事件出口 | → TUI | → 父 Run（#612）|
| 输入 | 常驻多轮 | 单次输入 |

> **差异 100% 由 RunSpec + RuntimeContext + Event adapter 表达，Loop Engine 零分支。**

## 9. 相关文档

- 模块边界：[02-module-boundaries.md](02-module-boundaries.md)
- 状态机与 Loop：[03-loop-and-state-machine.md](03-loop-and-state-machine.md)
- 防 stuck：[04-stuck-prevention.md](04-stuck-prevention.md)
- 恢复语义：[05-recovery-semantics.md](05-recovery-semantics.md)
- 端口与装配：[06-ports-and-adapters.md](06-ports-and-adapters.md)
- Session 聚合：[../context-management/01-session.md](../context-management/01-session.md)
- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：Run 聚合 + RunSpec + RuntimeContext 三元组、不变量、领域事件、控制权矩阵、安全铁律、差异矩阵 | #761 |
| 2026-07-11 | RuntimeContext 补入站端口 input（InputSource）；澄清 result 不进 RuntimeContext | #761 |
