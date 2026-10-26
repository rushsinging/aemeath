# 统一语言（Ubiquitous Language）

> 层级：01-system（系统级总体设计）
> 状态：Target（目标术语体系）｜Milestone：v0.1.0
> 本文定义跨 BC 通用的核心术语。**只描述目标术语与定义，不记录当前代码命名。** 术语在 BC 之间跨界时，经端口翻译（见 [03-context-map.md](03-context-map.md) 的 ACL / PL）。

## 0. 命名总则

- 术语首先服务**领域表达**。
- 层级对齐业界成熟模型：`Session → Run → Run Step`（≈ OpenAI Assistants API 的 `Thread → Run → Run Step`）。
- 同名不同义的术语（如领域 `Message` 与 provider 线格式消息）**必须**经 ACL 隔离，禁止跨界直接复用。

## 1. Agent Runtime（核心域）

| 术语 | 定义 |
|---|---|
| **Run** | 一次由用户输入（或父 Run 派生 SubAgent）触发的**一轮 agent 执行**，包含多个 Run Step；可在 `AwaitingUser` 暂停并由匹配答复恢复，最终进入完成 / 失败 / 取消之一。全系统唯一的 **Agent 执行生命周期状态机**，**内存态、不持久化**。标识 `RunId`。 |
| **Run Step** | Run 内的一次「模型调用 → 应用响应 →（可选）工具执行」往返。 |
| **Model Invocation** | 一次具体的 LLM 调用（请求 + 流式响应 + usage）。 |
| **Tool Call** | 一次工具调用。双 ID：领域 `ToolCallId`（UUIDv7）+ provider 边界标识。 |
| **Loop Engine** | 驱动 Run 前进的 ReAct 循环骨架（推理 → 行动 → 观察 + 停止条件），Main Agent 与 SubAgent 共用。 |
| **Main Agent** | 发起顶层 Run（**Main Run**）的主体，由用户输入直接触发，拥有完整交互（可 ask_user）与工具能力，是 SubAgent 的父级。 |
| **SubAgent** | 由 Main Agent（或另一 SubAgent）经工具派生的子执行主体，其执行是一个 **Sub Run**；共用同一状态机与 Loop，差异由 `ExecutionPolicy` 表达（受限交互、独立轮次 / timeout、结果回传父级）。 |
| **ExecutionPolicy** | 表达 Main Agent / SubAgent 差异的策略：输入源、交互能力、轮次上限、timeout、结果出口。 |
| **Interaction** | Run 执行中断、等待外部（人）决策、再恢复的**用例族**（非 BC）：ask_user / 权限审批 / pause-resume。对应状态 `AwaitingUser` / `AwaitingToolApproval`。 |
| **Activity** | Runtime 为观察执行过程而维护的**应用层观测实体集合**；每个 Activity 有稳定身份、父子关系、类型、状态、细节与计时，但不拥有 Run 的执行决策、不构成新的聚合根，也不替代 Run 状态机。 |
| **Activity Observation** | 一次由 `ActivityCoordinator` 创建或变更的完整 typed 事实，描述一个 Activity 在某个 revision 的状态与 detail；是 Runtime → SDK 的发布输入，不是领域命令。 |
| **Activity Snapshot** | 某个 Run 当前 Activity 事实集合及 revision 的一致性快照；用于 TUI 初始化、丢帧修复与重连，不是持久化的 Run checkpoint。 |
| **Activity Change** | Activity 的增量变化（Started / Updated / Finished）；按 Run 维度单调递增 revision 发布，消费者遇到 gap 时必须等待 Snapshot 修复，不能猜测中间状态。 |
| **Activity Summary** | TUI 从 Activity 事实镜像按用户受众和展示优先级派生的低噪声单行摘要；它是纯视图，不是 Runtime 事实或第二状态源。 |

### Run 状态机（内存态）

```text
Idle → DrainingInput ⇄ AwaitingInput
              │ Ready / InternalContinuation
              ▼
      PreparingContext ⇄ Compacting
              │
              ▼
        InvokingModel → ApplyingResponse
                              ├─ tool calls → AwaitingToolApproval → ExecutingTools
                              ├─ interaction → AwaitingInteraction → typed continuation
                              └─ end turn → FinalizingStep → DrainingInput

CancelRunStep: active Step → CancellingStep → FinalizingStep → DrainingInput
TerminateRun: 任意非终态 → Terminating → Terminated
失败: fatal invocation / unavailable interaction / finalization error → Failed
唯一终态: Completed / Failed / Terminated
```

