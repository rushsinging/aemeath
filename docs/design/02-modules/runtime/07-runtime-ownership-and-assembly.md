# Agent Runtime · 运行时所有权与统一装配

> 层级：02-modules / runtime（模块战术设计）  
> 状态：Target（目标设计）
>
> 本文记录 Runtime 的最终所有权、控制反转与装配边界。它只描述终态，不记录实现进度。任何按 Main/Sub 分类的生产类型、调用方手填的 `RunContextBindings`、fat `RunLoopPort` 或 Runtime 内第二 Composition Root 都不属于目标模型。

## 1. 决策摘要

Runtime 最终收敛为六个核心类型：

| 类型 | 生命周期 | 可变性 | 回答的问题 |
|---|---|---|---|
| `RuntimeServices` | Agent Runtime | 初始化后稳定 | Runtime 可以使用或创建哪些能力？ |
| `SessionState` | Session | 可变 | 当前会话处于什么状态？ |
| `RunSpec` | Run | 不可变 | 本次 Run 允许并需要什么能力？ |
| `RuntimeContext` | Run | 构造后冻结 | 本次 Run 实际绑定了哪些能力？ |
| `Run` | Run | 受状态机约束地可变 | 本次 Run 在领域上进行到哪一步？ |
| `RunExecutionState` | Run | 可变 | Loop 当前处理哪些工作数据？ |

创建与执行关系遵循控制反转：Runtime application 定义所需 factory/port，Composition 注入实现；调用路径只提交纯值 request，不创建具体能力。

```text
Composition Root
  ├─ RuntimeServices
  │    ├─ ProviderBindingFactory
  │    ├─ ContextBindingFactory
  │    ├─ ToolBindingFactory
  │    ├─ HookBindingFactory
  │    ├─ InteractionBindingFactory
  │    ├─ WorkspaceBindingFactory
  │    └─ 长生命周期共享 Port
  └─ RuntimeContextFactory

Runtime bootstrap
  └─ SessionState

RunCreationRequest
  ├─ RunSpec
  ├─ SessionSnapshot
  └─ ParentRunCapabilities?
                 │
                 ▼
        RuntimeContextFactory
          ├─ 校验能力 ceiling
          ├─ 选择/收缩 capability adapter
          └─ 创建 per-Run resource
                 │
                 ▼
          RunInstance
          ├─ Run
          ├─ RuntimeContext
          └─ RunExecutionState
                 │
                 ▼
            RunLauncher
                 │
                 ▼
            Loop Engine
```

依赖方向：

```text
Composition adapters ──implements──▶ Runtime-owned factory/port contracts
                                           ▲
                                           │ consumes
                                 Runtime application
                                           │
                                           ▼
                                      Runtime domain
```

强制结论：

1. 生产类型和 Loop 不再以 Main/Sub 分类；差异由 `RunSpec`、父子拓扑及能力 adapter 表达。
2. 静态活依赖与动态会话状态必须分离，禁止恢复综合资源袋。
3. `RuntimeContext` 只有统一装配入口；调用方不得构造具体 Port，也不得手填同构参数包。
4. Runtime application 拥有 factory/port 抽象、能力选择规则、状态转换与快照时机；Composition 只实现并连接这些抽象。
5. Loop Engine 拥有完整执行流程，不通过 fat port 把流程控制权反向交给 adapter。
6. `Run` 与 `RunExecutionState` 必须各有唯一状态所有权，不得复制同一事实。
7. Session Runtime 只有一个 typed `ChatRequest.ingress`；`SessionInputMailbox` 独占外部输入 source，`SessionIngress` 独占 interaction reply/cancel 路由，普通输入只进入每 Run 的 `RunInputBuffer`。
8. 普通输入等待与 Interaction 等待必须分离；前者消费 Session mailbox 并推进 Run buffer，后者只恢复唯一 `PendingInteraction`，不得共享 queue 或恢复路径。
9. 类型、trait 与模块必须按单一职责命名。`Projection` 不作为架构角色名；跨边界转换使用能说明目标或用途的 mapper/view/record 名称，职责混合的抽象必须先拆分。

### 1.1 命名与职责拆分

`Projection` 只描述“某种派生结果”，无法说明谁拥有状态、转换方向、生命周期或副作用，因此不能作为 Runtime 的结构性命名。目标模型采用以下职责词汇：

- `View`：面向特定消费者的只读值，例如 `SessionResumeView`；
- `Record`：已经提交或可持久化的事实，例如 `AcceptedInputRecord`、`FinalizedStepRecord`；
- `Mapper` / `map_*`：无状态、单方向的跨边界值转换，例如 `sdk_event_mapper`、`map_hook_outcome`；
- `Context`：一次操作所需的只读依赖和输入；
- `Lifecycle` / `Observer`：操作过程中的异步回调与通知，不拥有主流程。

原先混合状态访问、Reducer 构造、日志上下文、事件等待和生命周期回调的模型调用抽象拆为两个接口：`ModelInvocationContext` 只提供调用所需依赖与纯查询，`ModelInvocationLifecycle` 只提供窗口、重试、响应和终态分类回调。共享 model coordinator 仍独占调用流程，两类接口都不能重新吸收 Provider retry loop 或 Run 状态所有权。

跨边界的 Hook 类型转换由 `outcome_mapper` 负责，Runtime-owned 结果继续使用领域语义名 `RuntimeHookDispatch`、`RuntimeHookExecution` 和 `RuntimeHookDisplayMessage`；SDK 事件转换由 `sdk_event_mapper` 负责。模块名不再使用仅描述技术形态的 `projection`。

