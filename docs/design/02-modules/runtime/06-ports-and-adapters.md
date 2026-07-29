# Agent Runtime · 端口与适配器

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 Agent Runtime 的入站 OHS、所消费的能力契约、RuntimeContext 装配与 Composition Root。**只描述目标态**；实现缺口记入 `03-engineering/migration-governance`。
> **v0.1.0 scope（#921 收缩）**：Provider option resolver 领域模型已完成迁移但未接生产链路；Config `reasoning_graph` 已退役，五节点采用固定默认 effort；Main 已通过 ReasoningPort 接线；Provider resolver 尚未接线。Runtime/Context/TUI 尚未端到端消费 Provider resolver。是否接线由 v0.2.0 [#1142](https://github.com/rushsinging/aemeath/issues/1142) 决策。

## 1. 入站端口（OHS + Published Language）

`AgentClient` trait（`packages/sdk`）= 核心域对外的入站端口 + 发布语言，供 CLI/TUI/Server 消费。所有权属 Agent Runtime，独立成 crate 仅为依赖倒置。契约细节见 [../../01-system/03-context-map.md](../../01-system/03-context-map.md)。

### 同步打断入口

```rust
trait AgentClient {
    // 其他命令省略
    fn cancel_run_step(
        &self,
        run_id: RunId,
        step_id: Option<RunStepId>,
        deadline: ControlDeadline,
    ) -> CancelRunStepOutcome;
    fn terminate_run(
        &self,
        run_id: RunId,
        reason: RunTerminationReason,
        deadline: ControlDeadline,
    ) -> TerminateRunOutcome;
    fn reply_interaction(
        &self,
        request_id: InteractionRequestId,
        reply: InteractionReply,
    ) -> InteractionCommandOutcome;
    fn cancel_interaction(
        &self,
        request_id: InteractionRequestId,
        reason: InteractionCancelReason,
    ) -> InteractionCommandOutcome;
    /// /think 命令入站入口：设置下一个 Run 的 reasoning level 上限。
    /// 返回 Workflow-owned requested 值（Config user maximum clamp 已退役，#921；NEVER 经 Provider resolver 计算 effective）。
    fn set_reasoning_level(
        &self,
        session_hint: SessionId,
        level: ReasoningLevel,
    ) -> ReasoningLevelOutcome;
}

enum ReasoningLevelOutcome {
    Accepted { requested: ReasoningLevel }, // Workflow requested 值（Config user-max clamp 已退役，#921）
    Unsupported,
}

enum CancelRunStepOutcome {
    Accepted,              // 返回前当前 Step 已进入 CancellingStep，Step scope 已触发
    AlreadyCancelling,
    NoActiveStep,
    RunTerminating,
    RunTerminal,
    NotFound,
}

enum TerminateRunOutcome {
    Accepted,              // 返回前 Run 已进入 Terminating，Run root scope 已触发
    AlreadyTerminating,
    AlreadyTerminal,
    NotFound,
}

enum RunTerminationReason {
    UserExit,
    DoubleCtrlC,
    QuitCommand,
    ProcessSignal,
    SessionShutdown,
    ParentStepCancelled,
}

struct ControlDeadline { unix_millis: u64 } // wire-only absolute deadline

enum InteractionCommandOutcome {
    Accepted,                       // waiter 已在返回前完成一次性解析
    NotFound,
    AlreadyCompleted,
    InvalidReply(InteractionReplyError),
    RunCancelling,
}
```

- `cancel_run_step` 与 `terminate_run` 是同步、幂等、out-of-band 的控制命令，NEVER 经 `InputBuffer` 排队。
- `ControlDeadline` 是 wire-only 绝对时间；Runtime 在控制边界转换到注入的 monotonic clock，嵌套 Sub **NEVER** 重新分配 5s/10s。
- TUI 只持 `Arc<dyn AgentClient>` 或 SDK 提供的、绑定 `run_id` / `step_id` 的薄控制 handle；NEVER 持有 Runtime 实例、Run 聚合或 `CancellationToken`。
- `CancelRunStepOutcome::Accepted` 只确认 Step scope 已即时停止调度；完成由 `RunStepCancelled` / `RunDrainingInput` 异步确认。`TerminateRunOutcome::Accepted` 只确认 Run root scope 已触发；完成由 `RunTerminated` 确认。
- 迁移期旧 `cancel_run` / `CancelRunOutcome` 只允许为当前 TUI 生产兼容保留；#878 原子切换后由 #879 删除，**NEVER** 作为目标 OHS 的第二套语义。
- interaction reply / cancel 同样是同步、幂等、out-of-band command；它们只完成 Runtime-owned pending request，**NEVER** 经输入队列排队，也 **NEVER** 由 TUI 持有 channel sender。
- SDK Published Language 的 `RunStepId`、`AgentId`、`InteractionRequestId`、`InteractionReply`、`InteractionCancelReason`、`InteractionCommandOutcome` 与 `ChatEvent::InteractionRequested` **MUST** 可序列化且不含 channel / lock / Runtime handle。#874 已建立这些强类型 identity、纯值 DTO/outcome 与纯 event mapping；旧 `AskUserBatch.reply_tx` 只作为 #878 生产切换前的兼容路径存在，**NEVER** 进入新 Interaction PL。当前只要求 local adapter；远端帧、重连与 WSS 行为不在 v0.1.0 冻结。

