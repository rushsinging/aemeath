# Runtime IoC 与统一入站协议实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 以 `RunExecutionState + RuntimeContext` 统一 Runtime，并将首条/后续用户输入、legacy queue 与 Interaction mailbox 收口为 Session 唯一输入入口和单 Run 单 Interaction。

**Architecture:** Session 是外部 `ChatInputEvent` mailbox 的唯一 owner，负责按生命周期将 typed 输入路由给 active Run 的 `RunInputBuffer` 或保留到下一 Run；Run 不直接拥有外部输入 source。InteractionBridge 可跨 Run 保存 waiter，但每个 `RunExecutionState` 只允许一个 active Interaction receiver，后续交互工作留在明确的工作队列中。

**Tech Stack:** Rust、Tokio mpsc/oneshot、SDK Published Language、Runtime 六边形端口、CLI/TUI TEA、Cargo L0-L4 验证。

---

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
2. **单一装配入口**：所有 Run 来源只提交 `RunCreationRequest`，统一取得 `RunInstance`；调用方不得手填 `RuntimeContext` 同构参数包。
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
async fn create_run(
    runtime: &mut AgentRuntime,
    parent: Option<ParentRunCapabilities>,
) -> Result<RunInstance, RunCreationError> {
    let spec = RunSpec::from_parent(parent.as_ref())?;
    runtime.run_factory.create(RunCreationRequest {
        spec,
        session: runtime.session.snapshot_for_run()?,
        parent,
    })
}

// 输入独立于装配；首次和后续输入都通过同一个 Port 激活 Run。
async fn submit_input(
    prepared: &RunInstance,
    input: InputEnvelope,
) -> Result<(), InputError> {
    prepared.context.input().submit(input).await
}

// RunFactory：校验 ceiling，委托 Context factory 绑定窄能力，原子返回 Idle per-Run 对象。
async fn create(&self, request: RunCreationRequest) -> Result<RunInstance, RunCreationError> {
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
    Ok(RunInstance { run, context, execution })
}

// Loop Engine：只消费已绑定能力，不重新装配或按 Main/Sub 分支。
async fn execute(instance: RunInstance) -> Result<AgentRunTerminal, RunExecutionError> {
    run_launcher::launch(instance).await
}
```

实现任务与伪代码的对应关系：

| 伪代码阶段 | 计划阶段 | 必须验证 |
|---|---|---|
| `compose_runtime` | P3/P4 | concrete constructor 只在 Composition，factory contracts 可注入 |
| `snapshot_for_run` | P2/P4 | Session 变化不污染已准备 Run |
| `RunSpec::from_parent` | P2 | 能力只收缩不扩权，无 Main/Sub 控制分支 |
| `RunFactory::create` | P3 | 单一入口、mode adapter、typed unavailable、无参数袋、产出 Idle Run |
| `InputPort::submit` | P1/P5 | 首次与后续输入同路径，Idle 状态只由输入激活 |
| `ParentMediated` / `BoundaryOnly` | P3 | child identity 隔离、Hook invocation 入口过滤 |
| `RunInstance` | P1/P3 | Run、Context、Execution 一致创建且无双 owner |
| `loop_engine::run_loop` | P5/P6 | Engine 直接编排，fat `RunLoopPort` 不再拥有流程 |

## 2. 当前实现基线与主要缺口

当前分支已经删除 `RunKind` 和双 Context Resolver，并统一 Run 创建、`RunLauncher::launch`、Loop Engine 以及 Model/Tool/Interaction/Step/Stop 的核心 application orchestration，但尚未达到 #1397 的结构终态：

- 生产代码已删除 `LoopCapabilityAdapter`、`ChatLoopCapabilityAdapter`、`DerivedLoopCapabilityAdapter`；`RunLoop` 现在只借用独立的窄 Port，生产 Engine/Launcher 不再要求单个来源对象实现整组能力。
- Runtime 测试仍保留 `application/loop_engine/tests.rs` 中的 `ScriptedPort`、`DrainOnlyPort` 等完整或跨类别 Port 替身；这会继续掩盖窄 Port 独立组合是否成立。
- `SessionSnapshot` 仍通过 `SessionCapabilitySnapshot` 混入 `MainSessionWiring`、Provider、Interaction、Reasoning lock、Event sink 与 `RuntimeWorkspaceAccess` 等 live handles；Factory 动态 mutation 已删除，但 Session facts 与 live bindings 尚未物理分离。
- `RuntimeContextFactory::prepare` 已删除 `prepare_independent` / `prepare_derived` 并保持一次最终 Context 构造，但 `select_session_bindings` 与 `select_inherited_bindings` 仍分别完成整套来源装配；`RuntimeContextBindingDecision` 混合 capability bindings、resolved session、workspace、parent 与 lease，窄 selector 终态尚未完成。
- `ParentRunCapabilities` 同时携带 parent identity/spec 与 Runtime Context/workspace live handles，尚未拆成纯值 parent facts 和 live parent binding view。
- `RunContextBindings` 仅存在于 `#[cfg(test)]`，不属于生产残留；但测试 builder 仍需按职责收窄，不能继续作为平铺能力包扩展。
- 依赖模块 façade 白名单、按能力类别识别 fat implementation 的结构 Guard、retired-symbol hygiene 与 sanity fixtures 尚未按终态完成。
- 当前准确状态是：共享 Run/Engine 骨架、九个无角色 application owner、生产 fat adapter 退役、统一启动 façade 已完成；Session/Parent 纯值边界、Factory 窄 selector、测试替身拆分和终态 Guard 尚未完成。

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
5. 定义 `RunCreationRequest`、`ParentRunCapabilities`、`RunInstance` 和 typed preparation errors。准备请求不得包含输入内容，也不得暴露锁、完整父 Context、Composition wiring 或 concrete adapter；Factory 必须创建 `Idle Run + empty RunExecutionState`，首次和后续输入全部经 `InputPort` 激活。
6. 为 snapshot 隔离、parent ceiling 收缩、unavailable capability、request 字段完整性，以及 `prepare → Idle → InputPort 激活` 补 L1/L2/L3 契约测试。

**主要文件**：

- `agent/features/runtime/src/application/runtime_context.rs`
- `agent/features/runtime/src/application/runtime_context_factory.rs`
- `agent/features/runtime/src/domain/agent_run/spec.rs`
- `agent/features/runtime/src/application/client/accessors.rs`
- 新增或调整 `agent/features/runtime/src/application/session_state.rs`、`session_snapshot.rs`
- `agent/features/runtime/src/application/runtime_creation.rs` 及测试（如现有模块不适合承载）
- `agent/features/runtime/src/lib.rs`

**完成证据**：字段分类表落实到代码、snapshot/ceiling 契约测试、无 `*Parts` 替代参数袋进入新 API。

### P3：收口唯一 `RuntimeContextFactory` 与 capability adapter

**目的**：让 factory 真正负责按 `RunSpec` 选择和收缩能力，而不是被动复制调用方提供的 bindings。

**实施**：

1. 将公共生产创建入口收敛为 `RunFactory::create(RunCreationRequest) -> Result<RunInstance, RunCreationError>`；`RuntimeContextFactory` 只产出 `RuntimeContext`，并把 `RuntimeContext::new` 保持为 factory 私有可达入口。
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
- `agent/features/runtime/src/application/client/bootstrap.rs`
- `agent/features/runtime/src/application/client/bootstrap/*.rs`
- `agent/features/runtime/src/application/prompt/instructions_hook.rs`
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
  - 完成记录：Main/Sub launcher 当时将 `RunExecutionState` 作为 per-run 输入直接传给旧 `launch_prepared`，角色化 adapter 不再保存 execution；P6.9.9 已进一步改为由完整 `RunInstance` 保持聚合并交给 `RunLauncher::launch`。
  - 验证结果：`cargo test -p runtime --lib`（711 passed）；P6.1 三项定向测试通过；`cargo check -p runtime` 通过；生产源码搜索未发现 `ExecutionStatePort`、execution accessor、swap 或 `MainRunPort` / `SubAgentRun` execution 字段。