## 2. 去除 Main/Sub 生产类型区别

Main/Sub 是当前调用路径的历史分类，不是稳定领域类型。目标生产模型不得保留：

- `MainSessionShell`
- `MainRunPort`
- `SubAgentRun`
- `MainInputStrategy` / `SubInputStrategy`
- `MainEventStrategy` / `SubEventStrategy`
- `assemble_main_runtime_context`
- 以 `is_main` / `is_sub` 驱动行为的分支或超集字段

Run 的差异分成两个正交维度：

- **拓扑**：`parent_run_id` 表示是否由另一个 Run 派生。
- **能力**：`RunSpec` 表示 input、interaction、event、context、workspace、memory、tool 等模式和上限。

有父 Run 不等于固定的“Sub 类型”；独立 Run 也不等于必然拥有完整能力。后台 Run、Reflection Run、Scheduler Run 或未来可交互的派生 Run 均应由同一模型表达。

因此 `RunKind::Main/Sub` 应在消费点审计完成后删除。若日志或观测需要来源，应使用明确的 purpose/role 投影，不能让观测标签反向成为控制分支。

## 3. 静态依赖与动态状态分离

### 3.1 `RuntimeServices`

`RuntimeServices` 只持 Agent Runtime 生命周期内稳定的 Port、Factory 和基础设施：

- Provider、Context、Memory、Reasoning 的 factory 或绑定服务；
- Tool catalog、execution 与 execution-context binding；
- Policy、Hook、Task、Reflection、Session management；
- Config query/writer；
- Skill catalog/materializer；
- Agent runner、Tool result materializer；
- Event/Input adapter factory；
- Workspace service/control capability；
- Scheduler、并发器或其他长生命周期基础设施。

它不得保存：

- session identity、当前模型或 resume 状态；
- reminders、read-files、active parent frame；
- config snapshot、messages、context window；
- cancellation、event sink、input buffer、usage tracker 等 per-Run 实例。

判定依据是业务所有权，而不是 Rust 内部是否使用锁。一个长期 Port 即使内部可变，仍属于 `RuntimeServices`；当前会话对该 Port 的选择或投影属于 `SessionState`。

### 3.2 `SessionState`

`SessionState` 只持跨多个 Run 延续、并随会话变化的事实：

- session identity；
- 当前模型选择或 binding revision；
- committed config revision；
- resume/恢复状态；
- session reminders 与 read-files tracking；
- 当前 workspace/session 投影；
- active run identity；
- 当前 parent-run frame；
- 必要的 session 级交互登记状态。

`SessionState` 不得持有服务集合、Factory、`RuntimeContext` 或 `RunExecutionState`。内部应按一致性需求拆成小状态单元，只有必须原子读取或更新的字段才共享同一锁。

Run 创建时捕获窄的 session snapshot。Factory 不长期借用动态 `SessionState`；模型切换、配置刷新或 workspace 变化只影响后续 Run。

## 4. 核心类型边界

### 4.1 `RunSpec`：能力声明与上限

`RunSpec` 只描述本次 Run 的需求和限制，建议字段使用明确语义：

- `purpose` 或稳定用途标识；
- `timeout`；
- `input_mode`；
- `interaction_mode`；
- `event_route`；
- `context_mode`；
- `workspace_mode`；
- `memory_mode`；
- `tool_scope`；
- capability ceiling。

`RunSpec` 不持有 Port、Factory、锁、消息或动态执行状态。父 Run 派生出的规格必须通过 capability ceiling 校验，能力只能收缩或平移。

### 4.2 `RuntimeContext`：已绑定的 Run 能力

`RuntimeContext` 表示一次 Run 创建时已经选择、限制并冻结的活契约：

- bound Context、Provider、Tool、Policy、Hook、Memory、Task、Reasoning、Interaction 与 Reflection capability；
- `RunConfigSnapshot` 和 provider/model binding；
- cancellation、event sink、usage、input 等 per-Run 基础设施实例；
- 按 `workspace_mode` 绑定后的窄 Workspace capability。

它不持有 Factory、Config reader/writer、Session management、session query、model switch、reminders、messages、context window、turn count 或 terminal state。

判定规则：Loop 执行时若需要的是“已经确定的能力”，属于 `RuntimeContext`；若还要选择或创建能力，来源属于 `RuntimeServices`；若值会随会话变化，来源属于 `SessionState`；若由 Loop 执行产生，属于 `RunExecutionState`。

### 4.3 `Run`：领域状态机

`Run` 是唯一执行生命周期聚合，拥有：

- run identity、parent relation 和 `RunSpec`；
- `RunStatus` 与合法迁移；
- `RunStep` 及其领域状态；
- pending interaction identity/continuation；
- cancellation/termination 请求状态；
- drain epoch（若用于保证状态机线性化）；
- started time/deadline；
- domain events。

`Run` 不持有消息窗口、Provider/Tool Port、流式进度或 UI terminal view。

### 4.3.1 Session ingress、Session mailbox 与 Run input buffer

Session Runtime 对外只有一个 typed ingress。`ChatRequest` 只携带非可选 `ingress`；Composition 将它适配为 `ChatInputEventPort`，Session 级 `SessionInputMailbox` 是唯一允许读取该 source 的 Runtime owner。首条输入、后续输入、Skill 和控制命令均为同一 `ChatInputEvent` 协议，不存在 `user_input`、`initial_messages`、独立 seed 或额外 queue drain 旁路。

