# Feature #47：以 DDD 思路重新设计 Aemeath 架构

> **文档性质**：本 spec 是 **#47（DDD 基线）/ #61（架构债务收口）/ #62（audit/policy 落地）** 的共同**目标态架构基线**。
>
> **阅读约定**：本文**只描述目标态约束**，是判断"是否合规"的依据，不记录实现进展、迁移现状或历史债务——后者随代码演进、属易变信息，统一由 `docs/feature/active.md`、git history 和架构守卫脚本承载，不进入本文。**参考蓝本**：feature-boundary 分层参考 `wanaka-platform`（`services/core-api` 的 features/shared/composition + `context/` published-language 模式），见 §6.4。

## 1. 设计目标

本设计用 DDD（Domain-Driven Design）定义 Aemeath 的领域语言与边界，并以 feature-boundary + COLA（Clean Object-oriented & Layered Architecture）作为工程分层落地参考。它是后续重构、评审和功能设计的架构基线，不是一次性重构方案。

目标：

1. 明确 Aemeath 的核心域。
2. 建立 Agent Runtime 的统一语言。
3. 划分 Bounded Context，避免领域概念散落在技术分层中。
4. 定义 Context Map，明确上下文之间的依赖和防腐层。
5. 给 crate/module 重构提供判断标准。
6. 定义目标 workspace 目录与 feature 边界，使工程结构显式表达 Bounded Context。
7. 引入 feature-boundary + COLA 分层语言，明确 published language / open host service / adapter / application / domain / composition 的职责边界。

非目标：

1. 不在本设计直接移动 crate、拆文件或改运行逻辑；目录重排另按实施计划执行。
2. 不设计数据库 schema。
3. 不恢复 #36 已移除的 server/agents/proto/infra 运行代码。
4. 不把 DDD 等同于微服务化。
5. 不替换 #42 权限管控系统设计，而是在领域边界中吸收它。

## 2. 核心域判断

Aemeath 的核心域是：

> **Agent Runtime**

Agent Runtime 负责把一次用户输入推进成完整的 Agent 协作过程：构造上下文、调用模型、执行工具、调度 SubAgent、处理用户交互、维护任务进度、判断停止条件，并输出最终结果。

以下能力很重要，但不是核心域：

| 能力 | 定位 |
|---|---|
| Provider | provider 防腐层，把外部模型协议翻译为内部模型事件。 |
| Tool | 工具调用执行管线，把 ToolCall 转成受控 ToolResult。 |
| Project | 工程项目上下文，提供路径、worktree、配置来源等事实。 |
| Policy | 权限和风险判断支撑域。 |
| Audit | 独立审计域，记录谁在什么上下文中做了什么以及为什么。 |
| Hook | 生命周期事件到外部脚本的自动化适配。 |
| UI / Interface | CLI/TUI/REPL 适配层。定位为薄入口：通过 AgentClient SDK（`packages/sdk`）与 Agent Runtime 通信，不承载领域规则。 |

## 3. 统一语言

### 3.1 Agent Runtime 术语

| 术语 | 定义 | 需要避免的混淆 |
|---|---|---|
| Agent | 由 `ConfigurationSnapshot` 解析出的配置化执行者实体，定义 role、model profile、guidance profile、capability set、permission envelope、memory scope 和 collaboration policy。 | 不等同于模型、provider、一次执行或 messages 数组。 |
| Session | 用户与 Aemeath 的持续协作容器。 | 不等同于一次用户输入或持久化文件。 |
| Chat | 一次用户输入触发的完整处理单元。 | 不等同于一次模型调用；一个 Chat 可包含多次模型调用、工具执行和子代理执行。 |
| Agent Looping | Chat 内部的循环推进机制，协调模型调用、工具执行、SubAgent、用户交互、Task 更新和停止条件。 | 不等同于 UI event loop、tokio runtime 或 provider streaming loop。 |
| Turn | Agent Looping 中某个 Agent 针对一个目标的一次执行片段。 | 不等同于 Chat；Chat 是用户输入边界，Turn 是 Agent 执行片段。 |
| SubAgent | 由父 Turn 委托创建的 child Turn，通常使用不同 Agent 配置或 role。 | 不是独立 Session，也不能超过父 Turn 权限边界。 |
| Task | Agent Runtime 中用于规划和跟踪复杂工作的运行时状态，由 Agent Looping 创建、推进和完成。 | 不是独立 Bounded Context；持久化投影进入 Session History。 |

Agent Runtime 的核心结构：

```text
Session
  └── Chat
      └── Agent Looping
          ├── Main Turn
          ├── Child Turn*
          ├── ModelInvocation*
          ├── ToolExecution*
          ├── Task updates
          ├── AskUser pause/resume
          ├── Stop Hook
          └── Final response
```

> **AskUser 归属说明**：`AskUser pause/resume` 由模型发起的 `AskUserQuestion` 工具调用触发——消息协议上是一次 `tool_use → tool_result`，因此可视为一种特殊 ToolExecution。但 Agent Looping **特判接管**它：跳过普通工具执行管线，改以 pause/resume 等待用户回复，再把答案作为 `tool_result` 回灌。所以它在结构上与 `ToolExecution` 平级——**触发归 Tool，pause/resume 机制归 Runtime**。注意：由 Policy 触发的权限 Ask（`PermissionPrompt`，§8）是另一条**非工具**交互路径，二者不要混为一谈。

### 3.2 跨上下文术语

| 术语 | 所属上下文 | 定义 |
|---|---|---|
| ConfigurationSnapshot | Config | 多来源配置解析后的不可变运行配置快照。 |
| Project | Project Context | Agent 当前行动发生的工程项目上下文。 |
| ToolCall | Tool | 模型提出的工具调用请求。 |
| ToolInvocation | Tool | 经过 schema、上下文和权限准备后的可执行调用。 |
| ToolResult | Tool | 工具执行后可回灌给 Agent Runtime 的结构化结果。 |
| PermissionDecision | Policy | 安全策略对 actor/action/resource 的 Allow / Ask / Deny 等判断。 |
| HookDecision | Hook | 外部 hook 脚本对生命周期事件的 Continue / Block 等判断。 |
| AuditEvent | Audit | 记录运行、权限、hook、工具、provider 或最终 outcome 的审计事实。 |
| PromptContract | Prompt | Agent Runtime 可使用的行为指导和 prompt 组合契约。 |
| MemoryEntry | Memory | 跨 Session 保存的长期事实、偏好、决策、模式、陷阱或提醒。 |
| SessionRecord | Storage（Session History 子域） | Session 的持久化投影。Session History 是 Storage feature 内的子域，不是独立顶层 feature（§6.4.7）。 |

