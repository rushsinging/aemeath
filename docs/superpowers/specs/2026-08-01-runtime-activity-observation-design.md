# Runtime Activity 统一观测设计

> 状态：Approved Direction / Pending Written Review  
> 日期：2026-08-01  
> 适用范围：Agent Runtime、SDK Published Language、CLI/TUI ACL 与活动展示

## 1. 目标

本期建立一步到位的 Runtime Activity 统一观测机制，使 Context preparation、Model invocation、Tool call、Hook、Compact、Interaction、Sub Run、Step finalization、cancel/terminate 等运行中工作使用同一套创建、变更、计时、父子关联、收口与发布机制。

TUI 不把全部内部活动持续展示给用户。完整 Activity 事实进入 TUI Model 和诊断链路，默认界面只显示低噪声摘要：当前用户可理解阶段、Run 总计时、当前阶段计时，以及必要时的一条稳定活动摘要。

## 2. 核心决策

采用 **Runtime Activity Observation Model**：

- `Run`、`RunStep`、`ToolCall`、`ModelInvocation` 及各能力返回的 typed outcome 继续拥有业务事实；
- Activity 不成为聚合根，也不成为 `Run` 聚合内与 `ToolCall` 并列的业务实体；
- `ActivityCoordinator` 是 Runtime application 内的 per-Run 服务，拥有非持久化 Activity 观测注册表；
- Activity 只能由领域事实或 Runtime application 流程驱动，不能反向修改领域对象；
- SDK 发布完整 typed Activity observation、增量变更和全量快照；
- TUI 通过 ACL 转换并保存事实镜像，由独立展示策略生成低噪声摘要；
- 旧 spinner phase、专项计时、运行工具计数和分散可见性判断全部退役，不保留第二套活动来源。

## 3. Activity 的地位与所有权

### 3.1 与 Run 聚合的关系

`Run` 是唯一 Agent 执行生命周期状态机，也是 Activity 的生命周期边界：

- 一个 Activity 必须归属于一个 `RunId`；
- 除 Run 根活动外，工作活动通常还归属于一个 `RunStepId`；
- Run 进入终态后不得创建新 Activity；
- Run 终止时，Coordinator 必须统一收口仍未终结的 Activity；
- Activity 的终结不能改变 Run 的业务终态。

Activity 不进入 `Run` 字段，也不参与 Run 合法迁移判断。Run 只产生领域事件；ActivityCoordinator 消费领域事件和 application typed facts，形成观察结果。

### 3.2 与 RunExecutionState、RuntimeContext 的关系

Activity 注册表是 per-Run、非持久化的 application state。`ActivityCoordinator` 由统一 `RuntimeContextFactory` 创建并绑定：

```text
RuntimeContext
  └─ ActivityCoordinator
       ├─ Run identity
       ├─ monotonic Clock
       ├─ EventSink
       └─ ActivityRegistry（串行化的 per-Run observation state）
```

它不是外部 Port，也不是 Composition wiring。Composition 只注入 Clock/EventSink；Runtime application 定义 Activity 类型、生命周期规则和更新时机。

模型、工具、Hook、Compact 等外部能力不会获得 Activity callback。对应 Runtime coordinator 在调用能力前后更新 Activity，避免反向控制和跨 BC 泄漏。

### 3.3 与现有实体的关系

| 来源 | 业务真相所有者 | Activity 作用 |
|---|---|---|
| Run lifecycle | `Run` | 根活动与当前阶段活动 |
| Model invocation | Runtime Model coordinator / `ModelInvocation` | 调用、等待首 token、streaming、retry、终结观察 |
| Tool call | `ToolCall` + Tool round coordinator | approval、running、suspended、terminal 观察 |
| Hook | Hook BC typed outcome + Runtime Hook coordinator | dispatch、attempt、blocked/failure 观察 |
| Compact | Context BC typed progress/outcome + Runtime Compact coordinator | compact 阶段和进度观察 |
| Interaction | `Run.pending_interaction` + Interaction coordinator | waiting/resumed/cancelled 观察 |
| Sub Run | child `Run` + parent Tool coordination | 父活动挂载与 child root correlation |
| Finalization/control | `Run` + Step/Run finalizer | finalizing、cancelling、terminating 观察 |

