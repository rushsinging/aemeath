# Agent Runtime · 运行时所有权与统一装配

> 层级：02-modules / runtime（模块战术设计）  
> 状态：Target（目标设计）｜对应 Issue：[#1385](https://github.com/rushsinging/aemeath/issues/1385)、[#1397](https://github.com/rushsinging/aemeath/issues/1397)、[#1248](https://github.com/rushsinging/aemeath/issues/1248)\
> 本文记录 #1385 review 后确定的目标边界。#1248 已落地 RuntimeContextFactory、Interaction/Hook/Reasoning 三能力穷举装配与 Stop Hook 三分支；`RuntimeContextParts`、`assemble_main_runtime_context`、`ModelStep::StopHookBlocked`、`InteractionBridge::disabled()` 已退役。`MainSessionShell`、`MainRunPort`、`SubAgentRun` 和 `from_args.rs` 的部分旧路径仍由 #1397/#1399 收口，不构成目标模型。

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

创建与执行关系：

```text
Composition
  ├─ RuntimeServices
  └─ RuntimeContextFactory

Runtime bootstrap
  └─ SessionState

RuntimeServices + SessionState snapshot + RunSpec
                         │
                         ▼
              RuntimeContextFactory
                         │
                         ▼
                  RuntimeContext

RunSpec ────────────────▶ Run
launch input ───────────▶ RunExecutionState

Run + RuntimeContext + RunExecutionState
                         │
                         ▼
                    Loop Engine
```

强制结论：

1. 生产类型和 Loop 不再以 Main/Sub 分类；差异由 `RunSpec` 和父子拓扑表达。
2. 静态活依赖与动态会话状态必须分离，禁止恢复综合资源袋。
3. `RuntimeContext` 只有统一装配入口；禁止各调用路径手填同构参数包。
4. Composition 负责实例化、连接与生命周期；Runtime application 负责业务决策、状态转换与快照时机。
5. `Run` 与 `RunExecutionState` 必须各有唯一状态所有权，不得复制同一事实。

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
- Tool catalog、execution，以及从 Run snapshot 绑定的 execution context；
- Tool progress/event sink 的 factory 或 binding service；
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
- 当前 Run 冻结后的 Tool execution context 与 progress sink；
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

`RuntimeContextFactory` 是唯一允许构造 `RuntimeContext` 的入口。它负责：

1. 接收 `RunSpec`；
2. 接收或捕获窄的 session snapshot；
3. 从 `RuntimeServices` 取得 factory 与父能力；
4. 按能力模式选择 shared、isolated、restricted 或 disabled adapter；
5. 创建 cancellation、input、event、usage 与 Tool progress sink 等 per-Run 实例；
6. 由 factory 直接绑定当前 Run 的 Tool execution context；禁止 Loop 或 Agent 持有 `ToolExecutionContextBinding` 后按调用再次装配；
7. 校验派生 Run 不超过父 capability ceiling；
8. 返回完整且冻结的 `RuntimeContext`。

它不负责执行 Loop、恢复 Session、修改 `SessionState`、保存消息、处理模型响应或创建 `RunExecutionState`。

`RuntimeContext::new` 必须收窄为 factory 内部可见。`RuntimeContextParts` 与 `RuntimeContext` 字段一比一重复，没有领域语义、约束和所有权价值，应删除。父 Run 派生与独立 Run 创建是不同输入来源，不得形成两套公开 assembler。

## 6. Composition 与 Runtime application 边界

### 6.1 Composition 负责

Composition 回答“使用哪个实现、如何连接、生命周期多长”：

- 实例化具体 adapter；
- 连接跨 BC Port；
- 构造 `RuntimeServices`；
- 构造并注入统一 `RuntimeContextFactory`；
- 创建长期 registry、runner、materializer、scheduler 和 typed factory；
- 管理进程/Agent Runtime 级资源生命周期。

### 6.2 Runtime application 负责

Runtime application 回答“何时发生什么业务动作”：

- 创建或恢复 Session；
- 读取 committed snapshot 的时机；
- 初始化和更新 `SessionState`；
- 解析本次会话或 Run 的模型选择；
- 接纳输入并创建 `RunSpec`、`Run`、`RunExecutionState`；
- 在 Run 创建点调用 `RuntimeContextFactory`；
- 驱动 Loop 和领域状态迁移。

### 6.3 退役 `from_args.rs` 大装配器

当前 `from_args.rs` 同时承担参数解析、Session 恢复、模型绑定、Tool/Skill 查询、Prompt 构建、并发配置、Agent runner 创建、基础设施创建和 Client 构造，已形成 Runtime 内第二个 Composition Root。

目标不是把整个文件移动到 `agent/composition`，而是按职责拆解：

- 入站边界先将 CLI/SDK args 标准化为 typed bootstrap request；
- Composition 完成具体 adapter/object graph；
- Runtime bootstrap 只执行 Session 启动用例并创建 `SessionState`；
- Provider、Prompt、Skill 初始化委托各自 application service；
- Run 创建统一进入 `RuntimeContextFactory`。

最终 `from_args.rs` 应删除，或收敛为很薄的 `bootstrap_runtime(request, services)` 入口。

## 7. Workspace、Prompt、Skills 与 Config

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

## 8. 迁移映射

| 当前类型或入口 | 目标处理 |
|---|---|
| `RuntimeHandle` | 重构为 `AgentRuntime` |
| `MainSessionShell` | 删除，拆为 `RuntimeServices + SessionState` |
| `RuntimeBootstrapDependencies` 及分层参数袋 | 由 Composition 直接构造有职责的服务对象 |
| `RuntimeContextParts` | 删除 |
| `assemble_main_runtime_context` | 删除 |
| `derive_sub_run` 手填 Context | 改走统一 factory |
| `RunKind::Main/Sub` | 审计消费点后删除 |
| `MainRunPort` / `SubAgentRun` | 删除 |
| fat `RunLoopPort` | 删除或收窄为真正外部副作用契约 |
| `ChatLoopContext` | 拆为 bootstrap/launch input 与 session command driver |
| `ParentRunContextSource` | 归入 `SessionState`，按唯一槽/registry 真实语义重命名 |
| `DerivedSubRun` | 改为通用 prepared/launch result，或由统一 launcher 输入替代 |
| `RuntimeWorkspaceAccess` 旁路 | 收入 factory 的 workspace capability binding |

## 9. 事件语言与用户可见终态

Runtime 的领域状态、生命周期观测和用户可见终态是三个不同层次，禁止消费方跨层拼装语义：

```text
Run / RunStep 状态迁移
  → RunDomainEvent（领域事实）
  → Runtime Event Strategy（汇总与投影）
  → RuntimeStreamEvent（Runtime 出站）
  → SDK ChatEvent（公开传输）
  → TUI RuntimeEvent（无状态渲染）
```

强制约束：

1. `Run` 是 Run/RunStep 终态原因的唯一真相源；TUI **MUST NOT** 根据 `run_id + step_id` 关联多个生命周期事件来猜测最终提示。
2. Runtime Event Strategy 必须把完整领域事件序列汇总为一个权威的用户可见终态；SDK/TUI 只透传和渲染该语义。
3. `RunStepCancelled` 是生命周期观测事件，不是要求 TUI 缓存的控制信号。
4. Step 被用户取消后，Run 可以为了完成 drain/seal 在领域状态上进入 `Completed`；该 `Completed` 事件必须携带 `user_cancelled_step=true`，Runtime 对外投影为 `Cancelled`，不得投影为 `DoneWithDuration`。
5. 实时路径与 Resume 路径必须等价：实时使用 Runtime 汇总后的取消终态，Resume 使用持久化 `FinalizeCause::UserCancelledStep`，最终都渲染 `✻ Cancelled, ran <duration>`。
6. 一个 turn 只能发布一个用户可见终态。正常完成、用户取消、失败/终止互斥，不得先发取消再发正常完成。

### 9.1 `RunDomainEvent` 全量职责

| Event | 作用 | 是否用户可见终态 |
|---|---|---|
| `Transitioned` | 记录 Run 状态迁移及原因，供审计/诊断 | 否 |
| `Started` | Run 已启动 | 否 |
| `StepStarted` | 新 RunStep 已激活 | 否 |
| `StepCompleted` | RunStep 正常完成 | 否 |
| `StepCancellationRequested` | Step 取消请求已受理 | 否 |
| `StepFinalizationStarted` | Step 开始持久化与 Tool 收敛 | 否 |
| `StepCancelled` | Step 取消收口完成，`confirmed` 区分确认/未确认 | 否；仅生命周期观测 |
| `DrainingInput` | Run 等待或排空后续输入 | 否 |
| `TerminationRequested` | Run 终止请求已受理 | 否 |
| `Terminated` | Run 因退出、信号或父 Step 取消而终止 | 是，由 Runtime 投影 |
| `CancellationRequested` | 整个 Run 取消请求已受理 | 否 |
| `AwaitingUser` | Run 等待结构化用户交互 | 否 |
| `Resumed` | 用户交互完成，Run 恢复 | 否 |
| `StuckDetected` | StuckGuard 发现阻塞 | 否；投影诊断消息 |
| `Completed` | Run drain/seal 完成；`user_cancelled_step` 保留本轮终态原因 | 是，由 Runtime 根据原因投影 |
| `Failed` | Run 失败 | 是，由 Runtime 投影错误终态 |
| `Cancelled` | 整个 Run 取消收口完成 | 是，由 Runtime 投影取消终态 |

### 9.2 Runtime / SDK / TUI 事件职责

| 层 | Event | 作用 |
|---|---|---|
| Runtime 出站 | `DoneWithDuration` | 权威正常完成终态，包含 turn context 与耗时 |
| Runtime 出站 | `Cancelled` | 权威用户取消终态，包含 turn context 与耗时；既覆盖整 Run 取消，也覆盖 Step 取消后 drain/seal |
| Runtime 出站 | `RunStarted` / `RunCancelling` / `RunCancelled` | Run 生命周期观测与控制 ACK；不得用于 TUI 推导 terminal notice |
| Runtime 出站 | 领域事件投影 | `RunStep*`、drain、interaction、termination 等结构化观测 |
| SDK | `DoneWithDurationMs` | `DoneWithDuration` 的毫秒传输形式 |
| SDK | `Cancelled` | Runtime 权威取消终态的公开传输形式 |
| SDK | `RunStepCancelled` 等 | 生命周期 Published Language，保留 identity 给调试或其他客户端，不要求 TUI 关联 |
| SDK | `SessionResumed` | Context 持久化历史投影；每个 Step 带 `finalize_cause` 与 `duration_ms` |
| TUI | `TuiRuntimeEvent::Done` | 仅渲染正常完成提示并结束 processing |
| TUI | `TuiRuntimeEvent::Cancelled` | 仅渲染取消提示并结束 processing |
| TUI | `TuiRuntimeEvent::Run/RunStep` | 可选生命周期投影，不拥有终态语义，不改变 terminal cause |

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
- `RuntimeContext` 只能由统一 factory 构造，且创建后能力不可替换。
- Tool execution context 与 progress sink 只在 factory 按 Run 绑定；`ToolInvocation` 不反向携带 Runtime callback，Loop 不保留 binding factory。
- `RuntimeContextParts`、多套 assembler 和 Runtime 内第二 Composition Root 均退役。
- `Run` 与 `RunExecutionState` 无重复状态所有权。
- Workspace、Prompt、Skills、Config 均按本文生命周期边界流动。
- Step 取消后 Run 即使 drain/seal 为 `Completed`，Runtime 也只发布一个 `Cancelled` 用户可见终态；TUI 不缓存或关联 Run/Step identity 来推导提示。
- 每层装配、状态转换、能力不扩权和端到端场景都有相邻契约测试。