```rust
struct ChatRequest {
    ingress: ChatIngress,
}

struct SessionRuntime {
    input_mailbox: SessionInputMailbox,
    interaction_ingress: SessionIngress,
    active_run: Option<RunHandle>,
}

struct SessionInputMailbox {
    source: Arc<dyn ChatInputEventPort>,
    deferred: VecDeque<ChatInputEvent>,
    source_closed: bool,
}

struct RuntimeContext {
    input: RunInputBufferHandle,
}
```

入站所有权必须保持单向：

1. `SessionInputMailbox` 先读取 deferred，再读取外部 source，因而跨 Run 保持 producer identity 与 FIFO；任何其他 Runtime 类型都不得直接 poll `ChatInputEventPort`。
2. Session idle gate 对事件分类：可接纳的 `UserMessage` / `SkillRequest` 激活一个 Run；控制命令留在 Session 边界执行或调度；输入 source 关闭触发 Session shutdown。
3. 每个 `RuntimeContext` 拥有独立 `RunInputBufferHandle`。Session 将已接纳的原始事件推入当前 Run buffer；首条与后续输入走同一 `push_or_reject` 路径，保留 `InputId`、文本和图片。
4. Run buffer 以 `DrainEpoch` 线性化 drain/seal。Run 进入 sealed 后到达的用户输入不会丢弃或串入旧 Run，而是退回 `SessionInputMailbox::defer`，供下一 Run 优先消费。
5. Run 结束时，尚未归属该 Run 的控制事件同样退回 Session mailbox；Run buffer 不执行 Session 命令。

`RunInputBuffer` 只管理当前 Run 的输入接纳生命周期，不复制 Run 状态机：

```rust
enum BufferDrain {
    Ready { batch: Vec<LoopInput>, epoch: DrainEpoch },
    Empty { epoch: DrainEpoch },
    EmptyAndSealed { epoch: DrainEpoch },
    AlreadySealed { epoch: DrainEpoch },
    EpochMismatch { expected: DrainEpoch, actual: DrainEpoch },
}
```

FIFO、batch drain、epoch、seal 和 late-input defer 属于 `RunInputBuffer`；`Created → DrainingInput → ...` 属于 `Run`。Run buffer **MUST NOT** 定义 PreparingContext、ExecutingTools 或 Interaction 等业务状态。

Interaction reply/cancel 与普通输入完全正交：`SessionIngress` 只把 typed `InteractionCommand` 定向交给 `InteractionPort`。`Run` 持有唯一 `Option<PendingInteraction>`，因此单个 Run 同一时刻最多存在一个 active interaction continuation；`InteractionPort::register` 返回的唯一 oneshot receiver 仅等待该 request 的 reply/cancel。端口内部可按 request identity 保存多个 waiter，是为了隔离多个并发 Run，不表示单个 Run 可以同时等待多个 interaction。

```rust
struct Run {
    pending_interaction: Option<PendingInteraction>,
}

struct PendingInteraction {
    request_id: InteractionRequestId,
    continuation: InteractionContinuation,
}

struct InteractionBridge {
    pending_by_request: HashMap<InteractionRequestId, PendingWaiter>,
}
```

reply/cancel 必须匹配 request identity；重复、陈旧或不匹配的命令返回 typed outcome。Run cancel、terminate、timeout、parent disconnect 或 Session shutdown 必须先清理该 Run 的 waiter 与 execution-side pending work，再进入用户可见终态。

### 4.3.2 普通输入等待与 Interaction 等待语义

普通输入等待与 Interaction 等待不得共享 mailbox 或 continuation：

- **普通输入等待**：Session/Run 输入策略等待 `SessionInputMailbox` 的下一条 typed event；用户输入被接纳进当前 Run buffer 后，由 drain epoch 推进下一 Step。这是输入策略行为，不创建 `PendingInteraction`。
- **Interaction 等待**：领域状态当前由 `RunStatus::AwaitingUser + pending_interaction.is_some()` 表达；只有匹配 request 的 reply/cancel 才能按 typed continuation 恢复。这条路径不读取 `SessionInputMailbox`，也不把 reply 伪装成用户消息。

目标状态机应把当前组合条件显式命名为 `AwaitingInteraction { request_id }`。若未来需要让同一 Run 在 Step 之间等待普通用户输入，应另设 `AwaitingInput`，禁止恢复“先 poll interaction、再 await user input”的混合分支。

### 4.4 `RunExecutionState`：Loop 工作集

`RunExecutionState` 只拥有 Loop 执行过程中产生和更新的工作数据：

- messages 与 committed boundary；
- Step message ownership；
- accepted/adopted inputs；
- ContextRequest / ContextWindow；
- turn count 与 invocation usage snapshot；
- tool identity 与 continuation 工作数据；
- stream progress；
- prompt/request snapshot；
- terminal output view。

以下事实只属于 `Run`，不得复制到 `RunExecutionState`：status、active Step status、pending interaction、cancellation/termination 状态、domain events。

以下事实只属于 `RunExecutionState`，不得复制到 `Run`：messages、context request/window、token snapshot、input adoption、stream progress 和 tool execution working data。

Loop Engine 的顺序是：先请求 `Run` 完成合法状态迁移，再更新 `RunExecutionState`，外部副作用只经 `RuntimeContext`，最终由 `Run` 产生领域事件并由 adapter 投影。

## 5. 统一 `RuntimeContextFactory`

`RuntimeContextFactory` 是 Runtime application 定义的能力装配服务，也是唯一允许构造 `RuntimeContext` 的入口；它的产物严格是 `RuntimeContext`。Composition 注入它所消费的窄 factory/port 实现，但不得拥有 `RunSpec` 的能力决策。`RunFactory` 是独立的 Run 准备用例，协调 `Run`、`RunExecutionState` 与 `RuntimeContext` 的一致创建，并产出 `RunInstance`。