## 2. Runtime 消费的能力契约

### Loop Engine 与窄能力 Port

Loop Engine 直接编排 `Run + RunExecutionState + RuntimeContext`，不消费可替换整段流程的 application adapter。`RunLoopPort`、`MainRunPort`、`SubAgentRun` 及按 Main/Sub 分类的 Input/Event/LLM/Tool strategy 不属于目标模型。

Runtime-owned Port 只对应真实外部 seam：

- `InputPort`：SessionIngress 分类后的 Run 输入接纳端口，只接收 `UserMessage`，负责 FIFO、batch drain、epoch、Open/Sealed/Closed 生命周期和 AwaitingInput park；不拥有 Step 或 continuation；
- `EventSink`：接收领域事件；跨 SDK 边界的纯值转换由 `sdk_event_mapper` 完成；
- `ProviderPort`、`ToolCatalogPort`、`ToolExecutionPort`、`ContextPort`：执行单一外部能力；
- `InteractionPort`：一次 request/reply transport；reply/cancel 按 `run_id + request_id` 直接完成 pending interaction，**NEVER** 经普通输入队列排队；
- `CommandScheduler`：接收已分类的 Runtime command，按 ImmediateControl、AtRunBoundary、SessionQuery 三类决定执行时机，不把命令伪装成 UserMessage；
- `HookPort`：Hook BC 的 typed dispatch；
- `UsageSink`、`ActiveRunRegistryPort`：非业务流程的窄出口。

`freeze_step`、`accept_step_input`、`invoke_model`、`execute_tools`、`evaluate_stop_hook`、`finalize_step`、`claim_terminal` 等是 application 流程或领域动作，不得放进 Port。不同 Run 来源的差异必须在 `RuntimeContextFactory` 装配期绑定为 capability adapter，Engine 不按来源分支。

供应能力发布的 OHS **MUST** 只在各自战术文档定义完整签名；本文只登记 Runtime 的使用面，**NEVER** 复制第二份 trait 真相。Runtime 只消费这些 façade，**NEVER** 再定义同义 wrapper：

| 供应能力 | Runtime 消费的窄契约 | 用途 / 唯一真相 |
|---|---|---|
| Context Management | `ContextPort` | 构建 / 压缩 / 查询 compact 状态 / 追加持久化 Context；见 [Context Management](../context-management/02-compact.md) 与 [持久化摘要树](../context-management/06-persistent-summary-tree.md) |
| Tool | `ToolCatalogPort` / `ToolExecutionPort` | schema 投影与单次执行；见 [Tool ports](../tools/02-ports-and-lifecycle.md) |
| Policy | `PolicyPort` | 调用前决策；见 [Policy](../policy/README.md) |
| Memory | `MemoryPort` / `ReflectionWorkflow` / `ReflectionHistoryStore` | 当前项目 Memory，以及 Memory-owned prompt/parse/apply/history workflow；Runtime 只负责 Reflection 触发、Provider 调用和任务生命周期，见 [Memory ports](../memory/04-ports-and-adapters.md) |
| Task | `TaskAccess` | 日常 Task 命令 / 查询；`TaskPersist` **NEVER** 进入 Runtime；见 [Task contracts](../task/02-ports-and-published-language.md) |
| Hook | `HookPort` | 类型化 hook dispatch；Runtime 直接消费 Hook-owned façade，不定义同义 Port/Outcome。`HookOutcome` 经 application `outcome_mapper` 无损映射 directive、结构化 reason、全部 attempts 与 typed display messages；updated input 在 `tool_coordination` 重新经过 frozen Catalog、Tools-owned schema validation 与 Policy；见 [Hook](../hook/README.md) |
| Workflow | `ReasoningPort` | effort 调节；见 [Workflow](../workflow/01-reasoning-graph.md) |
| Config | `ConfigSnapshot` PL | 本 Run 的只读配置快照；见 [Config](../config/01-config-layer.md) |

`ProviderPort`、`InteractionPort`、`EventSink`、`UsageSink`、`InputPort`、`CommandScheduler` 与 `InteractionInbox` 隔离 Runtime 策略和易变外部 detail，因此由 Runtime 拥有。它们的**唯一签名真相源**分别是：`ProviderPort` 见本文 §2.1；`InteractionPort` 及其 Published Language 见本文 §2.2；`EventSink` / `UsageSink` 见本文 §2.3。Provider 文档只登记 adapter 实现面，本文其余章节和其他供应方文档只登记使用面或 adapter 行为，**NEVER** 复制第二份 trait：

### 2.1 Runtime-owned ProviderPort

```rust
trait ProviderPort: Send + Sync {
    fn capabilities(&self, model: &ModelId) -> Result<ModelCapability, ProviderError>;
    fn resolve_invocation_options(
        &self,
        model: &ModelId,
        requested: RequestedInvocationOptions,
    ) -> Result<ResolvedInvocationOptions, ProviderError>;
    async fn invoke(
        &self,
        request: InvocationRequest,
        cancellation: &dyn CancellationSignal,
    ) -> Result<InvocationStream, ProviderError>;
}
```

