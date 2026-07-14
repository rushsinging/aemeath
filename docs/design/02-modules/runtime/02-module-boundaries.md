# Agent Runtime · 模块边界

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> 本文定义 Agent Runtime 内部的模块划分、各模块的状态所有权、消费的 Port 与依赖方向。**只描述目标态**；与现状（两套 loop、三层 Context 重叠）的差距记入 `03-engineering/migration-governance`。

## 1. 内部模块总览（8 个）

```
                        api（入站适配器实现 + 装配入口）
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

## 2. 各模块职责

### agent_run（模块核心）
- **状态所有权**：`Run` 聚合、`RunStatus` 状态机、Run Step / Tool Call 实体
- **用例**：`start_run` / `resume_run`(ask_user 后) / `cancel_run` / `derive_sub_run`
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
- 消费：`ContextPort`（Context Management BC）、`MemoryPort`
- **注**：Session 对话历史属 Context Management，本模块只是 Runtime 侧调用协调。**Memory 边界**：检索归 Memory（`MemoryPort.retrieve`），注入进 Context Window 归 Context Management——记忆本体是独立 BC，不是 Context 的一部分

### interaction
- **职责**：处理执行中断——`AwaitingUser`（ask_user）、`AwaitingToolApproval`（权限门）、pause/resume
- 消费：`InteractionPort`（UI 交互）、`PolicyPort`（权限判断）
- 触发 Run 状态机迁移到 `AwaitingUser`/`AwaitingToolApproval`

### event_projection（横切）
- **职责**：领域事件 → SDK `ChatEvent`；**Main/Sub 路由与命名**（Main→TUI，Sub→父 Run）；补 `agent_id`（#612 缺口）
- 消费：`EventSink`

### model_invocation 的 Usage 出口
- Provider 返回 RawUsageSnapshot 后，model_invocation 构造带 SessionId / RunId / RunStepId / ModelInvocationId 的 UsageRecord
- 经 `UsageSink.try_record` 非阻塞提交；Audit 接受/丢弃均不改变 Run 状态

### api（入站适配器实现）
- **职责**：实现入站端口 `AgentClient`（OHS + PL）；`RuntimeContext` 装配入口（含入站 `InputBuffer`）；SubAgent 派生时装配子 RuntimeContext
- **注**：真正的生产装配收敛在 Composition Root（见 `06-ports-and-adapters`），api 模块只做 Runtime 内的接线

### Runtime / Hook 边界（跨模块）
Hook 是通用域 BC，Runtime 经 `HookPort` 消费——**Hook 判定，Runtime 编排**：

| | 拥有 |
|---|---|
| **Hook BC** | subscription 匹配、稳定顺序、脚本执行/回收、3 次执行故障重试、输出解析与类型化 directive |
| **Runtime** | 触发时机（UserPromptSubmit/Stop/PreToolCall/PostToolCall/SubRunStart-Stop/Notification）+ directive 响应编排；Stop 阻断累计 15 次后第 16 次 RunFailed |

触发点分布：loop_engine（Stop）、tool_coordination（Pre/PostToolCall）、agent_run（SubRunStart/Stop）。实现差距见 Migration Governance。

## 3. 状态所有权矩阵

| 状态 | 所有者模块 | 说明 |
|---|---|---|
| Run 聚合 / RunStatus 状态机 | **agent_run** | 唯一状态机 |
| Run Step / Tool Call 实体 | agent_run（Run 聚合内）| |
| StuckGuard 计数（stall/fuse）| loop_engine | 循环级 |
| ToolCall 双 ID 映射表 | tool_coordination | 运行时映射 |
| Context Window（临时）| context_coordination | 每轮构建 |
| RuntimeContext（活资源）| 由 api/派生逻辑装配，**流经各模块作参数** | 不属任何模块的持久状态 |
| InputBuffer 入站缓冲（追问排队）| loop_engine（经 RuntimeContext 注入）| Main 忙期排队；Sub 固定队列 |

## 4. 依赖方向（Clean）

```
api → agent_run → loop_engine → {model_invocation, tool_coordination,
                                        context_coordination, interaction} → *Port
event_projection：被各模块调用（emit），不反向依赖业务
```

- **MUST** 依赖只向内（api 最外，Port 最外侧适配器）
- **MUST NOT** coordinators 之间互相依赖（都经 loop_engine 编排）
- **MUST NOT** 任何模块私自 `new` Port 实现（经 RuntimeContext 注入）

## 5. 与现状的收敛方向（迁移提示）

现状两套 loop（`process_chat_loop` / `SubAgentRun::run_loop`）→ 收敛为单一 `loop_engine`；三层重叠 Context（`ChatRuntimeContext`/`RuntimeResources`/`ChatLoopContext`/`TuiLaunchContext`）→ 收敛为单一 `RuntimeContext`。详细迁移步骤见 `03-engineering/migration-governance`（S5 执行）。

## 6. 相关文档

- 领域模型：[01-domain-model.md](01-domain-model.md)
- 状态机与 Loop：[03-loop-and-state-machine.md](03-loop-and-state-machine.md)
- 防 stuck：[04-stuck-prevention.md](04-stuck-prevention.md)
- 端口与装配：[06-ports-and-adapters.md](06-ports-and-adapters.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：8 个内部模块划分、状态所有权、依赖方向、收敛方向 | #761 |
| 2026-07-11 | agent_execution→agent_run；loop_engine 补 InputBuffer 门禁+HookPort；tool 补 HookPort；补 Memory 边界、InputBuffer 状态、Runtime/Hook 边界子节 | #761 |
| 2026-07-11 | model_invocation 补错误重试职责（Retryable 退避 / context 超限 compact / Fatal fail）+ ModelInvocationRetrying | #761 |
| 2026-07-11 | 重试收敛为 T0-T1 退避（≤10 次/5 分钟封顶），去掉 T2 降级/T3 故障转移 | #761 |
| 2026-07-12 | tool_coordination 对齐 Catalog/Execution 双端口及 Runtime/Tool BC 职责分工 | #787 |
| 2026-07-12 | model_invocation 对齐 ProviderCompletion、RawUsageSnapshot 与可见输出重试门禁 | #788 |
| 2026-07-12 | Hook 边界补单端口与 3/15 两层重试；Usage 从 event_projection 分离到 model_invocation→UsageSink | #790 |