## 4. Activity 模型

### 4.1 Identity 与关联

Activity 使用 Runtime-owned `ActivityId`（UUIDv7），并携带显式关系：

```rust
struct ActivityObservation {
    id: ActivityId,
    run_id: RunId,
    run_step_id: Option<RunStepId>,
    parent_activity_id: Option<ActivityId>,
    source: ActivitySource,
    kind: ActivityKind,
    state: ActivityState,
    detail: ActivityDetail,
    audience: ActivityAudience,
    revision: u64,
    timing: ActivityTiming,
}
```

`ActivitySource` 保存来源 identity，而不是仅保存展示文字：

```rust
enum ActivitySource {
    Run,
    RunStep(RunStepId),
    ModelInvocation(ModelInvocationId),
    ToolCall(ToolCallId),
    HookDispatch(HookDispatchId),
    Interaction(InteractionRequestId),
    ChildRun(RunId),
}
```

父子关系表达展示和诊断树，不表达业务所有权。至少形成：

```text
Run root activity
  └─ Run phase activity
       ├─ Model invocation activity
       ├─ Tool call activity
       ├─ Hook dispatch activity
       ├─ Compact activity
       ├─ Interaction activity
       └─ Child run activity
```

### 4.2 Activity kind

首版完整覆盖：

```rust
enum ActivityKind {
    Run,
    RunPhase(RunPhaseKind),
    ModelInvocation,
    ToolCall,
    HookDispatch,
    Compaction,
    Interaction,
    ChildRun,
}

enum RunPhaseKind {
    DrainingInput,
    PreparingContext,
    ApplyingResponse,
    AwaitingToolApproval,
    ExecutingTools,
    FinalizingStep,
    CancellingStep,
    Terminating,
}
```

`InvokingModel`、`Compacting`、`AwaitingUser` 等 Run 状态仍由 Run 发布。为了避免重复表达，具有独立 Activity 的工作状态映射到对应 Activity kind；纯流程阶段映射为 `RunPhase`。当前阶段由 Coordinator 根据 Run transition 原子切换，不由 TUI 推断。

### 4.3 Activity state

Activity 生命周期不是第二套业务状态机，只描述观测状态：

```rust
enum ActivityState {
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
    Terminated,
}
```

合法关系：

```text
Running ⇄ Waiting
Running/Waiting → Succeeded | Failed | Cancelled | Terminated
```

规则：

1. 终态不可转出；
2. 同 revision 重复变更幂等；
3. unknown Activity 的非 start 变更返回 typed error，并记录诊断，禁止静默创建含糊状态；
4. Run terminal safety close 只能把未完成 Activity 收口为与 Run terminal cause 一致的 `Failed/Cancelled/Terminated`，不能伪造 `Succeeded`；
5. 业务实体和 Activity 终态必须来自同一个 Runtime application 决策点，但业务实体永远是权威真相。

### 4.4 Detail 与 audience

`ActivityDetail` 使用封闭 typed enum，禁止用任意字符串承担协议：

```rust
enum ActivityDetail {
    Run { purpose: RunPurposeView },
    Phase { phase: RunPhaseKind },
    Model { model: String, attempt: u32, stream: ModelStreamState },
    Tool { name: String, summary: Option<String>, parallel_count: u16 },
    Hook { point: HookPointView, attempt: u8 },
    Compact { stage: CompactStageView, current: Option<u32>, total: Option<u32> },
    Interaction { kind: InteractionKindView },
    ChildRun { role: String, model: String },
}
```

Runtime 只发布安全、已截断、无 secret 的摘要字段。完整参数、Hook stdout/stderr、provider raw response 不进入 Activity PL。

`ActivityAudience` 表达跨客户端稳定的观测价值，不编码 TUI 布局：

```rust
enum ActivityAudience {
    User,
    Operational,
    Diagnostic,
}
```

- `User`：Run phase、长耗时 Tool/Child Run、Compact、Interaction；
- `Operational`：并行工具数量、模型 retry、Hook block；
- `Diagnostic`：Hook attempt、内部 finalizer 子动作等。

TUI 决定具体可见性，Runtime 不发布 `visible`、颜色、spinner 文案或布局字段。