Provider BC 的 ACL / adapter 实现该 Runtime-owned SPI；完整 stream、client scope 与能力映射说明见 [Provider adapter design](../provider/02-ports-stream-and-client-scope.md)，但不得在那里复制 trait。

Runtime 同时拥有 `ProviderFactory` 与 `ProviderBinding`（定义在 `runtime::ports::provider_factory`）：

```rust
trait ProviderFactory: Send + Sync {
    fn build(&self, spec: ProviderBuildSpec) -> Result<ProviderBinding, ProviderError>;
}

struct ProviderBinding {
    provider: Arc<dyn ProviderPort>,
    model: ModelId,
    max_tokens: u32,
    requested_reasoning: ReasoningLevel,
    context_window: Option<usize>,
}
```

Runtime Main/Sub/Reflection/Compact 只依赖 `ProviderFactory` / `ProviderBinding` / `ProviderPort` 与 Provider Published Language，**NEVER** 直接持有具体 client、pool 或 driver。Composition 实现 `ProviderFactory`，经 Provider crate 的 `provider::composition` 模块独占构造——非 Composition crate **NEVER** 引用该模块或具体构造符号（`check-provider-construction-ownership.sh` 零白名单守卫锁定）。

> **v0.1.0 scope（#921 收缩）**：`resolve_invocation_options` 领域模型已完成迁移，但 Runtime **尚未**在生产链路调用该方法；effective reasoning 尚未端到端冻结。是否接线由 v0.2.0 #1142 决策。#1142 仍延期，**NEVER** 冒充已完成。

### 2.2 Runtime-owned InteractionPort 与交互语言

```rust
#[async_trait]
trait InteractionPort: Send + Sync {                 // Runtime-owned 出站端口
    async fn request(
        &self,
        request: InteractionRequest,
        cancellation: &dyn CancellationSignal,
    ) -> Result<InteractionCompletion, InteractionError>;
}

struct InteractionRequest {
    id: InteractionRequestId,                        // Runtime 在进入等待态前生成
    run_id: RunId,
    body: InteractionRequestBody,
}

enum InteractionRequestBody {
    UserQuestions(Vec<UserQuestion>),
    ToolApproval(ToolApprovalPrompt),
    PlanApproval(PlanApprovalPrompt),
    HardPause(StuckDiagnostic),
}

struct UserQuestion {
    prompt: String,                 // 向用户展示的问题文本
    options: Vec<String>,           // 可选选项；空 = 自由文本回答
    allow_multi: bool,              // 是否允许多选
}

struct ToolApprovalPrompt {
    tool_name: String,
    args_summary: String,           // 人可读的参数摘要（非完整 JSON）
    risk_level: RiskLevel,          // Low / Medium / High
}

struct PlanApprovalPrompt {
    plan_title: String,
    steps: Vec<String>,             // 计划步骤列表
}

struct StuckDiagnostic {
    reason: String,                 // StuckGuard 触发原因
    recent_actions: Vec<String>,    // 最近 N 个 action 描述
}

enum RiskLevel { Low, Medium, High }

enum ApprovalDecision {
    Approve,
    Deny { reason: Option<String> },
}

enum InteractionReply {
    UserQuestions(Vec<UserAnswer>),
    ToolApproval(ApprovalDecision),
    PlanApproval(ApprovalDecision),
    HardPauseContinue,
}

struct UserAnswer(String); // 与 UserQuestions 按位置一一对应；不得丢项、重排或附加隐式默认值

enum PlanApprovalOutcome {
    Approved,
    Deny { feedback: String }, // 作为下一 invocation 的 typed context input
}

enum InteractionCompletion {
    Replied(InteractionReply),
    Cancelled(InteractionCancelReason),
}
```

`InteractionPort` 只承载一次 request/reply 交换，**NEVER** 自行修改 Run 或发布 `RunResumed`。Runtime interaction coordinator 在调用前以 request id + continuation 进入 `AwaitingUser`，收到匹配 reply 后恢复 continuation并发布权威事件；取消与 reply 竞争时 cancellation 优先，陈旧 / 重复 id 返回结构化错误。Client adapter 把 request 映射为 SDK event 并等待 TUI / Server 回复；`ParentMediated` 必须装配独立的 child-scoped adapter，维护 `child_run_id + child_request_id → parent route` 映射，**NEVER** 直接复用父 `InteractionPort` Arc 或暗中共用 Main UI channel。child teardown 只 drain 自身请求，不能影响 parent 或 sibling；父级诊断/进度是 EventSink 投影，不是 interaction completion。

reply 必须与 request body 同 variant；`InvalidReply` 不消费 waiter。`InteractionCompletion::Cancelled` 是“取消这次交互”，不是 `CancelRunStep` / `TerminateRun` 的别名；Runtime 按 continuation 穷尽映射 typed 结果：

