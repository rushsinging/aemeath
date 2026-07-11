# 统一语言（Ubiquitous Language）

> 层级：01-system（系统级总体设计）
> 状态：Target（目标术语体系）｜Milestone：v0.1.0
> 本文定义跨 BC 通用的核心术语。**只描述目标术语与定义，不记录当前代码命名。** 术语在 BC 之间跨界时，经端口翻译（见 [03-context-map.md](03-context-map.md) 的 ACL / PL）。

## 0. 命名总则

- 术语首先服务**领域表达**。
- 同名不同义的术语（如领域 `Message` 与 provider 线格式消息）**必须**经 ACL 隔离，禁止跨界直接复用。

## 1. Agent Execution（核心域）

| 术语 | 定义 |
|---|---|
| **AgentRun** | 一次用户输入（或父 AgentRun / 编排器）触发的**一轮 agent 推进**，包含多个 Turn，直到完成 / 失败 / 取消 / 等待用户。系统唯一的领域状态机，**内存态、不持久化**。 |
| **Turn** | AgentRun 内的一次「模型调用 → 应用响应 →（可选）工具执行」往返。 |
| **Model Invocation** | 一次具体的 LLM 调用（请求 + 流式响应 + usage）。 |
| **Tool Call** | 一次工具调用。双 ID：领域 `ToolCallId`（UUIDv7）+ provider 边界标识。 |
| **Loop Engine** | 驱动 AgentRun 前进的 ReAct 循环骨架（推理 → 行动 → 观察 + 停止条件），Main 与 SubAgent 共用。 |
| **SubAgent** | 由父 AgentRun 创建的子 AgentRun，共用同一状态机与 Loop，差异由 `ExecutionPolicy` 表达。 |
| **ExecutionPolicy** | 表达 Main / SubAgent 差异的策略：输入源、交互能力、轮次上限、timeout、结果出口。 |
| **Interaction** | AgentRun 执行中断、等待外部（人）决策、再恢复的**用例族**（非 BC）：ask_user / 权限审批 / plan mode / pause-resume。对应状态 `AwaitingUser` / `AwaitingToolApproval`。 |

### AgentRun 状态机（内存态）

```
Created → PreparingContext → InvokingModel → ApplyingResponse
        → AwaitingToolApproval → ExecutingTools → (下一 Turn)
        → AwaitingUser（暂停，内存存活，不落盘）
        → Compacting → Finishing → Completed / Failed / Cancelled
```

> 崩溃后不恢复中间状态；用户重新发起即新建 AgentRun。

## 2. Workflow / Orchestration（核心域）

| 术语 | 定义 |
|---|---|
| **Reasoning Node** | reasoning graph 的阶段节点：IDLE / EXPLORE / PLAN / EXECUTE / VERIFY，用于调节 effort。 |
| **Reasoning Level** | 统一的推理强度抽象：Off / Low / Medium / High / Xhigh / Max，经三层 clamp（graph.desired ∩ provider.max ∩ user.max）。静态阈值归 Config。 |
| **Workflow Graph** | 多-agent 图编排（node / edge / state / checkpoint），v0.2.0 目标。 |

## 3. Context Management（支撑域）

| 术语 | 定义 |
|---|---|
| **Session** | 用户协作会话**容器**，持有对话历史（ChatChain）、workspace、tasks 快照、元数据，跨多次用户输入。**数据聚合，非状态机**。 |
| **ChatChain** | Session 内的对话历史链，由多个 ChatSegment 组成（compact 产生新段）。 |
| **ChatSegment** | 对话历史的一个压缩段。 |
| **Context Window** | 单次 Model Invocation 实际喂给模型的上下文（历史 + 注入记忆 + 提示装配后的结果）。 |
| **Compact** | 压缩历史以回收 token 的能力族：auto-compact（整链）/ micro-compact（陈旧工具结果）/ snip（历史级回收）。 |
| **Token Budget** | 上下文 token 预算估算与决策。 |
| **Memory Injection** | 把 Memory 检索结果注入 Context Window 的动作。 |
| **Prompt / Guidance** | 系统提示与按模型前缀匹配的 guidance 装配。 |

## 4. Memory（支撑域）

| 术语 | 定义 |
|---|---|
| **Memory Entry** | 一条持久化记忆，带 Layer（global / project）与 archive 状态。 |
| **Reflection** | 反思引擎：跑独立 LLM 调用，产出 Memory Suggestion（记忆建议）。 |
| **Memory Suggestion** | Reflection 产出的候选记忆。 |

## 5. Task Management（支撑域）

| 术语 | 定义 |
|---|---|
| **Task** | 任务聚合根：状态机 pending→in_progress→completed，含依赖（blocked_by）。类型是 Task BC 的 Published Language。 |
| **Batch** | 一批相关任务（任务列表）。 |
| **Task Snapshot** | Task 的可持久化快照（内嵌 Session 落盘）。 |

## 6. Project / Workspace（支撑域）

| 术语 | 定义 |
|---|---|
| **Workspace** | worktree 工作区上下文，单一可变状态源。 |
| **Workspace Frame** | 工作区上下文栈的一帧（进入 / 退出 worktree）。 |

## 7. 通用域术语

| 术语 | 定义 | 所属 BC |
|---|---|---|
| **Message** | 领域对话消息（role + content + tool calls）。**与 provider 线格式经 ACL 隔离**。 | Agent Execution / Context Management（Shared Kernel） |
| **Provider** | LLM 供应商适配器，内部 ACL 吸收各家差异。 | Provider |
| **Policy Decision** | 工具执行前的权限判断结果。 | Policy |
| **Audit Event** | 审计事件（执行 / 成本 / 用量）。 | Audit |
| **Cost / Usage** | 成本与 token 用量追踪，含 pricing。 | Audit |
| **Hook** | 生命周期钩子脚本。 | Hook |
| **Config Snapshot** | 只读配置快照（Config 的 Published Language）。 | Config |
| **ID（UUIDv7）** | 领域标识 newtype。 | 全域（Shared Kernel） |

## 8. 术语辨析（易混淆）

| A | B | 区别 |
|---|---|---|
| **Session** | **AgentRun** | Session=长生命周期数据容器（对话历史）；AgentRun=单次执行的状态机（内存态）。一个 Session 含多个 AgentRun。 |
| **Turn** | **Model Invocation** | Turn=一次「调模型 + 用响应 + 执行工具」往返；Model Invocation=其中那一次具体的 LLM 调用。一个 Turn 通常含一次 Model Invocation。 |
| **领域 Message** | **provider 线格式消息** | 前者是领域内部模型；后者是各家 API 的传输格式。经 Provider 内部 ACL 转换，禁止跨界直用。 |
| **Reasoning Node** | **AgentRun 状态** | 前者是 effort 调节状态机（Workflow）；后者是执行生命周期状态机（Agent Execution）。职责不同，不可混淆。 |
| **Memory Injection** | **Memory Entry** | 前者是"注入动作"（Context Management）；后者是"记忆数据"（Memory）。 |

## 9. 相关文档

- 产品与子域：[01-product-and-domain.md](01-product-and-domain.md)
- 集成关系与端口：[03-context-map.md](03-context-map.md)
- 系统架构：[04-system-architecture.md](04-system-architecture.md)
- 依赖规则：[05-dependency-rules.md](05-dependency-rules.md)
- 目录总览：[../README.md](../README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：核心术语表、AgentRun 状态机、术语辨析 | #760 |
| 2026-07-11 | 改为纯目标态（移除"当前代码命名 / 迁移说明"列）、文档引用链接化、新增修改历史 | #760 |