## 4. Bounded Context

### 4.1 Core Domain

#### Agent Runtime

职责：

- 维护 Agent、Session、Chat、Turn、Task。
- 在 Chat 内执行 Agent Looping。
- 调用 Provider、Tool、Memory、Prompt。
- 创建和调度 SubAgent。
- 维护 Task 状态机和 continue/resume 行为。
- 判断 Chat 完成、失败、取消或被阻止。

### 4.2 Supporting Domains

#### Config

职责：

- 统一加载 CLI 参数、环境变量、项目配置、Claude 兼容配置、全局配置和内置默认值。
- 解析为不可变 `ConfigurationSnapshot`。
- 为 Agent、Provider、Tool、Permission、Hook、Prompt、Memory、Logging 等提供 typed config view。

关键原则：运行时行为应基于配置快照，而不是各上下文自行读取文件或环境变量。

> **物理归属（三分法）**：Config 在领域上是一个 supporting domain，但工程落地拆三处，**不设独立 config feature**：
> 1. **契约 `ConfigurationSnapshot` 及各 typed view（schema）归 `shared/config/`**——它们稳定、协议无关、无编排逻辑、被几乎所有 feature 消费，属横切共享基础数据契约（§6.4.5）。
> 2. **加载分层编排（CLI/env/项目/全局/默认 的发现与合并）归 composition root / runtime bootstrap**——这才是"配置加载流程"，不得留在 `shared`。
> 3. 因此 `shared` 持有 config **schema** 合规（允许的是"配置数据契约"，禁止的是"加载流程"）。Config 也**不依赖 Project**——加载所需的项目路径由 composition 在编排时提供，不构成 `shared→project` 反向边。

#### Tool

职责：

- 管理 ToolCatalog、SkillCatalog 与 Slash Command catalog。
- 将 ToolCall / SkillInvocation / CommandInvocation 转换为受控执行。
- 执行 schema 校验、上下文解析、权限 gate、hook gate、能力 adapter 调用、输出截断和结果归一化。

#### Project Context

职责：

- 维护 project root、cwd、path_base、working_root、git branch、worktree stack。
- 发现项目级 instruction、config、skill、hook 来源。
- 提供路径和资源事实。

关键原则：Project 只提供事实，不做最终授权判断。

#### Policy

职责：

- 建模 PermissionRequest、PermissionDecision、PermissionGrant、PermissionMode、Capability、RiskAssessment。
- 判断 actor 是否可以对 resource 执行 action。
- 支持 #42 中的 AskMe / Auto / Plan / AllowAll 语义。
- 管理 SubAgent 权限继承规则。

关键原则：Policy 不执行 hook，也不负责审计持久化。

#### Audit

职责：

- 独立记录 Agent、Chat、Turn、ToolExecution、PermissionDecision、HookDecision、ModelInvocation 和 final outcome。
- 提供 correlation id，把 Session / Chat / Turn / Agent / Tool / Resource 串起来。

关键原则：Audit 只记录事实，不做权限判断，也不阻止执行。

#### Memory

职责：

- 管理 MemoryEntry、Reminder、ReflectionSuggestion。
- 支持长期知识检索、沉淀、置顶、完成提醒和范围隔离。

关键原则：Memory 不依赖 Prompt；由 Agent Runtime 编排二者。Memory 的持久化投影归 Storage。

#### Prompt

职责：

- 管理 GuidanceProfile、PromptContract 和 PromptMaterial。
- 读取并合并 AGENTS.md / CLAUDE.md / global AGENTS.md / guidance / system prompt fragments。
- 处理 model guidance、reasoning guidance、项目 instruction 和 system prompt 组合规则。

关键原则：Prompt 只负责模型指令材料加载、合并和裁剪；Skill / Slash Command 的注册、发现与执行归 Tools。

### 4.3 ACL / Infrastructure Domains

#### Provider

职责：

- 将内部 ModelRequest 转换为 provider request。
- 将 provider streaming chunk、tool call、usage、error 归一化为内部 ModelStreamEvent / ModelResponse。

关键原则：Provider Adapter 不直接暴露给 Agent Runtime；具体 SDK 调用归 `shared/adapter/provider/`。

#### Hook

职责：

- 把生命周期事件转换为 hook input。
- 执行外部 hook command。
- 解析 HookDecision。

关键原则：HookDecision 与 PermissionDecision 分离；外部脚本的子进程执行归 `shared/adapter/hook/`。

#### Session History

职责：

- 保存 Session、Chat、Turn、Task、ToolResult、Usage、Cost 等持久化投影。
- 支持恢复和查询。

关键原则：Session History 保存投影，不拥有 Agent Runtime 的运行规则。

#### External Adapters

包括 provider、filesystem、git、shell、web、MCP、terminal 等外部系统适配，统一归 `shared/adapter/<capability>/`。

## 5. Context Map

核心关系（依赖方向 = §6.4.7 feature 依赖图的领域视图，二者必须一致）：

```text
Interface
  → packages/sdk (AgentClient trait)
    → Agent Runtime                 # 唯一编排者
        → Config (shared/config schema)
        → Prompt
        → Provider
        → Tool
            → Project Context       # tools→project（worktree 上下文）
            → Storage               # tools→storage（memory 持久化）
            → Policy                # tool 执行前调用 policy gateway
            → Audit                 # tool 执行后写 audit gateway
        → Hook                      # 生命周期 gate 由 Runtime 编排
        → Audit
        → Storage                   # Session History / Memory / Task / Cost 持久化
```

