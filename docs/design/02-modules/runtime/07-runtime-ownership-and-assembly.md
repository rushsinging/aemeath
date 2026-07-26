# Agent Runtime · 运行时所有权与统一装配

> 层级：02-modules / runtime（模块战术设计）  
> 状态：Target（目标设计）｜对应 Issue：[#1385](https://github.com/rushsinging/aemeath/issues/1385)、[#1397](https://github.com/rushsinging/aemeath/issues/1397)、[#1248](https://github.com/rushsinging/aemeath/issues/1248)\
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

RunPreparationRequest
  ├─ RunSpec
  ├─ SessionSnapshot
  ├─ ParentRunCapabilities?
  └─ InitialInput
                 │
                 ▼
        RuntimeContextFactory
          ├─ 校验能力 ceiling
          ├─ 选择/收缩 capability adapter
          └─ 创建 per-Run resource
                 │
                 ▼
          PreparedRun
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

`Run` 不持有消息窗口、Provider/Tool Port、流式进度或 UI terminal projection。

### 4.4 `RunExecutionState`：Loop 工作集

`RunExecutionState` 只拥有 Loop 执行过程中产生和更新的工作数据：

- messages 与 committed boundary；
- Step message ownership；
- accepted/adopted inputs；
- ContextRequest / ContextWindow；
- turn count 与 invocation usage projection；
- tool identity 与 continuation 工作数据；
- stream progress；
- prompt/request projection；
- terminal output projection。

以下事实只属于 `Run`，不得复制到 `RunExecutionState`：status、active Step status、pending interaction、cancellation/termination 状态、domain events。

以下事实只属于 `RunExecutionState`，不得复制到 `Run`：messages、context request/window、token projection、input adoption、stream progress 和 tool execution working data。

Loop Engine 的顺序是：先请求 `Run` 完成合法状态迁移，再更新 `RunExecutionState`，外部副作用只经 `RuntimeContext`，最终由 `Run` 产生领域事件并由 adapter 投影。

## 5. 统一 `RuntimeContextFactory`

`RuntimeContextFactory` 是 Runtime application 定义的应用服务，也是唯一允许构造 `RuntimeContext` 的入口。Composition 注入它所消费的窄 factory/port 实现，但不得拥有 `RunSpec` 的能力决策。

### 5.0 IoC 端到端伪代码

下面的伪代码描述的是职责和数据流，不是要求照抄的 Rust API。关键约束是：Composition 负责“有哪些实现和 wiring”，Runtime application 负责“本次 Run 需要什么能力”，Factory 负责“把声明转换为受限 capability”；三者不能越界。

```rust
// 1. Composition Root：只在 agent/composition 创建具体实现和 opaque wiring。
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
        factory: Arc::new(factory),
        session_wiring: wiring.session,
    }
}

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
    Ok(AgentRuntime::new(assembly.factory.clone(), session))
}

// 3. Runtime application：在业务时机取得一致快照，并仅声明能力模式。
async fn prepare_root_run(
    runtime: &mut AgentRuntime,
    input: InitialRunInput,
) -> Result<PreparedRun, RunPreparationError> {
    let snapshot = runtime.session.snapshot_for_run()?;
    let spec = RunSpec::for_purpose(RunPurpose::Interactive)
        .with_input_mode(InputMode::LiveSession)
        .with_interaction(InteractionBindingMode::Client)
        .with_hook(HookBindingMode::Full);

    runtime.factory.prepare(RunPreparationRequest {
        spec,
        session: snapshot,
        parent: None,
        initial_input: input,
    }).await
}

// 4. 派生 Run 使用同一个入口；parent 只提供受限 capability view。
async fn prepare_child_run(
    runtime: &mut AgentRuntime,
    parent: &PreparedRun,
    input: InitialRunInput,
) -> Result<PreparedRun, RunPreparationError> {
    let snapshot = runtime.session.snapshot_for_run()?;
    let parent_caps = parent.run.child_capabilities();
    let spec = RunSpec::for_purpose(RunPurpose::AgentTask)
        .with_parent(parent.run.id())
        .with_interaction(InteractionBindingMode::ParentMediated)
        .with_hook(HookBindingMode::BoundaryOnly)
        .with_capability_ceiling(parent_caps.ceiling());

    runtime.factory.prepare(RunPreparationRequest {
        spec,
        session: snapshot,
        parent: Some(parent_caps),
        initial_input: input,
    }).await
}

// 5. Factory：校验声明、选择 adapter、原子地产生三件 per-Run 对象。
async fn prepare(
    &self,
    request: RunPreparationRequest,
) -> Result<PreparedRun, RunPreparationError> {
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
        HookBindingMode::BoundaryOnly =>
            self.services.hooks.bind_boundary_only(parent_ceiling).await?,
        HookBindingMode::Disabled => self.services.hooks.bind_disabled(),
    };

    let context = RuntimeContext::private_new(
        self.services.provider.bind(&request.spec, &request.session).await?,
        self.services.context.bind(&request.spec, &request.session).await?,
        self.services.tools.bind_restricted(&request.spec, parent_ceiling).await?,
        interaction,
        hooks,
        self.services.workspace.bind(&request.spec, &request.session).await?,
        request.session.run_config_snapshot(),
    );
    let run = Run::new(request.spec, request.initial_input.clone());
    let execution = RunExecutionState::from_initial_input(run.id(), request.initial_input);
    Ok(PreparedRun { run, context, execution })
}

// 6. Loop Engine 只消费已绑定能力，不回调 adapter 决定业务流程。
async fn execute(prepared: PreparedRun) -> Result<AgentRunTerminal, RunExecutionError> {
    let PreparedRun { mut run, mut execution, context } = prepared;
    loop_engine::run_loop(&mut run, &mut execution, &context).await
}
```

伪代码表达的 IoC 边界：

- `compose_runtime` 可以选择实现，但不能解析 `RunSpec` 决定能力升降；
- `prepare_root_run` / `prepare_child_run` 可以声明模式，但不能 `new` 具体 Port；
- `prepare` 可以按模式创建 capability adapter，但不能执行 Loop、恢复 Session 或持久化 Step；
- `execute` 只能使用 `PreparedRun`，不能重新装配 Context，也不能按 Main/Sub 分支；
- `RuntimeContext::private_new` 必须只对 factory 可达，失败时不得返回半装配的 `PreparedRun`。

### 5.1 输入与输出

调用方只提交纯值准备请求：

```rust
struct RunPreparationRequest {
    spec: RunSpec,
    session: SessionSnapshot,
    parent: Option<ParentRunCapabilities>,
    initial_input: InitialRunInput,
}

struct ParentRunCapabilities {
    run_id: RunId,
    context: RuntimeContextCapabilityView,
    cancellation: RunCancellationScope,
    workspace: ParentWorkspaceCapability,
}

struct PreparedRun {
    run: Run,
    context: RuntimeContext,
    execution: RunExecutionState,
}
```

`SessionSnapshot`、`InitialRunInput` 和 `ParentRunCapabilities` 只携带准备本次 Run 所需的窄事实或 capability view。它们不得暴露 `SessionState` 锁、Composition wiring、具体 adapter、完整父 `RuntimeContext` 或可越权的服务集合。

Factory 负责：

1. 校验 `RunSpec` 与 parent capability ceiling；
2. 从 `RuntimeServices` 的窄 factories 选择并绑定 Context、Provider、Tool、Policy、Hook、Memory、Task、Reasoning、Interaction、Reflection 和 Workspace capability；
3. 按模式创建 shared、isolated、restricted、parent-mediated 或 unavailable adapter；
4. 创建 cancellation、input、event、usage 等 per-Run 实例；
5. 冻结 `RunConfigSnapshot`、provider/model binding 与 workspace projection；
6. 创建相互一致的 `Run`、`RuntimeContext`、`RunExecutionState`，返回 `PreparedRun`。

Factory 不负责执行 Loop、恢复 Session、修改 `SessionState`、处理模型响应、持久化 Step 或发布终态事件。

### 5.2 禁止调用方手填依赖

`RuntimeContext::new` 仅对 factory 实现可见。以下形状不属于终态：

- `RuntimeContextParts`、`RunContextBindings` 或任何与 `RuntimeContext` 字段近似一一对应的参数袋；
- Main、Sub、Reflection、Scheduler 各自公开 assembler；
- 调用方先创建 Provider、Context、Interaction、Hook adapter，再让 factory 被动复制；
- 通过 `Option<Arc<dyn Port>>` 表示能力开关；
- 直接传入完整父 `RuntimeContext` 并复用其无关能力。

新 Run 来源可以不同，但准备入口只有一个。独立 Run、派生 Run、后台 Run 与未来 Scheduler Run 都构造 `RunPreparationRequest`，不能建立第二条装配路径。

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
- `BoundaryOnly`：由 `BoundaryHookPort` 只允许 Run/child start-stop 边界事件；
- 若未来存在 `Disabled`：绑定 typed no-op/disabled adapter，而不是 `Option`。

`BoundaryOnly` 不能只校验 parent 存在后复用完整 HookPort。过滤发生在 adapter 调用入口，禁止调用方绕过 adapter 取得底层 HookPort。Hook BC 继续拥有 subscription、脚本执行、重试和 typed directive；Runtime-owned `HookCoordinator` 统一拥有触发时机、Stop 三分支、反馈 materialization 与 Run 状态编排。Main/Sub 不得分别实现 Stop Hook 流程。

### 5.5 Factory Port 所有权

Runtime 定义自己需要的构造契约，例如 `ProviderBindingFactory`、`ContextBindingFactory`、`ToolBindingFactory`、`InteractionBindingFactory`、`HookBindingFactory` 和 `WorkspaceBindingFactory`。契约参数必须是 `RunSpec`、session snapshot、parent capability 等 Runtime 语言；返回值必须是 Runtime 消费的窄能力。

Composition 或供应 BC adapter 实现这些契约。Runtime 不依赖 `provider::composition`、`context::wire_*`、Workspace wiring 或具体 Registry；Composition 也不得解析 `RunSpec` 后自行决定能力升降。

## 6. Loop Engine 的控制反转

Loop Engine 是 Agent Execution 用例的流程 owner。它直接编排 `Run`、`RunExecutionState` 和 `RuntimeContext` 中的窄能力，不通过 adapter 回调决定业务顺序。

```rust
async fn run_loop(
    run: &mut Run,
    execution: &mut RunExecutionState,
    context: &RuntimeContext,
) -> Result<AgentRunTerminal, RunExecutionError>
```

Engine 负责：

- input drain/await、epoch 校验与 Step 创建；
- context window/compact、model invocation 与 retry；
- Tool coordination、Interaction continuation、Hook coordination；
- control/cancellation、Step finalization 和 terminal mutation；
- 每次领域 mutation 后立即 drain event 并交给 `EventSink`。

`RuntimeContext` 中的 Port 只执行外部能力；`RunExecutionState` 只保存工作集；二者均不得调用回 Engine 或决定下一阶段。fat `RunLoopPort` 必须删除，不能以“统一接口”为名保留以下混合职责：

- `freeze_step`、`accept_step_input`、`finalize_step` 等流程方法；
- `needs_compaction`、`invoke_model`、`execute_tools` 等 coordinator 包装；
- `claim_terminal`、`take_control` 等聚合/registry 操作；
- `store_interaction`、`set_pending_interaction_work` 等重复状态槽；
- `emit` 与具体 UI/progress 投影混合。

确有外部边界价值的能力拆为窄 Port，例如 `InputPort`、`EventSink`、`UsageSink`、`ActiveRunRegistryPort`；它们由 Runtime 定义、Composition 注入，不承载 Loop 流程。

Input 差异由绑定后的 `InputPort` 表达：live session input、fixed initial input 或未来 queue/scheduler input 都实现相同 drain/await 契约。Event 差异由 `EventSink` 表达。Provider、Tool、Hook 与 Interaction 差异同理由 capability adapter 表达。任何差异都不得重新形成 `MainInputStrategy` / `SubInputStrategy`、`MainEventStrategy` / `SubEventStrategy` 等生产类型。

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
- 接纳输入并创建 `RunSpec` 与 `RunPreparationRequest`；
- 在 Run 创建点调用 `RuntimeContextFactory::prepare` 取得 `PreparedRun`；
- 驱动 Loop 和领域状态迁移。

### 7.3 退役 `from_args.rs` 大装配器

当前 `from_args.rs` 同时承担参数解析、Session 恢复、模型绑定、Tool/Skill 查询、Prompt 构建、并发配置、Agent runner 创建、基础设施创建和 Client 构造，已形成 Runtime 内第二个 Composition Root。

目标不是把整个文件移动到 `agent/composition`，而是按职责拆解：

- 入站边界先将 CLI/SDK args 标准化为 typed bootstrap request；
- Composition 完成具体 adapter/object graph；
- Runtime bootstrap 只执行 Session 启动用例并创建 `SessionState`；
- Provider、Prompt、Skill 初始化委托各自 application service；
- Run 创建统一提交 `RunPreparationRequest`。

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
- 本次 Run 可见的 prompt/skill projection 在 Run 创建时冻结。
- 模型请求实际使用的消息和 prompt projection 属于 `RunExecutionState`。

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
| `RuntimeContextParts` / `RunContextBindings` | 删除；调用方只提交 `RunPreparationRequest` |
| `assemble_main_runtime_context` / `derive_sub_run` 手填 Context | 统一进入 `RuntimeContextFactory::prepare` |
| `RunKind::Main/Sub` | 删除；使用 purpose、父子拓扑和 capability modes |
| `MainRunPort` / `SubAgentRun` | 删除；执行统一为 `Run + RuntimeContext + RunExecutionState` |
| fat `RunLoopPort` | 删除；真实外部 seam 拆为窄 Port，流程归 Loop Engine |
| `MainInputStrategy` / `SubInputStrategy` | 删除；factory 绑定统一 `InputPort` adapter |
| `MainEventStrategy` / `SubEventStrategy` | 删除；factory 绑定统一 `EventSink` adapter |
| 直接复用父 `InteractionPort` | 改为 child-scoped `ParentMediatedInteractionPort` |
| `BoundaryOnly` 仅做存在性校验 | 改为装配 `BoundaryHookPort` 执行能力过滤 |
| `ChatLoopContext` | 拆为 session command driver、`RunPreparationRequest` 与 `RunExecutionState` |
| `ParentRunContextSource` | 以 parent capability registry/frame 的真实语义归入 `SessionState` |
| `DerivedSubRun` | 删除；所有来源统一返回 `PreparedRun` |
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
- 所有 Run 来源只提交 `RunPreparationRequest`，并取得统一 `PreparedRun`。
- `RuntimeContext` 只能由统一 factory 构造，且创建后能力不可替换。
- `RuntimeContextParts`、`RunContextBindings`、多套 assembler 和 Runtime 内第二 Composition Root 均退役。
- fat `RunLoopPort`、Main/Sub Loop adapter 和 Main/Sub strategy 类型均退役；流程只由 Loop Engine 拥有。
- `ParentMediated` 使用 child-scoped adapter，具备 request ownership、并发隔离和精确 teardown。
- `BoundaryOnly` 使用真实 Hook capability adapter，禁止访问非边界 Hook invocation。
- `Run` 与 `RunExecutionState` 无重复状态所有权。
- Workspace、Prompt、Skills、Config 均按本文生命周期边界流动。
- 每层装配、状态转换、能力不扩权、父子并发隔离和端到端场景都有相邻契约测试。
