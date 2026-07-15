# Agent 编排范式知识地图

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/358
>
> 本文档是 **知识储备** 类设计文档：整理 Agent 工程的几条主线（Context / Harness / Loop / Workflow / Graph）与业界取舍，并给出 Issue #358 的 PoC 评估框架。文档 **不直接约束代码**，也**不维护 Current 现状**；当前差距只见 [Migration Governance](03-migration-governance.md)，目标设计只见 `01-system` / `02-modules`。

## 1. 背景与目的

Issue #358 的历史动机是：开放式 Agent Loop 对不确定任务很合适，但高频确定流程若完全依赖 prompt，会带来成本、漂移与回归困难。本文不据此判断仓库当前哪条主线“成熟”或“空白”；Current 一律由 Migration Governance 给出，Workflow Target 见 [Reasoning Graph](../02-modules/workflow/01-reasoning-graph.md)。

纯 agent loop 在开放编程场景下是正确选择，但带来三个问题（见 Issue #358）：

1. **Token 成本与漂移**：高频路径靠 prompt 教模型走，token 贵、易漂移、偶尔不遵守。
2. **不可测试**：流程混在 LLM 行为里，无法对「流程正确性」做 CI 回归。
3. **细粒度可恢复性缺失**：暂停/恢复是会话级，无法在单轮 tool call 边界 checkpoint / 分叉重跑。

本文档的目的：

- **建立统一术语**：让后续讨论有共同语言，避免「workflow」「graph」「orchestration」混用。
- **保存历史调研背景**：只用于解释 Issue #358 当时为何提出，不作为当前代码事实。
- **给出决策框架**：对 Issue #358 的四个探索方向，列出触发条件、前置依赖与风险，**SHOULD** 交由 PoC 结果决定是否落地，**NEVER** 在此预先拍板。

## 2. Agent 工程的五条主线

这五条主线并非正交分类，而是观察 Agent 系统的五个切面，实际系统会在多条主线上同时演进。

### 2.1 Context Engineering（上下文工程）

- **核心问题**：在有限的上下文窗口内，向模型提供「当前任务最相关」的信息——系统提示、历史、工具结果、外部知识、用户意图。
- **关键手段**：
  - 分层 Prompt（system / guidance / user / tool）。
  - 上下文压缩（compact / summarization / truncation）。
  - 检索增强（RAG / memory / skill 注入）。
  - Prompt Contract：显式约定模型输入输出的结构契约。
- **代表实践**：Claude Code 的分层指令、Cursor 的 long context 管理、各类 compact 策略。

### 2.2 Harness Engineering（外壳工程）

- **核心问题**：把「模型能做什么」收敛为「模型被允许做什么」——工具集、权限、生命周期钩子、审计、沙箱。
- **关键手段**：
  - Tool trait + Registry（统一工具注册、schema、执行）。
  - 权限模型（policy / ask / allow / plan）。
  - 生命周期 Hook（pre/post tool use、stop）。
  - 审计（audit）与可观测性。
- **代表实践**：MCP（Model Context Protocol）的 tools/resources/prompts 三类能力、Claude Code 的 permission 系统。

### 2.3 Loop Engineering（循环工程）

- **核心问题**：如何把一次用户输入推进成多轮、带工具调用、可中断恢复的完整协作过程。
- **关键手段**：
  - 主循环（agent loop）：模型调用 → 解析工具调用 → 执行 → 回填 → 再调用。
  - 停止条件（stop reason）：`end_turn`、无工具调用、达到最大轮次、超时、取消。
  - 暂停 / 恢复 / 重试：cancel token、timeout、api-error 回流、防死循环。
  - 子代理（sub-agent）：把一段对话委托给独立上下文执行。

#### 2.3.1 ReAct：Agent Loop 的理论根基

**ReAct**（Reasoning + Acting，Yao et al. 2022）是现代 agent loop 的理论原型。其核心是把「推理」与「行动」交织成一个循环，每一步由模型显式输出三元组：