### 5.0 IoC 端到端伪代码

下面的伪代码描述的是职责和数据流，不是要求照抄的 Rust API。关键约束是：Composition 负责“有哪些实现和 wiring”，Runtime application 负责“本次 Run 需要什么能力”，Factory 负责“把声明转换为受限 capability”；三者不能越界。

```rust
// 2. RuntimeContextFactory：只创建已绑定的 RuntimeContext。
fn compose_runtime(graph: CompositionGraph) -> RuntimeAssembly {
    let wiring = graph.build_opaque_wiring();
    let services = RuntimeServices::new(
        ProviderBindingFactoryImpl::new(wiring.provider),
        ContextBindingFactoryImpl::new(wiring.context),
        ToolBindingFactoryImpl::new(wiring.tools),
        InteractionBindingFactoryImpl::new(wiring.interaction),
        HookBindingFactoryImpl::new(wiring.hooks),
        WorkspaceBindingFactoryImpl::new(wiring.workspace),
    );
    let factory = RuntimeContextFactoryImpl::new(services.clone());

    RuntimeAssembly {
        services,
        context_factory: Arc::new(factory),
        session_wiring: wiring.session,
    }}

// 2. Runtime bootstrap：从入站 args 得到 typed request，初始化 SessionState；
//    不创建 Provider、Tool、Hook、Workspace 或 RuntimeContext。
async fn bootstrap_runtime(
    request: BootstrapRequest,
    assembly: &RuntimeAssembly,
) -> Result<AgentRuntime, BootstrapError> {
    let session = SessionState::restore(
        assembly.services.session_store(),
        request.session_identity,
    ).await?;
    Ok(AgentRuntime::new(
        RunFactory::new(assembly.context_factory.clone()),
        session,
    ))
}

// 3. Runtime application：创建 Idle Run，只声明能力模式。
async fn prepare_root_run(
    runtime: &mut AgentRuntime,
) -> Result<RunInstance, RunCreationError> {
    let snapshot = runtime.session.snapshot_for_run()?;
    let spec = RunSpec::for_purpose(RunPurpose::Interactive)
        .with_input_mode(InputMode::LiveSession)
        .with_interaction(InteractionBindingMode::Client)
        .with_hook(HookBindingMode::Full);

    runtime.run_factory.create(RunCreationRequest {
        spec,
        session: snapshot,
        parent: None,
    }).await
}

// 4. 派生 Run 同样先创建 Idle Run；任务输入仍通过 InputPort 提交。
async fn prepare_child_run(
    runtime: &mut AgentRuntime,
    parent: &RunInstance,
) -> Result<RunInstance, RunCreationError> {
    let snapshot = runtime.session.snapshot_for_run()?;
    let parent_caps = parent.run.child_capabilities();
    let spec = RunSpec::for_purpose(RunPurpose::AgentTask)
        .with_parent(parent.run.id())
        .with_input_mode(InputMode::Fixed)
        .with_interaction(InteractionBindingMode::ParentMediated)
        .with_hook(HookBindingMode::BoundaryOnly)
        .with_capability_ceiling(parent_caps.ceiling());

    runtime.run_factory.create(RunCreationRequest {
        spec,
        session: snapshot,
        parent: Some(parent_caps),
    }).await
}

// 5. 输入与 Run 创建解耦：任何来源都通过 InputPort 激活 Idle Run。
async fn submit_input(
    prepared: &RunInstance,
    input: InputEnvelope,
) -> Result<(), InputError> {
    prepared.context.input().submit(input).await
}

// 6. RunFactory 校验声明并协调三件对象；RuntimeContextFactory 只选择 adapter 并创建 Context。
async fn create(
    &self,
    request: RunCreationRequest,
) -> Result<RunInstance, RunCreationError> {
    let parent_ceiling = request.parent.as_ref().map(|p| p.capabilities());
    request.spec.validate_against(parent_ceiling)?;

    let interaction = match request.spec.interaction_mode {
        InteractionBindingMode::Client =>
            self.services.interaction.bind_client(request.session.clone()).await?,
        InteractionBindingMode::ParentMediated =>
            self.services.interaction.bind_parent_mediated(
                request.parent.ok_or(MissingParent)?,
            ).await?,
        InteractionBindingMode::Unavailable =>
            self.services.interaction.bind_unavailable(),
    };
    let hooks = match request.spec.hook_mode {
        HookBindingMode::Full =>
            self.services.hooks.bind_full(request.spec.hook_scope).await?,
        HookBindingMode::BoundaryOnly => self.services.hooks.bind_empty(),        HookBindingMode::Disabled => self.services.hooks.bind_disabled(),
    };

    let context = self.context_factory.create(        self.services.provider.bind(&request.spec, &request.session).await?,
        self.services.context.bind(&request.spec, &request.session).await?,
        self.services.tools.bind_restricted(&request.spec, parent_ceiling).await?,
        interaction,
        hooks,
        self.services.workspace.bind(&request.spec, &request.session).await?,
        request.session.run_config_snapshot(),
    );
    let run = Run::idle(request.spec);
    let execution = RunExecutionState::empty(run.id());
    Ok(RunInstance { run, context, execution })
}

// 7. Loop Engine 只消费已绑定能力；Idle Run 由 InputPort 输入激活。
async fn execute(instance: RunInstance) -> Result<AgentRunTerminal, RunExecutionError> {
    loop_engine::run_loop(instance).await
}
```