> **gate 编排说明**：权限/hook gate 的*编排*由 Agent Runtime 完成——Runtime 在调用 Tool 前后串联 Policy/Hook/Audit。Tool 在执行管线内可直接调用 `policy::api` / `audit::api` 的 gateway（§6.4.7 已批准的横向 feature 依赖），但停止条件等生命周期 gate 由 Runtime 统一编排。

补充关系：

```text
Project Context
  → discovers project roots / config sources / instruction sources / skill-hook paths

Config
  → 加载分层（CLI/env/项目/全局/默认）由 composition root / runtime bootstrap 编排
  → produces ConfigurationSnapshot（不可变快照，作为共享契约归 shared/config，被各 feature 消费）

Prompt
  → consumes ConfigurationSnapshot
  → Project Context
  → produces PromptContract

Memory
  independent from Prompt
  coordinated by Agent Runtime
  persisted by Storage

Audit
  observes Agent Runtime / Tool / Policy / Hook / Provider events

Session History
  persists projections from Agent Runtime（驻留 Storage）
```

## 6. 入口与包边界设计要求

DDD 设计要求入口层保持薄，核心业务规则必须位于 Agent Runtime 及其支撑上下文中，而不是散落在 HTTP、CLI 或 TUI 中。

### 6.1 薄入口

CLI、TUI、REPL 等都属于 inbound adapter。它们可以负责：

- 解析终端输入。
- 将用户输入转换为应用服务命令。
- 展示 streaming event、状态投影、权限询问和最终响应。
- 管理终端、连接等入口相关细节。

它们不应该负责：

- Agent Looping。
- Task 状态机。
- PermissionDecision。
- Tool Execution pipeline。
- Project 路径和 worktree 规则。
- Memory / Skill / Guidance 组合规则。

### 6.2 统一应用服务

所有入口都应接入 `runtime` feature 暴露的同一组入口无关 API，例如：

- start session
- handle chat
- resume chat
- cancel chat
- stream runtime events
- answer interaction request
- apply permission choice

`runtime` 是唯一编排者，负责把入口命令编排到 Project、Policy、Prompt、Provider、Tools、Storage、Hook、Audit 等 supporting domains。HTTP、CLI、TUI 不应各自复制核心流程，也不应直接依赖 supporting domain。`ChatApplicationService` 只做薄校验与分发，不直接调用 `repl`/`tui::App`。

### 6.3 协议无关事件模型

Agent Runtime 和相关上下文应输出协议无关事件，例如：

- RuntimeEvent
- InteractionRequest
- PermissionPrompt
- ToolExecutionEvent
- AuditEvent

TUI 渲染、CLI stdout、HTTP SSE/WebSocket 都只是这些事件的不同 projection。

### 6.4 Agent 目录 feature-boundary 结构（目标态）

`agent/` 采用 **feature-boundary 纵向切分**（参考 `wanaka-platform` 的 `services/core-api`）：DDD feature boundary 决定外部边界，COLA 只负责每个 feature 内部的工程分层。每个 feature 是一个 Bounded Context，内部私有，**只有发布语言（`contract` + `gateway`，经 `api.rs` 暴露）可跨边界访问**。

#### 6.4.1 顶层结构

```text
agent/
  features/             # 业务 feature boundary，按能力纵向切分（每个 = 一个 Bounded Context）
    runtime/            # Agent Loop / turn / session state / compact / cost
    tools/              # Tool + Skill + Slash Command 能力注册与执行
    provider/           # LLM provider gateway（协议归一化）
    prompt/             # AGENTS.md / CLAUDE.md / guidance / system prompt material
    project/            # cwd / paths / worktree / git facts
    storage/            # session / memory / task / history 持久化投影
    policy/             # permission / risk 判断
    hook/               # 生命周期事件 → hook input → HookDecision
    audit/              # 审计事件 / 操作轨迹
  shared/               # 横切基础设施、横切 port、外部 adapter、shared kernel（ids/errors/types/config schema）
  composition/          # 组合根：唯一生产装配入口

packages/
  sdk/                  # AgentClient trait + 公共类型（CLI 与 Runtime 通信契约，非业务 feature）
  global/
    logging/            # 日志 projection 适配
```

语义：

1. `features/` 是业务 feature boundary。每个 feature 拥有自己的对外语言（`contract`）、对外服务入口（`gateway`）、内部编排与领域规则。
2. `shared/` **不是 Minimal Shared Kernel 的全部**；它是跨 feature 共享的基础设施、横切能力 port、外部系统 adapter，以及保持极小的 shared kernel（`ids`/`errors`/`types`/`config` schema）。
3. `composition/` 是 composition root，负责把 `features/*`、`shared/*`、`shared/adapter/*` 装配成可运行应用，是唯一知道"全部 feature 存在"的地方。
4. `packages/sdk` 是入口层与 Runtime 的外部通信契约，不是业务 feature。

#### 6.4.2 Feature 内部模板

每个 feature 内部统一使用 COLA 分层：

```text
agent/features/<feature>/src/
  contract/             # Published Language：DTO / Event / Command / Query（稳定对外数据契约）
  gateway/              # Open Host Service：该 feature 对外服务入口（trait + wire 工厂）
  core/                 # 内部编排 / use case / port
  business/             # 内部规则 / 领域模型 / 状态机 / 不变量
  utils/                # feature 私有工具
  api.rs                # 跨 feature 唯一出口，只 re-export contract + gateway
  lib.rs
```

约束：

1. `contract` 是 Published Language——跨 feature 共享的稳定 DTO / Event / Command / Query。
2. `gateway` 是 Open Host Service（OHS）——feature 对外开放的稳定服务入口 trait，及其 `wire_<feature>()` 装配工厂（供 composition 调用）。
3. `api.rs` 是跨 feature 的统一出口，**只允许 re-export `contract` 与 `gateway`**。
4. 跨 feature **禁止**直接依赖对方的 `core`、`business`、`utils`，也禁止绕过 `api.rs` 直接访问对方 `contract` / `gateway` 路径。
5. **按职责分层，不强制凑满五层**：只建实际有职责的层，无内容的层不建空目录。简单 feature 可能只有 `contract` + `business`；完整分层仅 runtime/tools 等真正需要的 feature。层名一旦出现必须取自上述固定集合。
6. Feature 内部不设 `acl/` 目录；外部协议、旧模型和第三方系统适配统一进入 `shared/adapter/*`。

