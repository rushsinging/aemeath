# 产品与领域

> 层级：01-system（系统级总体设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0
> 本文定义 aemeath 的产品目标、要解决的核心问题，以及领域的子域划分（问题空间）与 Bounded Context 清单（解决方案空间）。**本文只描述目标态，不记录当前代码状态。**

## 1. 产品目标

aemeath 是一个基于 Rust 的 **AI 编程助手**，以 TUI 为主要交付形态，支撑多 provider、多模型、子代理（sub-agent）与技能（skill）系统。它的价值主张是：在终端里提供一个**可观察、可控制、可扩展**的自主编程代理——用户用自然语言表达意图，代理通过"推理 → 调用工具 → 观察结果"的循环推进任务，并在关键节点（权限、澄清、计划）把控制权交还给人。

## 2. 要解决的核心问题

| 核心问题 | 说明 |
|---|---|
| **自主执行** | 把用户意图翻译为一系列工具调用（读写文件、执行命令、检索），自主推进直到完成或需要人介入。 |
| **上下文工程** | 在有限的模型上下文窗口内，持续维持"任务相关"的信息密度：压缩历史、注入记忆、装配提示、控制 token 预算。这是产品质量的核心变量。 |
| **人在环控制** | 危险操作要审批、意图不清要澄清、复杂任务要计划——代理必须能在执行中暂停、等待人的决策、再恢复。 |
| **可扩展工具生态** | 内置工具 + Skill + Slash 命令 + MCP，能力可插拔扩展。 |
| **多模型适配** | 屏蔽各家 LLM API 差异（消息格式、流式、reasoning 能力），对上层暴露统一模型调用。 |

## 3. 子域划分（问题空间）

子域是**业务问题空间**的划分，回答"这个产品由哪些世界组成"。按 DDD 战略设计分为三类：

### 3.1 核心域 Core Domain（差异化竞争力）

| 子域 | 说明 |
|---|---|
| **Agent 执行（Agent Runtime）** | 驱动"推理 → 工具 → 观察"循环、维护单次执行的状态机、编排模型调用与工具执行、派生与执行 SubAgent。这是产品的心脏，也是唯一核心域。 |

### 3.2 支撑子域 Supporting Subdomain（业务需要但非差异化）

| 子域 | 说明 |
|---|---|
| **Context Management** | 上下文工程：对话历史聚合（Session）、compact 家族（压缩 / 精简 / 回收）、token 预算、记忆注入、提示与 guidance 装配、会话身份与 resume。 |
| **Memory** | 跨会话记忆的存取与反思（Reflection 产出记忆建议）。 |
| **Task Management** | 任务聚合：状态机（pending→in_progress→completed）与依赖图不变量（blocked_by 不成环）。 |
| **Project / Workspace** | worktree 工作区上下文、git 状态供给。 |
| **Policy** | 权限评估（工具执行前的准入判断）。 |
| **Audit** | 审计事件记录，含成本 / 用量 / 定价（Cost）。 |
| **Tool & Skill & Command** | 工具生态：内置 Tool、Skill、Slash 命令、MCP 集成。 |
| **Workflow** | agent 行为的推理调节：reasoning effort 阶段调节（reasoning graph），经端口被 Agent Runtime 消费。不做多-agent 图编排；sub-agent 的派生与执行属 Agent Runtime。 |

### 3.3 通用子域 Generic Subdomain（可买可换的通用能力）

| 子域 | 说明 |
|---|---|
| **Provider** | LLM 供应商适配，内部 ACL 吸收各家 API 差异，对上暴露统一调用与流式。 |
| **Hook** | 生命周期钩子执行。 |
| **Storage** | 持久化机制（原子写、损坏兜底），**不拥有数据本体**，为各数据 BC 提供落盘能力。 |
| **Config** | 分层配置、只读快照发布；含 reasoning 静态阈值 / 级别。 |
| **Application Version Control** | 应用自身版本治理：当前版本、稳定 / 开发渠道、升级策略、自更新。 |
| **Logging** | 日志 target 路由与统一 schema。 |