> 崩溃后不恢复中间状态；用户重新发起即新建 Run。迁移期 `Cancelling → Cancelled` 仅是兼容输入，不属于目标状态机；交付层可以无损接纳兼容状态事实，但不得据此复制另一套执行生命周期或用户可见终态。

## 2. Workflow（支撑域）

> Workflow 是独立 Bounded Context，拥有 Session 级工作流 Graph；只编排 Graph 节点，不编排 Run，不调用 LLM、不执行 Tool。

| 术语 | 定义 |
|---|---|
| **Workflow** | Session 级独立聚合（0..n，同一时刻至多一个 Active）：工作流 Graph 的定义、状态与节点结论的拥有者。仅经五个始终可见的 `workflow_*` Tool（`workflow_create` / `workflow_list` / `workflow_query` / `workflow_update` / `workflow_node_update`）与 Runtime 协作；不创建 Run、不驱动 Run 生命周期，不从属 Runtime Goal。用户不直接操作 Workflow，只有 LLM 经 Tool 做语义操作。 |
| **Graph Definition** | Workflow 拥有的图定义：`objective`、`nodes`、`edges`、`initial_nodes`、`failure_routing` 的结构化提案与已提交 revision；允许 DAG、回边、自环、Join 与并行 `active_nodes`。 |
| **Node** | Graph 中的工作单元，带状态与节点结论（node conclusion）/证据引用；节点结论、证据与移动提议只能由 LLM 经 `workflow_node_update` 提交，Kernel 只做结构 / revision / 证据存在校验。**不是** effort 阶段节点。 |
| **Edge** | 节点间的条件迁移；回边与自环是普通 Edge，**Loop 不是 Runtime 类型，也不是特殊 Graph 节点**。 |
| **Failure Routing** | Graph 定义内声明的失败出口：节点执行失败时的目标路由，由 Graph Kernel 判定，不由 Runtime 计算。 |
| **ConfirmNode** | Graph 中表达“该节点存在需要用户确认的条件”的通用语义节点：LLM 自行用普通文本或 AskUser 完成确认并经 `workflow_node_update` 保存通过与证据；节点不驱动 Runtime / UI，Kernel 不理解确认语义。 |
| **WorkflowStatus** | Workflow 生命周期状态四种已知行为：`Active` / `Inactive` / `Completed` / `Cancelled`（四并列还是 Open / Closed 分层尚未拍板）；`Inactive` 可被 LLM 重新激活，`Completed` / `Cancelled` 不可逆仅历史查询；`Suspended` / `AwaitingUser` / `Sleeping` 不是 Workflow 状态。离开 `Active`（deactivate / complete / cancel）后 Session `active_workflow` 指针清空。 |
| **Runtime Goal** | Runtime 拥有的跨 Run 长任务聚合根（objective、执行模式、继续授权、run history），管理跨 Run 继续策略；与 Workflow 互不从属。 |
| **Reasoning Level** | 统一的推理强度抽象：Off / Minimal / Low / Medium / High / Xhigh / Max；`none` 仅是 Off 的 OpenAI wire alias。**与 Workflow 解耦**：只从配置 / 用户命令静态冻结，Provider 按 driver capability 独立向下 clamp。 |

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

## 4. Tool & Skill & Command（支撑域）

| 术语 | 定义 |
|---|---|
| **Tool** | 模型发起的函数或外部能力调用；经 Tool Catalog 发现、经 Tool Execution 执行。 |
| **Registry Scope** | 一次 Run 实际装配的 Tool 与资源集合，回答“有什么”。 |
| **Tool Profile** | 允许的 Tool Capability 集合，回答“能用什么”；只能收缩 Scope，不能扩权。 |
| **Tool Capability** | Tool 执行所需的安全能力标签，例如读写工作区、执行进程、用户交互或 Agent Dispatch。 |
| **Tool Outcome** | Tool 调用的领域结果：Success / Failure / Cancelled / Suspended；Suspended 表示完成同一调用前需要 Runtime 协调外部交互，其他结果包含模型可见内容、结构化数据与安全错误分类。 |
| **Skill** | 由模型按名称调用、在调用时动态加载正文的特殊 Tool；具体 Skill 以廉价元数据被发现，但不各自注册 Tool schema。 |
| **Skill Descriptor** | 可发现的 Skill 元数据：稳定 identity、描述、identity/slash aliases 与可选参数提示；**NEVER** 携带正文。 |
| **Skill Catalog Snapshot** | 按稳定顺序发布的 Skill Descriptor 全量快照及确定性 revision；同一快照同时派生 slash route 与客户端补全。 |
| **Skill Request** | 用户请求模型使用某个 Skill 的 typed 入站意图；携带 `InputId`、canonical identity、原始参考参数与 `raw_input`，不构造 Tool Call。Runtime 将它与普通 UserMessage 一起接纳为 `AcceptedUserInput`，但为模型生成内部 Skill 请求消息、为交付层保留 `raw_input`，二者不得互相替代或通过正文反向解析。 |
| **Loaded Skill** | `SkillLoadPort` 在 Skill Tool 调用时按 identity 读取的单个 Skill 正文、来源与内容 revision。 |
| **Slash Command** | 用户发起的 slash 输入；Skill 入口分类为 SkillRequest，普通 Command 按 SnapshotQuery / ApplicationControl 确定性路由。 |
| **MCP Tool** | 经 MCP adapter 与 ACL 转换为统一 Tool 语义的外部工具；MCP 不是独立 BC。 |