允许 / 禁止示例：

```text
# 允许：只经对方 api
runtime -> tools::api::{ToolGateway, ToolCall}
tools   -> policy::api::{PermissionGateway, PermissionRequest}
prompt  -> project::api::{ProjectGateway, ProjectContext}

# 禁止：穿透到对方内部层 / 绕过 api
runtime -> tools::core::Dispatcher
runtime -> tools::business::BuiltinTool
runtime -> tools::utils::PathSecurity
runtime -> tools::gateway::ToolGateway   # 必须统一经 tools::api
```

#### 6.4.3 Published Language、OHS 与 Context Map 关系类型

跨 feature 协作必须落到一个明确的 DDD 关系类型上，并固定其工程位置：

| DDD 关系类型 | 何时使用 | 在本项目的落地位置 |
|---|---|---|
| **Open Host Service / Published Language** | 默认：跨 feature 的同步 query / command | supplier 拥有 `<feature>/gateway/<port>.rs` + `<feature>/contract/*`；consumer 经 `<feature>::api::*` 调用 |
| **Anti-Corruption Layer** | consumer 想用自己的词汇而非 supplier 的模型 | consumer 内部 wrapper（`<consumer>/core/` 下的防腐转换），不污染 supplier |
| **Shared Kernel** | 两个 context 都同意的极小通用类型 | `shared/ids.rs`、`shared/types.rs`、`shared/errors.rs`、`shared/config`（保持极小） |
| **Domain Events（异步发布语言）** | supplier 需在状态变化时通知 | `<feature>/contract/events.rs` + runtime 编排的事件分发（暂以同步 port 表达，事件总线后续再引入） |
| **Conformist** | consumer 必须原样接受外部系统模型 | 仅对外部系统（provider/MCP/git/shell），归 `shared/adapter/*` |

原则：跨 feature 同步协作**默认走 OHS（gateway port）**；只有当 consumer 需要隔离 supplier 词汇时才加 ACL；真正通用的极小类型才进 shared kernel。

#### 6.4.4 Feature 边界职责

```text
runtime
  负责 Agent Loop / Chat Loop / turn 编排 / session state / context window / compact / cost / reflection / interrupt / resume。
  不负责 tool/skill/command 注册，不负责 provider 协议适配，不负责 prompt 文件扫描。

tools
  负责 built-in tools、MCP tools、skills、slash commands 的注册、发现、metadata 与执行。
  执行前调用 policy gateway，执行后写 audit gateway。

provider
  负责 LLM provider 访问、streaming response 解析、model profile、provider pool / fallback / retry、usage 解析。
  不负责 Agent Loop、Prompt 组装、Tool 执行或最终成本规则。具体 SDK 调用归 shared/adapter/provider。

prompt
  负责 AGENTS.md / CLAUDE.md / global AGENTS.md / guidance / model-specific guidance / reasoning guidance / system prompt fragments 的加载、合并和裁剪。
  不负责 skill 或 slash command 注册执行。

project
  负责 cwd、workspace root、project root、worktree、branch、git facts、路径安全基础事实和项目级配置路径发现。
  不读取 prompt 内容，不执行工具，不做权限判断。

storage
  负责 session、memory、task、history、cost_history、tool result 等持久化投影。
  不拥有 Agent Runtime 的 task 状态机、memory 召回策略或成本规则。

policy
  负责 PermissionRequest -> PermissionDecision，建模 risk / confirmation / deny / inherited permission。
  不执行 tool，不写 audit，不修改 runtime 状态。

hook
  负责生命周期事件 → hook input 映射、HookDecision 解析。子进程执行归 shared/adapter/hook。
  不判断 capability，不做权限继承。

audit
  负责记录操作事实、permission decision、tool/command/skill 执行事件，提供 correlation id。
  不判断 allow/deny，不执行工具，不修改 runtime 状态。
```

#### 6.4.5 Shared 语义

`shared/` 是跨 feature 共享的基础设施层，包含 shared kernel、横切能力 port 与所有外部 adapter：

```text
agent/shared/src/
  adapter/              # 所有具体 adapter（端口实现），仅 composition 可 import
    provider/
    mcp/
    filesystem/
    process/
    git/
    storage/
    hook/
    telemetry/
  logger/               # 横切能力 port
  telemetry/
  config/               # ConfigurationSnapshot + typed view（schema，非加载流程）
  filesystem/
  process/
  git/
  http/
  json/
  ids.rs                # shared kernel：稳定 id 类型
  errors.rs             # shared kernel：统一错误
  types.rs              # shared kernel：极小通用 value object
  lib.rs
```

规则：

1. 横切能力 **port** 放 `shared/<capability>/`；具体 **adapter** 一律放 `shared/adapter/<capability>/`。
2. 聚合自有能力的 port 放 `features/<feature>/src/core/`；其 adapter 仍放 `shared/adapter/<capability>/`。判断标准：能力属于某个业务聚合则 port 归该 feature，否则（日志、配置、文件系统、进程、git、HTTP、clock、id generator 等横切基础能力）归 `shared/<capability>/`。
3. `shared` 除 `shared/adapter/**` 与必要的 `shared/types.rs` 例外外，**不反向依赖 `features/**`**。
4. Feature 代码**不能**直接 import `shared/adapter/**`。
5. 生产代码中**只有 `composition`** 可以 import `shared/adapter/**`；测试可按需使用 fake / test adapter。
6. `shared` kernel（ids/errors/types/config schema）**只能放数据契约，不得承载行为或流程**——Chat、Tool pipeline、配置加载流程、权限评估、hook 执行、有状态 registry/store（ToolRegistry/TaskStore/MemoryStore）、并发原语（`Arc<Mutex>`/`Semaphore`/`CancellationToken`/`mpsc`）、时间或 IO（`SystemTime::now`/`Uuid::now_v7`/fs/process/network）一律不得驻留 shared kernel；它们归对应 feature 或 `shared/adapter`。

#### 6.4.6 Composition Root 与依赖装配

`composition/` 是唯一生产装配入口（对应 wanaka 的 `entry-node.ts`）：