- [x] **P6.2 将 Run 准备收口为纯值 request 单入口**
  - `RunFactory::create` 只接受 `RunCreationRequest`，按 `RunSpec + SessionSnapshot + parent ceiling` 绑定能力并返回 `RunInstance`。
  - 删除 Main/Sub 调用方手填的 `RunCapabilityBindings`、`RunContextBindings`、`RuntimeContextParts` 及同构参数袋。
  - `RuntimeContext` 构造保持 factory 私有；生产代码不得出现第二条 assemble/prepare 路径。
  - 完成门禁：prepare/ceiling/snapshot 契约测试通过；旧 bindings、公开 context 构造和多参数 prepare 搜索为空。
  - 完成记录：`RunFactory::create` 唯一业务输入为 `RunCreationRequest`；Main resolver 从 `MainSessionWiring` 语义源绑定 committed Context/Memory/Config 并让 `RuntimeContext` 持有 shared session lease；Sub resolver 从 parent/workspace/provider/skill 语义源一次性派生 isolated workspace、provider、skills context 与 restricted catalog，并回传 resolved `SessionSnapshot`。Main/Sub 生产调用点均不再构造 `RunCapabilityBindings` 或同构 source/parts 参数袋。
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
  - 若存在真实消费者，必须迁入统一 `RunCreationRequest → RunInstance → RunLauncher` 链路，禁止保留直接 provider stream。
  - 完成门禁：全仓调用点审计有记录；生产代码不存在绕过 Run Engine 的 provider invocation。
  - 调用点审计：全仓没有 `AgentDispatch::complete`、`AgentRunner::complete`、`runner.complete(...)` 或 agent dispatch completion 消费者；Tools 的正式 Agent tool 只调用 `run_agent(AgentRunRequest)`，该入口继续经 `derive_sub_run → RunFactory::create → RunInstance → RunLauncher::launch → shared Loop Engine`。
  - 删除记录：从 Tools Published Language 删除 `AgentDispatch::complete`；删除 `CliAgentRunner::complete` 中自行读取 Config、构建 Provider、拼 InvocationRequest、消费 provider stream 的完整旁路；相应移除 `CliAgentRunner.config_reader`、`build_agent_runner` 的冗余 ConfigReader 参数、全部测试替身方法，以及已无消费者的 `test_config_reader.rs`。
  - 契约与验证：新增 source contract 禁止 AgentDispatch completion、Sub direct provider invocation 和 completion-only config state。Tools Agent tests 10、Loop Engine tests 60、Sub runner tests 51 全部通过；Runtime/Tools production 与 all-targets clippy、格式、diff、Shared Run Loop guard 和完整 architecture guards 全部通过；生产搜索确认 Runtime Application 中 provider invocation 仅存在统一 model coordinator，Sub runner 无 InvocationRequest/provider stream 构造。