```
Thought（我该想什么）→ Action（我该调用什么工具）→ Observation（工具返回了什么）
```

ReAct 的贡献在于证明：**让模型边推理边行动**，比单纯 chain-of-thought 推理（只思考不行动）或单纯行动（只调工具不显式推理）更能解决复杂任务——推理为行动提供方向，行动的观察为推理提供新的事实。

#### 2.3.2 工程化形态：function calling 时代的 agent loop

原始 ReAct 要求模型显式生成 `Thought:` 文本块，依赖提示工程解析。现代实现把它工程化为 **function calling**：

- 推理被**隐式化**：模型不再输出 `Thought:` 文本，而是在 tool call 的决策中体现推理结果（「调什么工具、传什么参数」本身就是推理产物）。
- 循环推进由**代码保证**：`for turn in 0..max_turns`，每轮 = 一次模型调用 + 工具执行 + 结果回填。
- 停止条件由**协议保证**：provider 返回 `stop_reason: end_turn` 或空 tool_calls 即结束。

aemeath 的主循环整体上体现了这种工程化形态——推理隐式化、循环推进由代码保证、停止条件由协议保证；具体停止机制随迭代持续演进，**不再是**上文示意的有界 `max_turns`：当前 Loop Engine 内置 StuckGuard 用墙钟 TimeoutGuard 兜底替代了 `max_turns`（见 [04-stuck-prevention.md](../02-modules/runtime/04-stuck-prevention.md) L3），具体实现细节与差距 **MUST** 以 [Migration Governance](03-migration-governance.md) 为准，本文不逐版本追踪。它保留了 ReAct 「推理-行动-观察」循环的本质，但把推理外化为模型对工具的选择，把流程的机械推进交给代码——这正是 Loop Engineering 的核心权衡：**哪些交给模型自由度，哪些用代码锁死**。

### 2.4 Workflow（显式流程编排）

- **核心问题**：对于**高频且确定**的路径，与其靠 prompt 教模型走，不如用代码显式编排，让 LLM 只在「真正的判断点」决策。
- **关键手段**：
  - 轻量 router（规则匹配即可）识别意图，切入预定义流程。
  - 流程由代码驱动（步骤顺序、分支、循环），LLM 只在判断节点被调用。
  - 流程即代码 → 可测试、可版本化、可回归。
- **与 agent loop 的区别**（Anthropic *Building Effective Agents* 的经典区分）：
  - **agent**：模型自主决定路径与工具，灵活性高、可控性低。
  - **workflow**：路径由代码预先定义，模型只在指定节点决策，可控性高、灵活性低。
- **orchestrator-workers 模式**（Anthropic 提出、被 LangGraph/CrewAI 文档转引）：中心 orchestrator 把任务拆给多个 worker LLM 动态执行，再综合结果——专门适配子任务数量/内容无法预先确定的场景（如需要改未知数量文件的代码生成任务）。是 §2.3 sub-agent 委托模式的直接先例，也是业界「Workflow 编排多 agent」最常见的落地形态。
- **代表实践**：Claude Code 的 `/commit` 这类 skill，本质就是被 prompt 化的 mini-workflow——流程固定（收集变更 → 生成消息 → 提交），关键判断点（提交信息措辞）交给 LLM。业界生产级实现详见 §7.1。

### 2.5 Graph（状态图编排）

- **核心问题**：当流程复杂到需要**分叉、合并、回放、并行子图**时，线性 workflow 不够用，需要图抽象。
- **执行模型**（Pregel/BSP，Google 提出；LangGraph 与微软 Agent Framework 独立收敛到同一套算法家族，详见 §7.2）：
  - **superstep（超步）**：图执行切成离散轮次，同一超步内并行的节点属于同一轮，需等上游产出的节点属于下一轮。
  - **同步屏障（synchronization barrier）**：本超步内所有被触发的节点必须全部完成，才允许推进到下一超步；且超步具有事务性——超步内任一节点异常，本超步全部 state 更新一起回滚，不会出现部分生效的中间态。
  - **条件边（conditional edge）**：超步边界的路由决策。LangGraph 是「看全局 state」的 path 函数（可返回单个/多个目标节点，或 `END`）；微软 Agent Framework 是「逐边挂谓词、只看流经该边的消息」——两种作用域不同的实现，aemeath 设计时需明确选哪种。
