# Agent Runtime · 模块边界

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 Agent Runtime 内部的模块划分、各模块的状态所有权、消费的 Port 与依赖方向。**只描述目标态**；Current → Target 差距只见 [Migration Governance](../../03-engineering/03-migration-governance.md)。

## 1. 内部模块总览（8 个）

```
                  agent_client（稳定入站能力）
                          │
                          ▼
              agent_run（Run 聚合 + 状态机 + 用例编排）
                          │
                          ▼
                 loop_engine（统一 ReAct 循环骨架 + StuckGuard）
        ┌────────────┬─────────┴──────────┬──────────────┐
        ▼            ▼                     ▼              ▼
 model_invocation  tool_coordination  context_coord.   interaction
        │            │                     │              │
        ▼            ▼                     ▼              ▼
   ProviderPort   ToolCatalogPort +  ContextPort   InteractionPort/PolicyPort
                  ToolExecutionPort
        │            │                     │              │
        └────────────┴─── event_projection（横切：领域事件 → SDK ChatEvent）
```

## 2. 物理目录与能力边界

仓库级 `agent/features/*` 已按业务 Feature / Bounded Context 形成垂直切片；Runtime 内部又包含八个具有独立词汇、变化原因、状态所有权或测试边界的稳定能力，因此 **MUST** 继续按能力递归竖切。递归竖切不把每个子能力升级为 BC 或 crate，也不要求每个叶子复制 `domain/application/ports/adapters` 横向模板：叶子有共享领域不变量时才引入 model，有真实边界 seam 时才引入就近 Port/adapter，其余保持扁平。

```text
agent/features/runtime/src/
├── lib.rs                         # 窄 façade
├── agent_client.rs                # 入站命令路由与用例入口
├── agent_run.rs                   # Run 聚合、RunStatus、RunSpec、Run Step
├── agent_run/
│   ├── state.rs
│   ├── step.rs
│   └── event.rs
├── loop_engine.rs                 # ReAct 骨架与能力 façade
├── loop_engine/
│   ├── drive.rs
│   └── stuck_guard.rs
├── model_invocation.rs            # 模型调用编排；ProviderPort 就近归属
├── model_invocation/
│   └── retry.rs
├── tool_coordination.rs           # Tool 编排；Tool/Policy/Hook seam 就近归属
├── tool_coordination/
│   └── approval.rs
├── context_coordination.rs        # Context Window 编排；消费 ContextPort
├── interaction.rs                 # typed continuation；InteractionPort 就近归属
├── event_projection.rs            # 扁平 ACL：DomainEvent → SDK ChatEvent
└── runtime_context.rs             # 跨能力传递的活资源容器，不是通用 shared 层
```

组织与依赖规则：

- `agent_run` 拥有 `Run`、`RunStatus`、`RunSpec`、领域事件与状态迁移；其领域模型 **NEVER** 依赖 Loop、Port、adapter 或具体技术类型。
- `loop_engine` 只经各能力 façade 协调 `model_invocation`、`tool_coordination`、`context_coordination` 与 `interaction`；coordinator 之间 **NEVER** 穿透内部实现。
- Runtime-owned Port **MUST** 靠近实际消费能力；只有多个稳定 Port 确需独立导航时才 **MAY** 建聚合入口，**NEVER** 为目录对称建立全局 `ports/` 层。
- Runtime-owned adapter 靠近对应 seam 或投影能力；Provider、Storage、Tool 等 Feature 的生产实现仍由各自 Feature 提供，**NEVER** 搬入 Runtime。
- `RuntimeContext` 是跨能力传递的活资源容器，不是类型垃圾桶；有明确语义所有者的类型 **MUST** 留在对应能力，**NEVER** 用通用 `shared/` 规避循环依赖。
- Runtime 当前是内存态状态机与过程编排，没有独立读模型，也没有 HTTP delivery 端点，因此 **NEVER** 引入 CQRS-lite 或 REPR；未来证据变化时按系统级代码组织规范重新评估。
- `lib.rs` 只导出真实外部消费者需要的窄 façade；各能力默认 crate-private。
- 具体实现选择、factory 调用与生产对象图连接全部位于 `agent/composition`；Runtime feature 内 **NEVER** 建立 `bootstrap/`、service locator 或第二个 Composition Root。
- 使用 Rust 2018+ `capability.rs` + `capability/...` 形状，**NEVER** 新增 `mod.rs`。

## 3. 各模块职责