- [x] **P6.9 退役 `RunKind`、双 Context Resolver、fat Loop adapter 与双装配结构**
  - [x] **P6.9.1 用能力与 parent ceiling 取代 `RunKind` 生产决策**
    - 先修改 Domain 测试：证明独立 Run 与派生 Run 的能力合法性只由有效 capability 值、是否存在 parent ceiling 及 ceiling 比较决定，不再读取角色枚举。
    - 删除 `RunKind`、公开 `RunSpec.kind` 及 `RunSpec::main/sub` 中的 kind 写入；将 standalone restricted spec 的约束表达为显式 capability policy/ceiling 状态，不得用 `name == "main"`、timeout 或其他旁路重新推断角色。
    - `derive_sub` 仍可作为“从父能力派生”的领域动作，但产物不携带 Sub 类型；`with_*` builder 只依赖当前 policy 与 ceiling。
    - 完成门禁：Domain L1 测试覆盖 standalone、parent-derived、能力收缩、扩权拒绝；生产源码与测试均不再引用 `RunKind` 或 `.kind`。
    - 完成记录：物理删除 `RunKind` 与公开 `RunSpec.kind`；移除 `RunSpec::new` 基于 name/timeout 推断角色的构造旁路。`RunSpec` 内部以私有 `CapabilityPolicy::Full/Restricted` 表达无父场景的能力合同，`derive_sub` 只从父有效能力建立 ceiling，产物不携带角色标签。legacy fixed-profile 与 memory 约束统一由 policy + 可选 parent ceiling 执行。
    - 验证结果：RunSpec Domain 82 tests、derived launcher 契约、Runtime lib 658 tests 全部通过；`cargo check -p runtime`、`cargo clippy -p runtime --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check`、Shared Run Loop guard 与完整 fast architecture guards 通过；全仓 `agent/**/*.rs` 搜索无 `RunKind`，Runtime 搜索无 `RunSpec::new`、name/timeout 角色推断或 `enforce_sub_fixed`。
  - [x] **P6.9.2 完成 P6.2 遗留：Factory-owned 统一绑定输入与单一装配算法**
    - 先为 `RunCreationRequest → RunInstance` 建立失败契约：相同入口能处理无父和有父 Run，并验证 session snapshot、provider、context、tool catalog、interaction、hook、reasoning、workspace、cancel 与 event sink 的逐层绑定。
    - 删除 `RuntimeContextResolver` trait、`MainRunContextResolver`、`SubRunContextResolver`；`RunFactory` 不再注入来源特定 resolver，只持唯一 `RuntimeContextFactory`。
    - 将现有 resolver 使用的活依赖按职责移入 `RuntimeServices` 的窄 factory/port，或作为 `RunCreationRequest` 引用的明确 session/parent capability handle；禁止新增同构 `*Parts`、`*Source`、Main/Sub enum 或带大量 `Option` 的装配参数袋。
    - `RuntimeContextFactory` 统一根据 `RunSpec` capability、SessionSnapshot、可选 parent context/identity 选择 shared/isolated context、provider binding、restricted/full catalog、interaction、hook、reasoning、workspace、event route 与 cancellation scope，并原子返回 resolved session 与 `RuntimeContext`。
    - 完成门禁：Factory L1/L2、preparation L3、parent-child L4 场景通过；生产 RuntimeContext 构造只有 Factory 一处；三个 resolver 符号及调用点搜索为空。
    - 完成记录：物理删除 `RuntimeContextResolver`、`MainRunContextResolver`、`SubRunContextResolver`。`RunFactory` 构造只接收 `Arc<RuntimeContextFactory>`，独立 Run 与 parent-derived Run 都委托 Factory 的同一 `prepare` 入口。Factory 持有 session wiring 与 capability factory bindings，统一冻结 committed session/config、Provider、Context、Memory、Tool Catalog、Interaction、Hook、Reasoning、Workspace、event/input/usage/cancel；`ParentRunCapabilities` 仅增加准备派生 Run 所需的父 Context capability view 与 Workspace capability，不暴露服务集合。派生 workspace 随 `RunInstance` 原子返回，调用方不再通过 resolver 单槽取回。
    - 验证结果：Factory 34 tests、Preparation 7 tests、派生 Run launcher 契约、Runtime lib 659 tests、Composition MainSessionWiring 5 tests、Runtime bootstrap integration 1 test 全部通过；`cargo check -p runtime`、`cargo check -p composition`、`cargo clippy -p runtime --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` 通过；`RunKind`/三个 Context Resolver 生产符号搜索为空，`RuntimeContext::new` 生产调用只有 Factory 一处，Main chat 与派生 Agent 不直接创建 capability bindings；Shared Run Loop 与完整 fast architecture guards 通过。
  - [x] **P6.9.3 将公共 Loop 能力实现下沉为无角色 owner**
    - [x] Step persistence：新增 `application/loop_engine/step_persistence.rs::StepPersistenceCoordinator`，统一拥有 accepted input durable append 与 finalized Step append；独立 Run 的 adopted-input 事件通过窄 `AcceptedInputObserver` 投影，派生 Run 使用 NoOp observer。Main/Sub adapter 只保留 ContextRequest 构造与委派，不再各自调用 `append_accepted_input` / `append_finalized`。
    - [x] Compaction：新增 `application/loop_engine/compaction.rs::CompactionCoordinator`，统一拥有 window 构建、needs-compaction 决策、automatic compact 与 usage/window 失效；独立 Run 的 PreCompact Reflection 通过窄 `CompactionObserver` 提交，派生 Run 使用 NoOp observer。旧 `shared.rs` 中的两个 compact helper 已删除。
    - [x] Run lifecycle：新增 `application/loop_engine/run_lifecycle.rs::RunLifecycleCoordinator`，统一拥有 terminal/cancellation claim 与 Step scope registration；是否登记 active Step 由窄 `StepScopeObserver` 表达，独立 Run 使用 `ActiveStepScopeObserver`，派生 Run 使用 NoOp observer。
    - [x] Interaction completion：新增 `InteractionCompletionContext` 与 `InteractionCompletionContextProvider`，统一把 execution scope、Tool Execution、Tool Result Materializer、session identity 与 per-Step cancellation 交给 `InteractionCoordinator`；`InteractionCoordinator` 独占 completion 分支、批准后工具执行与结果物化。删除 `InteractionCompletionPort` 及 Main/Sub 上五组角色方法，adapter 只构造一个窄上下文。
    - [x] Stop Hook：新增 `StopHookExecutionContext` 与窄 `StopHookObserver`，`coordinate_stop_hook` 统一拥有状态启动、Hook dispatch、typed outcome、feedback 安装及 `RunExecutionState` 消息写入；Engine 只消费 typed decision。删除 `StopHookPort`，Main 仅保留 Hook UI/continuation observer，Sub 仅提供执行上下文；无 Hook 的测试 adapter 使用 observer 默认 Proceed。
    - [x] ModelInvocation lifecycle 与 ToolRound observer 窄策略收口：模型侧新增无角色 `ModelInvocationContext` owner，将角色能力拆为 `ModelInvocationSource + ModelInvocationObserver`；工具侧新增 `ToolRoundCoordinator` owner，统一拥有 context、observer 与 round 执行生命周期。Main/Sub 只实现输入来源、事件目标和进度差异，不再实现 `ModelInvocationLifecycle` 或声明 Main/Sub 角色命名的 observer。
    - [x] ContextRequest 与 Run finalization：新增无角色 `ContextRequestCoordinator` / `ContextRequestSource` 与 `RunFinalizationCoordinator` / `RunFinalizationObserver`，统一 ContextRequest 字段装配、派生 Run terminal fallback/分类及 Main/Sub finalization callback 分发；Main/Sub adapter 不再手写 `ContextRequest` literal，角色化 `finalize_sub_agent` 算法已删除。
    - [x] Event domain 分类与终态输出策略收口。
    - 先按端口比较两套实现，把完全相同或仅消费 `RuntimeContext` / `RunExecutionState` 的 Persistence、Compaction、Stop Hook、Interaction、Control 等实现迁入职责明确的共享 adapter；不得创建重新聚合全部 Port 的 fat trait/struct。
    - ContextRequest 构造、ModelInvocation lifecycle、ToolRound observer、Run finalization 中的差异拆成可组合的窄策略值或 callback，由 `RunSpec`/RuntimeContext 已绑定能力选择；差异名称按输入来源、事件目标或生命周期用途命名。
    - 每迁移一个端口先补或调整 L2 相邻契约，确保独立 Run 与 parent-derived Run 共用该 owner；禁止一次性复制后再清理。
    - 完成门禁：共享端口只有一个生产实现 owner；允许的窄差异均可独立构造和测试，不持有整组 Loop 能力。
    - 当前进度：P6.9.3 的九个公共 owner 已全部完成（100%）。新增 ContextRequest/Run finalization L2 source contract；ContextRequest 字段装配、terminal fallback/分类、Main done/task archival 与 SubRunStop Hook 均由无角色 coordinator 驱动，角色 adapter 只提供 source/observer。Runtime 668 unit tests 与 18 integration tests、all-targets check/clippy、格式、diff、Shared Run Loop、Runtime Capability Assembly 及完整 fast architecture guards 全部通过；两项 paused-time retry 用例首次全量并发运行超时，定向重跑与第二次全量均通过，未用重跑覆盖首次失败，记录为并发调度分类证据。下一阶段进入重新定义后的 P6.9.4，先固化 Chat/Derived 来源目录前置边界。
  - [x] **P6.9.4 固化来源目录前置边界**
    - 历史证据：提交 `4784f48c` 已删除 `MainRunCapabilities`、`SubRunCapabilities`、角色化 event strategy，并让 Engine/Launcher 擦除 concrete 类型；这些清理保留，不回滚。
    - 已建立迁移失败契约 `p6_9_4_source_directories_expose_only_source_observer_topology_and_mapping`：来源目录不得继续实现 Model Invocation、Tool Round、Context/Compaction、Interaction Completion、Step Persistence、Stop Hook 与 Run Lifecycle workflow port。当前测试按预期失败，首个证据是 Derived 仍实现 `ModelInvocationPort`；该红灯属于 P6.9.6-P6.9.7 的迁移门禁，不在 P6.9.4 伪装为已满足终态。
    - `loop_engine/chat/**` 与 `run/derived/**` 的允许职责已冻结。Chat 只允许 session ingress/input source、event/progress/active-step/finalization observer 和 client result mapping；Derived 只允许 parent-derived request/topology、capability ceiling、progress/final terminal mapping。
    - 来源目录禁止拥有或重新装配 Model Invocation、Tool Round、Context/Compaction、Interaction、Step Persistence、Stop Hook、Run Finalization 主流程；上述流程必须委托 P6.9.3 已建立的无角色 coordinator。
    - 字段审计结论：`RuntimeContext`、workspace/session/config/model/tool execution/materializer/agent/semaphore/cancellation/active-run 等执行依赖当前被两个大 adapter 聚合，属于非法 workflow/assembly 输入，应在 P6.9.5-P6.9.6 分别迁入不可变 Factory binding 与阶段 context；input strategy、queue/input events 属于 Input source；event/progress、accepted-input、compaction/stop-hook、active-step、finalization callback 属于窄 observer；derived prompt/role/model/spec 与 parent capability 只可保留在 request/topology 或 terminal mapping。
    - impl 审计结论：Input/Event 与窄 observer 可保留并拆成独立类型；`StepPersistencePort`、`CompactionPort`、`ModelInvocationPort`、`ToolOrchestrationPort`、`InteractionCompletionContextProvider`、`StopHookObserver`、`RunLifecyclePort` 以及 ContextRequest/Agent/ToolRound context 构造均为非法 workflow/assembly owner，迁移目标依次为 `StepPersistenceCoordinator`、`CompactionCoordinator`、Model Invocation phase、`ToolRoundCoordinator`、`InteractionCoordinator`、Stop Hook phase、`RunLifecycleCoordinator` 与 `RuntimeContextFactory`/phase context。
    - 完成证据：字段与 impl 已逐项归类并明确迁移 owner；L2 source 契约已加入，且当前失败真实记录。P6.9.4 只冻结前置边界，不宣称大 adapter 已退役；生产迁移继续由 P6.9.5-P6.9.7 完成。
  - [x] **P6.9.5 分离静态 `RuntimeServices`、纯值 Session facts 与 live Session bindings**
    - [x] 已建立字段归属契约 `p6_9_5_factory_has_no_mutable_session_binding_bag`，禁止 Factory 保存 `SessionBindings`、`RwLock<Option<_>>` 与 `bind_session_*` / `bind_derived_factories` mutation API。
    - [x] 已物理删除 `SessionBindings` 及七个延迟绑定槽；Provider factory 与 Skill materializer 改为 Factory 构造期不可变依赖，派生入口使用返回新 Factory 值的 `with_derived_bindings`，不再修改共享 Factory。
    - [x] `RuntimeContextFactory` 构造后不再改变依赖图；Composition 不再把 `MainSessionWiring` 注入 Factory，chat 启动也不再调用动态 bind API。
    - [x] `SessionSnapshot` 只保留 session identity、workspace root、model key、config 与 revision；已物理删除混入 live handles 的 `SessionCapabilitySnapshot`，Session update 继续通过既有冻结快照测试证明只影响新 Run。
    - [x] live Session bindings 已迁入 crate-private、构造后只读的 `SessionRunBindings`；Main Run 使用 `RunCreationRequest::for_session` 显式传入 typed view，不提供动态 map、`Any`、service locator、任意 capability 插入或 mutation API。
    - [x] 已将 `ParentRunCapabilities` 拆为公开纯值 `ParentRunFacts` 与 crate-private `ParentRunBindings`；前者只含 identity/spec 与 capability ceiling，后者只在 Factory 装配子 Run 时提供 parent `RuntimeContext`/workspace live view。
    - [x] TDD 证据：先新增 Session facts、Parent facts、Run request 三项结构契约并确认 3 项按预期失败，随后实现并通过；Session preparation 7 tests、Factory 35 tests、Derived 51 tests、Main Loop 44 tests通过，覆盖当前 Run 快照不漂移、parent-child cancellation、workspace/catalog/interaction 隔离。
    - 验证结果：`cargo check -p runtime`、`cargo fmt --all -- --check`、`git diff --check` 通过；生产搜索仅剩测试断言中的两个退役名称。
  - [x] **P6.9.6 按领域阶段重塑 Engine 窄契约**
    - 已新增 L2 source 契约 `p6_9_6_engine_expresses_context_model_and_tool_as_typed_narrow_phases`，要求 Engine 显式存在 Input/Drain、Step Input、Step Finalization、Context/Compaction、Model Invocation 与 Tool Round typed phase；每个 phase 的 generic bound 只包含该阶段的窄 Port（Step Input 额外依赖 RunLifecycle 注册 step scope）。
    - 新增 `InputDrainOutcome`、`StepInputOutcome`、`StepFinalizationOutcome`、`ContextCompactionOutcome`、`ModelInvocationOutcome`、`ToolRoundPhaseOutcome`，将 cancel/timeout/error/needs-compaction 从散落的 `await_interruptible` match 收口为 typed outcome；不新增 `*Parts`、能力集合 struct、trait alias 或 supertrait。
    - `run_loop` 的 drain、`execute_step` 的 step 接纳/持久化、compact/model/tool 等调用已切到 phase 函数；原有 Run 状态迁移、epoch、context-too-long 单次 compact、cancel/timeout/finalization 顺序保持不变。
    - 验证结果：P6.9.6 source 契约通过；Loop Engine 行为测试 59 项通过，覆盖 input→context 顺序、model/tool/compaction cancel、context-too-long 重建和 tool continuation；`cargo check -p runtime` 通过。剩余两个红灯仍仅为 P6.9.7 要删除的 fat `LoopCapabilityAdapter` 与来源型大 adapter。
    - 完成门禁：阶段依赖已可独立理解和测试，typed outcome 已覆盖核心异步边界；本阶段刻意不删除 fat adapter，物理退役进入 P6.9.7。
  - [x] **P6.9.7 物理删除生产 fat adapter，并拆除完整 Runtime 测试替身**
    - [x] 已物理删除 `LoopCapabilityAdapter`、`ChatLoopCapabilityAdapter`、`DerivedLoopCapabilityAdapter`。Engine/Launcher 改为接收 `RunLoop`，该调用上下文分别借用 Input、Event、Control、Lifecycle、Interaction、Persistence、Compaction、Model、Stop Hook、Tool、Stuck 与 Plan Approval 窄端口，不再要求一个来源对象实现完整端口交集。
    - [x] 已新增 `run_services.rs`，按职责提供 `RuntimeStepPersistence`、`RuntimeCompaction`、`RuntimeInteraction`、`RuntimeModelInvocation`、`RuntimeToolOrchestration`、`RuntimeStopHook`；Main 与 Derived topology 装配同一批职责型服务。
    - [x] 已由 `run_ports.rs` 统一承载 Active Run control/lifecycle、fixed plan approval 和 noop stuck seam，并删除旧生产大 adapter 及其接线。
    - [x] `application/loop_engine/tests.rs` 已将原 `ScriptedPort` / `DrainOnlyPort` 拆为共享 `ScriptedScenarioState`、仅组合 fake 的 `ScriptedPorts` fixture bundle，以及 Input、Event、Control、Lifecycle、Interaction、Persistence、Compaction、Model、Stop Hook、Tool、Stuck、Plan Approval 各职责型 fake；单个 fake 不跨能力类别实现完整 Port 集。
    - [x] fixture bundle 只组合窄 fake，自身不实现生产 Port；input-drain 场景改用最小职责 fake 组合，不再存在 `DrainOnlyPort`。
    - [x] 原平铺 `RunContextBindings` 测试参数包已删除；Factory 测试改用职责型 builder/typed binding 组合，测试未扩大生产可见性。
    - [x] 测试结构契约与 Runtime Capability Assembly Guard 同时扫描生产和 Runtime 测试源码；sanity fixtures 覆盖测试单对象跨类别 Port 的失败反例及独立窄 fake composition 正例。
    - 完成证据：生产搜索无三个 fat adapter；Runtime 源码搜索只在退役符号断言中出现旧名称；Runtime lib 652 tests、workspace tests、workspace clippy、完整 architecture guards 与 pre-push 门禁全部通过。
  - [x] **P6.9.8 将 Factory 来源总算法拆为单一 typed selector pipeline**
    - [x] 已物理删除 `prepare_independent`、`prepare_derived`，`prepare` 统一执行一次窄 selector pipeline → `bind_runtime_context` → `RuntimeContext::new`。
    - [x] 已禁止按 Main/Sub 名称或公开角色枚举选择装配路径，生产最终 Context 构造点保持唯一；原 `create` 仅作为 `cfg(test)` 低层装配测试 helper 保留。
    - [x] 已物理删除 `select_session_bindings`、`select_inherited_bindings`、`select_binding_decisions` 与 `RuntimeContextBindingDecision`，不再存在来源总 selector 或混合 binding decision。
    - [x] selector 已按职责拆为 session resolution、workspace、model/provider、context、memory、tool catalog、interaction、hook、reasoning、event route、cancellation/lifecycle；每项返回独立 typed selection。
    - [x] Derived role/model 解析与 Provider build 归 model/provider selector；restricted catalog 归 tool catalog selector；workspace isolation 归 workspace selector，并保持各自 typed error。
    - [x] Session snapshot/lease 与 workspace 仅进入 `RunCreationResources`；per-Run capability bindings 单独组合，不再把 parent、lease、workspace 和完整 bindings 堆入一个总参数包。
    - [x] 完成门禁：每个能力类别只有一个生产 selector owner；无 session/parent 两套完整 binding 算法；最终装配只消费各窄 selector 结果一次；provider/context/catalog/workspace/cancel 不存在第二生产 assembly owner。
    - TDD 记录：先新增 `p6_9_8_factory_uses_narrow_capability_selectors`，确认因 `RuntimeContextBindingDecision` 仍存在而按预期失败；实施后通过。
    - 验证结果：Factory 36 tests、Session creation 7 tests、Derived 51 tests、Main Loop 44 tests、`cargo check -p runtime`、`cargo check -p composition`、`cargo fmt --all -- --check` 与 `git diff --check` 全部通过；production check 无 warning。
  - [x] **P6.9.9 收口统一启动 façade 与来源目录终态**
    - 复核 `chat/main_run_port.rs`、`run/derived/loop_run.rs`、`derive_sub_run` 与 `launch_sub_run`：只保留 P6.9.4 允许的 topology/ingress/observer/mapping 差异，删除承担 Loop 能力实现或 Context assembly 的来源型代码。
    - Main chat 与 parent-derived Agent 必须共同消费 `RunCreationRequest → RunFactory::create → RunInstance → RunLauncher::launch → execute_prepared_loop`，启动入口只传完整 `RunInstance` 和独立窄 observer，禁止调用方拆散 Run、Execution 与 Context。
    - 文件按 owner 归档：chat 目录只负责流式输入/事件适配；derived 目录只负责 parent-derived request/topology 与 progress/final terminal 映射。任一文件不得再次形成完整运行时实现。
    - 命名与聚合边界：创建服务使用名词 `RunFactory`，行为使用动词 `create`，返回对象使用名词 `RunInstance`；物理退役 `RunPreparer`、`PreparedRun`、`RunPreparation*`、`launch_prepared` 与 `PreparedSubRun`。
    - 完成门禁：调用图审计证明两种来源只有 request/snapshot、topology 和窄 observer 差异；无第二 launch/create/model/tool/finalize 算法；生产文件职责与测试 owner 对齐。
    - 完成记录：创建链统一命名为 `RunCreationRequest → RunFactory::create → RunInstance → RunLauncher::launch`。物理删除 `preparer.rs`、`preparation.rs` 与对应测试文件；Main 和 Derived 启动调用不再执行 `RunInstance::into_parts`，Launcher 内部借用实例的 Run、Execution 与 Context。原 `PreparedSubRun` 收窄并改名为 `DerivedRun`，只组合完整 `RunInstance` 与派生元数据。
    - 验证结果：命名与启动 façade 两项 TDD 契约通过；Runtime 662 unit tests 全通过；`cargo check -p runtime`、`cargo fmt --all`、`git diff --check` 通过。Shared Run Loop Guard 已同步完整 `RunInstance` 入口，最终 Guard 集合与跨 crate 回归进入后续任务。
  - 历史记录：P6.9.1 删除 `RunKind`、P6.9.2 删除双 Context Resolver、P6.9.3 下沉九个无角色 application owner 均保持完成。P6.9.4 的旧符号清理是有效中间成果，但不能作为结构终态完成证明。