| continuation | Replied | Cancelled(reason) | 恢复后的 Run 状态 |
|---|---|---|---|
| `CompleteToolCall(id)` | answers → 同一 ToolCall 的 `ToolSuccess` | `ToolCancelled(UserInteractionCancelled(reason))` | `ExecutingTools`；继续下一个 suspension |
| `ContinueToolApproval(id)` | Approve → Ready；Deny → `ToolCancelled(ApprovalDenied)` | `ToolCancelled(ApprovalCancelled(reason))` | `AwaitingToolApproval`；继续处理其余原始调用 |
| `ContinuePlanApproval` | Approve → `PlanApproved`；Deny → `PlanRejected` feedback；决定随当前无 tool_calls 的 step 恰好一次提交 | `RunFailed(PlanApprovalCancelled(reason))` | reply 回 `PreparingContext` 并启动下一 invocation；cancel 回 `Failed` |
| `ContinueAfterHardPause` | `HardPauseContinue` | `RunFailed(HardPauseCancelled(reason))` | reply 回 `ExecutingTools` 并继续 continuation 记录的未完成 tool phase；cancel 回 `Failed` |

Run root / Step cancellation scope 若与 reply/cancel 竞争则永远优先：`CancelRunStep` 进入 `CancellingStep` 并收口到 `DrainingInput`；`TerminateRun` 进入 `Terminating` 并最终 `Terminated`，**NEVER** 套用上表的普通 completion。

并发 Tool execution 可以同时产生多个 `ToolOutcome::Suspended`，但 Runtime **MUST** 先收集 outcomes，再按 RunStep 原始 ToolCallId / 调用顺序逐个注册 request。前一个 continuation resolve 并清空 PendingInteraction 后才能注册下一个；全部调用得到 final outcome 后，按原调用顺序做 L1 budget reduction，并以一次 `append_and_persist` 提交 assistant + tool results。

Client interaction adapter **MUST** 在 Runtime-side bridge 中先注册 `InteractionRequestId → pending waiter`，再发出纯值 `ChatEvent::InteractionRequested { request_id, run_id, body }`。`AgentClient::reply_interaction` / `cancel_interaction` 回到同一 bridge，校验 body-specific reply 后恰好一次完成 waiter；stream、TUI 与 SDK event **NEVER** 携带 sender。processing teardown 不拥有 waiter，Run cancellation 才由 Runtime drain 该 Run 的 pending request 并发布权威 cancellation 事件。

### 2.3 Runtime-owned EventSink / UsageSink

```rust
trait UsageSink: Send + Sync {                         // Runtime-owned outbound port；Audit adapter 实现
    fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome;
}
trait EventSink: Send + Sync {                         // 纯投影出口；NEVER 承载 Sub Run 业务返回
    fn emit(&self, events: Vec<DomainEvent>);
}
```

`UsageRecord`、`UsageEmitOutcome` 与 `UsageDropReason` 是 Audit-owned Published Language，以 [Audit 模块设计](../audit/README.md) 为唯一类型真相；Runtime-owned `UsageSink` 只定义非阻塞提交对话，并直接 import/re-export Audit 类型，**NEVER** 复制同名 DTO。为避免 crate 循环，Audit crate 不依赖或实现 `UsageSink`；#931 由 Composition Root bridge 同时依赖 Runtime trait 与 Audit sender handle 并完成实现。`EventSink` 只投影 `Run` 聚合已产生的领域事实，Main 通常映射到 SDK/TUI，Sub 可映射到父级诊断流；父 Run 的 `tool_coordination` **MUST** 直接消费 `derive_sub_run` 返回的 typed `AgentRunTerminal`，**NEVER** 订阅 EventSink 来提取成功结果或错误。`UsageSink::try_record` 是 best-effort 非阻塞审计出口，接受或丢弃都不改变 Run 状态。

### 2.4 Reflection 异步执行 adapter（#899）

Runtime 拥有 Reflection 的执行编排；Memory 拥有 prompt/parse/apply 领域能力与 history append/query。Interval、PreCompact、Manual 三种 trigger **MUST** 全部提交到同一个 Runtime 单槽后台 adapter：

```text
submit(trigger, owned message snapshot)
  ├─ Accepted    → spawn Provider call → parse → optional apply
  │                 → ReflectionHistoryStore.append(record)
  │                 → safe completion metadata → release slot
  └─ BusySkipped → immediately return；不等待、不排队
```

- `Manual` 只表示 Runtime 显式执行 trigger；它不建立同步路径，也不能绕过 slot。
- `/reflect [limit]` 是独立的只读 control/query：Runtime 调用 Memory-owned `ReflectionHistoryQuery::list(limit)` 取得安全摘要，并仅映射为 SDK view；它 **NEVER** submit job、调用 Provider 或 apply Memory。
- 后台完成**不主动**经 `EventSink` / SDK 向 TUI 投影 `ReflectionOutput`、formatted content 或完成正文。只有用户显式 `/reflect [limit]` 时才返回 newest-first 的安全 metadata/count 视图。
- completion 与日志只能含 trigger、status、error category、token/count、duration、record id 等 metadata；**NEVER** 含 prompt、消息、Memory content、provider raw response、parsed/formatted output 或正文截断。
- Run teardown 必须 drain 该 Run 的 Reflection slot；到结束 deadline 仍未完成时 cancel，并等待 cancellation/timeout 终态清槽后再释放 Run lease，**NEVER** 留下 detached job。adapter 自身的执行 timeout 同样只产出安全终态 metadata。

## 3. RuntimeContext、SessionState 与 IoC 装配