- **关键原语**：
  - **node/executor**（节点）：一个执行单元（LLM 调用 / 工具 / 子图）。微软 Agent Framework 官方不用 "node" 这一术语，只用 **Executor**。
  - **edge**（边）：节点间的转移。LangGraph 的条件边 + **Send API**（动态 map-reduce 式并行分支——routing 函数返回运行时才确定数量的 `Send(目标, payload)` 列表，同批 `Send` 在同一 superstep 内并行执行）；微软 Agent Framework 固定五种一等公民 Edge 类型（Direct / Conditional / Switch-Case / Multi-Selection Fan-out / Fan-in，Fan-in 是显式命名的 Barrier 原语，语义是同步 join）。
  - **state**（状态）：LangGraph 用 `Annotated` 附加 reducer 函数控制合并语义（默认覆盖，可自定义拼接，内置 `add_messages` 按消息 ID 合并）；微软 Agent Framework 是「消息传递 + 按 scope 隔离的共享状态存储」混合模型，可见性同样受 superstep 屏障约束。
  - **checkpoint**（检查点）：两家都支持 superstep 边界持久化，用于分叉重跑、回放、A/B。LangGraph 额外拆分 checkpointer（thread 级短期状态）与 store（跨 thread 长期记忆）两套机制，并提供 `exit`/`async`/`sync` 三档可调的持久化强度。
- **与 workflow 的区别**：workflow 是图的特例（线性 / 树形），graph 引入了显式 state 与 checkpoint 语义，面向复杂控制流与可恢复性。
- **代表实践**：LangGraph（Python，Pregel/BSP + Send API 的先行实现）、微软 Agent Framework（.NET/Python，独立实现同款 Pregel/BSP 模型）——两者均验证了这套执行模型不是单一厂商偏好，但在 Rust 生态均无成熟同类，这是 aemeath 若引入 graph 抽象 **MUST** 直面的生态现实。

## 3. 范式光谱与权衡

把上述主线沿「模型自由度」轴排开，得到一条光谱：

```
纯代码驱动                                        纯 prompt 驱动
    │                                                  │
    ▼                                                  ▼
 Graph ──── Workflow ──── Agent Loop (ReAct) ──── Raw Prompt
 (图+checkpoint)  (显式流程)    (模型自主)         (无工具)
    └──────── 可控性高 / 灵活性低 ────────┘
                └──────── 可控性低 / 灵活性高 ────────┘
```

没有银弹——不同任务性质适配不同范式。权衡维度：

| 维度 | Agent Loop | Workflow | Graph |
|---|---|---|---|
| Token 成本 | 高（每轮都带完整上下文） | 低（流程步进免 LLM） | 中 |
| 可控性 | 低（模型可能漂移） | 高（代码锁路径） | 高 |
| 可测试性 | 差（需 mock LLM） | 好（流程即代码） | 好 |
| 恢复粒度 | 会话级 | 步骤级 | 节点级（checkpoint） |
| 灵活性 | 高 | 低 | 中 |
| 适用场景 | 开放探索 | 高频确定路径 | 复杂分叉/并行 |

判断准则：**当一条路径出现「高频、确定、易漂移、需回归」四个特征时，SHOULD 考虑从 agent loop 抽出为 workflow**。反之，探索性、一次性、高变数的任务，留在 agent loop 里更合适。

## 4. 历史调研盘点（Issue #358 PoC 阶段快照，非当前事实）