- [x] **P6.10 根除测试托活和 dead-code 豁免**
  - 记录完整测试清单后临时隔离 Runtime 测试模块与 integration tests，以 production-only `cargo check` / `cargo clippy` 暴露死代码；诊断结束必须完整恢复测试。
  - 删除模块级 `#![allow(dead_code)]`、无生产消费者的 service/scheduler/legacy API；测试辅助能力使用 `#[cfg(test)]`，禁止虚假 re-export 或测试引用制造生产可达性。
  - 完成门禁：隔离前后测试清单一致；production target 无 dead-code 告警；最终工作树不存在缺失或禁用的有效测试。
  - 隔离审计：基线为 Runtime lib 667 tests、含 6 个 integration test 文件共 685 tests、60 个外置 `*_tests.rs` 文件；临时将源码测试接线替换为恒 false cfg 并移出 integration tests 后，以 `RUSTFLAGS='-D dead-code -D unused-imports' cargo test -p runtime --lib --no-run` 验证 production-only 可达图，命令通过且源码摘要恢复一致。
  - 清理记录：删除仅测试读取的 `RunFactory::context_factory`；删除无消费者的 `RuntimeContext::{context_ref, tool_context_binding_ref, cancel_ref}`；删除无消费者的 `Agent::execute_tools_filtered`；将仅测试使用的 Agent 批量执行、并发判定和 prepared 执行 helper 收紧为 `#[cfg(test)]`；删除测试 harness 中 3 个未调用 helper 和 `CompactHarness` 的 12 个仅为延长局部构造值生命周期而遗留、实际已由 `RuntimeContext` 持有的重复字段。
  - dead-code 豁免：Runtime `application/**` 的 `#![allow(dead_code)]` / `#[allow(dead_code)]` 搜索为空；未新增虚假 re-export 或生产测试 API。
  - 恢复与验证：隔离前后 lib 测试清单均为 667、完整清单均为 685，逐行 diff 为 0；6 个 integration test 文件全部恢复。`RUSTFLAGS='-D dead-code -D unused-imports' cargo check -p runtime --lib`、同配置 production clippy、`cargo clippy -p runtime --all-targets -- -D warnings`、`cargo test -p runtime`（667 unit + 18 integration）、格式、diff、Shared Run Loop guard 与完整 architecture guards 全部通过。