### 4.5 Timing

所有计时基于注入的 monotonic Clock，不使用 SDK/TUI 到达时间：

```rust
struct ActivityTiming {
    total_elapsed_ms: u64,
    active_elapsed_ms: u64,
    state_elapsed_ms: u64,
    started_at_unix_ms: Option<u64>,
    finished_at_unix_ms: Option<u64>,
}
```

- `total_elapsed_ms` 包含等待；
- `active_elapsed_ms` 排除 `Waiting`；
- `state_elapsed_ms` 是当前 ActivityState 的连续耗时；
- Run 根活动提供 Run 总计时；
- 当前 phase/activity 提供分段计时；
- terminal observation 冻结最终耗时；
- TUI tick 只基于最近 observation 的 monotonic anchor 做本地平滑滚动，不拥有计时真相。

## 5. ActivityCoordinator

### 5.1 唯一写入口

Coordinator 提供 typed command，不暴露 registry 可变引用：

```rust
impl ActivityCoordinator {
    fn start(&self, command: StartActivity) -> Result<ActivityId, ActivityError>;
    fn update(&self, command: UpdateActivity) -> Result<(), ActivityError>;
    fn wait(&self, command: WaitActivity) -> Result<(), ActivityError>;
    fn resume(&self, command: ResumeActivity) -> Result<(), ActivityError>;
    fn finish(&self, command: FinishActivity) -> Result<(), ActivityError>;
    fn observe_run_events(&self, events: &[RunDomainEvent]) -> Result<(), ActivityError>;
    fn close_run(&self, terminal: ActivityTerminal) -> Result<(), ActivityError>;
    fn snapshot(&self) -> ActivitySnapshot;
}
```

业务代码不能直接构造 `ActivityObservation`，只能提交 typed command。Coordinator 负责：

- 分配 ActivityId；
- 校验 Run/Step/parent/source 关系；
- 维护单调 revision；
- 计算 timing；
- 原子结束旧 phase 并启动新 phase；
- 发布完整 observation；
- Run terminal safety close；
- 生成一致快照。

### 5.2 原子发布顺序

Run mutation 和 Activity observation 的顺序固定：

```text
Run mutation
  → drain RunDomainEvent
  → ActivityCoordinator.observe_run_events
  → publish Activity change
  → publish原有 Run lifecycle/terminal event
  → next business statement / await
```

对 Model/Tool/Hook/Compact 等 application work：

```text
Coordinator.start Activity
  → publish Started observation
  → invoke external capability
  → update/wait/resume as typed facts arrive
  → business outcome applied to owner
  → finish Activity from same typed outcome
  → publish terminal observation
```

如果 Activity 发布失败，业务流程不回滚；Runtime 记录结构化诊断并在下一次 snapshot 修复消费者。Activity 不能成为业务可用性的同步依赖。

### 5.3 并发与泄漏收口

- per-Run registry 内部串行化 mutation；
- 并行 Tool activity 各有独立 identity；
- 同 source identity 在同一 RunStep 同一时间最多一个 live Activity；
- explicit `finish` 是正常路径；
- Step finalize 关闭该 Step 下未完成 leaf Activity；
- Run terminal `close_run` 是最后 safety net；
- Drop 不发布成功，RAII guard 只能用于检测泄漏并记录诊断，不能猜测业务结果。

## 6. SDK Published Language

SDK 发布 Runtime-owned Activity PL：

```rust
struct ActivityView { /* ActivityObservation 的 wire-safe 完整值 */ }

struct ActivitySnapshotView {
    run_id: RunId,
    revision: u64,
    activities: Vec<ActivityView>,
}

enum ActivityChangeKind {
    Started,
    Updated,
    Finished,
}

enum ChatEvent {
    ActivityChanged {
        kind: ActivityChangeKind,
        activity: ActivityView,
    },
    ActivitySnapshot(ActivitySnapshotView),
    // 原有内容、terminal、interaction 等事件继续存在
}
```

每个增量事件携带完整 ActivityView，而不是 partial patch。消费者按 `(run_id, activity_id, revision)` 幂等 upsert。快照用于：

- stream 建立后的初始同步；
- consumer 检测 revision gap 后恢复；
- reconnect/未来 Server transport；
- 测试和诊断查询。