`RuntimeContext` **MUST** 只持有本 Run 消费的活契约，**NEVER** 持有 Project wiring、composition scope、Session coordinator 或 active resource slot。`RuntimeServices` 持有 Runtime 生命周期稳定的共享 Port 与 Runtime-owned factory contracts；`SessionState` 持有跨 Run 变化的会话事实。Composition 保存供应 BC 的 opaque wiring，并实现 Runtime 定义的 factory/port；Runtime application 决定 snapshot 时机与 `RunSpec` 能力选择。

调用方只提交 `RunCreationRequest { spec, session, parent }`。`RuntimeContextFactory` 负责绑定 `RuntimeContext`；Runtime-owned `RunFactory::create` 协调创建处于 `Idle` 的 `RunInstance { run, context, execution }`。调用方不得构造具体 Port 后通过 `RunContextBindings`、`RuntimeContextParts` 或同构参数袋手填 Context；输入内容也不得混入创建请求，首次和后续输入统一经 `InputPort` 激活 Run。

```rust
// Runtime-owned application contracts.
trait RuntimeContextFactory: Send + Sync {
    async fn create(
        &self,
        spec: &RunSpec,
        session: &SessionSnapshot,
        parent: Option<&ParentRunCapabilities>,
    ) -> Result<RuntimeContext, RuntimeContextError>;
}

struct RunFactory {
    context_factory: Arc<dyn RuntimeContextFactory>,
}

impl RunFactory {
    fn create(
        &self,
        request: RunCreationRequest,
    ) -> Result<RunInstance, RunCreationError>;
}

struct RunCreationRequest {
    spec: RunSpec,
    session: SessionSnapshot,
    parent: Option<ParentRunCapabilities>,
}

struct RunInstance {
    run: Run,
    context: RuntimeContext,
    execution: RunExecutionState,
}

// agent/composition 内部；实现 Runtime-owned factory contracts，
// 但不进入 Runtime 的业务 API。
struct RuntimeAssembly {
    runtime_services: RuntimeServices,
    session_state: SessionState,
    run_factory: RunFactory,
    workspace_wiring: project::WorkspaceWiring,
    session_wiring: context::SessionWiring,
    task_wiring: task::TaskWiring,
    config_wiring: config::ConfigWiring,
}
```

```rust
// Composition 只实现 Runtime-owned contract；具体 wiring 不穿越边界。
fn compose_runtime(graph: CompositionGraph) -> RuntimeAssembly {
    let wiring = graph.build_opaque_wiring();
    let services = RuntimeServices::from_factories(
        ProviderBindingFactoryImpl::new(wiring.provider),
        ContextBindingFactoryImpl::new(wiring.context),
        ToolBindingFactoryImpl::new(wiring.tools),
        InteractionBindingFactoryImpl::new(wiring.interaction),
        HookBindingFactoryImpl::new(wiring.hooks),
        WorkspaceBindingFactoryImpl::new(wiring.workspace),
    );
    RuntimeAssembly::new(
        services.clone(),
        Arc::new(RuntimeContextFactoryImpl::new(services)),
        SessionState::restore(wiring.session_store),
    )
}

// Runtime application 先创建 Idle Run；输入单独经 InputPort 激活。
async fn launch_run(
    assembly: &RuntimeAssembly,
    session: &SessionState,
    parent: Option<ParentRunCapabilities>,
) -> Result<RunInstance, RunError> {
    let request = RunCreationRequest {
        spec: RunSpec::from_parent(parent.as_ref())?,
        session: session.snapshot_for_run()?,
        parent,
    };
    assembly.run_factory.create(request)
}

async fn activate_run(
    instance: RunInstance,
    input: InputEnvelope,
) -> Result<AgentRunTerminal, RunError> {
    instance.context().input().submit(input).await?;
    run_launcher::launch(instance).await
}
```

`launch_run` 与 `activate_run` 形成唯一调用链：入站 args → typed bootstrap → Session snapshot → RunSpec → `RunCreationRequest` → `RunFactory::create` → `RuntimeContextFactory` → Idle `RunInstance`；随后任意来源输入 → `InputPort::submit` → `RunLauncher::launch` → 状态机激活 → Loop Engine。任何调用方直接构造 Provider、Tool、Interaction、Hook、Workspace 或 `RuntimeContext`，都表示绕过 IoC；任何把首次输入塞进创建请求、拆散 `RunInstance` 启动或按 Main/Sub 选择 assembler 的路径，都表示重新引入启动特例。

### 3.1 IoC 合同与验证伪代码

```rust
#[test]
fn composition_implements_runtime_contract_without_leaking_wiring() {
    let assembly = compose_runtime(test_graph());
    let prepared = block_on(assembly.run_factory.prepare(root_request())).unwrap();
    assert!(prepared.context.has_provider());
    assert!(!prepared.context.exposes_composition_wiring());
}

#[test]
fn child_preparation_cannot_expand_parent_capabilities() {
    let parent = parent_capabilities_with_tool_scope(ToolScope::ReadOnly);
    let request = child_request(parent).with_tool_scope(ToolScope::Write);
    assert!(matches!(prepare(request), Err(RunCreationError::CapabilityExceeded)));
}

#[test]
fn parent_mediated_interaction_is_child_scoped() {
    let child_a = prepare_child("a");
    let child_b = prepare_child("b");
    let request_a = child_a.request_interaction();
    let request_b = child_b.request_interaction();
    assert_ne!(request_a.route_key(), request_b.route_key());
    assert!(child_a.cancel(request_a.id()).is_ok());
    assert!(child_b.is_pending(request_b.id()));
}

#[test]
fn sub_hook_is_empty() {
    let hooks = prepare_sub_run().context.hooks();
    assert_proceed_without_dispatch(hooks, any_hook_invocation());
}
```