- [x] **P6.11 按所有权完成 Application 目录归档**
  - `application/run/`：active registry、config snapshot、execution state、launcher、factory、creation。
  - `application/session/`：ingress、session state；`interaction/`、`hook/`、`context/`、`model/`、`tool/` 分别承载对应能力；Run 绑定的 Workspace capability access 归入 `application/run/workspace.rs`。
  - 保留真实稳定的 `client/`、`loop_engine/`、`prompt/`、`reflection/`；删除顶层平铺的兼容 re-export、无生产消费者的 `cost/`、职责混杂的 `startup/` 和 Main/Sub 同义目录，不新增万能 `common/shared`。
  - 完成门禁：`application.rs` 与 crate exports 只暴露稳定边界；目录所有权审计无无主文件、重复实现或兼容转发层。
  - 根因级归档：物理删除 `application/main_loop{.rs,/**}` 与 `application/subagent{.rs,/**}`。共享 chat 执行及输入、事件、stream、tool batch、hook、反思触发等编排归入 `application/loop_engine/chat/**`；派生 Run 的准备、执行、进度和收尾归入 `application/run/derived/**`；Agent tool runtime 归入 `application/tool/agent/**`；chat launch 输入归入 `application/run/chat_launch.rs`。删除无消费者的 reflection 兼容转发和 Runtime input-validation 兼容 re-export。
  - 后续生产可达性复核继续删除无生产消费者的 `application/cost{.rs,/**}`，费用/usage 真相保留在 Runtime usage 事件与 Audit `UsageRecord`；同时删除仅测试托活的 `startup/claude_settings_adapter{.rs,_tests.rs}` 和未被生产调用的 `startup/config_paths.rs` 迁移/路径转发层。剩余并发、模型运行设置和 Agent runner 装配迁入 `application/client/bootstrap/**`，InstructionsLoaded Hook 迁入 `application/prompt/instructions_hook.rs`，无生产消费者的 `start_session` 删除，最终物理删除 `startup{.rs,/**}`。
  - 稳定边界：`application.rs` 仅声明 `client/context/hook/interaction/loop_engine/model/prompt/reflection/run/session/tool`；测试 fixture 跟随被测能力的 owner，顶层不保留万能 testing 模块；新增 crate 内目录白名单契约，禁止角色化顶层目录和平铺无主模块回归。
  - Workspace 归档：`RuntimeWorkspaceAccess` 只负责把 Project-owned `WorkspaceViews` 绑定到 Main/派生 Run，并向 Tool 暴露窄 read access，因此迁入 `application/run/workspace.rs`；物理删除顶层 `application/workspace{.rs,/**}`，所有生产与测试消费者切换到 Run owner。
  - Testing 退役：物理删除顶层 `application/testing.rs` 与 `testing_tests.rs`；Workspace/Tool execution context fixture 迁入 `run/workspace_test_support.rs`，Provider stream/binding/factory/retry fixture 迁入 `model/test_support.rs`，Tool Result materializer fixture 迁入 `tool/test_support.rs`，TaskAccess fixture 迁入 `run/test_support_tests.rs`。同时删除 COLA Guard 中原 `application/testing.rs -> adapters` 迁移豁免，测试 workspace cancellation 改用 Run-owned `RunCancellationScope`。
  - 数量结果：顶层 `.rs` 文件由初始 16 个降至 11 个，且 11 个全部为生产 owner；顶层测试 fixture owner 清零。
  - 路径同步：Runtime 源码、integration tests、架构 Guard、Guard sanity fixtures 与设计文档均切换到真实 owner 路径；旧 `application::main_loop`、`application::subagent` 生产引用及 Guard 旧路径搜索为空。
  - Reflection owner 校正：删除 1438 行 `application/reflection/runner.rs`；Memory 新增 `ReflectionWorkflow`，统一拥有 prompt 构建、output parse、apply 与 Running/terminal history 物化；Runtime 拆为 `reflection/execution.rs`（仅 Provider 调用桥）与 `reflection/task.rs`（仅触发后的单槽、cancel/timeout/drain）。退役 `ReflectionPromptPort` 和 Runtime 内 `run_complete_reflection`/`execute_and_record` 第二业务顺序，Context 仍只拥有 compact 与 Memory 注入时序。
  - Reflection 验证：Memory 51 unit tests、Runtime Reflection 7 unit tests、PreCompact 6 tests、trigger 2 tests、Reflection teardown/error 3 integration tests、格式与 diff 检查通过；COLA Guard 将 Memory target 层补齐为 `domain/application/ports/adapters`。