```text
agent/composition/src/
  app.rs                # 顶层装配，构造可运行应用
  runtime.rs            # wire runtime feature
  tools.rs / provider.rs / prompt.rs / project.rs / storage.rs / policy.rs / hook.rs / audit.rs
  context.rs            # 装配 AppContext / RuntimeContext
  lib.rs
```

职责：

1. 创建 shared 基础设施实现与 shared adapter。
2. 调用每个 feature 的 `wire_<feature>()` 工厂创建 gateway / service。
3. 注入依赖并组装 AppContext / RuntimeContext。
4. 为 CLI/TUI/server 等入口暴露启动入口。

`composition` **不承载**业务规则、provider 协议转换细节、工具执行细节、权限判断规则或 prompt 合并规则。装配方向：

```text
apps/cli
  -> agent/composition
      -> features/*/api      # 经各 feature 的 wire 工厂 + gateway
      -> shared/*            # 横切 port
      -> shared/adapter/*    # 具体 adapter（仅此处可 import）
```

> **DI 约定**：每个发布 gateway 的 feature 提供一个 `wire_<feature>(deps)` 工厂返回其 gateway 实现；composition 逐 feature 调用并汇总成统一上下文。不引入运行时 DI 容器、不按目录自动发现（避免启动顺序隐式化与非确定性）。

#### 6.4.7 依赖规则

```text
features/* 可以依赖 shared 横切 port。
features/* 可以单向依赖其他 feature 的 api.rs，但禁止循环依赖。
features/* 不能直接依赖其他 feature 的 contract / gateway / core / business / utils 路径（只能经 api.rs）。
features/* 不能直接依赖 shared/adapter/**。
shared/<capability> 原则上不依赖 features/**。
shared/adapter/** 可以依赖它实现的 feature-owned port。
composition 可以依赖 features/*/api、shared/*、shared/adapter/*。
features/* 和 shared/* 都不能依赖 composition。
任何 feature 都不能依赖 apps/cli。
```

推荐 feature 依赖层级（即 Context Map §5 的工程视图）：

```text
runtime
  -> tools::api / provider::api / prompt::api / project::api / storage::api / policy::api / hook::api / audit::api

tools
  -> project::api / storage::api / policy::api / audit::api    # 已批准的横向依赖

prompt
  -> project::api / storage::api

provider / project / storage / policy / hook / audit
  -> shared only
```

> **横向 feature 依赖的批准标准**：supporting feature 默认只依赖 `shared`；横向依赖另一 feature 必须满足——(1) 方向无环；(2) 经对方 `api`；(3) 在本 spec 登记原因与替代方案。已批准：`tools → project`（worktree 上下文，git+fs 行为不宜进 shared）、`tools → storage`（memory 持久化复用同一 store，避免 DRY 违规与 13 方法透传 trait 的无收益抽象）。

`apps/cli` 只能直接依赖 `packages/sdk`、`agent/composition`（composition root 装配）和纯技术库，**不得**直接依赖 supporting feature、`agent/shared` 或其他业务 crate。

#### 6.4.8 架构守卫

feature 边界必须由守卫脚本强制（对应 wanaka 的 `.dependency-cruiser.cjs` 规则集），而非仅靠人工约定。守卫应基于 **package name / 路径正则**而非目录字符串，避免移动目录后规则失效，并在 Stop hook 中执行；任何违规都应阻止完成。守卫须覆盖：

| 守卫维度 | 规则 |
|---|---|
| feature 跨界 import | feature A 只能经 `<feature B>::api::*` 访问 B；禁止 import B 的 contract/gateway/core/business/utils 直接路径 |
| 绕过 api | 跨 feature 必须经 `<feature>::api`，不得直连 `gateway`/`contract` 路径 |
| feature 依赖环 | 禁止 feature dependency cycle |
| adapter 隔离 | 生产代码中只有 `composition` 可 import `shared/adapter/**`；feature 不得直接 import |
| shared 不依赖 feature | `shared`（除 `shared/adapter/**` 与必要 `shared/types.rs`）禁止 import `features/**` |
| shared kernel 纯度 | `shared` kernel 不得出现 store/IO/行为/并发原语/时间（覆盖 `async_trait`、`Arc<Mutex>`、`tokio::sync::*`、`CancellationToken`、`SystemTime::now`、`Uuid::now_v7`、fs/process/net）——§6.4.5 rule6 |
| 单向向上依赖 | feature 内数据层（`utils`/repo/io）不得反向 import `core`/`gateway` 编排层 |
| COLA 分层纯度 | 层间纯度：`business` 不依赖 `core` 编排、`utils` 不含领域规则 |
| CLI 薄入口 | `apps/cli` 只直接依赖 `composition` + `sdk` + 纯技术库；源码不绕过 `sdk::AgentClient` |
| 单文件行数 | `apps/`、`agent/` 下单 `.rs` ≤ 400 行 |

> 守卫脚本的具体集合随实现演进，以 `.agents/hooks/check-architecture-guards.sh` 实际聚合为准；本表定义其**必须覆盖的约束维度**，不是脚本清单。

### 6.5 Chat 启动边界对象化

入口启动依赖应表达为稳定的 application 边界对象（context + options + mode-specific launch DTO），HTTP/SDK 接入时复用同一组对象，不复制 CLI/TUI 专属参数结构；`ChatApplicationService` 只校验和分发，不直接调用 `repl`/`tui::App`。这些边界对象定义在 `runtime` feature 的 `contract`，由 `gateway` 消费。

### 6.6 AgentClient SDK

AgentClient 是 Agent Runtime 对外暴露的统一客户端 SDK，trait + 公共类型定义在 `packages/sdk/`，实现在 `runtime` feature。它是 CLI（薄入口）与 Agent Runtime 之间的**唯一通信契约**。

**trait/impl 分层**：`packages/sdk` 只放 trait + 公共类型，零业务依赖：