测试伪代码要求每个断言绑定一个 IoC 不变量：Composition 实现契约、Factory 执行能力选择、ParentMediated 隔离 identity、Sub Hook 绑定独立空实现；不能只用最终 Loop 测试替代这些相邻契约。

Run 准备时，Runtime bootstrap/application 从 `SessionState` 捕获一致的 `SessionSnapshot`；若为派生 Run，只提供受限 `ParentRunCapabilities`。Factory 实现可借助 composition-private wiring 获得 lease、派生 workspace、打开 Context/Memory、构造 Provider/Tool/Hook adapter，但返回给 Runtime 的只有冻结能力。lease 必须由返回 capability 的生命周期守卫持有，调用方不能单独缓存或释放。

`InteractionBindingMode::ParentMediated` 创建独立 child-scoped adapter；Sub Hook capability 创建不持底层 HookPort 的 `EmptyHookPort`，所有 invocation 都无副作用返回。两者都必须在 factory 内完成，不能先返回完整父 Port 再要求调用方自律过滤。

`reasoning` 装配 **MUST** 只构造 Workflow-owned requested-level 状态：Adaptive 使用五节点固定默认 effort，Fixed 使用 RunSpec 声明值，Inherit 冻结父 requested value，NoOp 绑定无副作用实现。它 **NEVER** 接收具体 Provider client；每次 invocation 的 model clamp 由 Loop 在 `build_window` 前经 `ProviderPort` 完成。

> **v0.1.0 scope（#921 收缩）**：Provider option resolver 领域模型已完成迁移但 Runtime **尚未**在生产链路调用 `resolve_invocation_options`；ReasoningPort **尚未**接线到生产 loop。上述 clamp 链是 Target 设计，v0.1.0 未接生产链路。是否接线由 v0.2.0 [#1142](https://github.com/rushsinging/aemeath/issues/1142) 决策。

以 `WorkspaceMode::Inherit` 创建的 Run，factory 实现必须在一致性 gate 下取得当前 Session 的 Context / Memory / ConfigSnapshot / TaskAccess 与 owned shared lease；返回的能力必须绑定该 lease 的逻辑生命周期，不能单独缓存或逃逸。由此 Context、MemoryTool、TaskTool 与 Reflection 看到同一实例与项目配置，而 restore authority 仍留在 composition-private session wiring。

shared lease 必须保持到该 Run、全部 Tool、后台 Reflection job 与其派生 Run 均结束或取消收口。运行期 resume 只有取得 exclusive lease 后才可 prepare/commit；exclusive resume 前必须 join 或取消仍持 lease 的工作，旧 Memory/Task/Workspace capability 不得在切换后继续写旧项目。

session wiring 内部可持有唯一 SessionSwitchCoordinator、稳定 backing、TaskPersist、active Memory slot 与 Config participant view；这些 active slot 与 commit authority **NEVER** 进入 RuntimeContext。所有 project-scoped factory 必须显式消费 `SessionSnapshot` 中冻结的 config revision/snapshot，不得回读 Composition Root 的动态 current config。

`WorkspaceMode::Snapshot` 只允许从 parent capability 派生 isolated workspace，并按 `RunSpec` 创建 isolated Task/Context。`MemoryMode::Disabled` 绑定 NoOp；显式 shared memory 只能复用父 capability 所允许的 Arc，并由父 lease 覆盖派生 Run 生命周期。Registry Scope、Tool Profile、Policy、Hook 与 Interaction 能力都只能保持或收缩，不能因来源名称获得特权。

## 4. Composition Root