### agent_run（模块核心）
- **状态所有权**：`Run` 聚合、`RunStatus` 状态机、Run Step / Tool Call 实体
- **用例**：`start_run` / `cancel_run` / `derive_sub_run`。`AwaitingUser` 由同一次 `run_loop` 调用的同一 future 在内部 `.await` typed continuation；reply 到达后原地继续，既不是 Loop 退出边界，也不会二次入栈
- **不持有** RuntimeContext（作为参数流转），**不直接调 Port**（经 loop_engine + coordinators）

### loop_engine（Main/Sub 共用）
- **职责**：ReAct 循环骨架（推理→行动→观察）+ 停止条件 + 步进 + 门禁 `InputBuffer.drain`（纳入追问）
- **内置 StuckGuard**（4 层防 stuck，见 `04-stuck-prevention`）——Main/Sub 统一获得保护
- **零分支**：Main/Sub 差异全在传入的 `RuntimeContext` + `RunSpec`
- 消费：入站 `InputBuffer`（drain 输入）、`HookPort`（Stop hook 时机——**判定归 Hook，重试编排归本模块**）
- 依赖：调度 model_invocation / tool_coordination / context_coordination / interaction

### model_invocation
- **职责**：调 `ProviderPort` 发起 LLM 调用、组装流式响应、提取 tool_calls、记录 `RawUsageSnapshot`；**退避重试**：仅在本 attempt 无可见 delta 已提交（或可原子回滚）时，对 Retryable(超时/5xx/429/流中断)指数退避重试（≤10 次，退避封顶 5 分钟），Fatal(4xx) 直接失败，context 超限→compact（详见 `03-loop` §5）
- **状态**：无（产出 `ModelInvocation` VO 交回 Run Step）；重试期 emit `ModelInvocationRetrying{attempt}`
- 消费：`ProviderPort`（返回 Retryable/Fatal 分类错误）、`ReasoningPort`（取 effort）

### tool_coordination
- **职责**：ToolCall 双 ID 映射（领域 `ToolCallId` ↔ provider_id）、Policy/Hook/审批、timeout/cancellation、多调用并发、结果回收与 Run Step 写入
- **内置 ToolLoopGuard**（工具循环熔断，StuckGuard L2）
- 消费：`ToolCatalogPort`（schemas + Scope/Profile 投影）、`ToolExecutionPort`（单次函数调用）、`PolicyPort`、`HookPort`
- Tool BC 在执行边界复核 Scope/Profile/schema；Runtime 保留调用编排控制权
- SubAgent 派生工具 → 触发 agent_run 的 `derive_sub_run`

### context_coordination
- **职责**：构建本轮 Context Window（取历史 + compact 家族 + memory 注入 + prompt/guidance 装配 + token budget）
- 消费：仅 `ContextPort`（Context Management BC）
- **注**：Session 对话历史与把 Memory 注入 Context Window 的流程都属 Context Management；本模块只调用 `ContextPort`，**NEVER** 旁路再检索 Memory。Memory 本体仍属独立 BC；Runtime 的后台 Reflection 编排通过 RuntimeContext 中同一个 `MemoryPort` Arc 检索 / apply，并通过 `ReflectionPromptPort` 做纯 prompt / parse / format。

### interaction
- **职责**：将 Tool suspension、Policy approval 或 hard pause 统一映射为 Runtime-owned `InteractionRequest`，保存 request id + typed continuation，处理 reply / cancellation 竞争
- 消费：`InteractionPort`（UI / parent-mediated request-reply seam）、`PolicyPort`（权限判断）
- 触发 Run 状态机迁移到 `AwaitingUser`/`AwaitingToolApproval`，且只有匹配 reply 才能恢复原 continuation
- **NEVER** 让 Tool adapter 直接等待 TUI channel，也不让 `InteractionPort` 自行发布 `RunResumed`

### event_projection（横切）
- **职责**：领域事件 → SDK `ChatEvent`；**Main/Sub 路由与命名**（Main→TUI，Sub→父 Run）；补 `agent_id`（#612 缺口）
- 消费：`EventSink`

### model_invocation 的 Usage 出口
- Provider 返回 RawUsageSnapshot 后，model_invocation 构造带 SessionId / RunId / RunStepId / ModelInvocationId 的 UsageRecord
- 经 `UsageSink.try_record` 非阻塞提交；Audit 接受/丢弃均不改变 Run 状态