伪代码表达的 IoC 边界：

- `compose_runtime` 可以选择实现，但不能解析 `RunSpec` 决定能力升降；
- `prepare_root_run` / `prepare_child_run` 可以声明模式，但不能 `new` 具体 Port 或携带输入内容；
- `submit_input` 是外部输入进入 Run 的唯一入口，首次输入不具有特殊装配语义；
- `RunFactory::create` 可以校验声明并协调 `Run`、`RuntimeContext`、`RunExecutionState`，但不能执行 Loop、恢复 Session 或持久化 Step；
- `RuntimeContextFactory::create` 可以按模式创建 capability adapter，但不能创建 `Run` 或 `RunExecutionState`；
- `execute` 只能使用完整 `RunInstance`，不能拆包后从调用方分别传入 Run、Execution 与 Context，也不能按 Main/Sub 分支；
- `RuntimeContext::private_new` 必须只对 factory 可达，失败时不得返回半装配的 `RunInstance`。

### 5.1 输入与输出

调用方只提交纯值准备请求：

```rust
struct RunCreationRequest {
    spec: RunSpec,
    session: SessionSnapshot,
    parent: Option<ParentRunCapabilities>,
}

struct ParentRunCapabilities {
    run_id: RunId,
    context: RuntimeContextCapabilityView,
    cancellation: RunCancellationScope,
    workspace: ParentWorkspaceCapability,
}

struct RunInstance {
    run: Run,
    context: RuntimeContext,
    execution: RunExecutionState,
}
```

`SessionSnapshot` 和 `ParentRunCapabilities` 只携带准备本次 Run 所需的窄事实或 capability view。它们不得暴露 `SessionState` 锁、Composition wiring、具体 adapter、完整父 `RuntimeContext` 或可越权的服务集合。`RunInstance` 创建后处于 `Idle`，`RunExecutionState` 为空；首次输入和后续输入没有语义特例，全部经已绑定的 `InputPort::submit(InputEnvelope)` 进入 Run，并由状态机从 `Idle` 激活到 drain/step 流程。

Factory 负责：

1. 校验 `RunSpec` 与 parent capability ceiling；
2. 从 `RuntimeServices` 的窄 factories 选择并绑定 Context、Provider、Tool、Policy、Hook、Memory、Task、Reasoning、Interaction、Reflection 和 Workspace capability；
3. 按模式创建 shared、isolated、restricted、parent-mediated 或 unavailable adapter；
4. 创建 cancellation、input、event、usage 等 per-Run 实例；
5. 冻结 `RunConfigSnapshot`、provider/model binding 与 workspace snapshot；
6. 创建相互一致的 `Idle Run`、冻结 `RuntimeContext` 与空 `RunExecutionState`，返回 `RunInstance`；输入随后只经 `InputPort` 激活状态机。

Factory 不负责执行 Loop、恢复 Session、修改 `SessionState`、处理模型响应、持久化 Step 或发布终态事件。

### 5.2 禁止调用方手填依赖

`RuntimeContext::new` 仅对 factory 实现可见。以下形状不属于终态：

- `RuntimeContextParts`、`RunContextBindings` 或任何与 `RuntimeContext` 字段近似一一对应的参数袋；
- Main、Sub、Reflection、Scheduler 各自公开 assembler；
- 调用方先创建 Provider、Context、Interaction、Hook adapter，再让 factory 被动复制；
- 通过 `Option<Arc<dyn Port>>` 表示能力开关；
- 直接传入完整父 `RuntimeContext` 并复用其无关能力。

新 Run 来源可以不同，但准备入口只有一个。独立 Run、派生 Run、后台 Run 与未来 Scheduler Run 都构造 `RunCreationRequest`，不能建立第二条装配路径。

### 5.3 Interaction 的 IoC 绑定

`InteractionBindingMode` 的终态语义：

- `Client`：绑定 client-facing interaction adapter；
- `ParentMediated`：创建新的 `ParentMediatedInteractionPort`，不得直接复用父 `InteractionPort` Arc；
- `Unavailable`：绑定立即返回 typed unavailable 的 adapter。

`ParentMediatedInteractionPort` 拥有 child 请求到 parent 请求的映射，至少以 `child_run_id + child_request_id` 隔离 identity。它负责向父边界发布请求、将匹配 reply/cancel 路由回 child waiter，并在 child teardown 时只 drain 该 child 的请求。父 `InteractionPort` 只提供传输能力，不拥有 child continuation；continuation 和 pending interaction 仍由 child `Run` 所有。

并发 child 之间不得共享无命名 pending slot。任何 reply 必须同时匹配 parent route、child run 和 request identity；父 Run 取消可以向下传播，child 取消不得清空父或 sibling 的 pending request。可选的父级进度/诊断事件是 Event adapter 的投影，不进入 InteractionPort 的业务完成语义。

### 5.4 Hook 的 IoC 绑定

`HookBindingMode` 必须在装配期产生真实 capability adapter：

- `Full`：允许 RunSpec 声明范围内的全部 Hook invocation；
- `BoundaryOnly`：历史枚举名仅表达 Sub 的最低能力档；实际装配独立 `EmptyHookPort`，任何 invocation 都直接 `proceed`，不执行或转发到底层 Hook；
- 若未来存在 `Disabled`：绑定 typed no-op/disabled adapter，而不是 `Option`。