`RunTransitioned` 继续发布 Run 生命周期权威状态，但不再携带 TUI 活动专用 timing。Activity 链切换完成后，`RunTimingView` 和由 Run status 直接驱动的活动展示协议退役；terminal 和 interaction 等业务事件不合并进 Activity。

## 7. TUI 消费与低噪声展示

### 7.1 ACL 与 Model

唯一链路：

```text
sdk::ActivityView
  → event_mapping.rs（SDK DTO → TUI-owned Activity DTO）
  → agent_event.rs（Activity event → Conversation Intent）
  → root reducer
  → ConversationModel.activity_observations
  → ActivitySummaryAssembler
  → LiveStatusViewModel
```

TUI Model 保存完整事实镜像：

```rust
struct ActivityObservationModel {
    revision_by_run: HashMap<UiRunId, u64>,
    activities: Vec<UiActivityObservation>,
}
```

Model 不保存转换许可表，不根据 Tool/Hook/RunStatus 重建 Activity，也不在收到内容 delta 时自行切 phase。

### 7.2 默认摘要策略

默认状态行只显示：

```text
<spinner> <phase label> · <run total> · <phase elapsed> [· <stable detail>]
```

默认规则：

1. 只选择 `parent_run_id == None` 的 active Main Run；
2. phase label 来自当前 live phase Activity；
3. Run total 来自根 Activity；phase elapsed 来自 phase Activity；
4. 最多显示一个 `ActivityAudience::User` leaf detail；
5. 多个并行 Tool 不轮播名称，显示稳定聚合文案，例如 `Running 3 tools`；
6. Tool/Child Run detail 只有持续超过 500ms 才进入摘要，且最短驻留 750ms，避免闪烁；
7. Hook attempt、retry delay、参数 delta、token delta、finalizer 子动作不进入默认状态行；
8. `AwaitingInteraction` 显示等待用户，不继续滚动 active elapsed；
9. terminal Activity 到达后隐藏 spinner，最终业务成功/失败仍由原有 Runtime terminal event决定。

示例：

```text
Thinking… · 18s · 7s
Calling tools… · 23s · 5s · Read src/runtime.rs
Calling tools… · 24s · 6s · Running 3 tools
Waiting for approval · 31s · 4s
```

### 7.3 详情和诊断

本期不新增持续占屏的全量 Activity 面板。完整 Activity 事实用于：

- 现有 ToolCall/Agent/Hook 输出块的关联和后续按需展开；
- debug/trace 结构化日志；
- 未来显式详情视图或 Server 客户端；
- revision gap、未收口 activity、source 冲突诊断。

因此“一步到位”指 Activity 内核、PL、模型和退役链路完整，不意味着默认 UI 展示全部内部工作。

`RunActivityState` 最终只保留动画 frame 和本地 monotonic interpolation anchor；不得再保存 verb、phase、run identity 或业务可见性。

## 8. 错误与降级

| 场景 | 行为 |
|---|---|
| unknown activity update | typed error + diagnostic log；不隐式创建 |
| terminal activity 再更新 | 幂等同值忽略；冲突值报错 |
| SDK event revision gap | TUI 标记 stale，申请/等待 ActivitySnapshot；不从其他事件猜状态 |
| Activity event publish failure | 业务继续，记录 drop；下一 snapshot 修复 |
| Run terminal 存在 live activity | `close_run` 按 terminal cause 收口并记录 leak diagnostic |
| unsafe detail | Runtime mapper 删除或归类，不把 raw value 发给 SDK |
| TUI 无可用 Activity | 隐藏活动状态行；不回退旧 spinner 状态源 |

## 9. 测试策略

遵循跨层逐层测试，时间和 ID 全部可注入：