## 5. Memory（支撑域）

| 术语 | 定义 |
|---|---|
| **Memory Entry** | 一条持久化记忆，带 Layer（global / project）与 archive 状态。 |
| **Reflection** | 反思引擎：跑独立 LLM 调用，产出 Memory Suggestion（记忆建议）。 |
| **Memory Suggestion** | Reflection 产出的候选记忆。 |

## 6. Task Management（支撑域）

| 术语 | 定义 |
|---|---|
| **Task** | 任务聚合根：状态机 pending→in_progress→completed，含依赖（blocked_by）。类型是 Task BC 的 Published Language。 |
| **Batch** | 一批相关任务（任务列表）。 |
| **Task Snapshot** | Task 的可持久化快照（内嵌 Session 落盘）。 |

## 7. Project / Workspace（支撑域）

| 术语 | 定义 |
|---|---|
| **Workspace** | worktree 工作区上下文，单一可变状态源。 |
| **Workspace Frame** | 工作区上下文栈的一帧（进入 / 退出 worktree）。 |
| **Project Identity** | Project 发布的稳定项目身份：canonical initial cwd + optional canonical git common dir；Session resume 与项目级 Memory 分区以它为准，普通非 git 目录合法。 |
| **Workspace ID** | Project 发布的当前 canonical workspace root 标识；由 Project Identity + root 确定，进入 / 退出 worktree 后可变化。 |

## 8. 通用域术语

| 术语 | 定义 | 所属 BC |
|---|---|---|
| **Message** | 领域对话消息（role + content + tool calls）。**与 provider 线格式经 ACL 隔离**。 | Agent Runtime / Context Management（Shared Kernel） |
| **Accepted User Input** | Runtime 在 Session gate 接纳后、绑定 Run Step 前的唯一 typed 用户输入事实；穷举 `UserMessage { input_id, text, images }` 与 `SkillRequest { input_id, skill, arguments, raw_input }`。它统一 admission、FIFO、drain、freeze、持久化与 adoption 生命周期；**NEVER** 并行维护 message/event 两套 adopted 数据，也 **NEVER** 从模型正文重建 typed intent。 | Agent Runtime |
| **Provider** | LLM 供应商适配器，内部 ACL 吸收各家差异。 | Provider |
| **Policy Decision** | 工具执行前的权限判断结果。 | Policy |
| **Audit Event** | 不可变审计事实；v0.1.0 仅发布 Model Usage metadata。 | Audit |
| **Usage** | 成功 logical Model Invocation 的 provider-neutral token 用量事实，带 Session/Run/RunStep/Invocation 关联 ID。 | Audit |
| **Cost / Pricing** | 从 Usage 派生的 Future 能力；v0.1.0 不定义 Price、Cost 或迁移语义。 | Audit（Future） |
| **Hook** | 生命周期钩子脚本。 | Hook |
| **Config Snapshot** | 只读配置快照（Config 的 Published Language）。 | Config |
| **Domain ID** | 由所属 BC 发布的强类型标识；需要全局时间有序的新实体可采用 UUIDv7，但格式不是全域 Shared Kernel。TaskId / BatchId 在 v0.1.0 是单 Session 十进制标识，WorkspaceId 是 Project 派生的 opaque 标识。 | 各 BC Published Language |

## 9. 术语辨析（易混淆）