Sub Hook capability 不能通过 parent 存在性校验后复用完整 HookPort，也不能保留 start/stop 特例。Factory 必须装配独立 `EmptyHookPort`；它对 `dispatch` 与 `dispatch_at` 都无条件返回 `HookOutcome::proceed()`，且不持有底层 HookPort。Hook BC 继续拥有 Main Run 的 subscription、脚本执行、重试和 typed directive；Sub 生命周期若需观测，应使用 Runtime event/parent result 通道，而不是 Hook。

### 5.5 Factory Port 所有权

Runtime 定义自己需要的构造契约，例如 `ProviderBindingFactory`、`ContextBindingFactory`、`ToolBindingFactory`、`InteractionBindingFactory`、`HookBindingFactory` 和 `WorkspaceBindingFactory`。契约参数必须是 `RunSpec`、session snapshot、parent capability 等 Runtime 语言；返回值必须是 Runtime 消费的窄能力。

Composition 或供应 BC adapter 实现这些契约。Runtime 不依赖 `provider::composition`、`context::wire_*`、Workspace wiring 或具体 Registry；Composition 也不得解析 `RunSpec` 后自行决定能力升降。

## 6. Loop Engine 的控制反转

Loop Engine 是 Agent Execution 用例的流程 owner。它直接编排 `Run`、`RunExecutionState` 和 `RuntimeContext` 中已经绑定的能力，不通过来源 adapter 回调决定业务顺序。

```rust
async fn run_loop(
    run: &mut Run,
    execution: &mut RunExecutionState,
    context: &RuntimeContext,
) -> Result<AgentRunTerminal, RunExecutionError>
```

### 6.1 fat capability trait 是伪聚合根

`LoopCapabilityAdapter`、fat `RunLoopPort` 以及任何同构替代物都不是合法的领域抽象。它们没有领域 identity、生命周期或自身不变量，却要求同一个对象同时承担输入、事件、控制、生命周期、Interaction、Step 持久化、Compaction、模型调用、Stop Hook、工具轮次、stuck handling 与 plan approval，因而在调用图中成为与 `Run`、`RunExecutionState`、`RuntimeContext` 并列的第四个状态中心。

这种技术性能力全集会产生三个后果：

1. 来源类型反向拥有整个 Run 用例，Main/Sub 双轨即使改名为 Chat/Derived 仍会复发；
2. 已建立的 Model、Tool、Interaction、Persistence、Hook、Finalization coordinator 退化为大 adapter 的辅助函数，无法成为真实 application owner；
3. 跨 BC wiring、Application 编排和 Adapter 转换混入同一对象，领域不变量无法定位到唯一 owner。

因此，删除 fat trait 不能采用以下等价替代：

- 把全部端口展开成十几个无分组函数参数；
- 新增 `LoopExecutionParts`、`EnginePorts`、`RuntimeCapabilities` 或其他参数袋；
- 让 `RuntimeContext` 实现全部 workflow trait；
- 为 Chat/Derived 分别构造一套完整窄端口集合；
- 用 trait alias、supertrait 或泛型约束继续要求单一对象实现整组能力。

### 6.2 Engine 按领域阶段编排

Engine 的调用图必须按稳定领域阶段表达，而不是按端口清单机械拆分：

```text
RunLoop
├─ Input / Drain Phase
│  └─ InputPort
├─ Step Transaction Phase
│  ├─ Run
│  ├─ RunExecutionState
│  └─ StepPersistenceCoordinator
├─ Context / Compaction Phase
│  └─ CompactionCoordinator
├─ Model Invocation Phase
│  └─ ModelInvocationCoordinator
├─ Tool Round Phase
│  └─ ToolRoundCoordinator
├─ Interaction Phase
│  └─ InteractionCoordinator
├─ Stop Hook Phase
│  └─ StopHookCoordinator
└─ Finalization Phase
   └─ RunFinalizationCoordinator
```

阶段可以实现为职责明确的私有函数或 application service，不要求每个阶段都新增 struct。每个阶段必须只接收自身所需 owner 和窄外部 seam，并通过 typed outcome 与下一阶段通信；任何阶段对象都不得持有完整 Runtime 能力集合。

Engine 负责：

- input drain/await、epoch 校验与 Step 创建；普通输入只从 `SessionInputMailbox` 进入每 Run 的 `RunInputBuffer`；
- command scheduling：ImmediateControl 立即生效，AtRunBoundary 在安全边界执行，SessionQuery 不污染 Run 输入；
- interaction continuation：reply/cancel 只按 request identity 完成该 Run 唯一 `PendingInteraction`，不经过 Session mailbox 或 Run input buffer；
- 按阶段调用 context/compact、model invocation、Tool coordination 与 Hook coordination owner；
- control/cancellation、Step finalization 和 terminal mutation；
- 每次领域 mutation 后立即 drain event 并交给 `EventSink`。

`RuntimeContext` 中的 Port 只执行已经绑定的外部能力；`RunExecutionState` 只保存工作集；二者均不得调用回 Engine 或决定下一阶段。coordinator 独占其业务算法、重试/分类和副作用顺序，来源 adapter 不得再包装或覆写这些流程。

### 6.3 来源边界只表达差异

Chat 与 Derived 是 ingress/topology 来源，不是完整 Runtime 类型。来源目录只能拥有：

- input source 或 session ingress adapter；
- event、progress、active-step、finalization 等窄 observer；
- parent-derived request、topology 与 capability ceiling 映射；
- terminal result 到调用方协议的映射。

来源目录不得拥有或重新装配模型调用、工具轮次、Context/Compaction、Interaction、Step 持久化、Hook 或 Run finalization 主流程。Input 差异由绑定后的 `InputPort` 表达；Event 差异由 `EventSink`/窄 observer 表达；Provider、Tool、Hook 与 Interaction 差异由 `RuntimeContextFactory` 绑定的 capability 表达。任何差异都不得重新形成来源型大 adapter 或完整能力 bundle。