## 4. Bounded Context 清单（解决方案空间）

Bounded Context 是**解决方案空间**的边界，一个 BC 内部维持一套一致的模型与统一语言。本项目共 **15 个 BC**（1 核心 + 8 支撑 + 6 通用），与上述子域 1:1 映射。

| # | Bounded Context | 子域类型 | 目标职责 |
|---|---|---|---|
| 1 | **Agent Runtime** | 核心 | Loop Engine、唯一状态机 Run、tool / model / interaction 编排、SubAgent 派生与执行 |
| 2 | **Context Management** | 支撑 | Session 对话历史聚合、compact 家族、token 预算、记忆注入、prompt / guidance、会话身份与 resume |
| 3 | **Memory** | 支撑 | 记忆存取；Reflection 产出记忆建议 |
| 4 | **Task Management** | 支撑 | Task 聚合 + 状态机 + 依赖图不变量 |
| 5 | **Project / Workspace** | 支撑 | worktree、git 上下文 |
| 6 | **Policy** | 支撑 | 权限评估 |
| 7 | **Audit** | 支撑 | 审计事件；Cost / Usage / Pricing |
| 8 | **Tool & Skill & Command** | 支撑 | 内置 Tool、Skill、Slash 命令、MCP |
| 9 | **Workflow** | 支撑 | reasoning effort 阶段调节（reasoning graph），经端口被 Agent Runtime 消费 |
| 10 | **Provider** | 通用 | LLM 供应商 ACL、统一调用与流式 |
| 11 | **Hook** | 通用 | 生命周期钩子执行 |
| 12 | **Storage** | 通用 | 持久化机制（原子写、损坏兜底） |
| 13 | **Config** | 通用 | 分层配置、只读快照；reasoning 静态阈值 |
| 14 | **Application Version Control** | 通用 | 版本渠道、升级策略、自更新 |
| 15 | **Logging** | 通用 | 日志 target 路由与 schema |

### 4.1 Bounded Context 责任章程

本章程为后续战术设计提供第一层判断：一项能力只有在“负责”列中找到明确所有者，才能进入对应 BC；命中“不负责”列时，必须通过端口调用真正所有者，或回到 Context Map 重新定案。`不负责` 不是“不参与”，而是“不拥有该模型、不守护该不变量、不决定其业务策略”。