> 本节是 Issue #358 PoC 阶段对 `main` 分支代码做的**一次性核对（2026-06）**，记录当时对五条主线成熟度的判断依据，**仅作历史调研留存**，**NEVER** 当作当前代码现状引用——代码持续演进，下表出现过的具体路径、常量、阈值与「基本空白」结论都可能已经过期，其中「Workflow 基本空白」的结论**已确认过期**（现有 `runtime` crate 内的 `reasoning_graph` 模块）。**当前** Current → Target 差距、责任、进度与退出条件 **MUST** 只以 [Migration Governance](03-migration-governance.md) 为唯一治理真相；Workflow 主线的**当前**设计真相见 [`docs/design/02-modules/workflow/01-reasoning-graph.md`](../02-modules/workflow/01-reasoning-graph.md)。

| 主线 | 调研时（2026-06）的判断 | 判断依据（历史快照，不代表现状） |
|---|---|---|
| **Context** | 扎实 | guidance resolver / skill loader、compact 子系统（compact/microcompact/autocompact/token_estimation/summary/truncate）按输入占比分级 urgency 触发压缩 |
| **Harness** | 扎实 | TypedTool trait + registry（Agent/Task/WebSearch 等）、policy、hook、audit |
| **Loop** | 扎实，但无 checkpoint | 经典工程化 agent loop（推理隐式化 + 代码驱动推进 + 协议驱动停止）；调研时状态散在局部变量，无显式状态对象、无 turn 边界持久化 |
| **Workflow** | 调研时判断「基本空白」 | 调研时 grep `workflow` 仅命中字符串字面量，非抽象层；**此判断已过期**，Current 状态见 Migration Governance 与 [workflow/01-reasoning-graph.md](../02-modules/workflow/01-reasoning-graph.md) |
| **Graph** | 调研时判断「基本空白」 | 调研时未见 node/edge/state/checkpoint 原语；Current 状态同上，**MUST** 查 Migration Governance |
| **sub-agent** | 语义偏弱 | 作为普通 TypedTool 注册（name=`"Agent"`），内部复用共享 Loop Engine；调研时无子图表达、无独立 checkpoint、输入输出契约隐式 |

**结论（历史判断，供 §5 决策框架的背景参考）**：调研当时 Context / Harness / Loop 三条线支撑了「开放编程助手」场景，Workflow / Graph 的空白被认为是 Issue #358 三个痛点的直接成因。§5 的决策框架据此设计，框架本身不因本节判断过期而失效；但**不要**把本节任何一行当作现状依据——查现状一律去 Migration Governance。

## 5. 演进决策框架（对应 Issue #358）

以下框架 **SHOULD** 指导 PoC 选择，**NEVER** 视为既定路线图。每个方向列出触发条件、前置依赖、风险。

### 方向 A：把高频路径抽成显式 workflow（最小杠杆）

- **触发条件**：观察到某路径同时具备「高频、确定、易漂移、需回归」四特征。候选：发版、修 issue、commit。
- **前置依赖**：无（可纯叠加，不碰 loop 内部）。
- **实现思路**：轻量 router（规则匹配意图）→ 切入 workflow → 流程代码驱动 + 关键点 LLM 决策。
- **风险**：router 误判会让用户困惑；workflow 与现有 skill 系统的关系需厘清（见 §6）。
- **PoC 度量**：token 消耗、成功率、可测试性，对比纯 prompt loop。
- **建议**：作为 **首选 PoC 方向**——杠杆最高、风险最低、可逆。

### 方向 B：给 agent loop 加 checkpoint 语义（当前系统基线禁止，仅供知识储备）

> **系统级约束**：[`02-modules/runtime/05-recovery-semantics.md`](../02-modules/runtime/05-recovery-semantics.md) 已收敛原 #762「Durable Model Invocation」，明确 aemeath **NEVER** 建立引擎级 durable checkpoint。本方向与**当前系统架构基线直接冲突**，**NEVER** 在现行基线下实施。除非未来出现推翻该基线的**新 RFC**并正式修订 `05-recovery-semantics.md`，否则本方向不具备可实施性——「触发条件满足」不能作为绕过基线的理由。以下内容仅作为知识储备，对比业界实践（LangGraph checkpoint、Temporal durable execution 等）供架构评估参考，**不代表** aemeath 的可执行路线图。