- **唯一生产对象图入口**：`agent/composition`。Runtime 的 `domain/application/ports/adapters` 定义领域行为、应用用例、能力选择规则、Runtime-owned factory/port contracts 与转换；Composition 实例化具体实现并把 object graph 注入 Runtime。Runtime application 在业务时机调用统一 `RuntimeContextFactory`，但不触发供应 BC 的 concrete constructor。
- `agent/composition` 持有各 Port 的具体实现或供应模块提供的 composition-only opaque wiring（provider driver / tool registry / storage / workspace / hook …），并实现 Runtime-owned factory contracts。动态 Catalog 与 MCP lifecycle 仍由对应供应边界管理，不能泄漏 concrete wiring 给 Runtime。
- **Provider 构造独占（#907）**：Composition 实现 Runtime-owned `ProviderFactory`，经 Provider crate 的 `provider::composition` 模块独占具体 provider client / driver / transport 构造。非 Composition crate **NEVER** 引用 `provider::composition` 或具体构造符号（`LlmClient` / `LlmConfigOptions` / `InvocationScope` / `SystemBlock` / `LlmProvider`）。`check-provider-construction-ownership.sh` 守卫以零白名单与负向探针锁定此边界。
- Runtime feature 内 **NEVER** 建立 `bootstrap/`、service locator 或第二个 Composition Root；现有 Runtime `utils/bootstrap` 的生产构造责任迁入 `agent/composition`，其余代码按单一 `agent_execution` 能力的六边形职责归位。
- `RuntimeContext` 属 application：它只传递本 Run 的活契约，**NEVER** 进入 domain 或通用 shared，也 **NEVER** 保存具体 Provider、Registry、Store 或全局 Config reader。
- Runtime 当前只有一个完整业务能力，因此 **NEVER** 添加单元素 `capabilities/agent_execution` 包装；没有真实跨 capability 复用内容时也 **NEVER** 创建 `shared/`。
- Project workspace 的生产装配 **MUST** 经 Project-owned factory 取得 `WorkspaceWiring`，并 **MUST** 只保存在 `CompositionWorkspaceScope`；Composition **NEVER** 向 Runtime 或业务模块分发 handle / scope。
- 独立根 Run：Composition Root 初次建立 Workspace 与 Session wiring 并跨 Run 保留；每个 Run 由 Runtime 从 `SessionState` 捕获 snapshot 后调用统一 factory。resume 在 exclusive lease 内更新完整 live state，**NEVER** 重建 wiring。
- 派生 Run：Runtime 提交带 `ParentRunCapabilities` 的同一种 `RunCreationRequest`；factory adapter 从 parent workspace capability 隔离派生并装配相同结构。Runtime tool coordination 只消费统一 `RunInstance` / typed terminal。
- 任何模块 **NEVER** 私自 `new` Port 实现绕过 Composition Root。

## 5. 关键 ACL

1. **Provider 内部**：各家 LLM API → 统一 `InvocationDelta` + 领域 `Message`
2. **sdk_event_mapper**：领域 `DomainEvent` → SDK `ChatEvent`（Main/Sub 路由 + agent_id）
3. **Session 快照组装**：Context Management backing implementation 直接经注入的 `TaskPersist` / Project-owned `WorkspacePersist` 收集与恢复；Runtime 只有 `TaskAccess`，且 **NEVER** 中转 Workspace 能力
4. **Workspace / Session scope 隔离**：Composition 保留 Project 与 Context-owned opaque wiring；Main 在同一 active slot 内跨 Run 复用，Sub 从父 workspace scope 隔离派生；scope / wiring / lease **NEVER** 穿过 Runtime、Tool 或普通 ContextPort 边界
5. **Interaction ACL**：Tool-owned `UserInteractionSpec` / Policy 决策 → Runtime-owned `InteractionRequest` → adapter SDK DTO；reply 按 request id 回到 Runtime continuation，TUI DTO / channel **NEVER** 进入 Run 聚合或 Tool BC
6. **Reflection history ACL**：Memory-owned `ReflectionRecord` → Runtime safe-summary view → SDK `ReflectionHistoryView`；正文、prompt 与 raw response **NEVER** 进入 SDK/TUI 或日志

## 6. 契约治理

本文 **MUST** 只定义 Target 契约。Runtime 能力契约、取消链路与 composition-internal workspace scope 的 Current → Target 差距、责任和退出条件 **MUST** 只在 [迁移治理](../../03-engineering/03-migration-governance.md) 维护，**NEVER** 在本文复制进度表。

## 7. 相关文档