确有外部边界价值的 seam 可以保留为窄 Port，例如 `ChatInputEventPort`、`EventSink`、`UsageSink`、`ActiveRunRegistryPort`。`SessionInputMailbox` 独占普通输入 source 与跨 Run deferred FIFO；`SessionIngress` 仅定向分发 interaction reply/cancel；每个 `RuntimeContext` 绑定独立 `RunInputBuffer`。这些边界由 Runtime 定义、Composition 注入，不承载 Loop 流程。

### 6.4 派生 Run 进度的来源身份与挂载身份

派生 Run 的实时进度同时参与两个不同语义：事件归因和父级展示挂载。二者 **MUST** 使用不同字段表达，**NEVER** 为了让 TUI 找到父级 Agent ToolCall 而覆盖派生 Run 自己的 chat/turn identity。

```text
source_context
  = 实际产生进度的派生 Run chat/turn

attachment_context + tool_id
  = 父 Run 中承载该派生 Run 的 Agent ToolCall
```

身份在各边界的所有权如下：

1. 派生 Run 创建自己的 `source_context`；同一派生 Run 的 Started、Message、ToolCalls 和 ToolOutput 事件都保留该身份。
2. 父 Run 执行 Agent ToolCall 时创建 `attachment_context + tool_id`；进度转发器只补充挂载信息，不得改写 `source_context`。
3. Runtime 出站事件同时携带 `source_context` 与 `attachment_context`。SDK 和 Consumer Adapter 只做逐字段映射，禁止把二者折叠为单一 `context`。
4. TUI adapter 只使用 `attachment_context + tool_id` 生成 `UpdateAgentMeta` / `RecordAgentProgress`，确保内容进入父 Agent ToolCall block；`source_context` 保留给日志、诊断及未来嵌套展示。
5. Conversation Model 必须按显式 `attachment_context + tool_id` 定位 ToolCall，禁止按 active turn 或全局 `tool_id` 回退搜索。并发 Agent ToolCall 必须保持隔离。
6. Agent progress 不进入根级 timeline；它只更新 Agent ToolCall 的 `agent_meta` 与 `activities`。工具完成后由既有 ToolResult 渲染规则接管。

兼容读取旧事件时可以把旧单一 `context` 同时解释为来源和挂载身份，但新事件写入与运行时传递必须始终显式携带两套身份。兼容逻辑只允许存在于 wire/DTO 边界，不能进入 Runtime 或 TUI Model 的业务定位规则。

## 7. Composition 与 Runtime application 边界

### 7.1 Composition 负责

Composition 回答“使用哪个实现、如何连接、生命周期多长”：

- 实例化具体 adapter；
- 连接跨 BC Port；
- 构造 `RuntimeServices`；
- 构造并注入 `RuntimeContextFactory` 及其依赖的 factory/port 实现；
- 创建长期 registry、runner、materializer、scheduler 和 typed factory；
- 管理进程/Agent Runtime 级资源生命周期。

### 7.2 Runtime application 负责

Runtime application 回答“何时发生什么业务动作”：

- 创建或恢复 Session；
- 读取 committed snapshot 的时机；
- 初始化和更新 `SessionState`；
- 解析本次会话或 Run 的模型选择；
- 接纳唯一 typed `ChatRequest.ingress`，由 `SessionInputMailbox` 独占读取 `ChatInputEventPort`；
- 将可接纳输入送入当前 `RunInputBuffer`，将 sealed Run 拒绝的 late input defer 回 Session mailbox；
- 在 Session 边界执行或调度控制命令，并由独立 `SessionIngress` 定向分发 interaction reply/cancel；
- 创建 `RunSpec` 与 `RunCreationRequest`；
- 在 Run 创建点调用 `RunFactory::create` 取得 `RunInstance`；
- 驱动 Loop 和领域状态迁移。

### 7.3 退役 `from_args.rs` 大装配器

当前 `from_args.rs` 同时承担参数解析、Session 恢复、模型绑定、Tool/Skill 查询、Prompt 构建、并发配置、Agent runner 创建、基础设施创建和 Client 构造，已形成 Runtime 内第二个 Composition Root。

目标不是把整个文件移动到 `agent/composition`，而是按职责拆解：

- 入站边界先将 CLI/SDK args 标准化为 typed bootstrap request；
- Composition 完成具体 adapter/object graph；
- Runtime bootstrap 只执行 Session 启动用例并创建 `SessionState`；
- Provider、Prompt、Skill 初始化委托各自 application service；
- Run 创建统一提交 `RunCreationRequest`。

最终 `from_args.rs` 应删除，或收敛为很薄的 `bootstrap_runtime(request, services)` 入口。

## 8. Workspace、Prompt、Skills 与 Config

### Workspace

- `RuntimeServices`：Workspace service/control 与 capability factory。
- `SessionState`：当前 workspace identity/revision 或 session 投影。
- `RuntimeContext`：按 `workspace_mode` 绑定后的窄 capability。
- `RunExecutionState`：只保存已读路径、使用 revision 等纯执行事实。

Workspace 不得继续绕过 `RuntimeContext` 旁路进入 Loop adapter。

### Prompt 与 Skills

- Prompt builder、Skill catalog/materializer 属于 `RuntimeServices`。
- 当前 source revision 属于 `SessionState`。
- 本次 Run 可见的 prompt/skill snapshot 在 Run 创建时冻结。
- 模型请求实际使用的消息和 prompt snapshot 属于 `RunExecutionState`。