- [x] **P6.12 固化防双轨 Guard 并执行最终验证**
  - [x] **P6.12.1 固化宽泛命名与当前 owner 路径规则**
    - 已完成：Shared Run Loop、Runtime Capability Assembly、crate façade 与禁止 import 规则已同步当前 owner 路径；生产标识禁止 `Projection` / `projection` 宽泛命名。
    - 已完成：`ModelInvocationProjection` 拆为 `ModelInvocationContext` 与 `ModelInvocationLifecycle`；record/view/mapper 等名称按职责修正；根 `AGENTS.md`、`specs/rust-coding.md`、`specs/runtime.md` 与设计文档已同步。
    - 历史验证：Runtime/Context/CLI check、clippy、测试、格式、diff 与当时的 architecture guards 通过；该证据只证明命名和旧 Guard，不证明 #1397 终态。
  - [x] **P6.12.2 在终态边界上固化 façade 白名单与结构 Guard**
    - [x] 前置门禁已满足：P6.9.5 的 Session/Parent 纯值边界、P6.9.7 的测试替身拆分、P6.9.8 的窄 selector pipeline 均已完成。
    - [x] 被依赖模块已声明显式 façade 白名单：Run 创建模块只导出纯值 request/facts、`RunInstance`、`RunFactory::create` 与 typed error；Model/Tool/Interaction/Persistence/Stop Hook owner 只导出 Engine 需要的窄入口与 typed outcome，内部 binding、selector、assembly token、creation resource 与 concrete adapter 保持受限可见。
    - [x] 优先通过私有模块、`pub(crate)` / `pub(super)`、受限构造器和 assembly token 使非法依赖无法编译；Guard 只补充检查稳定 façade 与唯一 owner，不把内部方法名长黑名单作为主约束。
    - [x] Capability ownership Guard 按唯一 owner 检查 Context 构造、Provider invocation、Tool execution、finalized Step append 与 Stop Hook dispatch；检查稳定导出与调用 owner，不绑定容易改名的内部方法。
    - [x] Fat implementation Guard 同时覆盖生产与 Runtime 测试：禁止 fat supertrait、trait alias、blanket impl 和单个 struct 跨多个能力类别实现完整 Port 集；允许 fixture bundle 组合多个窄 fake，但 bundle 自身不得实现生产 Port。
    - [x] Snapshot purity Guard 检查 facts/snapshot/request 字段：禁止 trait object、同步锁、event/channel handle、wiring、Runtime workspace access、`RuntimeContext`、capability map、`Any` 与 service locator。
    - [x] Factory pipeline Guard 证明每个能力类别只有一个 selector owner、最终 Context 构造唯一、Factory 不保存动态 Session binding 槽、不存在 binding mutation API、Run request 不携带 live capabilities，且无来源总 selector 返回完整 binding 集合。
    - [x] `RunLoop` 仅作为顶层 orchestrator 持有多个独立窄依赖；不实现这些 Port、Clone、导出内部 Port、被 Session/Factory/source object 保存或成为跨模块 capability source，phase 只取得对应窄能力。
    - [x] retired-symbol hygiene 已与终态结构证明分离，仅防旧实现 revert/cherry-pick。
    - [x] sanity fixtures 覆盖改名 fat adapter、fat supertrait、trait alias/blanket impl、生产/测试单对象跨类别 Port、Snapshot 新名称 live port、动态 service map/`Any`、第二来源总算法、Factory 外 Context 构造、未审批 façade export、wildcard export，并包含独立窄 fake composition 与合法 `RunLoop` orchestration 正例。
    - 完成证据：`check-runtime-capability-assembly-tests.sh` 终态 boundary probes 通过；完整 architecture guards 通过。
  - [x] **P6.12.3 执行分层回归与最终旧结构审计**
    - L1：RunSpec capability/ceiling、纯值 Session/Parent facts、typed live binding source、Factory 每项窄 selector、职责型 fake 与 RuntimeServices 字段归属已由 Runtime unit/source-contract tests 覆盖。
    - L2/L3：Session update→new Run/active Run 隔离、RuntimeContext 每项 binding、每个 Loop phase 相邻契约、独立/派生 Run 同一 owner/pipeline、parent-child cancellation/workspace 隔离已由 Runtime、Composition 与边界探针覆盖。
    - L4：Main chat 与 Agent tool parent-child journey 覆盖 input epoch、retry/cancel、compact/reflection、interaction/control、tool continuation、Stop Hook 与 finalization，workspace 全量测试通过。
    - 结构审计同时搜索旧符号、fat supertrait/trait alias/blanket impl、生产与测试单对象整组 Port impl、Snapshot live handles、Factory 动态绑定槽、来源总 selector、混合 binding decision和来源目录中的模型/工具/Context 重装配；旧名称只保留在明确的退役符号断言或历史文档中。
    - 最终验证：`scripts/setup-dev-env.sh --check` 通过（仅报告可选 `llvm-tools-preview` 未安装）；`cargo test --workspace` 通过；`cargo clippy --workspace --all-targets --all-features -- -D warnings` 通过；`cargo fmt --all -- --check`、`git diff --check`、Runtime terminal boundary probes、完整 architecture guards 与 workspace pre-push 门禁全部通过。
    - 完成门禁：Issue #1397 的 P0-P6 完成定义均已有源码、测试或 Guard 证据；P7 Issue/PR 回写尚未执行。

**依赖顺序**：已完成阶段保持历史证据；剩余主链固定为 P6.9.5（Session facts/live bindings 与 Parent facts/live view 分离）→ P6.9.8（按能力职责拆窄 selector，并拆除混合 binding decision）→ P6.9.7（拆 Runtime fat 测试替身与平铺测试 binding builder）→ P6.12.2（定义依赖模块 façade 白名单、结构 Guard、retired-symbol hygiene 与 sanity fixtures）→ P6.12.3（最终全仓回归）→ P7。P6.9.6、P6.9.9 和 P6.12.1 保持完成。不得先做 Guard：否则会把 capability snapshot、来源总 selector 或 fat 测试替身作为永久允许结构固化。若任一项验证失败，保持未勾选并记录失败证据；不得跳到依赖它的 Guard、Issue 回写或完成声明。

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

### P8：统一 Session 入站协议并退役 legacy queue

**目的**：补齐此前“首次和后续输入同路径”的未完成实现，使 Session 成为底层输入 source 的唯一消费者，并让单个 Run 只持有一个 active Interaction receiver。

**文件结构与职责**：

- `packages/sdk/src/chat.rs`：`ChatRequest` 只携带唯一 typed ingress，不携带业务输入或 legacy queue。
- `packages/sdk/src/tui.rs`：保留唯一 `ChatInputEventPort`，物理删除 `QueueDrainPort` / `QueueFuture`。
- `agent/features/runtime/src/application/session/ingress.rs`：承载 Session-owned mailbox、关闭状态与 deferred typed events。
- `agent/features/runtime/src/application/loop_engine/chat/input_gate.rs`：只消费 Session ingress 已路由的 typed events，不再合流文本 queue。
- `agent/features/runtime/src/application/loop_engine/chat/run_input_buffer.rs`：仅负责 active Run 的 epoch、seal、withdraw 与 Step batching。
- `agent/features/runtime/src/application/run/execution_state.rs`：用单槽保存 active Interaction receiver。
- `agent/features/runtime/src/application/interaction/coordinator.rs`：注册、轮询、完成和清理单个 active Interaction。
- `apps/cli/src/tui/**`、`apps/cli/src/chat/no_tui.rs`、`apps/cli/src/subcommand/*.rs`：所有首条和后续输入都先生成 typed `ChatInputEvent`，再写入同一 ingress。

#### Task 1：冻结单 Run 单 Interaction 领域不变量

**Files:**
- Modify: `agent/features/runtime/src/application/interaction/coordinator_tests.rs`
- Modify: `agent/features/runtime/src/application/run/execution_state.rs`
- Modify: `agent/features/runtime/src/application/interaction/coordinator.rs`

- [x] **Step 1: 写入失败测试**

新增测试，构造两个不同 metadata/receiver，第一次注册成功，第二次注册必须返回 `ActiveInteractionAlreadyRegistered`，并断言原 receiver 的 request identity 未被覆盖。

- [x] **Step 2: 运行 RED**

Run: `cargo test -p runtime application::interaction::coordinator::tests::execution_rejects_second_active_interaction -- --exact`

Expected: FAIL，当前 `store_mailbox_receiver` 返回 `()` 且 `Vec` 接受第二项。

- [x] **Step 3: 实现单槽**

将 `RunExecutionState.interaction_receivers: Vec<_>` 改为 `active_interaction: Option<ActiveInteractionReceiver>`；注册返回 typed result；轮询只处理该单槽的 `Empty / Resolved / Closed`，删除 `take_interaction_receivers`、`replace_interaction_receivers`、multiple-completion warning 和 oneshot 重包装。

