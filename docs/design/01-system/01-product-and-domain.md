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
| **Agent 执行（Agent Execution）** | 驱动"推理 → 工具 → 观察"循环、维护单次执行的状态机、编排模型调用与工具执行。这是产品的心脏。 |
| **编排（Workflow / Orchestration）** | 用图 / 状态机调节与编排 agent 行为：reasoning effort 阶段调节，以及多-agent 图编排（后者为 v0.2.0 Workflow Graph MVP 的目标）。 |

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

Bounded Context 是**解决方案空间**的边界，一个 BC 内部维持一套一致的模型与统一语言。本项目共 **15 个 BC**（2 核心 + 7 支撑 + 6 通用），与上述子域 1:1 映射。

| # | Bounded Context | 子域类型 | 目标职责 |
|---|---|---|---|
| 1 | **Agent Execution** | 核心 | Loop Engine、唯一状态机 AgentRun、tool / model / interaction 编排 |
| 2 | **Workflow / Orchestration** | 核心 | reasoning effort 阶段调节；多-agent 图编排（v0.2.0） |
| 3 | **Context Management** | 支撑 | Session 对话历史聚合、compact 家族、token 预算、记忆注入、prompt / guidance、会话身份与 resume |
| 4 | **Memory** | 支撑 | 记忆存取；Reflection 产出记忆建议 |
| 5 | **Task Management** | 支撑 | Task 聚合 + 状态机 + 依赖图不变量 |
| 6 | **Project / Workspace** | 支撑 | worktree、git 上下文 |
| 7 | **Policy** | 支撑 | 权限评估 |
| 8 | **Audit** | 支撑 | 审计事件；Cost / Usage / Pricing |
| 9 | **Tool & Skill & Command** | 支撑 | 内置 Tool、Skill、Slash 命令、MCP |
| 10 | **Provider** | 通用 | LLM 供应商 ACL、统一调用与流式 |
| 11 | **Hook** | 通用 | 生命周期钩子执行 |
| 12 | **Storage** | 通用 | 持久化机制（原子写、损坏兜底） |
| 13 | **Config** | 通用 | 分层配置、只读快照；reasoning 静态阈值 |
| 14 | **Application Version Control** | 通用 | 版本渠道、升级策略、自更新 |
| 15 | **Logging** | 通用 | 日志 target 路由与 schema |

> **交付层（非 BC）**：CLI / TUI / REPL / Server 是**入站适配器**，不占 BC 名额。SDK 是核心域的**入站端口 + Published Language**，所有权归 Agent Execution。详见 [03-context-map.md](03-context-map.md) 与 [04-system-architecture.md](04-system-architecture.md)。

## 5. 关键设计约束

1. **单状态机**：整个系统只有一个领域状态机——`AgentRun`（Agent Execution 内），且**内存态、不持久化、崩溃从头开始**。Session 不是状态机，是数据聚合。
2. **Session 属 Context Management**：Session 的主体是对话历史（喂给 LLM 的上下文本体），因此归 Context Management，而非独立 BC。
3. **无 durable invocation**：aemeath 是人在环的交互式 CLI，崩溃后由用户重新发起、LLM 基于真实文件系统状态重新决策，副作用一致性由"人 + 文件系统"兜底。因此不做引擎级 durable checkpoint，仅保留 turn 级 session 快照。
4. **Loop 是模块非 BC**：Agent Loop 复杂性通过 Agent Execution 内部模块拆分（Loop Engine + 各 Coordinator + 端口）解决，不制造伪 BC。
5. **Workflow 为 v0.2.0 预留**：reasoning graph 是 Workflow BC 的雏形；多-agent 图编排在 v0.2.0 扩展。设计上 AgentRun 的触发源必须抽象（用户输入 / 父 AgentRun / 未来编排器），以便 v0.2.0 加编排层时 Agent Execution 不改。

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