- **若基线被推翻，可能的触发条件**：出现「换模型/换 prompt 重跑同一段对话」「回放调试」「A/B 对比」等真实诉求。
- **若基线被推翻，可能的前置依赖**：需要新 RFC 先定义显式、单一所有者的运行状态对象；本文不描述当前 Runtime 字段或目录。
- **若基线被推翻，可能的实现思路**：每轮 turn 边界持久化 state 快照（messages + ctx + turn number），支持从任意 checkpoint 分叉。
- **风险**：状态对象重构面较大；持久化格式向前兼容；性能（每轮序列化）；更根本的风险是与系统基线冲突本身。
- **建议**：在现行基线下**不进入任何实施排期**；仅当有新 RFC 正式推翻 `05-recovery-semantics.md` 的基线后，才值得重新按上述要点评估。

### 方向 C：Human-in-the-loop 升级为图节点（仅业界对比，非实施候选）

> 现行基线不引入 durable graph/checkpoint，因此本方向与 B 一样不进入实施排期；未来只有新 RFC 同时重定义 Runtime recovery 与交互状态所有权后才能重新评估。

- **若基线被推翻，可能的触发条件**：阻塞式交互不足以支撑修改输入、补充上下文、改方向或异步处理。
- **若基线被推翻，可能的前置依赖**：新 RFC 定义 graph state 与交互 continuation 的单一所有者。
- **风险**：交互模型复杂化，需所有交付 adapter 同步改造。
- **建议**：现行基线下仅保留知识对比。

### 方向 D：多 agent 编排用子图表达

> **⚠️ 系统级约束**：aemeath 系统级决策 **无多-agent graph 长期计划**，本方向与系统基线冲突，**NEVER** 在 aemeath 中实施。此处仅作为知识储备对比业界实践（如 LangGraph subgraph、Microsoft Agent Framework 等），供架构评估参考。

- **触发条件**：sub-agent 的「当普通 tool 调用」语义不够（需要独立 checkpoint、清晰输入输出契约、并行编排）。
- **前置依赖**：方向 B 的 checkpoint（子图需要独立可恢复）。
- **风险**：过度工程化——当前 sub-agent 作为 tool 已满足多数场景。
- **建议**：谨慎，除非有明确的「子图独立测试/恢复」硬需求。

## 6. 开放问题

以下问题 **MUST** 在落地任何编排层前回答，本文档 **不预设答案**：

1. **PoC 是否需要独立 capability / crate？**
   - 本文不预先指定 `agent/features/orchestration/**` 目录。先在新 RFC 中证明独立生命周期、复用或安全边界收益，再按 [代码组织规范](../01-system/06-code-organization.md) 决定共置或升格；知识地图不对物理布局投票。
2. **自研 vs 复用 LangGraph 思路？**
   - 事实：Rust 生态无成熟 LangGraph 同类；但 LangGraph 与微软 Agent Framework 两个独立团队都收敛到 Pregel/BSP + superstep + 同步屏障这套执行模型（§2.5、§7.2），说明这不是单一厂商偏好，而是图编排问题的某种自然解。
   - 倾向：自研，但 **SHOULD** 先复刻这套已经两方独立验证的 node/edge/state/checkpoint 四原语语义（含 superstep 屏障与条件边路由），而非另起概念。
3. **workflow 与现有 skill 系统的关系？**
   - 选项一：skill 升级为 workflow 的载体（skill = 声明式 workflow 描述）。
   - 选项二：两套并行（skill 负责 prompt 化能力注入，workflow 负责代码化流程编排）。
   - 倾向：PoC 阶段并行，跑通后按实际耦合度决定合并与否。

## 7. 业界调研成果（Issue #358 补充材料，2026-07）

本节整理针对 Workflow / Graph 两条主线的多轮 deep-research 调研结果（6 轮独立调研、100+ 信源、3 票对抗式验证），作为 §2.4 / §2.5 摘要陈述的详细佐证。Context / Harness / Loop 三条线业界证据同样充分（Anthropic context engineering 官方文章、MCP 授权规范、ReAct 论文、Claude Code 源码级验证），但 aemeath 在这三线已有扎实实现（§4），故此处不展开，仅列入 §8 参考。