```rust
// packages/sdk/src/lib.rs
#[async_trait]
pub trait AgentClient: Send + Sync + 'static {
    // ── 只读视图：快照（无锁，永不阻塞），不返回内部业务类型引用 ──
    fn session_snapshot(&self) -> SessionSnapshot;       // cheap clone
    fn cost(&self) -> CostInfo;                          // Atomic 读取
    fn task_list(&self) -> Vec<TaskSummary>;             // 快照
    fn project(&self) -> ProjectContext;                 // Copy 值类型
    fn changes(&self) -> watch::Receiver<ChangeSet>;     // 变更通道（只推标记）

    // ── 写操作 ──
    async fn chat(&self, input: ChatInput) -> Result<ChatStream>;
    fn cancel(&self);
    async fn save_session(&self) -> Result<()>;
    async fn load_session(&self, id: &SessionId) -> Result<()>;
    async fn list_sessions(&self) -> Result<Vec<SessionSummary>>;
    async fn delete_session(&self, id: &SessionId) -> Result<()>;
    async fn compact(&self) -> Result<CompactResult>;
}
```

关键约束：

1. **不暴露内部业务类型引用**。trait 只返回 sdk 自有的快照/值类型（`SessionSnapshot`、`CostInfo`、`TaskSummary`、`ProjectContext`、`ChangeSet`），由 runtime 投影填充。这样 sdk 保持**零业务依赖**——若返回 `&Session`/`&TaskStore`，sdk 就被迫依赖 storage/runtime，且 `&` 引用要持锁阻塞 TUI 帧。
2. **异步方法 MUST 用 `#[async_trait]`**（裸 `async fn` in trait 不满足 dyn-safe）。trait 不带 `Clone` bound——`dyn AgentClient` 不能要求 `Clone`；多态共享用 `Arc<dyn AgentClient>`。
3. **`new()` 不在 trait 里**——不同部署模式（真实 Runtime vs Mock）需要不同构造签名，trait 只管运行时行为。
4. **初始化编排归 runtime**：所有 build_provider / build_llm / build_tooling / build_agent_runner / hooks / logging / session 创建恢复 / system prompt 组装归 `AgentClientImpl::new()`（在 runtime feature），CLI 不承载。

**只读视图用快照而非引用**：`&Session` 做不到无锁（`RwLock::read()` 被写方持有就会卡 TUI 帧）。TUI 需要的是即时快照 + 变更通道。变更通道只推标记不推数据：

```rust
bitflags::bitflags! {
  #[derive(Clone, Copy, Debug)]
  pub struct ChangeSet: u8 {
      const SESSION = 0b0001;
      const COST    = 0b0010;
      const TASKS   = 0b0100;
      const PROJECT = 0b1000;
  }
}
```

Runtime 在 session/cost/tasks/project 变更时 `change_tx.send(...)`；CLI 侧 `changes().changed()` 唤醒后按标记拉取对应快照。

| 方案 | 问题 |
|------|------|
| `Arc<RwLock<Session>>` 传 CLI | 打破 trait 边界——CLI 知道内部数据布局 |
| `&Session` 引用 | 需要持有锁，阻塞渲染 |
| `SessionSnapshot` + `watch` | 快照无锁、CLI 不知道内存布局、变更精确 |

**ChatStream 设计**——mpsc 而非 Stream trait（TUI 需 `recv().await` 阻塞等待，终端事件循环是轮询模型）：

```rust
pub enum ChatEvent {
    Token(String),
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, delta: String },
    ToolCallEnd { id: String },
    ToolResult { id: String, content: String },
    PermissionRequest(PermissionPrompt),
    Status(StatusInfo),
    Done(ChatResult),
    Error(AemeathError),
}

pub struct ChatStream { rx: tokio::sync::mpsc::UnboundedReceiver<ChatEvent> }
impl ChatStream {
    pub async fn recv(&mut self) -> Option<ChatEvent> { self.rx.recv().await }
}
```

**cancel 机制**：`cancel()` 置 `AtomicBool`，chat() 内部每处理一个 ChatEvent 检查，无需 AbortHandle。

核心原则：

```text
CLI TUI/业务代码只知道 AgentClient（trait）
AgentClientImpl 只是 RuntimeHandle 的薄代理
Runtime 拥有全部初始化逻辑和编排能力
```

CLI 的 TUI/业务代码只依赖 `packages/sdk` 契约，不直接依赖 `agent/` 的任何 feature；单二进制部署下，composition root 经 `agent/composition` 装配真实 `AgentClientImpl`。

### 6.7 CLI 薄边界（目标态）

CLI 只做三件事——**解析启动参数、加载配置、在 composition root 构造 AgentClient 并启动 TUI/REPL 循环**。所有 Runtime 初始化编排归 `AgentClientImpl::new()`（runtime feature），CLI 不承载。TuiApp 只注入单个 `Arc<dyn AgentClient>`，不再注入散落的 runtime 内部对象。

```text
CLI                          AgentClient (runtime feature)
├── parse args               AgentClient::new(config, args)
├── load config              │  ├── build provider / llm / tooling / agent_runner
└── run_tui(client)          │  ├── init hooks, logging, session
                             │  └── return AgentClient
   client.session_snapshot() / cost() / task_list() / project() / changes() / chat(input)
```

TUI 与 runtime 的唯一通道是 `AgentClient`——`ChatInput` 只含 TUI 拥有的 `messages`；`cwd`/`workspace_context`/`read_files`/`session_reminders`/`CancellationToken`/`TaskStore`/`LlmClient`/`ToolRegistry`/system prompt/并发限制/hook/logger 均为 runtime 内部状态，不进入 SDK request。TUI 只把 `ChatEvent` 映射为 `UiEvent`、经队列端口提供排队输入、经 `cancel()` 取消、经 `task_list()` 渲染任务、经 `save_session()` 保存。

## 7. COLA 工程分层规范

DDD 用于回答领域边界和统一语言是什么，COLA 用于约束代码如何分层落地：先按 DDD 确定 feature boundary，再用 COLA 风格组织 published language、application、domain 和 adapter。

### 7.1 分层定义

