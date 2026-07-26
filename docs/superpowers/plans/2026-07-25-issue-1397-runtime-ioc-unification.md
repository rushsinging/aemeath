# #1397 Runtime：以 RunExecutionState + RuntimeContext 统一 Main/Sub Loop adapter 实施计划

> 对应 Issue：[#1397](https://github.com/rushsinging/aemeath/issues/1397)
> 设计基线：[07-runtime-ownership-and-assembly.md](../../design/02-modules/runtime/07-runtime-ownership-and-assembly.md)
> 前置：[#1382](https://github.com/rushsinging/aemeath/issues/1382)、[#1385](https://github.com/rushsinging/aemeath/issues/1385)、[#1248](https://github.com/rushsinging/aemeath/issues/1248)
> 计划状态：待用户确认后实施

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
    runtime.factory.prepare(RunPreparationRequest {
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

// Factory：校验 ceiling，按 mode 绑定窄 adapter，原子返回 Idle per-Run 对象。
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
| `factory.prepare` | P3 | 单一入口、mode adapter、typed unavailable、无参数袋、产出 Idle Run |
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

1. 将 `RuntimeContextFactory` 的公共生产入口收敛为 `prepare(RunPreparationRequest) -> Result<PreparedRun, RunPreparationError>`；把 `RuntimeContext::new` 保持为 factory 私有可达入口。
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

**目的**：消除 Runtime 内第二 Composition Root，让 `from_args.rs` 只保留入站 bootstrap 用例。

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

### P6：迁移生产消费者并物理退役旧路径

**目的**：在新入口稳定后清理历史角色化生产类型、旧 assembler 和测试专用绕过。

**实施**：

1. Main session 启动和 Sub Agent dispatch 都通过统一 `RunPreparationRequest / PreparedRun / RunLauncher` 创建 Idle Run；其任务输入都封装为同一种 `UserMessage`，经 `SessionIngress` 分类后提交到目标 Run 的 `InputPort`。
2. 迁移 SDK/TUI 到单一 `SessionIngress`，删除 `ChatRequest.prompt`、`input_rx`、`interaction_rx` 及 interaction-as-input 兼容桥；不得保留隐式第二输入源。
3. 删除 `MainSessionShell`、`MainRunPort`、`SubAgentRun`、`DerivedSubRun`、Main/Sub strategy 和对应 module exports；不能留下只被测试引用的死代码。
4. 删除 `RuntimeContextParts`、`RunContextBindings`、旧 `assemble_main_runtime_context` / `derive_sub_run` 和第二条 Context 装配路径。
5. 清理 `ports/legacy.rs`、兼容 re-export、旧测试 fixture 和注释中把迁移形状描述为长期模型的内容。
6. 审计 `main_loop` / `subagent` 目录命名；只有仍表达真实用例边界的模块保留，不能用目录历史名称制造生产类型差异。
7. 更新 public API、crate root exports、测试夹具和 source guards，确保旧符号既无生产引用也无隐性测试可达路径。

**主要文件**：

- `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- `agent/features/runtime/src/application/subagent/runner/loop_run.rs`
- `agent/features/runtime/src/application/subagent/runner/setup.rs`
- `agent/features/runtime/src/application/run_launcher.rs`
- `agent/features/runtime/src/ports/legacy.rs`
- `agent/features/runtime/src/application/runtime_context.rs`
- `agent/features/runtime/src/application/runtime_context_factory.rs`
- `agent/features/runtime/src/application.rs`
- `agent/features/runtime/src/lib.rs`
- 所有受影响的 Runtime / Composition / SDK / CLI 测试

**完成证据**：旧符号全仓搜索为空或只存在明确历史文档；无测试专属生产 API；`cargo build`、source guard、dead-code 检查通过。

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