### 7.1 Workflow：四个代表实现的设计取舍

| 实现 | 核心抽象 | 编排方式 | 与 aemeath 的关联 |
|---|---|---|---|
| **Temporal** | Workflow（确定性编排层）/ Activity（非确定性工作单元） | Workflow 只发 Command、不直接执行副作用；LLM/工具调用全部包在 Activity 里；崩溃恢复靠重放 event-sourced Event History（非快照），已完成的 Activity 不重新执行，只取历史结果 | 平台级强确定性约束，代价是 Workflow 代码必须可重放；⚠️ 不能假设它能完全替代 LangGraph 式 checkpointer（相关强结论已被验证推翻） |
| **CrewAI** | Agent / Task / Crew / Process | Sequential（固定顺序，前一任务输出作下一任务上下文）；Hierarchical（**代码级强制**要求 `manager_llm`/`manager_agent`，缺失直接报错，构成 orchestrator-workers，而非图拓扑涌现路由） | Hierarchical 模式是「角色化 workflow」的清晰参考；显式声明优于隐式约定的设计值得借鉴 |
| **AutoGen** | GroupChatManager + 可插拔 speaker selection | pub/sub 广播消息 + 集中式 manager 选人（默认 LLM 选择器，可换规则/自定义函数/legacy 四种字符串策略）；终止条件用可组合 `TerminationCondition`（按位或组合） | 对话驱动、无显式图结构（但生态内另有独立 `GraphFlow`/`DiGraphBuilder` 服务显式流程场景，与对话驱动模式并列非融合） |
| **AutoGPT** | 从 Classic（自主循环 + Forge 组件）转向 **AutoGPT Platform**（用户拼接 Block 的可视化工作流） | Classic 已官方废弃（无安全维护）；Platform 的核心交互是「连接 Block」而非 LLM 运行时自主任务分解——原始「自主 agent」项目自身完成了向 Workflow 范式的转型 | 印证 §1 的判断：「高频确定路径应从 agent loop 抽出为 workflow」不是 aemeath 一家的孤立判断，而是行业已验证的演化方向 |

`orchestrator-workers` 模式（Anthropic 提出，被 LangGraph、CrewAI 文档转引）是四个实现中出现频率最高的公共模式，也是 §2.3 sub-agent 委托的直接先例。

### 7.2 Graph：Pregel/BSP 在两个独立生态的收敛

| 维度 | LangGraph | 微软 Agent Framework |
|---|---|---|
| 节点术语 | node | **Executor**（官方明确不用 "node"） |
| 执行模型 | 显式自称 "modified Pregel" BSP | 显式自称 "modified Pregel" BSP（与 LangGraph **算法同源、独立实现**） |
| 同步屏障 | 超步末尾全部节点 inactive 且无消息在途才终止；超步事务性（异常则整批更新回滚） | 同一同步屏障语义；官方文档明确指出 fan-out 场景下的木桶效应（短分支需等长分支） |
| 条件边 | `add_conditional_edges` path 函数，读**全局 state** 决定路由 | 逐边布尔谓词，只读**流经该边的消息**，不接触全局 state——作用域比 LangGraph 更局部 |
| 并行分支 | **Send API**：routing 函数返回运行时确定数量的 `Send(目标, payload)` 列表，同批 `Send` 同一 superstep 内并行 | 五种一等公民 Edge 类型（Direct/Conditional/Switch-Case/Multi-Selection Fan-out/Fan-in），Fan-in 是显式命名的 `AddFanInBarrierEdge` 同步 join 原语 |
| 状态模型 | `Annotated` + reducer 函数（默认覆盖，可自定义合并，内置 `add_messages` 按消息 ID 合并） | 消息传递 + 按 scope 隔离的共享状态存储，可见性受 superstep 屏障约束 |
| 持久化 | checkpointer（thread 级短期状态）+ store（跨 thread 长期记忆）；`exit`/`async`/`sync` 三档可调持久化强度 | superstep 边界 checkpoint（第三方评论：目前偏基础能力，手动续跑、无自动故障检测，生产级容错还需配 Azure Durable Task Extension） |