| COLA 层 | 职责 | Aemeath 对应 |
|---|---|---|
| Adapter | 接收外部输入，做协议转换和展示投影。 | `apps/cli` 的 CLI command、TUI event handler、REPL adapter；`shared/adapter/*` 外部系统适配；未来 HTTP endpoint。 |
| Application | 编排用例流程，调用领域上下文和端口，不承载核心业务规则。 | `runtime` feature 的 application service：session/chat、resume、cancel、permission choice、runtime event stream。 |
| Domain | 表达领域模型、聚合、不变量、领域服务和端口定义。 | 各 feature 的 `business`（领域规则）+ `core`（端口）模块。 |
| Infrastructure | 实现外部系统适配和 I/O 细节。 | `shared/adapter/*`：provider SDK、filesystem、git、shell、web、MCP、hook runner、session storage。 |
| Published Language / OHS | 定义对外契约和服务入口。 | 各 feature 的 `contract`（DTO）+ `gateway`（服务 port），经 `api.rs` 暴露；`packages/sdk` 的 AgentClient。 |

### 7.2 目标映射

```text
apps/cli
  → adapter（薄入口）

agent/features/runtime/{contract,gateway}
  → published language + application facade（runtime::api）

agent/features/runtime/{core,business,utils}
  → 核心域：core(编排+端口) / business(领域规则) / utils(私有工具)

agent/features/{tools,provider,prompt,project,storage,policy,hook,audit}/{contract,gateway} + 按需的 {core,business,utils}
  → supporting domains（按职责分层，无内容的层不建；层名取自固定集合）

agent/shared
  → shared kernel + 横切 port + 外部 adapter（infrastructure）

agent/composition
  → composition root（依赖装配，无业务规则）
```

### 7.3 COLA 约束

1. Adapter 层必须薄，只处理协议、终端、UI、连接生命周期和结果展示。
2. Application 层负责编排 Chat、resume、cancel、permission choice、interaction answer 等用例。
3. Domain 层（feature `business`/`core`）拥有业务规则和不变量，不依赖 HTTP、CLI、TUI、数据库、文件系统或 provider SDK。
4. Infrastructure 层（`shared/adapter`）只能通过 feature/shared 定义的 port 或 gateway 接入。
5. Published Language / OHS 层只定义契约和服务入口，不实现领域规则。
6. DTO / Command / Response 不应泄漏为领域实体。
7. 领域事件和 runtime event 应保持协议无关，再由不同 Adapter 投影为 TUI、CLI、HTTP SSE/WebSocket 或 SDK 输出。

## 8. PermissionDecision、HookDecision 与 Audit

`PermissionDecision` 和 `HookDecision` 必须分离。

| 维度 | PermissionDecision | HookDecision |
|---|---|---|
| 来源 | 内部安全模型 | 用户配置的外部脚本 |
| 规则归属 | Policy | Hook |
| 输入 | actor / action / resource / risk / grant | hook event JSON |
| 输出 | Allow / Ask / Deny | Continue / Block |
| 是否影响 capability | 是 | 否 |
| 是否用于权限继承 | 是 | 否 |

执行链路：

```text
ToolCall
  → PermissionRequest
  → PermissionDecision
  → PreToolUse HookRun
  → HookDecision
  → ToolExecution
  → PostToolUse HookRun
  → Audit
```

Stop 阶段：

```text
Agent wants to stop
  → Stop HookRun
  → HookDecision
  → final outcome
  → Audit
```

这允许系统明确表达：

> 权限允许，但 hook 阻止。

例如工具可以被 Policy 允许，但 Stop hook 仍可因架构检查失败而阻止完成。Audit 需分别记录 policy decision、hook decision 和 final outcome。

## 9. 聚合草案

本节只定义高层聚合，详细字段留给后续重构计划。聚合根名与 §3 术语表对齐；下表新引入而 §3.2 未列的术语在「术语补充」中给出定义。

| Bounded Context | Aggregate Root |
|---|---|
| Runtime | `AgentDefinition`（即 §3.1 的 Agent 配置实体）、`Session`、`Chat`、`TaskBoard` |
| Project | `ProjectContext` |
| Policy | `PermissionSession`、`GrantSet` |
| Prompt | `PromptContract`、`SkillCatalog`、`GuidanceProfile` |
| Provider | `ModelInvocation` |
| Tools | `ToolCatalog`、`ToolExecutionBatch` |
| Storage | `SessionRecord`、`MemoryCollection` |
| Hook | `HookRun` |
| Audit | `AuditTrail` |

**术语补充**（§3.2 未列、本节首次作为聚合根出现的）：

| 术语 | 所属上下文 | 定义 |
|---|---|---|
| `AgentDefinition` | Runtime | §3.1「Agent」的聚合根表述：role/model profile/guidance/capability/permission envelope/memory scope/collaboration policy 的配置实体。 |
| `ModelInvocation` | Provider | 一次模型调用的聚合：request 组装、streaming、usage、停止原因与归一化结果。 |
| `MemoryCollection` | Storage | `MemoryEntry` 的集合聚合，维护范围隔离、去重、置顶与检索一致性。 |
| `TaskBoard` | Runtime | task 状态机 + blocked_by/blocks 依赖 + 完成条件的聚合（持久化投影入 Storage）。 |

### Runtime 聚合

`AgentDefinition`：配置化 Agent 的领域定义，包含 role、model profile、guidance profile、capability set、permission envelope、memory scope 和 collaboration policy。

`Session`：Runtime 主聚合根，维护多个 Chat、全局 usage summary 和 recovery state 的一致性。

`Chat`：一次用户输入触发的完整处理聚合，维护 Chat 状态、Turn 列表、Tool batch 生命周期、Model invocation 生命周期、ask-user pause/resume、stop condition 和 final response。

`TaskBoard`：规划任务聚合，维护 task 状态流转、blocked_by / blocks 依赖、task list 完成条件和 continue/resume 恢复规则。

`ProjectContext`：Project 聚合根，维护 cwd、path_base、workspace root、worktree stack、git branch、项目级 config / instruction / skill 来源。其不变量包括：path_base 必须属于当前 project/worktree 语义边界，EnterWorktree / ExitWorktree 必须成对维护 stack，Bash 更新 cwd 后必须同步 path_base。

`PermissionSession` / `GrantSet`：Policy 聚合根，维护 grant scope、capability、expiration、actor inheritance、AskMe / Auto / Plan / AllowAll 不变量。