| Bounded Context | 负责什么 | 不负责什么 |
|---|---|---|
| **Agent Runtime** | `Run` 聚合与 Agent 执行生命周期状态机；Main/Sub 共用 Loop Engine；model/tool/hook/interaction 的调用时机和 Context Window 构建请求编排；Sub Run 派生、取消、终态与领域事件 | Session 对话历史和恢复数据；Provider 协议；Tool/Skill/Command 实现；Policy 规则；Memory/Task/Workspace 不变量；持久化机制；TUI/Server 投影 |
| **Context Management** | Session、ChatChain/ChatSegment；Context Window；compact 家族；token budget；Prompt/Guidance/AGENTS/Skill/Memory 内容的窗口组装；会话身份；Session 与跨 BC Snapshot 的组装及 resume 数据 | Run 执行状态机；Memory 检索与记忆本体；Task/Workspace 聚合不变量；Provider 线协议；Tool 函数调用；物理路径、文件格式、原子写与损坏兜底 |
| **Memory** | Memory Entry、Layer、检索、写入、去重、归档；Reflection 产出 Memory Suggestion 及其应用规则；Memory 的可序列化视图 | Memory 在 Context Window 的注入位置/预算；Session；Run loop；物理存储机制；把所有历史消息自动视为记忆 |
| **Task Management** | Task/Batch 聚合；Task 局部状态机与依赖图无环不变量；Task Snapshot Published Language 与查询语言；该状态机不驱动 Run | Agent Runtime 的执行步骤或计划模式；TUI 展示；Workspace 生命周期；物理存储格式、路径和原子写机制 |
| **Project / Workspace** | Workspace 聚合与单一可变状态；worktree enter/exit/switch；frame/stack 隔离；git context 与 git 操作端口；Workspace Snapshot Published Language | 通用 shell/file Tool；Session 所有权；Task 业务状态；Agent Run；Snapshot 的物理落盘机制；项目代码本身的业务建模 |
| **Policy** | 权限规则、路径/能力约束与 `PolicyDecision`；工具执行前的准入判断 | 用户审批交互；Hook 执行；Tool 函数调用；Runtime 阻断后的控制流；Audit 记录 |
| **Audit** | 不可变 Audit Event；原始 usage 的审计语义、Cost/Pricing 与聚合；不可变事件 sink 策略、查询与派生投影；物理持久化经 Storage Port | 阻塞或驱动 Runtime；权限决策；普通诊断日志及 Logging sink；Provider 调用编排；业务实体状态 |
| **Tool & Skill & Command** | Tool Catalog/Execution；Registry Scope 与 capability Profile；ToolOutcome；Skill 发现与 PromptFragment 物化；Slash Command 解析/路由；MCP Tool adapter 与局部连接状态机；连接状态不驱动 Run | Policy/Hook/审批；timeout、跨 Tool 并发和 Run Step 编排；Context Window 注入策略；目标 BC 的查询/写入不变量；终端渲染 |
| **Workflow** | Reasoning Node 局部 effort 调节状态机、effort 推断与 `ReasoningPort`；应用 Config 提供的静态上限，并根据观察信号调节推理强度；该状态机不驱动 Run | 静态阈值和默认值所有权；Run 流程编排或执行状态机；SubAgent 图；Tool gate；Policy；模型调用；Slash Command 的写入所有权 |
| **Provider** | 各厂商 API ACL；统一 invoke/stream；模型能力声明；reasoning 参数映射；原始 token usage 提取（未定价）与供应商错误分类 | Run loop；Context Window 构建；跨调用业务重试/故障转移策略；Tool 执行；Cost/Pricing 聚合；UI 事件投影 |
| **Hook** | Hook 配置匹配；子进程执行、环境变量、timeout；输出解析；阻断/放行与 feedback 语义 | Hook 触发时机；阻断后的 Runtime 重试/停止编排；Policy 规则；Tool 执行；Audit 事件存储 |
| **Storage** | 原子读写、物理路径/文件格式 adapter、损坏检测与安全兜底、存储后端机制 | Session/Memory/Task/Workspace 等数据及 Snapshot 语义所有权；领域 schema 与业务迁移决策；retention/compact/eviction 业务策略；Run checkpoint |
| **Config** | 配置来源分层与优先级合并；校验；只读 ConfigSnapshot 与变更订阅；静态阈值和默认值单一真相 | 动态 Run/Session 状态；业务策略和行为；消费方内部状态；任意 BC 绕过 Snapshot 的散点 env 读取 |
| **Application Version Control** | aemeath 应用版本、更新渠道、版本检查、升级策略、校验与自更新；经 ConfigSnapshot 消费默认渠道、检查频率等静态值 | 静态默认值所有权；Project/Workspace 的 git 分支和 tag；Cargo 项目版本管理；milestone/release PR 编排；依赖升级策略 |
| **Logging** | 诊断日志 target 命名、统一 schema、过滤、路由、通用 log sink、rotation 与 retention 机制 | Audit Event 与不可变审计 sink；Cost/Usage；业务事件所有权；Policy 决策；把日志当领域持久化 |

### 4.2 判断规则