1. **Domain/Application L1**：ActivityCoordinator 状态、parent/source 校验、revision、timing、并发、terminal safety close；
2. **Runtime integration L2**：每个阶段 owner 在调用前 start、typed outcome 后 finish；Run transition 原子切 phase；
3. **Runtime Adapter L2**：ActivityObservation → SDK ActivityView 穷举无损映射，安全 detail 不泄漏；
4. **SDK Contract L2/L3**：枚举序列化 shape、完整 view、snapshot、revision；
5. **TUI event mapping L2**：SDK → TUI DTO 每个 variant 穷举映射；
6. **TUI ACL/Reducer L2**：event → intent → model upsert、snapshot replace、gap handling；
7. **ViewAssembler L2**：低噪声选择、并行聚合、500ms 门槛、750ms 驻留、总/阶段计时；
8. **Scenario L4**：Context → Model → Tool → Hook → Compact → terminal 的完整链路，证明默认状态行稳定且无内部事件刷屏；
9. **Architecture guards**：禁止旧活动字段和 direct activity construction，禁止 TUI 从 RunStatus/Tool/Hook 分别写活动状态。

## 10. 文档同步

实现必须同步更新以下 `docs/design/` Target 文档：

- `01-system/02-ubiquitous-language.md`：Activity、Observation、Audience、Source；
- `01-system/03-context-map.md`：Runtime Activity PL 与 TUI ACL；
- `02-modules/runtime/01-domain-model.md`：Activity 与 Run/RunStep/ToolCall 关系；
- `02-modules/runtime/03-loop-and-state-machine.md`：Run mutation 后 Activity 原子观察顺序；
- `02-modules/runtime/06-ports-and-adapters.md`：Coordinator、SDK mapping、snapshot；
- `02-modules/runtime/07-runtime-ownership-and-assembly.md`：per-Run Activity 所有权和 factory 装配；
- `02-modules/tools/02-ports-and-lifecycle.md`：ToolCall/ChildRun 活动事实映射；
- `02-modules/hook/01-run-loop-integration.md`：Hook activity 与 Hook outcome 边界；
- `02-modules/tui/01-architecture-and-dataflow.md`：Activity Model 与低噪声摘要；
- `02-modules/tui/02-model.md`：Activity 事实镜像；
- `02-modules/tui/03-event-flow-and-acl.md`：SDK → TUI typed Activity 链；
- `02-modules/tui/04-view-layer.md`：摘要选择与 ViewState 限制；
- `03-engineering/03-migration-governance.md`：旧活动链退役清单和退出条件。

Target 文档只写目标态；Current 差距和迁移顺序只写入 Migration Governance。

## 11. 迁移与退役顺序

迁移采用同一 PR 内可验证的小步提交，但最终不保留双轨：

1. 建立 Activity 类型、Coordinator、registry 和测试；
2. 接通 Run root/phase Activity 与 snapshot；
3. 接通 Model、Tool、Hook、Compact、Interaction、Child Run、finalization/control；
4. 发布 SDK Activity PL；
5. 接通 TUI ACL 和 Model；
6. 让 LiveStatus 只消费 Activity Model；
7. 删除 `RunTimingView` 的活动展示职责；
8. 删除 TUI `RunActivityState` 中业务字段和旧 `RunStatus → phase text/timing` 展示路径；
9. 删除 Tool/Hook/Compact 对 spinner 的专项影响入口；
10. 增加守卫，禁止第二套状态源复发；
11. 同步全部 Target/Migration 文档并运行门禁。

原有内容事件、ToolCall 输出块、Hook notice、terminal、Interaction body 等继续承担各自业务和内容职责；只退役它们对“当前活动状态、计时和可见性”的旁路控制。

## 12. 验收标准

- Runtime 中所有进行态工作通过 ActivityCoordinator 创建、变更和收口；
- Activity 不反向修改 Run、RunStep、ToolCall 或任何供应 BC；
- Run root、phase、leaf 活动具有稳定 identity、parent/source、revision 和 monotonic timing；
- SDK 同时支持完整增量 observation 和全量 snapshot；
- TUI 不从 RunStatus、Tool、Hook、Compact 事件分别推断当前活动；
- 默认状态行仅显示低噪声摘要，并在并行、高频更新时保持稳定；
- TUI 不持续展示 Hook attempt、retry、delta 等内部细节；
- terminal 后不存在 live Activity；失败、取消、终止不会被 Activity 伪装为成功；
- 旧 spinner phase、业务 verb、专项计时、running-tool counter 和可见性写入口物理删除；
- 每一层具有相邻契约测试，完整场景测试通过；
- `docs/design/` Target 文档和 Migration Governance 与代码一致。