**两个独立团队收敛到同一套执行模型**（Pregel/BSP + superstep + 同步屏障），是本次调研对 aemeath 最有分量的信号：Workflow BC 当前把控制流编排（DAG / 状态图）列为「远期方向」、暂缓实现（见 [workflow/01-reasoning-graph.md §9](../02-modules/workflow/01-reasoning-graph.md)）；若该方向未来真正启动，这套「superstep 边界同步、事务性批量提交、条件边路由」的设计已经过两方独立验证，风险低于自创一套并行同步语义。

### 7.3 尚未确证的开放问题

- **微软 Agent Framework 与 Semantic Kernel Process Framework 的血缘关系**：三轮调研均未获得证据，需要专门再查官方迁移指南。
- **Temporal 能否替代 LangGraph 式 checkpointer**：相关强结论已被验证推翻（1-2 票）——不能假设引入 Temporal 就能省去应用层状态持久化设计。
- **AutoGen 是否存在隐藏的最小状态机**：「无显式 state machine/graph 结构」的定性表述本身是 2-1 分裂票，manager 侧确实维护了 `_previous_participant_topic_type` 之类的最小标量状态，不宜视为板上钉钉的结论。
- **原始 AutoGPT 项目与 AutoGPT+P 论文需严格区分**：AutoGPT+P 是 2024 年一篇无关的机器人任务规划学术论文（同名不同源），调研中曾被误用，已在本轮修正为直接查 AutoGPT 官方仓库/文档。

## 8. 参考

- **Anthropic**, *Building Effective Agents*（workflow vs agent 的经典区分、orchestrator-workers 等模式）。
- **Anthropic**, *Effective context engineering for AI agents*（compaction / just-in-time 检索 / 结构化笔记记忆）。
- **Yao et al., 2022**, *ReAct: Synergizing Reasoning and Acting in Language Models*（agent loop 的理论根基）。
- **Model Context Protocol**，官方规范 *Authorization*（OAuth 2.1 子集、RFC 8707、RFC 9728）。
- **LangGraph** 官方文档：*Graph API overview*、*Durable execution*（node / edge / state / checkpoint / Send API / Pregel-BSP 原语）。
- **微软 Agent Framework** 官方文档：`learn.microsoft.com/en-us/agent-framework/workflows/`（Executor / Edge / superstep / checkpoint 原语）。
- **Temporal** 官方文档与博客：`docs.temporal.io/workflows`、*Durable Execution meets AI*（Workflow/Activity 分层、event-sourced 崩溃恢复）。
- **CrewAI** 官方文档：`docs.crewai.com/en/concepts/processes`（Sequential/Hierarchical process）。
- **AutoGen** 官方文档：`microsoft.github.io/autogen`（GroupChat、speaker selection、TerminationCondition、GraphFlow）。
- **AutoGPT** 官方仓库与文档：`github.com/Significant-Gravitas/AutoGPT`、`agpt.co/docs/classic`（Classic → Platform 的架构转型）。
- Issue #358：本方向的原始动机与四个探索方向。
- [`docs/design/01-system/01-product-and-domain.md`](../01-system/01-product-and-domain.md) §3.1 核心域 / §4 Bounded Context 清单：aemeath **当前**的领域划分，编排层的落点 **MUST** 遵循。
- `docs/design/02-modules/runtime/01-domain-model.md`：现有 Agent Looping 的设计真相源，任何 loop 改造 **MUST** 先核对此文档。
- [`docs/design/02-modules/workflow/01-reasoning-graph.md`](../02-modules/workflow/01-reasoning-graph.md)：Workflow BC **当前**设计真相源（ReasoningGraph effort 调节 5 节点状态机 + §9 Workflow Engine 远期方向）。