- 领域模型（RunSpec/RuntimeContext）：[01-domain-model.md](01-domain-model.md)
- 模块边界：[02-module-boundaries.md](02-module-boundaries.md)
- Context Management 战术设计（ContextPort 与私有 PromptPipeline）：[../context-management/02-compact.md](../context-management/02-compact.md)
- 上下文地图（BC 集成）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 系统架构（Composition Root）：[../../01-system/04-system-architecture.md](../../01-system/04-system-architecture.md)
- Provider 端口、流与 Invocation Scope：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- Project Workspace 端口与 wiring：[../project/02-ports-and-adapters.md](../project/02-ports-and-adapters.md)
- 代码组织规范：[../../01-system/06-code-organization.md](../../01-system/06-code-organization.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

> **#899 durable lifecycle / compact boundary:** accepted job 先 append `Running`，成功、失败、partial apply、timeout/cancel 均以同 id `upsert` 终态；cancel 不删除 durable fact。PreCompact 只在 compact 成功产生 outcome 后 submit 预先冻结的“将被丢弃”快照；compact 失败不 submit，busy 结构化 warn 后立即 skip，绝不排队。

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-23 | #1296 以聚合 Guard 验收 Session、Config Store、Runtime Tool 与 Hook 的 Composition-only concrete construction；MCP lifecycle 继续唯一归 #1327 | [#1296](https://github.com/rushsinging/aemeath/issues/1296) / [#1327](https://github.com/rushsinging/aemeath/issues/1327) |
| 2026-07-22 | #1295 将 Hook dispatcher 的 production construction 上移至 Composition；Runtime Main/Sub 只复用 injected HookPort；MCP lifecycle 仍由 #1327 承接 | [#1295](https://github.com/rushsinging/aemeath/issues/1295) / [#1327](https://github.com/rushsinging/aemeath/issues/1327) |
| 2026-07-21 | #1294 将 Tool Catalog/Execution/binding、Skill Catalog/materializer、Tool Result materializer 与 ActiveRunRegistry 的 production assembly 上移至 Composition；MCP Ready lifecycle 接线延期至 #1327 | [#1294](https://github.com/rushsinging/aemeath/issues/1294) / [#1327](https://github.com/rushsinging/aemeath/issues/1327) |
| 2026-07-20 | #1285 为 Run teardown 落地有界 drain→cancel→terminal 收口；Manual 显式入口由 #1289（归 #860）承接 | #1285/#1289/#860 |
| 2026-07-20 | #1284 接通 compact 成功后的 PreCompact 冻结快照单槽提交；Manual 显式入口与有界 teardown/cancel 分别由 #1289/#1285 承接 | #1284/#1289/#1285 |
| 2026-07-20 | #1283 将 Reflection history query 收窄为 Memory 直接返回安全摘要，Runtime 只映射 SDK view，完整 record 不越过 query 边界 | #1283 |
| 2026-07-19 | #907 补入 Runtime-owned `ProviderFactory` / `ProviderBinding` / `ProviderBuildSpec` 契约定义；明确 Runtime Main/Sub/Reflection/Compact 只依赖这三个 port 与 PL，Composition 独占 `provider::composition` 构造面；#1142 resolver `build_window` 接线仍延期 | [#907](https://github.com/rushsinging/aemeath/issues/907) |
| 2026-07-18 | #899 完成 Reflection 三 trigger Runtime 单槽异步、busy skip、静默完成、Memory-owned history append/query、`/reflect [limit]` 只读安全投影及 Run teardown drain/cancel timeout | #899 |
| 2026-07-11 | 初稿：入站端口、出站端口签名、RuntimeContext 按 RunSpec 装配、Composition Root、ACL、实现缺口 | #761 |
| 2026-07-11 | RuntimeContext/assemble 补入站端口 InputBuffer（Main=TUI 通道+buffer，Sub=固定队列）| #761 |
| 2026-07-12 | 定义同步幂等 `cancel_run(run_id)`、per-Run cancellation scope 及 Provider/Tool/Compact/Hook 传播边界 | #700 |
| 2026-07-15 | OHS 目标从旧 `cancel_run` 修正为 `cancel_run_step` + `terminate_run`，冻结 pure DTO、绝对 deadline 与迁移兼容边界 | [#700](https://github.com/rushsinging/aemeath/issues/700) / [PR #1036](https://github.com/rushsinging/aemeath/pull/1036) |
| 2026-07-12 | ToolPort 拆为 Catalog/Execution 双端口，补 Skill/Command 独立端口边界与 Scope/Profile 装配 | #787 |
| 2026-07-12 | ProviderPort 补能力查询、取消、结构化错误与单 attempt InvocationStream 契约 | #788 |
| 2026-07-12 | ContextPort 签名收敛为 4 方法（build_window / needs_compaction / compact / append_and_persist），详见 context-management/02-compact.md | #786 |
| 2026-07-12 | Policy 装配收缩为 AllowAll；Hook 收敛单 dispatch；Audit 出站收缩为非阻塞 UsageSink | #790 |
| 2026-07-14 | 移除 Runtime Workspace 端口；由 active-session-slot CompositionWorkspaceScope 保留 Main wiring，Sub 在 AgentDispatch 内派生；Context / Runtime / Tool 共享同一 Task、Memory 与 Project view；补齐 Runtime-owned InteractionPort | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 固化 Provider option resolver、reasoning_for 边界与四类 typed interaction continuation；并发 suspension 串行化为单 PendingInteraction | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | `ReasoningLevelOutcome::Accepted` 字段从 `effective` 改为 `requested`，对齐 Workflow 的 `/think` 反馈决策：命令层只暴露 user-max-clamped requested 值，NEVER 承诺尚未计算的 provider-ceiling-resolved effective 值 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-15 | 经能力事实复核，Runtime 当前只有单一 `agent_execution` 能力；端口与适配器作为 crate 根六边形层组织，`agent/composition` 保持唯一对象图与 factory 入口 | [#995](https://github.com/rushsinging/aemeath/issues/995) |
| 2026-07-15 | 曾按多个稳定能力递归竖切并把 Port/adapter 就近分散；此结论已由上一条复核记录取代 | [#995](https://github.com/rushsinging/aemeath/issues/995) |
| 2026-07-17 | #921 收缩范围：Config `reasoning_graph` 退役后 `reasoning_for` 移除 config graph 参数；Provider resolver 领域迁移完成但未接生产链路；Main 已通过 ReasoningPort 接线；Provider resolver 尚未接线；Runtime/Context/TUI 均未端到端消费 resolver 或 ReasoningPort；是否接线由 v0.2.0 #1142 决策 | [#921](https://github.com/rushsinging/aemeath/issues/921) |
| 2026-07-18 | ContextPort 增加只读 `compact_status`，Runtime 只消费 coverage / phase / usage Published Language，不接触 scheduler、manifest 或 shard | [#1162](https://github.com/rushsinging/aemeath/issues/1162) |