完整 `skills_map` 不得成为第二份长期真相；SDK/TUI 视图按需投影。

### Config

- Config query/writer 属于 `RuntimeServices`。
- committed revision 属于 `SessionState`。
- 本次 Run 的裁剪配置属于 `RuntimeContext`。
- Step 只能消费冻结快照，禁止读取动态 Config service。

`RunConfigSnapshot` 只有在裁剪并守护 Run 配置边界时才保留；若仅代理完整 `ConfigSnapshot` 而不提供不变量，应消除无价值包装。

## 9. 终态类型映射

| 非终态形状 | 终态处理 |
|---|---|
| `RuntimeHandle` 综合句柄 | 拆为入站 `AgentRuntime` façade、`RuntimeServices` 与 `SessionState` |
| `MainSessionShell` | 删除，动态事实进入 `SessionState`，长生命周期能力进入 `RuntimeServices` |
| `RuntimeBootstrapDependencies` 及分层参数袋 | 由 Composition 构造有职责的 services/factories，bootstrap 只接 typed request |
| `RuntimeContextParts` / `RunContextBindings` | 删除；调用方只提交 `RunCreationRequest` |
| `assemble_main_runtime_context` / `derive_sub_run` 手填 Context | 统一进入 `RunFactory::create`，由 `RuntimeContextFactory` 创建 Context |
| `RunKind::Main/Sub` | 删除；使用 purpose、父子拓扑和 capability modes |
| `MainRunPort` / `SubAgentRun` | 删除；执行统一为 `Run + RuntimeContext + RunExecutionState` |
| fat `RunLoopPort` / `LoopCapabilityAdapter` | 删除；Engine 按领域阶段调用真实 coordinator 与窄外部 seam，禁止能力全集替代物 |
| `ChatLoopCapabilityAdapter` / `DerivedLoopCapabilityAdapter` | 删除；来源目录只保留 input/source、observer、topology/request 与 terminal mapping |
| `MainInputStrategy` / `SubInputStrategy` | 删除；factory 绑定统一 `InputPort` adapter |
| `MainEventStrategy` / `SubEventStrategy` | 删除；factory 绑定统一 `EventSink` adapter |
| 直接复用父 `InteractionPort` | 改为 child-scoped `ParentMediatedInteractionPort` |
| Sub Hook 复用或过滤完整 HookPort | 为 Sub 装配无底层委托的 `EmptyHookPort` |
| `ChatLoopContext` | 拆为 session command driver、`RunCreationRequest` 与 `RunExecutionState` |
| `ParentRunContextSource` | 以 parent capability registry/frame 的真实语义归入 `SessionState` |
| `DerivedSubRun` | 删除；所有来源统一返回 `RunInstance` |
| `RuntimeWorkspaceAccess` 旁路 | 收入 `WorkspaceBindingFactory` 产生的窄 Run capability |
| `from_args.rs` 大装配器 | 删除或收敛为 `bootstrap_runtime(request, services)` 薄入口 |

## 10. 命名约束

目标类型名必须表达领域角色、所有权和生命周期：

- `Settings` / `Config` 只表示配置值；
- `State` 表示明确 owner 的可变状态；
- `Services` 表示长生命周期活依赖；
- `Context` 表示明确执行边界内的冻结上下文；
- `Factory` 表示受约束的构造入口；
- 避免语义过空的 `Scope`、`Parts`、`Handle`、`Manager` 或来源命名 `from_args`；
- 能力字段使用 `*_mode`、`*_route`、`*_scope` 等后缀，避免把模式值命名成对象；
- 可由数据表达的差异不得固化进生产类型名。

全仓命名原则应在根 `AGENTS.md` 登记，Rust 细则归 `specs/rust-coding.md`；本文只记录 Runtime 模型中的具体应用。

## 11. 验收条件

- 生产类型、Factory 和 Loop Engine 不含 Main/Sub 分类或行为分支。
- `RuntimeServices` 与 `SessionState` 无字段类别交叉。
- Runtime 拥有 factory/port 抽象和能力选择规则；Composition 只提供实现与对象图。
- 所有 Run 来源只提交 `RunCreationRequest`，并取得统一 `RunInstance`。
- `RuntimeContext` 只能由统一 factory 构造，且创建后能力不可替换。
- `RuntimeContextParts`、`RunContextBindings`、多套 assembler 和 Runtime 内第二 Composition Root 均退役。
- fat `RunLoopPort`、`LoopCapabilityAdapter`、来源型大 Loop adapter 和 Main/Sub strategy 类型均退役；流程只由按领域阶段组织的 Loop Engine 与对应 coordinator 拥有。
- Engine、Launcher 及任一阶段对象均不要求同一类型实现整组 Runtime 能力；不存在 trait alias、supertrait、fat struct、参数袋或展开参数列表形式的能力全集。
- Chat/Derived 来源目录只拥有 source、observer、topology/request 与 terminal mapping，不拥有模型、工具、Context、Interaction、Persistence、Hook 或 Finalization 主流程。
- `ParentMediated` 使用 child-scoped adapter，具备 request ownership、并发隔离和精确 teardown。
- Sub Hook 使用独立 `EmptyHookPort`，禁止执行或转发任何 Hook invocation。
- `Run` 与 `RunExecutionState` 无重复状态所有权。
- Workspace、Prompt、Skills、Config 均按本文生命周期边界流动。
- 每层装配、状态转换、能力不扩权、父子并发隔离和端到端场景都有相邻契约测试。