聚合根对外只能通过各 feature 的 `api`（gateway）接收 command/query；外部不得直接修改聚合内部状态。状态变化应产出 domain event，再由 runtime 编排 storage、audit、hook 或 adapter projection。

Runtime 重要不变量：

1. 一个 Session 可以包含多个 Chat。
2. 一个 Chat 对应一次用户输入触发的完整处理流程。
3. Agent Looping 只存在于 Chat 内部。
4. 一个 Chat 至少创建一个 Main Turn，除非创建前失败或取消。
5. 一个 Turn 必须引用一个 AgentDefinition。
6. child Turn 权限不得超过 parent Turn。
7. Agent 是配置化实体，不保存单次执行状态。
8. Task 由 Agent Looping 创建、推进和完成，TaskBoard 统一维护任务状态机和依赖不变量。
9. Task 持久化投影进入 Storage。
10. Audit 使用 SessionId / ChatId / TurnId / AgentId 建立链路。

## 10. 关键设计决策

1. Runtime 是核心域。
2. Agent 是配置化实体。
3. Agent Looping 属于 Chat 内部。
4. Turn 是某个 Agent 的一次执行片段。
5. Task 属于 Runtime，由 Agent Looping 推进，TaskBoard 维护状态机和依赖不变量。
6. Task 持久化投影进入 Storage。
7. Project 是独立 supporting domain（feature `project`）。
8. Tools 是独立 supporting domain（feature `tools`）。
9. Policy 独立。
10. Audit 独立。
11. PermissionDecision 与 HookDecision 分离。
12. Prompt 独立，统一承载 skills、guidance、instruction 与 system prompt 组合规则。
13. Storage 统一承载 session history、memory、cost history 与 task persistence。
14. Provider 是 provider ACL，不是核心域；具体 SDK 调用归 `shared/adapter/provider`。
15. Hook 是生命周期自动化适配，不是权限模型。
16. CLI/TUI/HTTP/SDK 入口必须保持薄，通过 `packages/sdk::AgentClient` 契约接入 Runtime。
17. `apps/cli` 严格只直接依赖 `packages/sdk`、`agent/composition`（composition root 装配）和纯技术库，不直接依赖 `shared` 或 supporting features。
18. Runtime 是唯一编排者，可以依赖 supporting features。
19. Supporting features 默认只依赖 `shared`，横向依赖必须经对方 `api` 并进入 §6.4.7 批准清单。
20. `shared` 是横切基础设施 + 极小 shared kernel，**不是所有领域概念的混合仓库**；kernel 部分只放数据契约，不得承载行为/IO/并发/时间。
21. 目标 workspace 采用 `apps/`、`agent/{features,shared,composition}`、`packages/` 顶层目录；crate 名不添加 `aemeath-` 前缀。
22. feature-boundary 决定外部边界，COLA 是 feature 内部工程落地参考，不替代领域建模。
23. Published Language（contract）/ OHS（gateway）/ Application / Domain / Infrastructure / Composition 的职责必须分离。
24. Cargo dependency graph、forbidden import、feature api 边界、adapter 隔离、shared kernel 纯度和 Stop hook 必须共同防止双向依赖与边界绕过。

## 11. 与既有 feature 的关系

| Feature | 关系 |
|---|---|
| #36 Multi-Agent 框架 | 只参考历史 DDD 设计，不恢复已移除的分布式 server/agents/proto/infra。 |
| #40 Claude 优先兼容 | 归入 Project、Prompt 的 source discovery / compatibility ACL；配置快照由 Runtime 通过 Project/Prompt gateway 获取。 |
| #42 权限管控系统 | Policy feature 的主要设计来源；Audit 独立后补足审计边界。 |
| #43 worktree cwd 同步 | 归入 Project 的 path_base / working_root / worktree 一致性规则。 |
| #45 EnterWorktree / ExitWorktree | 归入 Project 与 Tools 的上下文切换能力。 |
| #46 TUI status line | 归入 UI 薄入口对 ProjectContext 快照的视图。 |
| #50 CLI TUI 目录整理 | 已并入本设计，为 AgentClient SDK 物理边界提供基础。 |
| #51 UI Domain DDD 设计 | 已并入本设计；UI 回归支撑域（薄入口），AgentClient SDK 保留并纳入 §6.4/§6.6。 |
| #61 架构债务收口 | 以本基线推进 feature 边界、shared kernel 纯度与架构守卫。 |
| #62 audit/policy 落地 | 以本基线落地 Policy 完整权限模型与 Audit 审计链路。 |

## 12. 迁移原则

目标态落地采用**渐进迁移**，每步保持可编译与行为可验证，不一次性搬完。迁移顺序与门禁：

1. **skeleton**：建立 `agent/features/`、`agent/shared/`、`agent/composition/` 骨架与最小 re-export，不迁业务逻辑。
2. **shared**：迁横切能力（errors/ids/logger/config schema/filesystem/process/git/json/telemetry）；横切 port 进 `shared/<capability>/`，adapter 进 `shared/adapter/<capability>/`。
3. **support features**：优先迁低依赖 feature——audit、policy、project、storage、prompt、hook。
4. **capability features**：迁 provider、tools。
5. **runtime**：最后迁 runtime，使其通过其他 feature gateway 编排完整 Agent Loop。
6. **guard**：补齐 §6.4.8 全部架构守卫。

迁移约束：

1. 不恢复 #36：不创建 `apps/server`、`apps/agents`、`packages/proto`、`infra`。
2. 允许重命名 crate 和公开 API，但必须保持 CLI/TUI 行为不变。
3. 每个 checkpoint 必须保持可编译（至少 `cargo check`）；最终运行完整验收门禁。
4. 目录迁移与 `.agents/hooks/*` 架构守卫更新必须处于同一 checkpoint，避免 hook 与源码结构脱节。
5. `.agents` 中 `build_cli.sh`、`check-unit-tests.sh`、`check-architecture-guards.sh`、`check-rust-file-lines.sh`、TUI 单源守卫等的扫描目标须与迁移后的 `apps/`、`agent/{features,shared,composition}` 路径保持一致。

> 本节定义迁移的**目标顺序与门禁约束**；具体落地进度、已完成阶段与遗留缺口记录在 `docs/feature/active.md` 与 git history，不在本文。
