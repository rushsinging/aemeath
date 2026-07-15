# Agent Runtime · 端口与适配器

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 Agent Runtime 的入站 OHS、所消费的能力契约、RuntimeContext 装配与 Composition Root。**只描述目标态**；实现缺口记入 `03-engineering/migration-governance`。

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
    /// 返回 Workflow-owned requested 值（已受 user maximum clamp，NEVER 经 Provider resolver 计算 effective）。
    fn set_reasoning_level(
        &self,
        session_hint: SessionId,
        level: ReasoningLevel,
    ) -> ReasoningLevelOutcome;
}

enum ReasoningLevelOutcome {
    Accepted { requested: ReasoningLevel }, // Workflow user-max clamp 后的 requested 值
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
- SDK Published Language 的 `InteractionRequestId`、`InteractionReply`、`InteractionCancelReason`、`InteractionCommandOutcome` 与 `ChatEvent::InteractionRequested` **MUST** 可序列化且不含 channel / lock / Runtime handle。当前只要求 local adapter；远端帧、重连与 WSS 行为不在 v0.1.0 冻结。

## 2. Runtime 消费的能力契约

供应能力发布的 OHS **MUST** 只在各自战术文档定义完整签名；本文只登记 Runtime 的使用面，**NEVER** 复制第二份 trait 真相。Runtime 只消费这些 façade，**NEVER** 再定义同义 wrapper：

| 供应能力 | Runtime 消费的窄契约 | 用途 / 唯一真相 |
|---|---|---|
| Context Management | `ContextPort` | 构建 / 压缩 / 追加持久化 Context；见 [Context Management](../context-management/02-compact.md) |
| Tool | `ToolCatalogPort` / `ToolExecutionPort` | schema 投影与单次执行；见 [Tool ports](../tools/02-ports-and-lifecycle.md) |
| Policy | `PolicyPort` | 调用前决策；见 [Policy](../policy/README.md) |
| Memory | `MemoryPort` / `ReflectionPromptPort` | 当前项目 Memory 与纯 Reflection prompt / parse；见 [Memory ports](../memory/04-ports-and-adapters.md) |
| Task | `TaskAccess` | 日常 Task 命令 / 查询；`TaskPersist` **NEVER** 进入 Runtime；见 [Task contracts](../task/02-ports-and-published-language.md) |
| Hook | `HookPort` | 类型化 hook dispatch；见 [Hook](../hook/README.md) |
| Workflow | `ReasoningPort` | effort 调节；见 [Workflow](../workflow/01-reasoning-graph.md) |
| Config | `ConfigSnapshot` PL | 本 Run 的只读配置快照；见 [Config](../config/01-config-layer.md) |

`ProviderPort`、`InteractionPort`、`EventSink` 与 `UsageSink` 隔离 Runtime 策略和易变外部 detail，因此由 Runtime 拥有。它们的**唯一签名真相源**分别是：`ProviderPort` 见本文 §2.1；`InteractionPort` 及其 Published Language 见本文 §2.2；`EventSink` / `UsageSink` 见本文 §2.3。Provider 文档只登记 adapter 实现面，本文其余章节和其他供应方文档只登记使用面或 adapter 行为，**NEVER** 复制第二份 trait：

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

`InteractionPort` 只承载一次 request/reply 交换，**NEVER** 自行修改 Run 或发布 `RunResumed`。Runtime interaction coordinator 在调用前以 request id + continuation 进入 `AwaitingUser`，收到匹配 reply 后恢复 continuation并发布权威事件；取消与 reply 竞争时 cancellation 优先，陈旧 / 重复 id 返回结构化错误。Main adapter 把 request 映射为 SDK event 并等待 TUI / Server 回复；Sub 只能使用 Composition 明确装配的 parent-mediated adapter，否则返回 unavailable，**NEVER** 暗中共用 Main UI channel。

reply 必须与 request body 同 variant；`InvalidReply` 不消费 waiter。`InteractionCompletion::Cancelled` 是“取消这次交互”，不是 `CancelRunStep` / `TerminateRun` 的别名；Runtime 按 continuation 穷尽映射 typed 结果：

| continuation | Replied | Cancelled(reason) | 恢复后的 Run 状态 |
|---|---|---|---|
| `CompleteToolCall(id)` | answers → 同一 ToolCall 的 `ToolSuccess` | `ToolCancelled(UserInteractionCancelled(reason))` | `ExecutingTools`；继续下一个 suspension |
| `ContinueToolApproval(id)` | Approve → Ready；Deny → `ToolCancelled(ApprovalDenied)` | `ToolCancelled(ApprovalCancelled(reason))` | `AwaitingToolApproval`；继续处理其余原始调用 |
| `ContinuePlanApproval` | Approve → `PlanApproved`；Deny → `PlanRejected` feedback；决定随当前无 tool_calls 的 step 恰好一次提交 | `RunFailed(PlanApprovalCancelled(reason))` | reply 回 `PreparingContext` 并启动下一 invocation；cancel 回 `Failed` |
| `ContinueAfterHardPause` | `HardPauseContinue` | `RunFailed(HardPauseCancelled(reason))` | reply 回 `ExecutingTools` 并继续 continuation 记录的未完成 tool phase；cancel 回 `Failed` |

Run root / Step cancellation scope 若与 reply/cancel 竞争则永远优先：`CancelRunStep` 进入 `CancellingStep` 并收口到 `DrainingInput`；`TerminateRun` 进入 `Terminating` 并最终 `Terminated`，**NEVER** 套用上表的普通 completion。

并发 Tool execution 可以同时产生多个 `ToolOutcome::Suspended`，但 Runtime **MUST** 先收集 outcomes，再按 RunStep 原始 ToolCallId / 调用顺序逐个注册 request。前一个 continuation resolve 并清空 PendingInteraction 后才能注册下一个；全部调用得到 final outcome 后，按原调用顺序做 L1 budget reduction，并以一次 `append_and_persist` 提交 assistant + tool results。

Main adapter **MUST** 在 Runtime-side interaction bridge 中先注册 `InteractionRequestId → pending waiter`，再发出纯值 `ChatEvent::InteractionRequested { request_id, run_id, body }`。`AgentClient::reply_interaction` / `cancel_interaction` 回到同一 bridge，校验 body-specific reply 后恰好一次完成 waiter；stream、TUI 与 SDK event **NEVER** 携带 sender。processing teardown 不拥有 waiter，Run cancellation 才由 Runtime drain 该 Run 的 pending request 并发布权威 cancellation 事件。

### 2.3 Runtime-owned EventSink / UsageSink

```rust
trait UsageSink: Send + Sync {                         // Runtime-owned outbound port；Audit adapter 实现
    fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome;
}
trait EventSink: Send + Sync {                         // 纯投影出口；NEVER 承载 Sub Run 业务返回
    fn emit(&self, events: Vec<DomainEvent>);
}
```

`EventSink` 只投影 `Run` 聚合已产生的领域事实，Main 通常映射到 SDK/TUI，Sub 可映射到父级诊断流；父 Run 的 `tool_coordination` **MUST** 直接消费 `derive_sub_run` 返回的 typed `AgentRunTerminal`，**NEVER** 订阅 EventSink 来提取成功结果或错误。`UsageSink::try_record` 是 best-effort 非阻塞审计出口，接受或丢弃都不改变 Run 状态。

## 3. RuntimeContext、active Session 与 Workspace 装配

`RuntimeContext` **MUST** 只持有本 Run 消费的活契约，**NEVER** 持有 Project wiring、composition scope、Session coordinator 或 active resource slot。Composition 同时保存 Project-owned `WorkspaceWiring` 与 Context-owned `MainSessionWiring`；前者守护 workspace 隔离，后者守护稳定 Session backing、Task / Memory 身份与同一个 shared/exclusive `session-switch` gate。

```rust
// agent/composition 内部类型；NEVER 进入 feature 的业务 API。
struct CompositionWorkspaceScope {
    workspace: WorkspaceWiring,
}

struct MainAgentAssembly {
    workspace_scope: Arc<CompositionWorkspaceScope>,
    task: TaskWiring,        // Task-owned opaque handle；只由 Composition 持有
    config: ConfigWiring,    // Config-owned opaque handle；只由 Composition 持有
    session: MainSessionWiring, // Context-owned opaque composition handle
}

// bind_main_run().await 在 owned shared lease 下返回同一 active resource slot 的快照。
struct BoundMainRun {
    context: Arc<dyn ContextPort>,
    memory: Arc<dyn MemoryPort>,
    config: ConfigSnapshot,
    lease: MainRunLease,
}

// lease 必须活到 Main Run、全部 Tool 与其派生 Sub 均收口。
struct AssembledRun {
    runtime: RuntimeContext,
    workspace_scope: Arc<CompositionWorkspaceScope>,
    main_lease: Option<MainRunLease>,
}

async fn open_main_agent(cwd: PathBuf, root: &CompositionRoot)
    -> Result<MainAgentAssembly, AssemblyError>
{
    let workspace = project::wire_production_workspace(cwd)?;
    let task = task::wire_task();
    let config_location = map_project_to_config_location(
        &workspace.read().project_identity(),
    )?;
    let config = config::wire_project_config(
        root.config_sources(),
        &config_location,
    )
        .await?;
    let initial_config = config.participant().snapshot();
    let memory_opener = root.memory_factory();
    let initial_memory = memory_opener
        .open_for_project(
            &workspace.read().project_identity(),
            initial_config.memory_config(),
        )
        .await?;
    let session = context::wire_main_session(MainSessionDependencies {
        workspace_read: workspace.read(),
        workspace_persist: workspace.persist(),
        task_persist: task.persist(),
        memory_opener,
        initial_memory,
        config: config.participant(),
        guidance_source: root.guidance_source(),
        skill_materialization: root.skill_materialization(),
        // Session storage/config 省略；factory 内建立唯一 gate 与 resume coordinator。
    })?;

    Ok(MainAgentAssembly {
        workspace_scope: Arc::new(CompositionWorkspaceScope { workspace }),
        task,
        config,
        session,
    })
}

fn assemble_with_scope(
    spec: &RunSpec,
    parent_runtime: Option<&RuntimeContext>,
    workspace_scope: Arc<CompositionWorkspaceScope>,
    context: Arc<dyn ContextPort>,
    task: Arc<dyn TaskAccess>,
    memory: Arc<dyn MemoryPort>,
    config: ConfigSnapshot,
    main_lease: Option<MainRunLease>,
    root: &CompositionRoot,
) -> Result<AssembledRun, AssemblyError> {
    let workspace_read = workspace_scope.workspace.read();
    let (tool_catalog, tool_execution) = root.tools_for(
        &spec.tools,
        &config,
        workspace_read,
        workspace_scope.workspace.control(),
        Arc::clone(&task),
        Arc::clone(&memory),
    );
    let inherited_requested = parent_runtime
        .map(|parent| parent.reasoning.current_requested_level());
    let reasoning = root.reasoning_for(
        &spec.reasoning,
        inherited_requested,
        config.reasoning_graph(),
    );

    let runtime = RuntimeContext {
        context,
        provider:  root.provider_for(&spec.model, &config),
        tool_catalog,
        tool_execution,
        policy:    root.policy_for(&config),
        interaction: root.interaction_for(spec, parent_runtime),
        memory,
        task,
        hooks:     root.hooks_for(&config),
        reasoning,
        reflection: root.reflection_prompt_for(&config),
        usage:     root.usage_sink(),
        config,
        clock:     root.clock(),
        input:     root.input_for(spec),
        events:    root.event_sink_for(spec, parent_runtime),
        cancel:    root.cancel_scope_for(parent_runtime),
    };

    Ok(AssembledRun { runtime, workspace_scope, main_lease })
}
```

`reasoning_for` **MUST** 只构造 Workflow-owned requested-level 状态：GraphDriven 使用本 ConfigSnapshot 的 graph + user maximum，`EffortOnly(level)` 固定该 requested level，`Inherit` 使用上面冻结的父 requested value。它 **NEVER** 接收 ProviderPort、ModelCapability 或 provider ceiling。每次 invocation 的 model clamp 由 loop 在 `build_window` 前调用 `provider.resolve_invocation_options` 完成。

Main 装配 **MUST** 要求 `WorkspaceMode::Inherit`，先 await `MainSessionWiring::bind_main_run()` 取得 Context / Memory / ConfigSnapshot / owned shared lease，再从 `MainAgentAssembly.task.access()` 取得同一 Task backing 的低权限 view，并原样传给 `assemble_with_scope`。`bind_main_run` 取得 lease 后才读取 active slots；返回的 Arc、snapshot 与 `TaskAccess` **MUST** 绑定该 lease 的逻辑生命周期，**NEVER** 缓存或逃逸到 lease 之外。由此 Context、Runtime、MemoryTool、TaskTool 与 Reflection 都看到同一实例与项目配置，而 restore authority 只留在 session wiring 持有的 `TaskPersist`。

`MainRunLease` **MUST** 保持到 Main Run、全部 Tool、后台 Reflection job 与该 Run 派生的 Sub 全部结束或取消收口；运行期 resume 只有取得 exclusive lease 后才可 prepare / commit。无 active Run 的 Session / Task / Workspace / Memory query、control、snapshot 或 mutation 也 **MUST** 临时取得同一 shared lease。exclusive resume 前必须 join 或取消仍持 lease 的 Reflection / Tool；因此旧 Memory Arc **NEVER** 在切换后继续写旧项目。

`MainSessionWiring` 内部持有唯一 `SessionSwitchCoordinator`、稳定 Session backing、同一 backing 的 `TaskPersist`、active Memory slot 与 Config participant view；Config-owned `ConfigAppService` 才是 active project config 的唯一真相，session wiring **NEVER** 复制第二个 Config slot。resume 在 exclusive lease 下先取得 Project 验证 identity，经 ACL 映射 `ProjectConfigLocation` 并 await Config prepare，再以 candidate MemoryConfig eager-open Memory，最后 prepare Task；无失败提交段替换 Task / Workspace、发布 Session、安装目标 Memory Arc，并以 Config commit + watch publish 收尾。Runtime / Tool 只能获得 `TaskAccess`；active slot、participant commit authority 与 coordinator **NEVER** 进入 RuntimeContext、ToolInvocation、Published Language 或普通 ContextPort 方法。

所有 project-scoped factory（Provider / Tool scope / Hook / Policy / Reflection）**MUST** 显式接收 `BoundMainRun.config`，**NEVER** 再读取 Composition Root / ConfigReader 的 current snapshot。Config prepare **MUST** 加载并验证目标项目的 `.agents/aemeath.json` / 兼容配置及由其决定的 provider / tool / hook 必要输入；失败时整个 resume 保持旧 Project、Memory 与 Config。Sub 继承父 BoundMainRun 的同一只读 ConfigSnapshot。

Project production factory 与 `wire_main_session` **MUST** 都只在 Main agent 启动时调用一次。active Main session slot 的 Run N、Run N+1 复用同一两组 wiring；运行期 resume 只替换 wiring 内的完整 live state，**NEVER** 重建 wiring。

Sub 装配 **MUST** 要求 `WorkspaceMode::Snapshot`，只从父 `workspace_scope.workspace.derive_isolated()` 创建 child scope，并新建 isolated Task / Context。`MemoryMode::Disabled` 使用 NoOp；显式 `share_memory` 则 clone 父 `BoundMainRun.memory` 的同一 Arc，并由父 `MainRunLease` 覆盖整个 Sub 生命周期，**NEVER** 为同一 project 再打开第二个 Memory service。其他 Main / Sub 与 WorkspaceMode / MemoryMode 组合 **MUST** 拒绝。

注入 dispatch Tool 的 composition-provided `AgentDispatch` / Sub-run factory **MUST** 捕获父 scope 与 lease，或以 RunId 在 composition-private registry 中索引它们，再调用同一 `assemble_with_scope`。这些装配类型 **NEVER** 进入 RuntimeContext、ToolInvocation、ContextRequest、ContextPort 或 ToolExecutionPort。

**安全铁律落地**（见 [01-domain-model.md](01-domain-model.md) §7）：Registry Scope 只能移除 Tool/Resource，Tool Profile 的 capability 集只能收缩；`policy` 不放宽；Sub workspace **MUST** 通过 parent scope 的 `derive_isolated()` 派生。

## 4. Composition Root

- **唯一生产装配入口**：`agent/composition`。Runtime 的 `domain/application/ports/adapters` 只定义领域行为、应用用例、边界契约与 Runtime-owned 转换，**NEVER** 选择具体生产实现或触发生产 factory。
- `agent/composition` 持有各 Port 的具体实现或模块提供的 composition-only opaque wiring（provider driver / tool registry / storage / workspace / hook …），提供 `assemble()` 所需的 `root.*()` 工厂。
- Runtime feature 内 **NEVER** 建立 `bootstrap/`、service locator 或第二个 Composition Root；现有 Runtime `utils/bootstrap` 的生产构造责任迁入 `agent/composition`，其余代码按单一 `agent_execution` 能力的六边形职责归位。
- `RuntimeContext` 属 application：它只传递本 Run 的活契约，**NEVER** 进入 domain 或通用 shared，也 **NEVER** 保存具体 Provider、Registry、Store 或全局 Config reader。
- Runtime 当前只有一个完整业务能力，因此 **NEVER** 添加单元素 `capabilities/agent_execution` 包装；没有真实跨 capability 复用内容时也 **NEVER** 创建 `shared/`。
- Project workspace 的生产装配 **MUST** 经 Project-owned factory 取得 `WorkspaceWiring`，并 **MUST** 只保存在 `CompositionWorkspaceScope`；Composition **NEVER** 向 Runtime 或业务模块分发 handle / scope。
- Main agent：Composition Root 初次建立 active-session-slot 的 Workspace 与 MainSession 两组 wiring 并跨 Main Run 保留；每个 Main Run 只在 shared lease 下取得同一 Context / Task / Memory 实例。resume 在 exclusive lease 内更新完整 live state，**NEVER** 重建 wiring。
- Config-owned `ConfigWiring` **MUST** 先按当前 Project identity 的 ACL location 准备 project-aware snapshot；Memory opener 再消费同一 candidate 的 MemoryConfig。Context-owned `MainSessionWiring` 把同一 Task / Memory Arc 与 Config participant snapshot 绑定到每个 Main Run；resume 复用相同 Config → Memory 顺序，全部 participant 成功后才进入无失败提交段。
- Sub Run：dispatch Tool 经 composition-provided AgentDispatch 从 parent scope 执行 `derive_isolated()`，再装配相同结构；Runtime tool coordination 只编排 `ToolExecutionPort` 调用。
- 任何模块 **NEVER** 私自 `new` Port 实现绕过 Composition Root。

## 5. 关键 ACL

1. **Provider 内部**：各家 LLM API → 统一 `InvocationDelta` + 领域 `Message`
2. **event_projection**：领域 `DomainEvent` → SDK `ChatEvent`（Main/Sub 路由 + agent_id）
3. **Session 快照组装**：Context Management backing implementation 直接经注入的 `TaskPersist` / Project-owned `WorkspacePersist` 收集与恢复；Runtime 只有 `TaskAccess`，且 **NEVER** 中转 Workspace 能力
4. **Workspace / Session scope 隔离**：Composition 保留 Project 与 Context-owned opaque wiring；Main 在同一 active slot 内跨 Run 复用，Sub 从父 workspace scope 隔离派生；scope / wiring / lease **NEVER** 穿过 Runtime、Tool 或普通 ContextPort 边界
5. **Interaction ACL**：Tool-owned `UserInteractionSpec` / Policy 决策 → Runtime-owned `InteractionRequest` → adapter SDK DTO；reply 按 request id 回到 Runtime continuation，TUI DTO / channel **NEVER** 进入 Run 聚合或 Tool BC

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

| 日期 | 变更 | 关联 |
|---|---|---|
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