| A | B | 区别 |
|---|---|---|
| **Session** | **Run** | Session=长生命周期数据容器（对话历史）；Run=单次执行的状态机（内存态）。一个 Session 含多个 Run。 |
| **Run Step** | **Model Invocation** | Run Step=一次「调模型 + 用响应 + 执行工具」往返；Model Invocation=其中那一次具体的 LLM 调用。一个 Run Step 通常含一次 Model Invocation。 |
| **Main Agent** | **SubAgent** | 前者是用户输入直接触发的顶层执行主体（发起 Main Run）；后者是父级经工具派生的子执行主体（发起 Sub Run）。共用状态机与 Loop，差异在 ExecutionPolicy。 |
| **领域 Message** | **provider 线格式消息** | 前者是领域内部模型；后者是各家 API 的传输格式。经 Provider 内部 ACL 转换，禁止跨界直用。 |
| **Workflow Node** | **Run 状态** | 前者是 Graph 节点状态与节点结论（Workflow 拥有，含 `WorkflowStatus` 与 Graph State）；后者是执行生命周期状态机（Agent Runtime）。Runtime 只按 Session 最新状态组织上下文、路由 `workflow_*` Tool，不推进 Graph；职责不同，不可混淆。 |
| **Workflow** | **Runtime Goal** | Workflow 是 Session 级 Graph 聚合（0..n，至多一个 Active），管理 Node / Edge / State / Failure Routing 与节点结论；Runtime Goal 是跨 Run 的长任务聚合根，管理继续策略与 Run 归属。二者互不从属：Goal 生命周期不创建 / 销毁 / 切换 Workflow，Graph 回边与失败路由也不属于 Goal 的继续策略。 |
| **Activity** | **Run** | Run 是唯一 Agent 执行生命周期状态机；Activity 是由 Runtime 从 Run、Step、Model、Tool、Hook、Compact 与 Interaction 事实派生的观测集合。Activity 可以表达父子拓扑和局部计时，但不能推进、暂停或终止 Run。 |
| **Activity Observation** | **Activity Summary** | Observation 是 Runtime 发布的完整 typed 事实；Summary 是 TUI 按用户受众和优先级生成的低噪声展示行。前者跨边界传输，后者只存在于展示层。 |
| **Activity Snapshot** | **Activity Change** | Snapshot 是按 revision 的全量修复 / 初始化事实；Change 是同一 revision 序列中的增量事实。消费者发现 revision gap 时必须请求或等待 Snapshot，禁止用增量猜测缺失事实。 |
| **Memory Injection** | **Memory Entry** | 前者是"注入动作"（Context Management）；后者是"记忆数据"（Memory）。 |
| **Tool** | **Skill** | Tool 是模型调用能力的通用协议；Skill 是其中唯一稳定注册、按 identity 动态加载正文的特殊 Tool，具体 Skill 不各自发布 schema。 |
| **Registry Scope** | **Tool Profile** | Scope 决定本次 Run 装配了什么；Profile 决定其中哪些 capability 被允许。 |
| **Slash Command** | **Tool Call** | Slash Command 由用户发起并路由应用用例；Tool Call 由模型发起并调用函数。 |

## 10. 相关文档

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
| 2026-07-11 | 术语改名：Agent Execution→Agent Runtime、AgentRun→Run、Turn→Run Step；补 Main Agent 与 SubAgent 对照 | #760 |
| 2026-07-11 | Workflow 降为支撑域（第 2 节标题），移除不做的 Workflow Graph 编排术语 | #760 |
| 2026-07-28 | #1438 将 Skill 重新定义为由 LLM 按需调用的特殊动态 Tool，并增加 metadata snapshot、SkillRequest、SkillLoadPort 与 LoadedSkill 术语 | [#1438](https://github.com/rushsinging/aemeath/issues/1438) |
| 2026-10-07 | Workflow 术语同步 workflow/01-workflow-design.md 冻结决策：Session 0..n Workflow、四状态（层级待决）、五个 `workflow_*` Tool、ConfirmNode 最小语义；移除 `WorkflowRuntimeView` / `WorkflowBindingSnapshot` / tool policy 旧口径 | workflow/01-workflow-design.md |
| 2026-07-12 | 新增 Tool/Skill/Command 统一语言，明确 Scope/Profile、Prompt Fragment 与 MCP Tool 边界 | #787 |
| 2026-07-12 | 将 Run 精确为唯一 Agent 执行生命周期状态机，避免与其他 BC 局部聚合状态机冲突 | #743 / #787 |
| 2026-07-14 | 新增 Project Identity / Workspace ID，统一 Session resume、Memory 分区与 Tool Scope 的身份语言 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-08-01 | 增加 Activity、Activity Observation、Activity Snapshot、Activity Change 与 Activity Summary 统一语言，并明确其与 Run 的边界 | 统一 Activity 观测 |
