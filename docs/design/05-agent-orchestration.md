# Agent 编排范式知识地图

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/358
>
> 本文档是 **知识储备** 类设计文档：整理 Agent 工程的几条主线（Context / Harness / Loop / Workflow / Graph），梳理 aemeath 在各主线的现状与缺口，给出后续引入编排层的决策框架。文档 **不直接约束代码**，**MAY** 作为 Issue #358 PoC 与后续架构决策的参考底座。

## 1. 背景与目的

aemeath 当前架构可定位为：Context / Harness / Loop 三条主线已有扎实基础，而 **Workflow / Graph 维度基本是空的**——所有任务路径都由模型在 agent loop 内自主决定，连「修 issue → 改代码 → 验证 → 提 PR」这类高频确定性流程也完全靠 system prompt / Guidance 指导模型走。

纯 agent loop 在开放编程场景下是正确选择，但带来三个问题（见 Issue #358）：

1. **Token 成本与漂移**：高频路径靠 prompt 教模型走，token 贵、易漂移、偶尔不遵守。
2. **不可测试**：流程混在 LLM 行为里，无法对「流程正确性」做 CI 回归。
3. **细粒度可恢复性缺失**：暂停/恢复是会话级，无法在单轮 tool call 边界 checkpoint / 分叉重跑。

本文档的目的：

- **建立统一术语**：让后续讨论有共同语言，避免「workflow」「graph」「orchestration」混用。
- **呈现现状事实**：以代码为依据给出各主线成熟度与缺口，而非凭直觉。
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

aemeath 的主循环正是这种工程化形态（见 §4）。它保留了 ReAct 「推理-行动-观察」循环的本质，但把推理外化为模型对工具的选择，把流程的机械推进交给代码——这正是 Loop Engineering 的核心权衡：**哪些交给模型自由度，哪些用代码锁死**。

### 2.4 Workflow（显式流程编排）

- **核心问题**：对于**高频且确定**的路径，与其靠 prompt 教模型走，不如用代码显式编排，让 LLM 只在「真正的判断点」决策。
- **关键手段**：
  - 轻量 router（规则匹配即可）识别意图，切入预定义流程。
  - 流程由代码驱动（步骤顺序、分支、循环），LLM 只在判断节点被调用。
  - 流程即代码 → 可测试、可版本化、可回归。
- **与 agent loop 的区别**（Anthropic *Building Effective Agents* 的经典区分）：
  - **agent**：模型自主决定路径与工具，灵活性高、可控性低。
  - **workflow**：路径由代码预先定义，模型只在指定节点决策，可控性高、灵活性低。
- **代表实践**：Claude Code 的 `/commit` 这类 skill，本质就是被 prompt 化的 mini-workflow——流程固定（收集变更 → 生成消息 → 提交），关键判断点（提交信息措辞）交给 LLM。

### 2.5 Graph（状态图编排）

- **核心问题**：当流程复杂到需要**分叉、合并、回放、并行子图**时，线性 workflow 不够用，需要图抽象。
- **关键原语**（LangGraph 思路）：
  - **node**（节点）：一个执行单元（LLM 调用 / 工具 / 子图）。
  - **edge**（边）：节点间的转移，可为条件边（依据 state 决定下一节点）。
  - **state**（状态）：贯穿全图的显式状态对象，节点读写它。
  - **checkpoint**（检查点）：在每个节点边界持久化 state，支持分叉重跑、回放、A/B。
- **与 workflow 的区别**：workflow 是图的特例（线性 / 树形），graph 引入了显式 state 与 checkpoint 语义，面向复杂控制流与可恢复性。
- **代表实践**：LangGraph（Python）、其在 Rust 生态尚无成熟同类——这是 aemeath 若引入 graph 抽象 **MUST** 直面的生态现实。

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

## 4. aemeath 现状评估

以下评估基于对 `main` 分支代码的核对（2026-06），非凭 Issue 描述。

| 主线 | 成熟度 | 代码落点 | 缺口 |
|---|---|---|---|
| **Context** | ✅ 扎实 | `prompt/`（guidance resolver + skill loader）、`runtime/business/compact/`（compact/microcompact/autocompact/token_estimation/summary/truncate）、`compact_if_needed` 按 `api_input*100/ctx_context_size` 的 urgency（50%/35% 阈值）触发 | — |
| **Harness** | ✅ 扎实 | `tools/`（TypedTool trait + registry + Agent/Task/WebSearch 等）、`policy/`、`hook/`、`audit/` | — |
| **Loop** | ✅ 扎实 | `runtime/business/agent/runner/loop_run.rs::run_loop`：`for turn in 0..max_turns` 经典工程化 agent loop；停止条件 `EndTurn \|\| tool_calls.is_empty()`；暂停 `ctx.cancel`；超时 `max_duration`；重试与防死循环（见 Issue #372/#374） | **无 checkpoint**：状态散在 `self.messages`/`self.ctx` 等局部变量，无显式状态对象，无 turn 边界持久化 |
| **Workflow** | ❌ 基本空白 | —（grep `workflow` 仅命中字符串字面量，非抽象层） | 全部：router、显式流程步进、流程级测试 |
| **Graph** | ❌ 基本空白 | — | 全部：node/edge/state/checkpoint 原语 |
| **sub-agent** | ⚠️ 语义偏弱 | `tools/business/agent_tool.rs`：实现 `TypedTool`，name=`"Agent"`，作为普通工具注册；内部复用 `SubAgentRun::run_loop` | 无子图表达、无独立 checkpoint、输入输出契约隐式 |