- [x] **Step 4: 运行 GREEN**

Run: `cargo test -p runtime application::interaction::coordinator::tests -- --nocapture`

Expected: PASS，且 coordinator 测试无失败。

#### Task 2：冻结唯一 typed ingress 与 legacy queue 退役契约

**Files:**
- Modify: `packages/sdk/src/chat_tests.rs` 或现有同级 SDK 测试文件
- Modify: `agent/features/runtime/src/application/loop_engine/chat/input_gate_tests.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/input_gate_reset_withdraw_tests.rs`

- [x] **Step 1: 写入 SDK 失败契约**

断言 `ChatRequest` 只接受 `Arc<dyn ChatInputEventPort>`，且 SDK crate 不再导出 `QueueDrainPort` / `QueueFuture`、不再定义 `UserInput` 启动字段。

- [x] **Step 2: 写入 Runtime 失败契约**

将 input gate 测试改为只传 typed input source；断言 UserMessage 的 `InputId`、text 和 images 原样进入 adopted event，不存在 queue text 的二次 `classify_text`。

- [x] **Step 3: 运行 RED**

Run: `cargo test -p sdk && cargo test -p runtime application::loop_engine::chat::input_gate::tests -- --nocapture`

Expected: FAIL，当前公开面仍包含 `user_input`、`queue_drain`，Runtime gate 仍要求 queue 参数。

#### Task 3：建立 Session-owned ingress mailbox

**Files:**
- Create: `agent/features/runtime/src/application/session/ingress.rs`
- Create: `agent/features/runtime/src/application/session/ingress_tests.rs`
- Modify: `agent/features/runtime/src/application/session.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/idle_lifecycle.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/input_strategy.rs`

- [x] **Step 1: 写 mailbox 状态转换失败测试**

覆盖 `recv_next`、`drain_available`、`defer`、source close；证明 idle 输入可创建 Run、active 输入可进入当前 Run、sealed 输入保留给下一 Run、控制事件不会被当作模型消息。

- [x] **Step 2: 运行 RED**

Run: `cargo test -p runtime application::session::ingress::tests -- --nocapture`

Expected: FAIL，`SessionInputMailbox` 尚不存在。

- [x] **Step 3: 实现最小 mailbox**

`SessionInputMailbox` 独占 `Arc<dyn ChatInputEventPort>` 和 deferred `VecDeque<ChatInputEvent>`；底层 source 只由该类型读取。Loop 获得已路由事件或 `RunInputBufferHandle`，不保存 source clone。

- [x] **Step 4: 运行 GREEN**

Run: `cargo test -p runtime application::session::ingress::tests -- --nocapture`

Expected: PASS。

#### Task 4：迁移 Runtime chat 与 input gate

**Files:**
- Modify: `agent/features/runtime/src/application/client/trait_chat.rs`
- Modify: `agent/features/runtime/src/application/client/accessors.rs`
- Modify: `agent/features/runtime/src/adapters/input_buffer.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/loop_context.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/loop_runner.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/chat/input_gate.rs`
- Delete: `agent/features/runtime/src/application/loop_engine/chat/queue.rs`

- [x] **Step 1: 让 Runtime 相邻测试保持 RED**

运行 Task 2/3 测试，确认失败仍由旧 queue/双 source 形状导致。

- [x] **Step 2: 删除 Runtime queue 合流**

删除 `RuntimeQueueDrainPort`、Queue generic、`drain_sources` 的文本分支、`EmptyQueueDrainPort` / `SequenceQueueDrainPort`；`apply_gate` 只处理 mailbox 已提供的 typed events。

- [x] **Step 3: 删除 initial_messages**

删除 `trait_chat.rs` 局部变量、`ChatLoopContext.initial_messages`、loop runner 解构和消息初始化旁路；首条消息只能来自 ingress → RunInputBuffer。

- [x] **Step 4: 验证 Runtime**

Run: `cargo test -p runtime --lib`

Expected: PASS。

#### Task 5：收窄 SDK ChatRequest 并迁移全部 producer

**Files:**
- Modify: `packages/sdk/src/chat.rs`
- Modify: `packages/sdk/src/tui.rs`
- Modify: `packages/sdk/src/lib.rs`
- Modify: `apps/cli/src/tui/effect/session/processing.rs`
- Modify: `apps/cli/src/chat/no_tui.rs`
- Modify: `apps/cli/src/subcommand/model_selection.rs`
- Modify: `apps/cli/src/subcommand/sessions_command.rs`

- [x] **Step 1: 收窄 SDK Published Language**

`ChatRequest` 只保留非可选的唯一 `ingress: Arc<dyn ChatInputEventPort>`；删除 `UserInput`、`queue_drain`、`input_events` 多源字段和 Queue re-export。

- [x] **Step 2: 迁移 TUI**

TUI 建立一次 input channel 后将 receiver 作为 `ChatRequest.ingress`；首条、busy 插话、SkillRequest 和命令继续通过同一 sender，保持 producer 创建的 `InputId`。

- [x] **Step 3: 迁移 no-TUI 与子命令**

每个入口先创建 channel，再发送 typed UserMessage / SkillRequest / Query event，随后传唯一 ingress；不得把 text 再交给 Runtime 分类。

- [x] **Step 4: 验证 SDK/CLI 相邻层**

Run: `cargo test -p sdk && cargo test -p cli tui::effect::session::processing -- --nocapture`

Expected: PASS。

#### Task 6：补齐完整 Session/Run/Interaction 场景

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/chat/loop_runner_tests.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/tests.rs`
- Modify: `agent/features/runtime/tests/interaction_routing.rs`
- Modify: `apps/cli/src/tui/scenario_tests.rs` 或现有 chat 场景 owner

- [x] **Step 1: 首条与追加输入同轨场景**

发送 A 创建 Run，等待下一输入后发送 B；断言二者都从同一 ingress 到达，`InputId` 不变且没有重复 adopt。

- [x] **Step 2: sealed 转交场景**

Run seal 后发送 B；断言 B 不进入旧 Run，当前 Run 完成后由下一 Run 接纳。

- [x] **Step 3: 多 Run Interaction 场景**

两个 Derived Run 各自注册一个 Interaction，分别回复；断言只恢复对应 Run。单个 Run 注册第二个 active receiver 必须失败且不覆盖首项。

- [x] **Step 4: 终止清理场景**

Run 在 AwaitingUser 时 terminate；断言 Bridge waiter、active receiver、Domain pending 和 queued work 全部清空，且不会启动下一项。

- [x] **Step 5: 运行 L4 场景**

Run: `cargo test -p runtime --test interaction_routing && cargo test -p cli scenario_tests -- --nocapture`

Expected: PASS。

#### Task 7：退役审计与最终验证

**Files:**
- Modify: `.agents/hooks/check-shared-run-loop.sh` 或当前对应 Runtime ingress guard
- Modify: `docs/superpowers/plans/2026-07-25-issue-1397-runtime-ioc-unification.md`

- [x] **Step 1: 旧符号零匹配**

Run: `rg 'QueueDrainPort|QueueFuture|RuntimeQueueDrainPort|queue_drain|initial_messages|interaction_receivers|take_interaction_receivers|replace_interaction_receivers' packages agent apps`

Expected: 0 matches；测试中的退役断言若需保留，必须限定为明确字符串契约。

- [x] **Step 2: 生产可达性与格式**

Run: `cargo run -p xtask -- production-reachability . && cargo fmt --all -- --check && git diff --check`

Expected: PASS。

- [x] **Step 3: 全量 Rust 门禁**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --all-features -- -D warnings`

Expected: PASS。

- [x] **Step 4: 架构守卫**

Run: `.agents/hooks/check-architecture-guards.sh --full`

Expected: PASS。

- [x] **Step 5: 回写完成证据**

只在所有对应 RED/GREEN、相邻契约、L4 场景和全量门禁通过后勾选 P8；记录首次失败，不使用重跑成功覆盖首次失败。

**验证证据**：