1. **状态所有权**：谁定义合法迁移和不变量，谁负责；观察、展示或持久化状态不等于拥有状态。Task、Workflow、MCP Connection 等局部状态机不得复制、驱动或替代 Run 的执行生命周期。
2. **策略与机制分离**：数据 BC 决定业务策略和 Snapshot 语义；Storage/Logging/Hook 等通用 BC 提供物理机制。
3. **编排与能力分离**：Agent Runtime 决定调用时机和控制流；Provider、Tool、Policy 等 BC 守护各自局部语义。
4. **数据、Snapshot 与投影分离**：领域聚合由对应 BC 拥有；BC 的可序列化 Snapshot / 派生视图属于其 Published Language，物理落盘经 Storage；TUI/Server/SDK 再消费 PL 形成交付层投影。
5. **跨界必须显式**：一项需求同时命中两个 BC 时，先确定不变量所有者，再通过 Port + Published Language / Event 集成；不得复制模型。典型分层包括 Provider raw usage → Audit Cost/Usage 聚合、Config 静态值 → 策略 BC 应用、数据 BC Snapshot → Storage 物理落盘。
6. **静态值与业务策略分离**：Config 拥有默认值、阈值和来源优先级；消费 BC 拥有如何使用这些值的业务行为。
7. **未归属先停设计**：若职责在表中无明确所有者，必须先更新本章程和 Context Map，再进入模块设计或代码实现。

> **交付层（非 BC）**：CLI / TUI / REPL / Server 是**入站适配器**，不占 BC 名额。SDK 是核心域的**入站端口 + Published Language**，所有权归 Agent Runtime。详见 [03-context-map.md](03-context-map.md) 与 [04-system-architecture.md](04-system-architecture.md)。

## 5. 关键设计约束

1. **唯一 Agent 执行生命周期状态机**：`Run` 是 Agent Runtime 中唯一描述 Agent 执行生命周期的状态机，且**内存态、不持久化、崩溃从头开始**。其他 BC 可以拥有守护自身局部聚合不变量的状态机（如 Task、Workflow、MCP Connection），但不得复制、驱动或替代 Run 的执行生命周期。Session 不是状态机，是数据聚合。
2. **Session 属 Context Management**：Session 的主体是对话历史（喂给 LLM 的上下文本体），因此归 Context Management，而非独立 BC。
3. **无 durable invocation**：aemeath 是人在环的交互式 CLI，崩溃后由用户重新发起、LLM 基于真实文件系统状态重新决策，副作用一致性由"人 + 文件系统"兜底。因此不做引擎级 durable checkpoint，仅保留 Run Step 级 session 快照。
4. **Loop 是模块非 BC**：Agent Loop 复杂性通过 Agent Runtime 内部模块拆分（Loop Engine + 各 Coordinator + 端口）解决，不制造伪 BC。
5. **Workflow 是支撑域 BC，非核心、非编排**：仅承载 reasoning effort 调节（reasoning graph），经端口被 Agent Runtime 消费；**不做多-agent 图编排（无此长期计划）**。v0.2.0 的"单 main + 多 sub"场景由 Agent Runtime 的 SubAgent 能力承担，不属编排。

## 6. 相关文档

- 统一语言：[02-ubiquitous-language.md](02-ubiquitous-language.md)
- 上下文地图与集成关系：[03-context-map.md](03-context-map.md)
- 系统架构与六边形：[04-system-architecture.md](04-system-architecture.md)
- 依赖规则与铁律：[05-dependency-rules.md](05-dependency-rules.md)
- 目录总览：[../README.md](../README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：产品目标、子域三分类、15 BC 清单、关键约束 | #760 |
| 2026-07-11 | 改为纯目标态（移除当前代码落点列）、文档引用链接化、新增修改历史 | #760 |
| 2026-07-11 | 术语改名：Agent Execution→Agent Runtime、AgentRun→Run | #760 |
| 2026-07-11 | Workflow 从核心域降为支撑域 BC（仅 reasoning 调节，不做编排）；核心域仅剩 Agent Runtime | #760 |
| 2026-07-12 | 为 15 个 BC 增加负责/不负责责任章程与判断规则；明确 Run 是唯一 Agent 执行生命周期状态机 | #743 / #787 |