**结论**：Context / Harness / Loop 三条线支撑了当前「开放编程助手」场景，但 Workflow / Graph 的空白导致 Issue #358 所列三个痛点无解——高频路径的成本与漂移无法靠 prompt 根治。

## 5. 演进决策框架（对应 Issue #358）

以下框架 **SHOULD** 指导 PoC 选择，**NEVER** 视为既定路线图。每个方向列出触发条件、前置依赖、风险。

### 方向 A：把高频路径抽成显式 workflow（最小杠杆）

- **触发条件**：观察到某路径同时具备「高频、确定、易漂移、需回归」四特征。候选：发版、修 issue、commit。
- **前置依赖**：无（可纯叠加，不碰 loop 内部）。
- **实现思路**：轻量 router（规则匹配意图）→ 切入 workflow → 流程代码驱动 + 关键点 LLM 决策。
- **风险**：router 误判会让用户困惑；workflow 与现有 skill 系统的关系需厘清（见 §6）。
- **PoC 度量**：token 消耗、成功率、可测试性，对比纯 prompt loop。
- **建议**：作为 **首选 PoC 方向**——杠杆最高、风险最低、可逆。

### 方向 B：给 agent loop 加 checkpoint 语义（中期）

- **触发条件**：出现「换模型/换 prompt 重跑同一段对话」「回放调试」「A/B 对比」等真实诉求。
- **前置依赖**：**需要一个显式状态对象贯穿 loop**（当前 runtime 状态较散，散在 `SubAgentRun` 的十几个字段里）。
- **实现思路**：每轮 turn 边界持久化 state 快照（messages + ctx + turn number），支持从任意 checkpoint 分叉。
- **风险**：状态对象重构面较大；持久化格式向前兼容；性能（每轮序列化）。
- **建议**：中期，待方向 A 验证 workflow 价值后再评估。

### 方向 C：Human-in-the-loop 升级为图节点

- **触发条件**：当前阻塞式 yes/no 不足以支撑「修改输入 / 补充上下文 / 改方向 / 异步」诉求。
- **前置依赖**：与方向 B 的显式状态对象耦合（节点化需要 state）。
- **风险**：交互模型复杂化，需 TUI/CLI/Server 三端同步改造。
- **建议**：长期，依赖 B 落地。

### 方向 D：多 agent 编排用子图表达

- **触发条件**：sub-agent 的「当普通 tool 调用」语义不够（需要独立 checkpoint、清晰输入输出契约、并行编排）。
- **前置依赖**：方向 B 的 checkpoint（子图需要独立可恢复）。
- **风险**：过度工程化——当前 sub-agent 作为 tool 已满足多数场景。
- **建议**：谨慎，除非有明确的「子图独立测试/恢复」硬需求。

## 6. 开放问题

以下问题 **MUST** 在落地任何编排层前回答，本文档 **不预设答案**：

1. **是否需要独立的 `agent/features/orchestration/**` feature？**
   - 倾向：方向 A 可先在 runtime 叠加（纯 router + workflow 函数），验证后再决定是否独立 feature；方向 B/C/D 几乎必然需要独立 feature（checkpoint 状态机横切多个 feature）。
2. **自研 vs 复用 LangGraph 思路？**
   - 事实：Rust 生态无成熟 LangGraph 同类。
   - 倾向：自研，但 **SHOULD** 先复刻 LangGraph 的 node/edge/state/checkpoint 四原语语义，而非另起概念。
3. **workflow 与现有 skill 系统的关系？**
   - 选项一：skill 升级为 workflow 的载体（skill = 声明式 workflow 描述）。
   - 选项二：两套并行（skill 负责 prompt 化能力注入，workflow 负责代码化流程编排）。
   - 倾向：PoC 阶段并行，跑通后按实际耦合度决定合并与否。

## 7. 参考

- **Anthropic**, *Building Effective Agents*（workflow vs agent 的经典区分）。
- **Yao et al., 2022**, *ReAct: Synergizing Reasoning and Acting in Language Models*（agent loop 的理论根基）。
- **LangGraph** 文档：node / edge / state / checkpoint 原语。
- Issue #358：本方向的原始动机与四个探索方向。
- `docs/design/01-outline.md` §核心域 / §Bounded Context：aemeath 的领域划分，编排层的落点 **MUST** 遵循。
- `docs/design/03-runtime-design.md`：现有 Agent Looping 的设计真相源，任何 loop 改造 **MUST** 先核对此文档。