- `cargo test --workspace`：通过。
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`：通过。
- `cargo run -p xtask -- production-reachability .`：通过。
- `.agents/hooks/check-architecture-guards.sh --full`：通过。
- `cargo fmt --all -- --check`、`git diff --check` 与旧符号零匹配审计：通过。
- 首次 Runtime 全量验证暴露 Interaction receiver 被 `try_recv` 二次消费与既有双输入测试时序问题；根因修复后 Runtime 668 个单元测试及所有集成测试通过。

**完成定义**：

- [x] SDK/Runtime/CLI 全仓不存在 legacy Queue contract 或 adapter。
- [x] `ChatRequest` 不携带业务输入旁路，只绑定唯一 typed ingress。
- [x] `initial_messages` 物理删除。
- [x] Session 是底层 input source 唯一消费者，Run 只消费 `RunInputBuffer`。
- [x] 首条、后续、Skill 和命令输入保持 producer 生成的 identity 与字段。
- [x] 单 Run 最多一个 active Interaction receiver；多 Run 可并发且不串线。
- [x] cancel/terminate/timeout 能清理 active receiver、Bridge、Domain pending 和 queued work。
- [x] L1-L4 相邻测试、production reachability、workspace test/clippy 和 architecture guards 全部通过。

## 5. 测试策略矩阵

| 领域 | L1/L2 | L3 | L4 | L0 / 守卫 |
|---|---|---|---|---|
| Run / RunExecutionState | 状态迁移、唯一 owner、pending interaction、无 `RunKind` 的 capability/ceiling 约束 | Run preparation 字段与 terminal 契约 | 完整 Run 生命周期 | source/API/无重复状态/无角色 enum 守卫 |
| RuntimeServices / SessionState | 字段归属、snapshot 不变、Factory 无动态 binding 槽 | 单一 RuntimeContextFactory typed binding pipeline 输入输出契约 | Session 创建→Run→后续 Session 变更隔离 | 唯一 Factory、无 Resolver、无双 prepare/assembly、无 `bind_session_*` ownership |
| Capability factory | mode/ceiling/unavailable 组合 | Interaction/Hook/Provider/Tool/Workspace adapter 契约 | parent-child dispatch / interaction journey | capability assembly、hexagonal dependency、双 Resolver 禁止 |
| Loop Engine | 每个领域阶段的输入/typed outcome、epoch、command scheduling、input/interaction 等待态隔离、finalization | 阶段 coordinator 与窄 source/observer 相邻契约，统一 Engine 无能力全集 | user input / command / interaction → model/tool → terminal journey | shared loop、fat supertrait/fat struct/参数袋、双大 adapter 与单对象整组 Port impl absence |
| Composition/bootstrap | factory 注入、生命周期、SessionIngress 分类 | SDK/CLI bootstrap boundary、单一输入源 | 启动→分类→目标 mailbox→Run→终态 | cross-BC construction ownership |
| TUI/SDK projection | 现有纯值映射单元 | Interaction / event field completeness | 用户交互闭环（受影响时） | crate/API boundary |

测试必须遵循：生产逻辑前先建立失败证据；跨层改动每层都有相邻测试；不得以 L4 替代 L1-L3；不得扩大测试专用生产 API；测试文件按 owning layer 分离，禁止新增 `mod.rs`、万能 `test_utils` 或 inline test 违背仓库守卫。

## 6. PR / Issue 门禁

创建或更新实施 PR 前必须：

- [x] PR 分支基于最新 `origin/main`，且已执行 `git pull origin main`。
- [x] #1397 milestone、parent / blocked-by 关系未被擅自改动；#1248 已完成的生产能力被统一模型消费。
- [x] Issue body 的全部 checklist 已逐项核对；未完成项不得静默勾选。
- [x] `RuntimeServices`、`SessionState`、`RunExecutionState`、`RuntimeContextFactory` 唯一所有权有源码和测试证据。
- [x] `RuntimeContextParts`、生产 `RunContextBindings`、fat `RunLoopPort` / `LoopCapabilityAdapter`、`RunKind`、双 Context Resolver、双 Factory prepare/assembly 与来源型大 Capabilities adapter 均按完成定义退役，或明确记录未完成原因和后续边界。
- [x] Main chat 与派生 Agent 的同一 Factory、capability composition 和 Loop 生产可达性，以及 capability 不扩权、ParentMediated 隔离、BoundaryOnly 过滤有测试证据。
- [x] `from_args.rs` 已删除或收敛为薄 bootstrap，并有 Composition boundary 测试。
- [x] 文档、Migration Governance、Guard registry 和 PR Test plan 同步。
- [x] 全量验证通过，失败项有真实报告，不使用 `--no-verify` 隐藏失败。

## 7. 风险与止损策略

| 风险 | 处理 |
|---|---|
| 迁移时同时改变 Loop 行为和所有权 | 先以旧行为测试锁定状态机与跨层契约，再单次迁移一条 owner 边界；每阶段运行相邻测试 |
| `RunExecutionState` 与 `Run` 继续双写 | 以字段归属表和 source guard 阻止重复字段；发现双 owner 立即停在 P1，不继续搬消费者 |
| Factory 退化为被动 bindings 复制器或动态 Session 容器 | `prepare` 只接受纯值 request；Factory 只持不可变 RuntimeServices/factories 并消费 SessionSnapshot；删除 `SessionBindings`、`bind_session_*` 与双 prepare helper，由 typed selector 测试证明单一 pipeline |
| 删除 `RunKind` 后用名称或 Option 形状偷偷恢复角色判断 | capability policy 与 parent ceiling 必须有显式领域类型和测试；Guard 禁止 `name == "main"`、`is_sub`、Main/Sub enum 及来源推断 |
| 统一 Capabilities 时形成新的超集大对象 | 先稳定来源边界与 Factory/Session 输入，再按领域阶段重塑 Engine；禁止 fat supertrait、fat struct、参数袋、参数展开和“单对象实现整组 Port”，差异只保留窄 source/observer/topology/mapping |
| Parent interaction 泄露或 sibling 互相取消 | child-scoped identity + route mapping + teardown 测试；失败时禁止进入统一 Loop 退役阶段 |
| BoundaryOnly 只做存在性校验 | adapter 入口做真实 invocation filter，并以 forbidden invocation 契约测试锁定 |
| `from_args.rs` 搬家后形成第二 Composition Root | 以 construction ownership guard 和 bootstrap contract 测试检查 concrete constructor 位置 |
| 一次性删除旧 adapter 导致行为不可追踪 | 先迁移单一生产入口和测试，再删除类型；每次删除后运行旧符号搜索与 crate 测试 |
| 计划过大无法在一个 PR 完成 | 保持一个 Issue，但若实际需要多个独立 PR，先向用户报告候选边界并等待明确拆分决定；不得自行创建 sub-issue |

## 8. 交付顺序

1. [x] P0-P5、P6.1-P6.8：统一 Run/Engine 基础、能力边界和公共 owner。
2. [x] P6.9.1-P6.9.6：退役角色决策与双 Resolver，分离 Session/Parent facts 和 live bindings，建立阶段窄契约。
3. [x] P6.9.8-P6.9.9：拆分 Factory typed selector pipeline，统一 `RunCreationRequest → RunFactory::create → RunInstance → RunLauncher::launch` façade。
4. [x] P6.9.7：删除生产 fat adapter，拆分 `ScriptedPort` / `DrainOnlyPort` / 平铺测试 binding bag 为职责型 fake 组合。
5. [x] P6.10-P6.11：根除测试托活、dead-code 豁免和无所有权 Application 平铺模块。
6. [x] P6.12.1-P6.12.2：固化命名规则、façade 白名单、唯一 owner Guard、snapshot purity、Factory pipeline、fat implementation 和 sanity fixtures。
7. [x] P6.12.3：完成开发环境、workspace tests、workspace clippy、格式、diff、Runtime 定向 Guard、完整 architecture guards 与 pre-push 门禁。
8. [x] P7：已拉取 Issue #1397 当前状态并逐项回写 15 项 checklist；已更新 PR #1413 Test plan；最终提交 `fdff27f3` 已推送，pre-push 完整架构守卫与分层测试通过。

除非某阶段验证失败需要回到根因调查，否则不得跳过前置阶段。每个步骤均为单一、具体、可验证任务；完成后必须记录变更文件、RED/GREEN 测试证据、剩余旧结构、Guard 状态和下一阶段依赖。