### agent_client（入站能力）
- **职责**：实现入站端口 `AgentClient`（OHS + PL）；`RuntimeContext` 装配入口（含入站 `InputBuffer`）；SubAgent 派生时装配子 RuntimeContext
- **注**：真正的生产装配收敛在 Composition Root（见 `06-ports-and-adapters`），`agent_client` 只负责 Runtime 内的命令路由与接线；该名称表达稳定能力，**NEVER** 引入通用 `api/` 层

### Runtime / Hook 边界（跨模块）
Hook 是通用域 BC，Runtime 经 `HookPort` 消费——**Hook 判定，Runtime 编排**：

| | 拥有 |
|---|---|
| **Hook BC** | subscription 匹配、稳定顺序、脚本执行/回收、3 次执行故障重试、输出解析与类型化 directive |
| **Runtime** | 触发时机（UserPromptSubmit/Stop/PreToolCall/PostToolCall/SubRunStart-Stop/Notification）+ directive 响应编排；Stop 阻断累计 15 次后第 16 次 RunFailed |

触发点分布：loop_engine（Stop）、tool_coordination（Pre/PostToolCall）、agent_run（SubRunStart/Stop）。

## 4. 状态所有权矩阵

| 状态 | 所有者模块 | 说明 |
|---|---|---|
| Run 聚合 / RunStatus 状态机 | **agent_run** | 唯一 Agent 执行生命周期状态机 |
| Run Step / Tool Call 实体 | agent_run（Run 聚合内）| |
| StuckGuard 计数（stall/fuse）| loop_engine | 循环级 |
| ToolCall 双 ID 映射表 | tool_coordination | 运行时映射 |
| Context Window（临时）| context_coordination | 每轮构建 |
| RuntimeContext（活资源）| 由 agent_client / 派生逻辑发起装配，**流经各模块作参数** | 不属任何模块的持久状态 |
| InputBuffer 入站缓冲（追问排队）| loop_engine（经 RuntimeContext 注入）| Main 忙期排队；Sub 固定队列 |

## 5. 依赖方向（Clean）

```
agent_client → agent_run → loop_engine → {model_invocation, tool_coordination,
                                        context_coordination, interaction} → *Port
event_projection：被各模块调用（emit），不反向依赖业务
```

- **MUST** 依赖只指向稳定策略（agent_client 发起用例，外部 detail 实现相应 port）
- **MUST NOT** coordinators 之间互相依赖（都经 loop_engine 编排）
- **MUST NOT** 任何模块私自 `new` Port 实现（经 RuntimeContext 注入）

## 6. 迁移边界

本文的 Target 模块图与依赖规则是验收目标；源码现状、迁移顺序、责任与退出条件 **MUST** 只在 [Migration Governance](../../03-engineering/03-migration-governance.md) 维护，本文 **NEVER** 复制 Current 类型或进度。

## 7. 相关文档

- 领域模型：[01-domain-model.md](01-domain-model.md)
- 状态机与 Loop：[03-loop-and-state-machine.md](03-loop-and-state-machine.md)
- 防 stuck：[04-stuck-prevention.md](04-stuck-prevention.md)
- 端口与装配：[06-ports-and-adapters.md](06-ports-and-adapters.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：8 个内部模块划分、状态所有权、依赖方向、收敛方向 | #761 |
| 2026-07-14 | 移除 Target 文档中的 Current 类型清单，将迁移事实收口到 Migration Governance | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-15 | 明确 Runtime 内按八个稳定能力递归竖切；叶子按领域规则与真实 seam 证据引入 model/Port/adapter，生产装配留在 `agent/composition` | [#995](https://github.com/rushsinging/aemeath/issues/995) |
| 2026-07-11 | agent_execution→agent_run；loop_engine 补 InputBuffer 门禁+HookPort；tool 补 HookPort；补 Memory 边界、InputBuffer 状态、Runtime/Hook 边界子节 | #761 |
| 2026-07-11 | model_invocation 补错误重试职责（Retryable 退避 / context 超限 compact / Fatal fail）+ ModelInvocationRetrying | #761 |
| 2026-07-11 | 重试收敛为 T0-T1 退避（≤10 次/5 分钟封顶），去掉 T2 降级/T3 故障转移 | #761 |
| 2026-07-12 | tool_coordination 对齐 Catalog/Execution 双端口及 Runtime/Tool BC 职责分工 | #787 |
| 2026-07-12 | model_invocation 对齐 ProviderCompletion、RawUsageSnapshot 与可见输出重试门禁 | #788 |
| 2026-07-12 | Hook 边界补单端口与 3/15 两层重试；Usage 从 event_projection 分离到 model_invocation→UsageSink | #790 |
