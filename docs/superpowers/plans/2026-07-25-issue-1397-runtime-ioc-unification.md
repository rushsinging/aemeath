# #1397 Runtime：以 RunExecutionState + RuntimeContext 统一 Main/Sub Loop adapter 实施计划

> 对应 Issue：[#1397](https://github.com/rushsinging/aemeath/issues/1397)
> 设计基线：[07-runtime-ownership-and-assembly.md](../../design/02-modules/runtime/07-runtime-ownership-and-assembly.md)
> 前置：[#1382](https://github.com/rushsinging/aemeath/issues/1382)、[#1385](https://github.com/rushsinging/aemeath/issues/1385)、[#1248](https://github.com/rushsinging/aemeath/issues/1248)
> 计划状态：P0-P5 已完成；P6 生产消费者迁移与旧路径退役进行中

## 1. 目标与实施原则

本计划把当前 Runtime 从 Main/Sub 两套生产 Loop adapter 收敛为统一生产模型：

```text
RuntimeServices + SessionState
              ↓ RuntimeContextFactory(RunSpec, snapshot)
Run + RunExecutionState + RuntimeContext
              ↓
       单一 Loop Engine
```

实施必须遵循以下原则：

1. **根因级收敛**：不通过保留 `MainRunPort` / `SubAgentRun` 外壳或新增兼容超集字段止血；流程控制权最终归 Loop Engine，外部能力拆成窄 Port。
2. **单一装配入口**：所有 Run 来源只提交 `RunPreparationRequest`，统一取得 `PreparedRun`；调用方不得手填 `RuntimeContext` 同构参数包。
3. **所有权先于搬运**：先建立 `RuntimeServices`、`SessionState`、`RunExecutionState` 的字段归属和快照边界，再迁移消费者，避免同一事实双写。
4. **按依赖方向迁移**：领域模型 → Runtime-owned Port / Published Language → Factory → Composition adapter → Loop consumer → 退役旧路径 → Guard / 文档回写。
5. **逐层 TDD**：核心逻辑先补失败测试或确认现有覆盖，再实施；跨层链路必须覆盖每个相邻边界，不能只测最终 Loop。
6. **保持行为不变**：`#1272` input epoch、compact/reflection、interaction/control、retry/cancel、tool continuation、Stop Hook 和持久化顺序必须在迁移中保持既有语义。

## 1.1 IoC 伪代码与实现对应关系

以下伪代码是实施时的接口和责任基线；具体 Rust 类型名可以按现有模块调整，但不得改变调用方向：

```rust
// Composition Root：唯一创建 concrete adapter 和 opaque wiring 的位置。
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
        wiring.session,
    )
}

// Runtime application：捕获 snapshot、声明 RunSpec，创建 Idle Run。
async fn prepare_run(
    runtime: &mut AgentRuntime,
    parent: Option<ParentRunCapabilities>,
) -> Result<PreparedRun, RunPreparationError> {
    let spec = RunSpec::from_parent(parent.as_ref())?;
    runtime.preparer.prepare(RunPreparationRequest {
        spec,
        session: runtime.session.snapshot_for_run()?,
        parent,
    }).await
}

// 输入独立于装配；首次和后续输入都通过同一个 Port 激活 Run。
async fn submit_input(
    prepared: &PreparedRun,
    input: InputEnvelope,
) -> Result<(), InputError> {
    prepared.context.input().submit(input).await
}

// RunPreparer：校验 ceiling，委托 Context factory 绑定窄能力，原子返回 Idle per-Run 对象。
async fn prepare(&self, request: RunPreparationRequest) -> Result<PreparedRun, RunPreparationError> {
    request.spec.validate_against(request.parent.as_ref())?;
    let interaction = self.interaction.bind(request.spec.interaction_mode,
                                              request.parent.as_ref()).await?;
    let hooks = self.hooks.bind(request.spec.hook_mode,
                                 request.parent.as_ref()).await?;
    let context = RuntimeContext::private_new(
        self.provider.bind(&request.spec, &request.session).await?,
        self.context.bind(&request.spec, &request.session).await?,
        self.tools.bind_restricted(&request.spec, request.parent.as_ref()).await?,
        interaction,
        hooks,
        self.workspace.bind(&request.spec, &request.session).await?,
        request.session.run_config_snapshot(),
    );
    let run = Run::idle(request.spec);
    let execution = RunExecutionState::empty(run.id());
    Ok(PreparedRun { run, context, execution })
}

// Loop Engine：只消费已绑定能力，不重新装配或按 Main/Sub 分支。
async fn execute(prepared: PreparedRun) -> Result<AgentRunTerminal, RunExecutionError> {
    let PreparedRun { mut run, mut execution, context } = prepared;
    loop_engine::run_loop(&mut run, &mut execution, &context).await
}
```

实现任务与伪代码的对应关系：

| 伪代码阶段 | 计划阶段 | 必须验证 |
|---|---|---|
| `compose_runtime` | P3/P4 | concrete constructor 只在 Composition，factory contracts 可注入 |
| `snapshot_for_run` | P2/P4 | Session 变化不污染已准备 Run |
| `RunSpec::from_parent` | P2 | 能力只收缩不扩权，无 Main/Sub 控制分支 |
| `RunPreparer::prepare` | P3 | 单一入口、mode adapter、typed unavailable、无参数袋、产出 Idle Run |
| `InputPort::submit` | P1/P5 | 首次与后续输入同路径，Idle 状态只由输入激活 |
| `ParentMediated` / `BoundaryOnly` | P3 | child identity 隔离、Hook invocation 入口过滤 |
| `PreparedRun` | P1/P3 | Run、Context、Execution 一致创建且无双 owner |
| `loop_engine::run_loop` | P5/P6 | Engine 直接编排，fat `RunLoopPort` 不再拥有流程 |

## 2. 当前实现基线与主要缺口

当前分支已具备迁移基础，但仍保留明显的终态缺口：

- `RuntimeContextFactory` 已存在，但仍接收 `RunContextBindings`，并以公开 `assemble` / 多参数构造形状承载 per-Run 绑定；这与终态的纯值 `RunPreparationRequest → PreparedRun` 不一致。
- `RuntimeServices` 已存在，但尚未与真正的 `SessionState` 形成清晰的静态依赖 / 会话事实边界。
- `RuntimeContext` 已通过 assembly token 收窄部分构造，但 `RuntimeContextParts` 等参数包和多路径调用仍存在。
- Loop Engine 仍以 fat `RunLoopPort` 驱动大量流程协调；`MainRunPort` 和 `SubAgentRun` 分别实现该 trait。
- `MainRunPort` 的生产创建点仍在 `main_loop/looping/loop_runner.rs`，Sub runner 仍在 `subagent/runner/*` 自行构造 bindings 和角色化对象。
- `from_args.rs` 仍是 Runtime 内的重要 bootstrap / client 装配入口，虽然部分具体对象图已经移至 `agent/composition`。
- 现有测试大量直接构造 `RunContextBindings`、`RuntimeContextFactory` 和 `MainRunPort`，需要随着生产边界迁移；测试迁移不得扩大生产 API。
- 设计文档已经描述终态，但源码、架构守卫和 Issue checklist 尚未达到终态完成定义。

计划中的“删除”均指生产路径和目标模型中的退役；只有在无生产引用、测试替代完成、Guard 更新且验证通过后才物理删除旧类型。

## 3. 阶段与依赖图

```text
P0 基线与契约冻结
 └─▶ P1 领域状态与 RunExecutionState
      └─▶ P2 RuntimeServices / SessionState / Preparation PL
           └─▶ P3 Factory 收口与 capability adapter
                ├─▶ P4 Composition / bootstrap 收口
                └─▶ P5 统一 Loop Engine
                       └─▶ P6 消费方迁移与旧路径退役
                              └─▶ P7 Guard、跨层验证、文档与 Issue 回写
```

P3 完成后，P4 与 P5 可以在同一分支中按文件冲突情况交错实施，但都依赖 P3 的最终准备契约。P6 必须等待 P4/P5 的统一生产入口稳定，避免旧 adapter 与新 Engine 并存形成第二条生产路径。

## 4. 详细实施步骤

### P0：基线、门禁与测试契约冻结

**目的**：把 Issue 完成定义转成可执行检查，防止迁移过程中只凭编译通过判断完成。

**工作项**：

1. 在 `origin/main` 最新基线重新确认 #1397 body、#1248 生产依赖和 #1413 设计 PR 状态；记录当前 HEAD、测试基线和架构 Guard 输出。
2. 建立 Runtime 旧符号清单：`MainSessionShell`、`MainRunPort`、`SubAgentRun`、`RunLoopPort`、`RuntimeContextParts`、`RunContextBindings`、`RunKind::Main/Sub`、Main/Sub strategies、`from_args` 大装配引用。
3. 建立行为保护矩阵，至少覆盖：Run 状态迁移、RunStep finalization、input drain epoch、context/compact、model retry、tool continuation、interaction reply/cancel、control cancellation、Hook Stop 三分支、Sub capability ceiling、Session snapshot 隔离。
4. 确认每项行为的 L1/L2/L3/L4 证据位置；已有测试不足时先新增失败契约测试，测试文件按 owning layer 放置。

**主要文件 / 检查点**：

- `agent/features/runtime/src/application/loop_engine/engine.rs`
- `agent/features/runtime/src/application/runtime_context.rs`
- `agent/features/runtime/src/application/runtime_context_factory.rs`
- `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- `agent/features/runtime/src/application/subagent/runner/loop_run.rs`
- `agent/features/runtime/src/application/client/from_args.rs`
- `agent/composition/src/runtime.rs`
- `.agents/hooks/check-*.sh` 与 `.agents/aemeath.json`

**完成证据**：基线测试结果、旧符号引用清单、Issue checklist 到测试层的映射表。

### P1：确立 `Run` 与 `RunExecutionState` 的唯一所有权

**目的**：先消除领域状态和 Loop 工作集的重叠，再迁移能力依赖。

**实施**：

1. 审计 `Run` 及 `RunStep` 当前字段，把 status、active Step 状态、pending interaction identity/continuation、cancel/terminate、drain epoch、domain events 保留在 `Run`。
2. 新增或收敛 `RunExecutionState`，只保存 messages、accepted input、ContextRequest/Window、turn/token projection、tool/continuation working data、stream/terminal projection。
3. 将 Loop 当前持有的消息、窗口、usage、tool working data 从 Main/Sub adapter 移入 `RunExecutionState`；不得把 Port、Session coordinator、Composition wiring 或完整 Context 放入其中。
4. 为关键状态转移补 L1/L2 测试：非法迁移、cancel/terminate 优先级、pending interaction 唯一性、Run 与 execution 无重复字段事实。
5. 保留行为兼容的临时转换仅限迁移边界，并记录删除条件；不得以复制字段长期维持双 owner。

**主要文件**：

- `agent/features/runtime/src/domain/agent_run.rs`
- `agent/features/runtime/src/domain/agent_run/domain.rs`
- `agent/features/runtime/src/domain/agent_run/state.rs`
- `agent/features/runtime/src/domain/agent_run/step.rs`
- 新增或调整 `agent/features/runtime/src/application/run_execution_state.rs` 及其 `*_tests.rs`
- `agent/features/runtime/src/application/loop_engine/engine.rs`
- `agent/features/runtime/src/application/loop_engine/tests.rs`

**完成证据**：领域状态测试、execution 工作集测试、生产编译无重复状态访问、旧 adapter 行为测试仍通过。

### P2：建立 `RuntimeServices`、`SessionState` 与准备 Published Language

**目的**：把静态长生命周期依赖、会话动态事实和单次 Run 准备输入分开。

**实施**：

1. 定义 Runtime-owned `RuntimeServices`，只保存跨 Run 稳定的 Port、factory 和基础设施；不得包含 session identity、当前模型、resume 状态、messages、window、input buffer、usage 或 cancellation。
2. 定义 Runtime-owned `SessionState`，只保存 session identity、模型/config/workspace revision、resume/reminder/read-files、active run 和 parent frame 等会话事实；不得保存 services、factory、RuntimeContext 或 RunExecutionState。
3. 定义窄 `SessionSnapshot`，明确 snapshot 时机、不可变字段和 revision；后续 Session 变更不得污染已准备 Run。
4. 定义 `RunSpec` 的 capability modes、purpose、parent relation 和 ceiling；能力差异由数据表达，不再由 Main/Sub 类型表达。
5. 定义 `RunPreparationRequest`、`ParentRunCapabilities`、`PreparedRun` 和 typed preparation errors。准备请求不得包含输入内容，也不得暴露锁、完整父 Context、Composition wiring 或 concrete adapter；Factory 必须创建 `Idle Run + empty RunExecutionState`，首次和后续输入全部经 `InputPort` 激活。
6. 为 snapshot 隔离、parent ceiling 收缩、unavailable capability、request 字段完整性，以及 `prepare → Idle → InputPort 激活` 补 L1/L2/L3 契约测试。

**主要文件**：

- `agent/features/runtime/src/application/runtime_context.rs`
- `agent/features/runtime/src/application/runtime_context_factory.rs`
- `agent/features/runtime/src/domain/agent_run/spec.rs`
- `agent/features/runtime/src/application/client/accessors.rs`
- 新增或调整 `agent/features/runtime/src/application/session_state.rs`、`session_snapshot.rs`
- `agent/features/runtime/src/application/runtime_preparation.rs` 及测试（如现有模块不适合承载）
- `agent/features/runtime/src/lib.rs`

**完成证据**：字段分类表落实到代码、snapshot/ceiling 契约测试、无 `*Parts` 替代参数袋进入新 API。

### P3：收口唯一 `RuntimeContextFactory` 与 capability adapter

**目的**：让 factory 真正负责按 `RunSpec` 选择和收缩能力，而不是被动复制调用方提供的 bindings。

**实施**：

1. 将公共生产准备入口收敛为 `RunPreparer::prepare(RunPreparationRequest) -> Result<PreparedRun, RunPreparationError>`；`RuntimeContextFactory` 只产出 `RuntimeContext`，并把 `RuntimeContext::new` 保持为 factory 私有可达入口。
2. 把现有 seven explicit port parameters / `RuntimeServices` 组装改为 Composition 注入 Runtime-owned 的窄 binding factories；Runtime 决定 capability mode 和 ceiling，Composition 只实现契约。
3. 删除 `RuntimeContextParts`、`RunContextBindings` 及同构参数袋；所有 per-Run Input/Event/Usage/Control/Interaction/Hook/Reasoning/Workspace 绑定由 factory 按 request 产生。
4. 实现三类关键受限 adapter：
   - `ParentMediatedInteractionPort`：child-scoped request ownership、parent route、reply/cancel identity 匹配和 child teardown；不得复用父 InteractionPort Arc。
   - `BoundaryHookPort`：只允许 Run/child start-stop 边界调用，不能靠调用方自律绕过。
   - `Unavailable` typed adapter：不使用 `Option<Arc<dyn Port>>` 表达禁用能力。
5. 为 Full / BoundaryOnly / Disabled、Client / ParentMediated / Unavailable、Reasoning Fixed/Inherit/NoOp 和 parent capability ceiling 补 L1/L2/L3 测试。
6. 更新 Runtime capability assembly、Hook assembly、Tool assembly 架构守卫，确保只允许 Composition 注入实现、Runtime 不依赖 concrete provider/context/workspace wiring。

**主要文件**：

- `agent/features/runtime/src/application/runtime_context.rs`
- `agent/features/runtime/src/application/runtime_context_factory.rs`
- `agent/features/runtime/src/application/runtime_context_factory_tests.rs`
- `agent/features/runtime/src/application/runtime_context_tests.rs`
- `agent/features/runtime/src/ports/*.rs`
- `agent/features/runtime/src/adapters/*.rs`
- `agent/features/runtime/src/application/interaction_coordinator.rs`
- `agent/features/runtime/src/application/hook_types.rs`
- `agent/composition/src/runtime.rs`
- `agent/composition/src/runtime_tests.rs`
- `agent/features/runtime/tests/bootstrap_dependencies.rs`

**完成证据**：Factory 单一入口的编译期 / source guard、capability matrix 契约测试、parent child 并发隔离测试、所有旧 bindings 引用清零。

### P4：收敛 Composition 与 Runtime bootstrap

**完成证据（已达成部分）**：

- 初始 Provider、Prompt、Skill、AgentRunner、并发和 RuntimeContextFactory 均由 Composition typed assembly 注入。
- `SessionState`、`SessionModelState` 与 `SessionRuntime` 的静态 / 动态所有权已分离。
- Runtime bootstrap 通过 `SessionRuntime::new` 构造，生产 `from_args` 不再直接写 SessionRuntime struct literal。
- `SessionIngress` 已成为 Runtime 的单一输入分类入口；UserMessage 与控制事件先分类，再进入对应的 Run input 或 command 路径。
- `ChatRequest` 不再携带初始输入、独立 queue drain 和第二输入端口，只持有统一 `ingress`。
- Runtime/Composition 测试、格式检查、diff 检查与架构守卫均已通过。

P4 与 P5 的交接边界：`SessionIngress` 当前完成入口分类和 interaction 定向 seam；P5 继续把分类结果接入统一 Loop Engine 的 InputQueue、CommandScheduler 与 InteractionInbox，并移除剩余兼容路径。


**实施**：

1. 将 CLI/SDK args 先转换为 typed bootstrap request；参数解析不直接创建 Provider、Tool、Hook、Workspace、RuntimeContext 或具体 Client 依赖。
2. 将具体 adapter、factory、registry、runner、materializer 和跨 BC object graph 留在 `agent/composition`。
3. Runtime bootstrap 只负责创建 / 恢复 Session、初始化 `SessionState`、确定 snapshot 时机，并把 SDK 的唯一 ingress 分类为 `UserMessage`、`Command` 或 `InteractionCommand`；首次与后续 `UserMessage` 均通过同一个 `InputPort` 提交，不得形成含 initial input 的特殊启动入口。
4. 删除 `ChatRequest` 中 `prompt` / `input_rx` / `interaction_rx` 多输入源，替换为单一 `SessionIngress`；分类后分别投递到 Run InputQueue、CommandScheduler 与 InteractionInbox。
5. 拆解或删除 `application/client/from_args.rs` 中的参数解析、Session 恢复、模型绑定、Tool/Skill 查询、Prompt 构建、并发配置、runner 创建和 Client 构造职责。
6. 更新 `AgentClient` / SDK / CLI 的公开入站 façade，不让调用方看到 `RuntimeServices` 内部 wiring 或 bindings。
7. 补 Composition L2/L3 组装契约和 runtime bootstrap 场景测试，覆盖独立 Run 与派生 Run 的同一准备入口，以及三类 ingress 的无损分类。

**主要文件**：

- `agent/features/runtime/src/application/client/from_args.rs`
- `agent/features/runtime/src/application/client.rs`
- `agent/features/runtime/src/application/startup.rs`
- `agent/features/runtime/src/application/startup/*.rs`
- `agent/composition/src/runtime.rs`
- `agent/composition/src/app.rs`
- `agent/composition/tests/main_session_wiring.rs`
- `agent/features/runtime/tests/bootstrap_dependencies.rs`

**完成证据**：`from_args.rs` 收敛或删除、Composition construction ownership guard 通过、bootstrap / Session snapshot 场景通过。

### P5：把 Loop Engine 改为统一 `Run + RunExecutionState + RuntimeContext`

**目的**：让 Engine 直接拥有完整流程，删除 fat `RunLoopPort` 对流程的反向控制。

**实施**：

1. 设计并落地统一 Engine API：`run_loop(&mut Run, &mut RunExecutionState, &RuntimeContext)`，返回 typed terminal outcome。
2. 将 input drain/await、epoch 校验、Step 创建、command scheduling、context/compact、model invocation/retry、tool coordination、interaction continuation、Hook coordination、control/cancel、finalization 和 event drain 改为 Engine 内部流程。
3. 把 `RunLoopPort` 中真正的外部边界拆为窄 Port：InputPort、CommandScheduler、InteractionInbox、EventSink、UsageSink、ActiveRunRegistryPort 及对应 capability contracts；不得把 freeze/finalize/invoke/execute/claim/store 等流程方法留在统一 Port。
4. 让普通输入只进入 Run InputQueue；ImmediateControl command 立即生效，AtRunBoundary command 在安全边界执行，SessionQuery 不污染 Run；interaction reply/cancel 按 `run_id + request_id` 定向完成 pending interaction，绝不经过 InputQueue。
5. 将现有 `MainRunPort` 和 `SubAgentRun` 的差异迁移到 factory 绑定的 Input/Event/Provider/Tool/Interaction/Hook capability adapter；Engine 不出现 `is_sub`、Main/Sub enum 或角色分支。
6. 保持 `#1272` drain epoch、internal continuation、input adoption 和 termination seal 语义；保持 compact/reflection、retry/cancel、tool result adjacency、Stop Hook 三分支和 pending interaction 语义。
7. 先为 Engine 的单阶段协作补 L2 测试，再用 Runtime crate integration / scenario tests 验证完整 Run journey，包括 AwaitingInput 与 AwaitingInteraction 互不消费对方消息。

**主要文件**：

- `agent/features/runtime/src/application/loop_engine/engine.rs`
- `agent/features/runtime/src/application/loop_engine.rs`
- `agent/features/runtime/src/application/loop_engine/input.rs`
- `agent/features/runtime/src/application/loop_engine/shared.rs`
- `agent/features/runtime/src/application/main_loop/looping/*.rs`
- `agent/features/runtime/src/application/subagent/runner/*.rs`
- `agent/features/runtime/src/application/interaction_coordinator.rs`
- `agent/features/runtime/src/application/tool_coordination.rs`
- `agent/features/runtime/src/application/stop_hook_coordination.rs`
- 相邻 `*_tests.rs` 与 `agent/features/runtime/tests/*.rs`

**完成证据**：Engine 无角色分支、fat `RunLoopPort` 不再作为流程接口、统一 Loop L2/L3/L4 测试通过、shared Run Loop guard 通过。

### P6：迁移生产消费者、根除死代码并按所有权重组 Application

**目的**：在新入口稳定后清理历史角色化生产类型、旧 assembler、测试专用绕过和模块级 dead-code 豁免；最终目录必须表达 Run、Session 与能力所有权，而不是 Main/Sub 历史来源。

**执行规则**：以下项目按编号顺序推进。每个一级 checkbox 都是一个单一、可验证的交付项；只有其测试、源码搜索和相邻边界验证全部通过后，才把 `[ ]` 改为 `[x]`，并在该项下追加完成 commit、验证命令和结果。不得批量预勾选，也不得因后续项目通过而倒推前项完成。

**双轨收敛 Checklist**：

- [x] **P6.1 让 Engine 成为 `RunExecutionState` 唯一 owner**
  - 从 `MainRunPort`、`SubAgentRun` 删除 `execution` 字段和访问实现。
  - 删除 `execute_prepared_loop` 入口前后的 `std::mem::swap`，Engine 全程直接持有 `&mut RunExecutionState`。
  - 删除只为 adapter 暴露 execution 的 `ExecutionStatePort`；确认 `Run` 与 execution 无重复事实。
  - 完成门禁：execution ownership L1/L2 测试通过；搜索 adapter execution 字段、swap 和 `ExecutionStatePort` 均为空。
  - 完成记录：Main/Sub launcher 将 `RunExecutionState` 作为 per-run 输入直接传给 `launch_prepared`，角色化 adapter 不再保存 execution；新增源码契约覆盖 Main/Sub adapter 字段。
  - 验证结果：`cargo test -p runtime --lib`（711 passed）；P6.1 三项定向测试通过；`cargo check -p runtime` 通过；生产源码搜索未发现 `ExecutionStatePort`、execution accessor、swap 或 `MainRunPort` / `SubAgentRun` execution 字段。

- [x] **P6.2 将 Run 准备收口为纯值 request 单入口**
  - `RunPreparer::prepare` 只接受 `RunPreparationRequest`，按 `RunSpec + SessionSnapshot + parent ceiling` 绑定能力并返回 `PreparedRun`。
  - 删除 Main/Sub 调用方手填的 `RunCapabilityBindings`、`RunContextBindings`、`RuntimeContextParts` 及同构参数袋。
  - `RuntimeContext` 构造保持 factory 私有；生产代码不得出现第二条 assemble/prepare 路径。
  - 完成门禁：prepare/ceiling/snapshot 契约测试通过；旧 bindings、公开 context 构造和多参数 prepare 搜索为空。
  - 完成记录：`RunPreparer::prepare` 唯一业务输入为 `RunPreparationRequest`；Main resolver 从 `MainSessionWiring` 语义源绑定 committed Context/Memory/Config 并让 `RuntimeContext` 持有 shared session lease；Sub resolver 从 parent/workspace/provider/skill 语义源一次性派生 isolated workspace、provider、skills context 与 restricted catalog，并回传 resolved `SessionSnapshot`。Main/Sub 生产调用点均不再构造 `RunCapabilityBindings` 或同构 source/parts 参数袋。
  - 验证结果：`cargo test -p runtime --lib`（673 passed）；P6.2 pure-request、生产调用方、parent identity、session snapshot 定向契约通过；`cargo check -p runtime`、`cargo check -p composition`、`cargo fmt --all -- --check` 通过；Runtime Capability Assembly guard 与完整 fast architecture guards 通过；生产 Main/Sub 搜索未发现 `RunCapabilityBindings`、`RunContextBindings`、`RuntimeContextParts`、`PreparedCapabilityResolver`、`SubRunCapabilitySource`、公开 context 构造或直接 factory create。

- [x] **P6.3 合并模型调用为单一 orchestration**
  - 将 ContextWindow、InvocationRequest、provider stream、retry/compact、usage、assistant message、tool call 和 terminal 判定统一迁入 Engine-owned model coordinator。
  - Main 仅保留可见 delta/event 投影和运行中输入泵；Sub 仅通过 capability 数据关闭或替换这些能力，不再保留第二套 `invoke_model_impl`。
  - 完成门禁：Main/Sub model 相邻边界测试与 retry/cancel/usage 测试通过；生产 `invoke_model_impl` 只剩一个统一实现，Engine 无 Main/Sub 分支。
  - 完成记录：`application/model/invocation.rs::orchestrate_model_invocation` 统一拥有 ContextWindow 构建、LLM 视图与 InvocationRequest、provider stream/reducer、retry/compact/cancel、waiting companion 生命周期、usage、assistant message 写入、tool call 提取和 terminal 分类调度。Main/Sub 通过窄 `ModelInvocationProjection` 提供 request 日志上下文、delta/event sink、运行中输入泵、retry/usage/progress 投影、tool identity、reflection 与 max-output continuation；角色 adapter 不再直接调用 provider 或实例化 retry coordinator。
  - 验证结果：`cargo test -p runtime --lib`（672 passed）；P6.3 单编排契约、model retry/cancel/reducer、Main logging、Sub 并发 logging/provider scope 测试通过；`cargo check -p runtime`、`cargo check -p composition`、`cargo fmt --all -- --check` 通过；Runtime Capability Assembly guard 与完整 fast architecture guards 通过；生产搜索确认 `invoke_model_impl` 仅在 model coordinator 出现，Main/Sub 无 `ModelInvocationCoordinator::new()` 与 `provider.invoke(`。

- [x] **P6.4 合并工具执行为单一 tool-round pipeline**
  - 统一 catalog/profile、policy/hook、并发执行、approval/suspension、结果排序、materialization 和 continuation 标记的编排所有权。
  - Main/Sub 差异只由 Tool capability adapter、EventSink 和 RunSpec 表达；禁止一边调用共享 round、一边直接驱动 `Agent`。
  - 完成门禁：tool preparation、execution、result adjacency、approval 和 cancellation 的逐层测试通过；生产 `execute_tools_impl` 只剩一个统一实现。
  - 完成记录：`application/tool/coordination.rs::orchestrate_tool_round` 统一拥有 catalog snapshot 消费、Policy/fuse 准备、Hook-aware 执行、并发/顺序执行、AskUser suspension、approval、稳定结果排序、取消补齐、result materialization 和 interaction 分流。原万能 `ToolRoundProjection` 已根因级拆除：`ToolRoundContext` 只承载本轮执行依赖与纯值，窄 `ToolRoundObserver` 只接收日志、进度、任务快照及 post-batch 通知，`ToolRoundOutcome + ToolRoundContinuation` 显式表达下一步。Engine 读取 outcome 后通过 `InputPort::schedule_internal_continuation` 调度 ToolResults；统一工具管线和 Main/Sub tool adapter 均不再隐式修改 continuation。角色 adapter 也不再直接调用 `prepare_tool_round`、`execute_tool_round` 或 `Agent::execute_prepared_tools`。
  - 验证结果：`cargo test -p runtime --lib`（673 passed）；P6.4 单 pipeline 契约、tool coordination、Sub derived wiring、binding/policy/catalog、父取消传播测试通过；`cargo check -p runtime`、`cargo check -p composition`、`cargo fmt --all -- --check` 通过；Runtime Capability Assembly guard 与完整 fast architecture guards 通过；生产搜索确认 `execute_tools_impl` 仅在 tool coordinator 出现，Main/Sub 各只有统一入口调用。

- [x] **P6.5 合并 interaction mailbox 与 continuation 收尾**
  - 统一 reply/cancel 轮询、closed/resolved 分类、AskUserQuestion result、ToolApproval approve/deny、批准后执行和 pending queue 推进。
  - `InteractionCoordinator` 拥有领域转换，统一 continuation use case 拥有业务完成；Main/Sub 只投影事件。
  - 完成门禁：`run_id + request_id` 定向、reply/cancel、sibling 隔离、child teardown 和 tool approval 场景测试通过；重复 `poll_interaction` / `finish_interaction_work` 实现清零。
  - 完成记录：`application/interaction/coordinator.rs` 现统一拥有 execution mailbox 的 receiver 存取与 resolved/closed 分类，以及 AskUserQuestion、ToolApproval approve/deny、批准后 Tool 执行、结果物化和 pending queue outcome 计算。`RunExecutionState` 成为 mailbox 与 pending interaction work 的唯一运行态 owner，Main/Sub adapter 的重复 poll/finish 实现及测试替身双轨均已删除；Engine 只负责领域 reply/cancel 转换、应用 coordinator outcome、推进 tool call/queue/step 和事件投影。
  - 死代码清理：删除仅供旧 Sub 测试使用的 `runner/loop_helpers.rs` 和 Main 的 `tool_results_for_api` 包装，测试改为直接覆盖统一 `loop_engine::shared::materialize_tool_results`；删除测试专用的重复 mailbox 字段与未使用 execution accessor。
  - 验证结果：interaction coordinator 32 tests、interaction routing 9 tests、Main tool 5 tests、Sub runner 51 tests 全部通过；`cargo clippy -p runtime --all-targets -- -D warnings` 与 `cargo fmt --all -- --check` 通过；生产搜索确认 `poll_interaction` / `finish_interaction_work` / 旧 tool-result 包装无残留。

- [x] **P6.6 合并 Step 持久化事务**
  - Engine 统一计算 freeze、accepted input、finalized/cancelled messages 和 execution commit 游标。
  - Context/Persistence Port 只负责提交，不由 Main/Sub 分别决定 pending/finalized 范围。
  - 完成门禁：正常、取消、continuation 和 input adoption 的相邻测试通过；Main/Sub 不再实现两套 finalize/commit 流程。
  - 完成记录：统一 Engine 的 `freeze_step` 现拥有 LoopInput 文本/图片物化、stop-hook prefix 合并、accepted user input、adopted InputId 和 ContextRequest 安装；Main/Sub 只通过 `take_step_input_prefix` 与 `build_context_request` 提供窄差异。`prepare_step_commit` 统一生成 typed `StepCommit`，正常和取消路径都通过同一 `finalize_step` 提交并在成功后清理 execution step working set；Persistence adapter 只执行 `append_finalized`，不再计算消息切片或 commit cursor。
  - 删除记录：移除 Main/Sub 的 `freeze_step`、`finalize_step`、`finalize_cancelled_step` 双轨实现；删除 Sub 的 `committed_message_count + accepted_input_len` 索引推断，以及 `RunExecutionState` 中已无消费者的 committed-message cursor、slice/commit API。
  - 验证结果：Loop Engine 59 tests、Main loop runner 44 tests、Sub runner 51 tests 全部通过；`cargo clippy -p runtime --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` 通过；生产搜索确认旧 freeze/finalize/cursor 计算无残留。

- [x] **P6.7 合并 Stop Hook 调度**
  - Main/Sub 全部通过 typed stop coordinator 执行 Proceed/Continue/Block 决策；Main 的 TUI 行为降为 outcome/event projection。
  - 删除 Main 直接 `dispatch_hook` 的旁路，BoundaryOnly 过滤在 Hook adapter 入口强制执行。
  - 完成门禁：三分支、boundary filter、UI/progress projection 测试逐层通过；生产 Stop Hook dispatch 只有一条路径。
  - 完成记录：`application/hook/stop_coordination.rs::orchestrate_stop_hook` 统一拥有 Stop invocation、Hook dispatch、typed directive 投影、Block feedback materialization 与标准 `Message::stop_hook_feedback` 构造；Engine 统一写入 execution 消息并推进 Stop block 计数。Main adapter 仅提供 Hook/context capability、Running/结果 UI 投影及 continuation relay，Sub adapter 仅提供 Hook/context capability，不再各自执行或解释 Stop Hook。
  - BoundaryOnly：新增 `BoundaryHookPort`，在 Hook adapter 入口只转发 Session/SubRun start-stop 生命周期 invocation，内部 Stop/Tool/Compact 等 invocation 统一返回 Proceed；Factory 不再把 BoundaryOnly 错配为 EmptyHookPort。Runtime Capability Assembly guard 与守卫文档已同步目标契约。
  - 验证结果：Stop coordinator 6 tests、Loop Engine 59 tests、Main loop runner 44 tests、Sub runner 51 tests、RuntimeContextFactory 33 tests 全部通过；`cargo clippy -p runtime --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` 通过；完整 architecture guards 通过；生产搜索确认 `HookInvocation::Stop` 仅存在于统一 coordinator，Main/Sub 无 `evaluate_stop_hook` 或 Stop direct dispatch。

- [x] **P6.8 删除 Sub 的直接 LLM completion 旁路**
  - 核实 `AgentDispatch::complete` 的生产消费者；无消费者则删除 trait 方法、`CliAgentRunner::complete` 和专属测试/替身。
  - 若存在真实消费者，必须迁入统一 `RunPreparationRequest → PreparedRun → RunLauncher` 链路，禁止保留直接 provider stream。
  - 完成门禁：全仓调用点审计有记录；生产代码不存在绕过 Run Engine 的 provider invocation。
  - 调用点审计：全仓没有 `AgentDispatch::complete`、`AgentRunner::complete`、`runner.complete(...)` 或 agent dispatch completion 消费者；Tools 的正式 Agent tool 只调用 `run_agent(AgentRunRequest)`，该入口继续经 `derive_sub_run → RunPreparer → PreparedRun → RunLauncher::launch_prepared → shared Loop Engine`。
  - 删除记录：从 Tools Published Language 删除 `AgentDispatch::complete`；删除 `CliAgentRunner::complete` 中自行读取 Config、构建 Provider、拼 InvocationRequest、消费 provider stream 的完整旁路；相应移除 `CliAgentRunner.config_reader`、`build_agent_runner` 的冗余 ConfigReader 参数、全部测试替身方法，以及已无消费者的 `test_config_reader.rs`。
  - 契约与验证：新增 source contract 禁止 AgentDispatch completion、Sub direct provider invocation 和 completion-only config state。Tools Agent tests 10、Loop Engine tests 60、Sub runner tests 51 全部通过；Runtime/Tools production 与 all-targets clippy、格式、diff、Shared Run Loop guard 和完整 architecture guards 全部通过；生产搜索确认 Runtime Application 中 provider invocation 仅存在统一 model coordinator，Sub runner 无 InvocationRequest/provider stream 构造。

- [x] **P6.9 退役角色化执行外壳与 fat capability 聚合**
  - 在 P6.1-P6.8 完成后删除 `MainRunPort`、`SubAgentRun`、`SubAgentLaunch` / `DerivedSubRun`、Main/Sub strategy 和仅为这些类型服务的 wrappers/exports。
  - 删除把全部窄 Port 重新聚合成流程对象的 fat `LoopEnginePort`；Engine 直接依赖明确的 per-stage context/capabilities。
  - 保留的 Main/Sub 差异只能是窄 Input/Event/Control/Workspace/capability adapter，不能重新形成角色化大对象。
  - 完成门禁：旧符号全仓搜索为空；Shared Run Loop 场景证明独立 Run 与派生 Run 走同一 Engine。
  - fat capability 退役：删除 `LoopEnginePort` marker/supertrait 聚合及其 Runtime export、Main/Sub/Test 空实现；`execute_prepared_loop`、`run_loop`、阶段辅助函数与 `RunLauncher::launch_prepared` 直接声明所消费的 Input/Event/Control/Lifecycle/Interaction/Persistence/Compaction/Model/Hook/Tool/Stuck/Plan 窄能力，不再通过单一 fat trait 隐藏依赖。
  - 角色外壳退役：`MainRunPort` 更名为无角色执行语义的 `MainRunCapabilities`，`SubAgentRun` 更名为 `SubRunCapabilities`；`SubAgentLaunch` wrapper 物理删除并改为函数式 `launch_sub_run`；`DerivedSubRun` 更名为准备结果 `PreparedSubRun`。输入差异类型由 Main/Sub strategy 更名为来源语义明确的 `BufferedInputAdapter` / `FixedInputAdapter`。
  - 保留边界：Main/Sub 仅保留外部输入、事件投影、workspace/progress 和 per-capability adapter 差异；Run 创建、active registry 生命周期、Engine 启动、状态迁移、model/tool/interaction/persistence/stop orchestration 均继续由统一链路拥有。
  - 验证结果：Loop Engine 52 tests、Sub runner 51 tests、Main loop runner 44 tests 全部通过；`cargo check -p runtime`、`cargo clippy -p runtime --all-targets -- -D warnings`、格式与 diff 检查通过；Shared Run Loop guard 和完整 architecture guards 通过；生产旧符号与 Main/Sub input strategy 搜索为空，生产 `run_loop` 调用只剩 Engine 内部统一入口。

- [x] **P6.10 根除测试托活和 dead-code 豁免**
  - 记录完整测试清单后临时隔离 Runtime 测试模块与 integration tests，以 production-only `cargo check` / `cargo clippy` 暴露死代码；诊断结束必须完整恢复测试。
  - 删除模块级 `#![allow(dead_code)]`、无生产消费者的 service/scheduler/legacy API；测试辅助能力使用 `#[cfg(test)]`，禁止虚假 re-export 或测试引用制造生产可达性。
  - 完成门禁：隔离前后测试清单一致；production target 无 dead-code 告警；最终工作树不存在缺失或禁用的有效测试。
  - 隔离审计：基线为 Runtime lib 667 tests、含 6 个 integration test 文件共 685 tests、60 个外置 `*_tests.rs` 文件；临时将源码测试接线替换为恒 false cfg 并移出 integration tests 后，以 `RUSTFLAGS='-D dead-code -D unused-imports' cargo test -p runtime --lib --no-run` 验证 production-only 可达图，命令通过且源码摘要恢复一致。
  - 清理记录：删除仅测试读取的 `RunPreparer::context_factory`；删除无消费者的 `RuntimeContext::{context_ref, tool_context_binding_ref, cancel_ref}`；删除无消费者的 `Agent::execute_tools_filtered`；将仅测试使用的 Agent 批量执行、并发判定和 prepared 执行 helper 收紧为 `#[cfg(test)]`；删除测试 harness 中 3 个未调用 helper 和 `CompactHarness` 的 12 个仅为延长局部构造值生命周期而遗留、实际已由 `RuntimeContext` 持有的重复字段。
  - dead-code 豁免：Runtime `application/**` 的 `#![allow(dead_code)]` / `#[allow(dead_code)]` 搜索为空；未新增虚假 re-export 或生产测试 API。
  - 恢复与验证：隔离前后 lib 测试清单均为 667、完整清单均为 685，逐行 diff 为 0；6 个 integration test 文件全部恢复。`RUSTFLAGS='-D dead-code -D unused-imports' cargo check -p runtime --lib`、同配置 production clippy、`cargo clippy -p runtime --all-targets -- -D warnings`、`cargo test -p runtime`（667 unit + 18 integration）、格式、diff、Shared Run Loop guard 与完整 architecture guards 全部通过。

- [x] **P6.11 按所有权完成 Application 目录归档**
  - `application/run/`：active registry、config snapshot、execution state、launcher、preparer、preparation。
  - `application/session/`：ingress、session state；`interaction/`、`hook/`、`context/`、`model/`、`tool/`、`workspace/` 分别承载对应能力。
  - 保留真实稳定的 `client/`、`loop_engine/`、`prompt/`、`reflection/`、`startup/`、`cost/`；删除顶层平铺的兼容 re-export 和 Main/Sub 同义目录，不新增万能 `common/shared`。
  - 完成门禁：`application.rs` 与 crate exports 只暴露稳定边界；目录所有权审计无无主文件、重复实现或兼容转发层。
  - 根因级归档：物理删除 `application/main_loop{.rs,/**}` 与 `application/subagent{.rs,/**}`。共享 chat 执行及输入、事件、stream、tool batch、hook、反思触发等编排归入 `application/loop_engine/chat/**`；派生 Run 的准备、执行、进度和收尾归入 `application/run/derived/**`；Agent tool runtime 归入 `application/tool/agent/**`；chat launch 输入归入 `application/run/chat_launch.rs`。删除无消费者的 reflection 兼容转发和 Runtime input-validation 兼容 re-export。
  - 稳定边界：`application.rs` 仅声明 `client/context/cost/hook/interaction/loop_engine/model/prompt/reflection/run/session/startup/tool/workspace`，测试 harness 单独受 `cfg(test)` 约束；新增 crate 内目录白名单契约，禁止角色化顶层目录和平铺无主模块回归。
  - 路径同步：Runtime 源码、integration tests、架构 Guard、Guard sanity fixtures 与设计文档均切换到真实 owner 路径；旧 `application::main_loop`、`application::subagent` 生产引用及 Guard 旧路径搜索为空。
  - 验证结果：production-only `RUSTFLAGS='-D dead-code -D unused-imports' cargo check -p runtime --lib`、`cargo clippy -p runtime --all-targets -- -D warnings`、Runtime 668 unit + 18 integration tests、格式、diff、目录契约、Guard sanity fixtures 与完整 architecture guards 全部通过。

- [x] **P6.12 固化防双轨 Guard 并执行最终验证**
  - Guard 收敛：Shared Run Loop、Runtime Capability Assembly、crate façade 与禁止 import 规则均扫描当前 owner 路径；Runtime Capability Assembly 新增生产标识命名门禁，禁止 `Projection` / `projection` 宽泛命名，且无路径白名单。
  - 命名根治：`ModelInvocationProjection` 按职责拆为 `ModelInvocationContext` 与 `ModelInvocationLifecycle`；Session durable/resume 类型改为 `AcceptedInputRecord`、`FinalizedStepRecord`、`SessionResumeView`；SDK/Hook 转换模块改为 `sdk_event_mapper` 与 `outcome_mapper`；context state、waiting event、Stop Hook status 方法均改为职责名。
  - 规则与设计：根 `AGENTS.md` 和 `specs/rust-coding.md` 固化全仓命名原则；`specs/runtime.md` 只登记 MUST/SHOULD/NEVER 决策；职责拆分、mapper/view/record 词汇和 Guard 设计写入 Runtime design 与 architecture guard 文档。
  - 验证结果：`cargo check -p runtime -p context -p cli`、`cargo clippy -p runtime -p context -p cli --all-targets -- -D warnings`、Context 全量测试（106 unit + 全部 integration）、Runtime 669 unit tests、`cargo fmt --check`、`git diff --check`、Runtime 定向 Guard 与全部 fast architecture guards 通过；源码与设计旧 Projection 类型/模块名搜索清零。

**依赖顺序**：P6.1 → P6.2 → P6.3 → P6.4 → P6.5 → P6.6 → P6.7 → P6.8 → P6.9 → P6.10 → P6.11 → P6.12。若某项验证失败，保持未勾选并在该项下记录失败证据；不得跳到依赖它的删除或 Guard 项。

**主要文件**：

- `agent/features/runtime/src/application.rs`
- `agent/features/runtime/src/application/**`
- `agent/features/runtime/src/ports/legacy.rs`
- `agent/features/runtime/src/lib.rs`
- `.agents/hooks/check-shared-run-loop.sh`
- 受影响的 Runtime / Composition / SDK / CLI 测试

**完成证据**：

- 临时测试隔离前后清单一致，最终工作树不存在被移除或禁用的有效测试；
- `application/**` 不存在模块级 `#![allow(dead_code)]`，也不存在用于掩盖整类死代码的宽泛 item 级豁免；
- 旧符号全仓搜索为空或只存在明确历史文档；
- `service`、`scheduler` 和其他确认无生产消费者的实现已物理删除；
- `application/` 顶层只保留真实稳定边界，Run/Session/能力模块按所有权归档；
- 无测试专属生产 API；不带测试的生产 `cargo check` / `cargo clippy`、all-targets 验证、source guard 和 dead-code 检查全部通过。

### P7：跨层验证、Guard、文档与 Issue 回写

**目的**：证明终态不只是源码可编译，而是每层边界、能力安全和用户旅程均已闭合。

**实施**：

1. Runtime domain：Run 状态机、RunExecutionState 唯一所有权、非法迁移和 terminal 收口。
2. Runtime application：Factory 准备、snapshot 隔离、Loop 各阶段、retry/cancel/compact/interaction/control/Hook。
3. Runtime ports/adapters：Interaction、Hook、Input/Event、Provider/Tool capability contracts 及资源 teardown。
4. Composition：具体 object graph、factory 注入、跨 BC construction ownership、Main/Sub 仅由 RunSpec 数据表达。
5. SDK/TUI：Runtime 纯值事件、Interaction reply/cancel、Session/Run 生命周期投影和字段完整性；如本阶段不改对应消费层，至少运行已有跨层契约并记录不适用理由。
6. 完整运行：`scripts/setup-dev-env.sh --check`、架构 Guard、`cargo fmt --check`、相关 crate `cargo test`、`cargo clippy --all-targets --all-features -- -D warnings`、workspace pre-push 门禁。
7. 更新 `docs/design/`、Runtime Migration Governance、Issue #1397 checklist、PR Test plan 和相关架构 Guard 白名单；只勾选有验证证据的项目。
8. 发现旧路径、死代码或过期兼容层时，在同一实施范围内清理；无法清理时必须在 Issue/PR 记录原因、影响和后续边界。

**主要文件**：

- `docs/design/01-system/03-context-map.md`
- `docs/design/02-modules/runtime/README.md`
- `docs/design/02-modules/runtime/01-domain-model.md`
- `docs/design/02-modules/runtime/02-module-boundaries.md`
- `docs/design/02-modules/runtime/03-loop-and-state-machine.md`
- `docs/design/02-modules/runtime/06-ports-and-adapters.md`
- `docs/design/02-modules/runtime/07-runtime-ownership-and-assembly.md`
- Runtime migration governance / architecture guard registry
- GitHub Issue #1397 与实施 PR

**完成证据**：Issue 全部完成定义逐项有链接或命令证据；所有 Guard、测试、构建和 clippy 通过；PR review 可独立复核。

## 5. 测试策略矩阵

| 领域 | L1/L2 | L3 | L4 | L0 / 守卫 |
|---|---|---|---|---|
| Run / RunExecutionState | 状态迁移、唯一 owner、pending interaction | Run preparation 字段与 terminal 契约 | 完整 Run 生命周期 | source/API/无重复状态守卫 |
| RuntimeServices / SessionState | 字段归属、snapshot 不变 | RuntimeContextFactory 输入输出契约 | Session 创建→Run→后续 Session 变更隔离 | construction ownership |
| Capability factory | mode/ceiling/unavailable 组合 | Interaction/Hook/Provider/Tool/Workspace adapter 契约 | parent-child dispatch / interaction journey | capability assembly、hexagonal dependency |
| Loop Engine | 每阶段编排、epoch、command scheduling、input/interaction 等待态隔离、finalization | 统一 Engine 与外部 Port 契约 | user input / command / interaction → model/tool → terminal journey | shared loop、Main/Sub branch absence |
| Composition/bootstrap | factory 注入、生命周期、SessionIngress 分类 | SDK/CLI bootstrap boundary、单一输入源 | 启动→分类→目标 mailbox→Run→终态 | cross-BC construction ownership |
| TUI/SDK projection | 现有纯值映射单元 | Interaction / event field completeness | 用户交互闭环（受影响时） | crate/API boundary |

测试必须遵循：生产逻辑前先建立失败证据；跨层改动每层都有相邻测试；不得以 L4 替代 L1-L3；不得扩大测试专用生产 API；测试文件按 owning layer 分离，禁止新增 `mod.rs`、万能 `test_utils` 或 inline test 违背仓库守卫。

## 6. PR / Issue 门禁

创建或更新实施 PR 前必须：

- [ ] PR 分支基于最新 `origin/main`，且已执行 `git pull origin main`。
- [ ] #1397 milestone、parent / blocked-by 关系未被擅自改动；#1248 已完成的生产能力被统一模型消费。
- [ ] Issue body 的全部 checklist 已逐项核对；未完成项不得静默勾选。
- [ ] `RuntimeServices`、`SessionState`、`RunExecutionState`、`RuntimeContextFactory` 唯一所有权有源码和测试证据。
- [ ] `RuntimeContextParts`、`RunContextBindings`、fat `RunLoopPort`、Main/Sub 角色化生产类型均按完成定义退役，或明确记录未完成原因和后续边界。
- [ ] Main/Sub 同一 Loop 生产可达性、capability 不扩权、ParentMediated 隔离、BoundaryOnly 过滤有测试证据。
- [ ] `from_args.rs` 已删除或收敛为薄 bootstrap，并有 Composition boundary 测试。
- [ ] 文档、Migration Governance、Guard registry 和 PR Test plan 同步。
- [ ] 全量验证通过，失败项有真实报告，不使用 `--no-verify` 隐藏失败。

## 7. 风险与止损策略

| 风险 | 处理 |
|---|---|
| 迁移时同时改变 Loop 行为和所有权 | 先以旧行为测试锁定状态机与跨层契约，再单次迁移一条 owner 边界；每阶段运行相邻测试 |
| `RunExecutionState` 与 `Run` 继续双写 | 以字段归属表和 source guard 阻止重复字段；发现双 owner 立即停在 P1，不继续搬消费者 |
| Factory 退化为被动 bindings 复制器 | `prepare` 只接受纯值 request；删掉 `RunContextBindings` 后由 mode/ceiling 测试证明选择发生在 Runtime |
| Parent interaction 泄露或 sibling 互相取消 | child-scoped identity + route mapping + teardown 测试；失败时禁止进入统一 Loop 退役阶段 |
| BoundaryOnly 只做存在性校验 | adapter 入口做真实 invocation filter，并以 forbidden invocation 契约测试锁定 |
| `from_args.rs` 搬家后形成第二 Composition Root | 以 construction ownership guard 和 bootstrap contract 测试检查 concrete constructor 位置 |
| 一次性删除旧 adapter 导致行为不可追踪 | 先迁移单一生产入口和测试，再删除类型；每次删除后运行旧符号搜索与 crate 测试 |
| 计划过大无法在一个 PR 完成 | 保持一个 Issue，但若实际需要多个独立 PR，先向用户报告候选边界并等待明确拆分决定；不得自行创建 sub-issue |

## 8. 交付顺序

1. P0 基线与契约冻结
2. P1 `RunExecutionState` 与领域状态所有权
3. P2 `RuntimeServices` / `SessionState` / preparation PL
4. P3 Factory 与 capability adapter
5. P4 Composition/bootstrap
6. P5 Loop Engine
7. P6 消费方迁移和旧路径物理退役
8. P7 Guard、全量验证、文档和 Issue 回写

除非某阶段验证失败需要回到根因调查，否则不得跳过前置阶段。每阶段完成后应记录：变更文件、测试证据、剩余旧符号、Guard 状态和下一阶段依赖。
