# Feature #47：以 DDD 思路重新设计 Aemeath 架构

## 1. 设计目标

本设计用 DDD（Domain-Driven Design）重新定义 Aemeath 的领域语言与边界，并以 COLA（Clean Object-oriented & Layered Architecture）作为工程分层落地参考。它不是立即重构代码的实施方案，而是后续重构、评审和功能设计的架构基线。

目标：

1. 明确 Aemeath 的核心域。
2. 建立 Agent Runtime 的统一语言。
3. 划分 Bounded Context，避免领域概念继续散落在技术分层中。
4. 定义 Context Map，明确上下文之间的依赖和防腐层。
5. 给后续 crate/module 重构提供判断标准。
6. 定义目标 workspace 目录和 crate 命名，使工程结构显式表达 Bounded Context。
7. 引入 COLA 分层语言，明确 adapter / application / domain / infrastructure / client 的职责边界。

非目标：

1. 不在本设计提交中直接移动 crate、拆文件或改运行逻辑；目录重排需另按实施计划执行。
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
| Security / Policy | 权限和风险判断支撑域。 |
| Audit | 独立审计域，记录谁在什么上下文中做了什么以及为什么。 |
| Interface | TUI/REPL/AskUserQuestion 等输入输出适配层。 |

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
| SessionRecord | Session History | Session 的持久化投影。 |

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

#### Tool

职责：

- 管理 ToolCatalog。
- 将 ToolCall 转换为 ToolInvocation。
- 执行 schema 校验、上下文解析、权限 gate、hook gate、工具 adapter 调用、输出截断和 ToolResult 归一化。

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

关键原则：Memory 不依赖 Prompt；由 Agent Runtime 编排二者。

#### Prompt

职责：

- 管理 SkillCatalog、GuidanceProfile、PromptContract。
- 处理内置、项目、全局 skill 的优先级和覆盖。
- 处理 model guidance、reasoning guidance、项目 instruction 和 system prompt 组合规则。

关键原则：Prompt 独立于 Config；Config 提供来源和配置视图，Prompt 负责行为规则解析。

### 4.3 ACL / Infrastructure Domains

#### Provider

职责：

- 将内部 ModelRequest 转换为 provider request。
- 将 provider streaming chunk、tool call、usage、error 归一化为内部 ModelStreamEvent / ModelResponse。

关键原则：Provider Adapter 不直接暴露给 Agent Runtime。

#### Hook

职责：

- 把生命周期事件转换为 hook input。
- 执行外部 hook command。
- 解析 HookDecision。

关键原则：HookDecision 与 PermissionDecision 分离。

#### Interface

职责：

- TUI、REPL、AskUserQuestion、slash command UI、status projection、streaming display。
- 将用户输入转换为 Agent Runtime 命令，将运行状态呈现给用户。

#### Session History

职责：

- 保存 Session、Chat、Turn、Task、ToolResult、Usage、Cost 等持久化投影。
- 支持恢复和查询。

关键原则：Session History 保存投影，不拥有 Agent Runtime 的运行规则。

#### External Adapters

包括 provider、filesystem、git、shell、web、MCP、terminal 等外部系统适配。

## 5. Context Map

核心关系：

```text
Interface
  → Agent Runtime
      → Config
      → Prompt
      → Memory
      → Provider
      → Tool
          → Project Context
          → Policy
          → Hook
          → Audit
      → Session History
      → Audit
```

补充关系：

```text
Project Context
  → discovers project roots / config sources / instruction sources / skill-hook paths

Config
  → Project Context
  → produces ConfigurationSnapshot

Prompt
  → Config
  → Project Context
  → produces PromptContract

Memory
  independent from Prompt
  coordinated by Agent Runtime

Audit
  observes Agent Runtime / Tool / Policy / Hook / Provider events

Session History
  persists projections from Agent Runtime
```

## 6. 入口与包边界设计要求

DDD 设计要求入口层保持薄，核心业务规则必须位于 Agent Runtime 及其支撑上下文中，而不是散落在 HTTP、CLI 或 TUI 中。

### 6.1 薄入口

HTTP、CLI、TUI、SDK 等都属于 inbound adapter。它们可以负责：

- 解析协议或终端输入。
- 将用户输入转换为应用服务命令。
- 展示 streaming event、状态投影、权限询问和最终响应。
- 管理连接、终端、HTTP 生命周期等入口相关细节。

它们不应该负责：

- Agent Looping。
- Task 状态机。
- PermissionDecision。
- Tool Execution pipeline。
- Project 路径和 worktree 规则。
- Memory / Skill / Guidance 组合规则。

### 6.2 统一应用服务

所有入口都应该接入 `crates/runtime` 暴露的同一组入口无关 API，例如：

- start session
- handle chat
- resume chat
- cancel chat
- stream runtime events
- answer interaction request
- apply permission choice

`crates/runtime` 是唯一编排者，负责把入口命令编排到 Project、Policy、Prompt、Provider、Tools、Storage、Hook、Audit 等 supporting domains。HTTP、CLI、TUI 不应各自复制一套核心流程，也不应直接依赖 supporting domain crate。

当前 Phase 4 的 `ChatApplicationService` 仍是过渡形态：它只做薄校验与分发，通过 `ChatRuntimePort` 调用现有 REPL/TUI adapter，以避免在一次重构中改写 agent loop 或 Tool Execution pipeline。目标形态会逐步把 CLI/TUI 初始化之外的 use case 编排上移到 `crates/runtime`；本阶段先收束启动 DTO、运行上下文和 bootstrap 边界，为后续迁移铺路。

### 6.3 协议无关事件模型

Agent Runtime 和相关上下文应输出协议无关事件，例如：

- RuntimeEvent
- InteractionRequest
- PermissionPrompt
- ToolExecutionEvent
- AuditEvent

TUI 渲染、CLI stdout、HTTP SSE/WebSocket 都只是这些事件的不同 projection。

### 6.4 上下文驱动包边界

包或模块边界应该逐步靠近 Bounded Context：

```text
runtime
project
policy
prompt
provider
tools
storage
hook
audit
core
```

这要求：

1. 入口层不得承载核心领域逻辑。
2. `apps/cli` 严格保持薄入口，只能直接依赖 `crates/runtime` 和必要的纯技术库；禁止直接依赖 `core`、`project`、`policy`、`prompt`、`provider`、`tools`、`storage`、`hook`、`audit`。
3. `crates/runtime` 通过 `runtime::api` 暴露 CLI/TUI/HTTP/SDK 所需的 request、command、event、interaction、error display 契约；入口需要的领域 DTO 必须由 runtime API 重新导出或映射，不能从 supporting domains 旁路获取。
4. `crates/runtime` 是核心域和唯一编排者，可以依赖 supporting domain crate。
5. supporting domain crate 不反向依赖 `runtime` 或 `apps/cli`，必要协作通过 runtime 编排或通过 `core` 中稳定共享类型表达。
6. `core` 只能放最小共享内核，例如 Result、错误、基础 value object、协议无关 DTO；不能承载 Chat、Tool pipeline、配置加载、权限评估或 hook 执行流程。
7. 技术分层可以存在，但不能压过领域边界。
8. 依赖方向应保持：inbound adapter → runtime API/application → domain context → outbound port → external adapter。
9. 禁止 domain context 反向依赖 HTTP、CLI、TUI 等入口层。

目标依赖图：

```text
apps/cli
  → crates/runtime
      → crates/project
      → crates/policy
      → crates/prompt
      → crates/provider
      → crates/tools
      → crates/storage
      → crates/hook
      → crates/audit
          → crates/core
```

实际 Cargo 依赖应以 architecture guard 固化：`apps/cli/Cargo.toml` 不得声明上述 supporting domain 和 `core` 依赖；Rust import 中也不得出现 `use project::`、`use policy::`、`use prompt::`、`use provider::`、`use tools::`、`use storage::`、`use hook::`、`use audit::`、`use core::` 等绕过 runtime 的引用。

#### 6.4.1 Cargo 依赖图守卫

必须新增基于 `cargo metadata` 的依赖图检查，而不是只依赖人工约定。守卫使用显式 allowlist，默认拒绝未声明的业务 crate 依赖。

目标 allowlist：

| Crate | 允许直接依赖的业务 crate |
|---|---|
| `cli` | `runtime` |
| `runtime` | `core`, `project`, `policy`, `prompt`, `provider`, `tools`, `storage`, `hook`, `audit` |
| `project` | `core` |
| `policy` | `core` |
| `prompt` | `core` |
| `provider` | `core` |
| `tools` | `core` |
| `storage` | `core` |
| `hook` | `core` |
| `audit` | `core` |
| `core` | 无 |

规则：

1. `apps/cli` 只能直接依赖 `runtime` 和纯技术库，禁止直接依赖 `core` 或任何 supporting domain。
2. `runtime` 是唯一编排者，可以依赖所有 supporting domains 和 `core`。
3. supporting domain 默认只能依赖 `core`，不能互相横向依赖；如果确实需要横向依赖，必须先进入 architecture allowlist，并在 spec 中说明原因、方向和替代方案。
4. `core` 不能依赖任何业务 crate。
5. 任何业务 crate 都不能依赖 `cli`。
6. 任何 supporting domain 都不能依赖 `runtime`。
7. 检查应覆盖 package name，而不是目录字符串，避免移动目录后规则失效。

需要阻断的例子：

```text
cli -> tools
cli -> core
tools -> policy
policy -> provider
audit -> storage
core -> project
tools -> runtime
provider -> cli
```

#### 6.4.2 Rust import 守卫

Cargo 依赖图之外，还必须检查源码 import，防止代码绕过 `runtime::api` 或引入边界泄漏。

`apps/cli/src/**/*.rs` 禁止出现：

```text
use core::
use project::
use policy::
use prompt::
use provider::
use tools::
use storage::
use hook::
use audit::
```

`apps/cli` 只能通过：

```text
use runtime::api::...
```

supporting domain 的源码禁止出现：

```text
use runtime::
use cli::
```

除 `core` 外，supporting domain 之间的 `use <other_support>::` 也默认禁止。所有例外必须和 Cargo allowlist 同步维护。

#### 6.4.3 Public API 与可见性约束

每个业务 crate 对外只应暴露稳定 API 面：

```text
pub mod api;
```

内部实现默认保持 crate-private：

```text
mod application;
mod domain;
mod infrastructure;
```

约束：

1. 外部 crate 只能使用 `<crate>::api::*`。
2. 聚合根、实体和值对象不应无选择地从 crate root 暴露。
3. `runtime::api` 是入口层唯一可见的业务 API，负责重新导出或映射 CLI/TUI/HTTP/SDK 需要的 request、command、event、interaction 和错误展示契约。
4. support domain 的 `api` 暴露 use case / query / command / DTO，不暴露内部 repository、adapter 或 provider SDK 细节。
5. 如果某个类型被多个 domain 共享，应优先判断它是否是真正稳定的共享 value object；只有满足稳定、协议无关、无编排逻辑时才下沉到 `core`。

#### 6.4.4 Hook 集成

`.agents/hooks/check-architecture-guards.sh` 最终必须聚合以下检查：

```text
check-cargo-dependency-graph.sh
check-forbidden-imports.sh
check-cli-thin-entry.sh
check-core-no-upstream-deps.sh
```

这些脚本必须在 Stop hook 中执行；任何依赖图违规、import 违规、`core` 上游依赖或 `apps/cli` 直接依赖 support/core 都应阻止完成。

### 6.5 目标 workspace 目录结构

目录结构调整采用一次性目标设计、分 checkpoint 实施的方式。最终 workspace 应让目录和 crate 名直接表达产品语义与 Bounded Context，同时避免 `contexts` / `shared` 这类顶层抽象词造成理解成本。

目标结构：

```text
apps/
  cli/                 # 薄入口：参数解析、TUI/REPL 事件适配、启动 runtime

crates/
  runtime/             # 核心域：Agent Runtime，编排 Chat / Turn / Tool / Model / Task
  project/             # Project Context：cwd、path_base、worktree、项目配置和指令来源发现
  policy/              # Permission / capability / risk / approval
  prompt/              # guidance、skills、system prompt、prompt bundle
  provider/            # LLM provider 防腐层
  tools/               # tool catalog、tool execution、MCP tool adapter
  storage/             # session history、memory、cost history、task persistence
  hook/                # hook event、runner、decision
  audit/               # audit event、correlation id、审计日志
  core/                # 最小共享内核：错误、基础消息类型、通用 value object
```

crate 名与目录名保持一致，不再添加 `aemeath-` 前缀：

```text
runtime
project
policy
prompt
provider
tools
storage
hook
audit
core
```

命名约束：

1. `apps/cli` 是唯一当前可执行入口，保持薄入口；不设置 `crates/interface`，TUI/REPL adapter 暂留 `apps/cli`，后续如需多入口共享 projection，再从 runtime API 抽公共 adapter 类型。
2. `runtime` 表达核心域 Agent Runtime；不使用 `agent-runtime`，避免 crate 名过长。
3. `project` 表达项目上下文；不使用 `project-context`，避免目录名重复 context 概念。
4. `provider` 表达模型 provider 防腐层；不使用 `model-gateway`。
5. `tools` 使用复数，表达工具集合、执行管线和 MCP adapter；不使用 `tool-execution`。
6. `prompt` 统一承载 skills、guidance、CLAUDE/AGENTS instruction 与 system prompt 组合规则。
7. `storage` 统一承载 session history、memory、cost history、task persistence 等持久化投影；不再拆 `session-history` / `memory` 顶层 crate。
8. `hook` 使用短名；不使用 `hook-automation`。
9. `audit` 独立记录运行事实和 correlation id。
10. `core` 必须保持小而稳定，禁止变成新的大杂烩。

迁移约束：

1. 不恢复 #36：不创建 `apps/server`、`apps/agents`、`packages/proto`、`packages/sdk`、`infra`。
2. 当前 `shared/kernel`、`contexts/provider`、`contexts/tool` 是上一轮过渡结构，下一轮迁移应收束到 `crates/core`、`crates/provider`、`crates/tools`，并创建其余目标 crates。
3. 允许重命名 crate 和公开 API，但必须保持 CLI/TUI 行为不变。
4. 每个 crate 内部再按 COLA 分层组织，例如 `api`、`application`、`domain`、`infrastructure`；但顶层只表达产品/领域语义。
5. `apps/cli` 只能依赖 `crates/runtime` 和纯技术库；不能直接依赖 supporting domains 或 `core`。（已在首轮实施中通过 Cargo 依赖收束和 architecture guards 固化）
6. supporting domains 之间依赖必须按 Context Map 方向收敛，禁止 domain 反向依赖 `apps/cli`、TUI 或 REPL。
7. 实施必须按 checkpoint 保持可编译：每个 checkpoint 至少运行 `cargo check`，最终运行完整验收门禁。

建议 checkpoint：

1. 建立 `crates/core` 和 `crates/runtime`，先由 `runtime::api` re-export 或包装 CLI 当前需要的启动 DTO，使 `apps/cli` 依赖逐步收束到 runtime。
2. 将 `contexts/provider` 迁移为 `crates/provider`，保持 provider API、streaming、pricing、model pool 行为不变。
3. 将 `contexts/tool` 迁移为 `crates/tools`，保持 tool schema、registry、MCP 生命周期、权限/hook gate 行为不变。
4. 从 `shared/kernel` 拆出 `crates/project`、`crates/policy`、`crates/prompt`、`crates/storage`、`crates/hook`、`crates/audit` 的低耦合类型和端口；剩余稳定共享类型进入 `crates/core`。
5. 让 `crates/runtime` 成为唯一编排者，逐步接管 Chat、Turn、Task、Tool batch、Model invocation、Permission prompt、Hook、Audit 的 use case 编排。（Phase 2 checkpoint：已迁移低 UI 耦合的 chat application contract、agent_runner，以及 runtime bootstrap 中的 concurrency、permissions、model_runtime、provider_client、runtime_support 到 runtime；TUI/REPL adapter、prompt/tooling adapter、logging adapter 仍暂留 apps/cli）
6. 增加 architecture guard：禁止 `apps/cli` 直接依赖 `core` 和 supporting domain crate；禁止 supporting domain 反向依赖 `runtime` / `apps/cli`。
7. 移除 `contexts/`、`shared/` 过渡目录，更新 `.agents/aemeath.json` 与 `.agents/hooks/*` 中的旧路径、旧 package 名和架构守卫，再运行完整验收。（首轮实施已完成，后续继续拆分 support domain 内部职责）

`.agents` 迁移要求：

1. `build_cli.sh` 与 `.agents/aemeath.json` 的 Stop hooks 必须在 CLI 迁移到 `apps/cli` 后持续匹配目标 workspace 路径。
2. `check-unit-tests.sh` 必须从当前 package 名 `kernel` / `provider` / `tool` / `cli` 迁移到目标 crate 名 `core`、`runtime`、`project`、`policy`、`prompt`、`provider`、`tools`、`storage`、`hook`、`audit`、`cli`。
3. `check-architecture-guards.sh` 应继续聚合所有架构守卫，但守卫目标需从 `contexts/`、`shared/` 切换到 `apps/`、`crates/`。
4. `check-rust-file-lines.sh` 的扫描范围最终应限定为 `apps/`、`crates/`，并继续排除 `target`、`.git`、`.worktrees`。
5. `check-tui-tea-purity.sh` 与 `check-unsafe-text-ops.sh` 的 TUI 路径保持 `apps/cli/src/tui`。
6. 新增 architecture guard 必须检查 `apps/cli` 只直接依赖 `runtime` 和纯技术库，且 Rust import 不绕过 `runtime::api`。
7. hook 脚本调整应和对应目录迁移处于同一 checkpoint，保证每个 checkpoint 的 hook 与源码结构一致。

### 6.6 Chat 启动边界对象化（实施进展附注）

本节记录 #47 分阶段重构的当前落地进展，用于衔接目标架构与现有 CLI/TUI 代码。它不是新的领域边界定义，而是 Phase 3/4 对入口启动边界的过渡性实现约束。

Phase 3 继续沿薄入口推进，重点整理 Chat 启动参数边界。Phase 1 已让 CLI no-TUI 与 TUI 主入口通过 `ChatApplicationService` 分发到现有 runtime；Phase 2 已让 `ChatApplicationService` 依赖 `ChatRuntimePort`，并把 `repl` / `tui::App` 调用移动到 runtime adapter。Phase 3 不改变运行行为，而是把“传什么”整理为更稳定的 application 边界对象。

当前 `ChatLaunchRequest` 同时承载 no-TUI 与 TUI 字段，`NoTuiChatDependencies` / `TuiChatDependencies` 重复承载大量共同依赖。后续应拆为三类对象：

```text
ChatRuntimeContext
  client
  registry
  system_blocks
  system_prompt_text
  user_context
  agent_runner
  task_store
  skills_map
  hook_runner
  memory_config
  json_logger
  agent_semaphore

ChatLaunchOptions
  cwd
  verbose
  markdown
  context_size
  resume
  allow_all
  max_tool_concurrency

NoTuiChatLaunch
  options: ChatLaunchOptions

TuiChatLaunch
  options: ChatLaunchOptions
  max_agent_concurrency
  session_id: String
  model_display: String
```

调整后的 port 边界应表达为：

```text
run_no_tui_chat(NoTuiChatLaunch, ChatRuntimeContext)
run_tui_chat(TuiChatLaunch, ChatRuntimeContext)
```

设计约束：

1. `ChatRuntimeContext` 只承载启动 Chat 所需的共享运行依赖，不拥有 Agent Runtime 业务规则。
2. `system_blocks` / `system_prompt_text` 是 Guidance / PromptContract 的启动快照，当前作为过渡字段随 context 传递，后续应沉淀到入口无关的 prompt 构建用例。
3. `agent_semaphore` 是 Agent Runtime 并发执行资源的 runtime handle，当前由 CLI bootstrap 创建并透传，后续应由 Agent Runtime 或执行环境边界统一管理。
4. `json_logger` 是 Audit / logging projection 的适配器句柄，当前保留在 context 中以维持现有日志行为，不应扩展为 application 层业务规则。
5. `ChatLaunchOptions` 只承载 no-TUI / TUI 共同启动选项，不包含 `session_id`、`model_display`、`max_agent_concurrency` 等入口模式专属字段。
6. `NoTuiChatLaunch` 与 `TuiChatLaunch` 用类型表达入口模式差异，避免继续使用 `mode + Option<String>` 表达 TUI 必填项。
7. `ChatApplicationService` 继续只负责校验和分发，不直接调用 `repl`、`tui::App` 或任何入口实现。
8. runtime adapter 继续负责把 application port DTO 映射到现有 `repl::run_repl` / `tui::App::run` 参数，不重写 agent loop。
9. HTTP / SDK 后续接入时应复用同一组 context、options 和 mode-specific launch DTO，而不是复制 CLI/TUI 专属参数结构。
10. `run_orchestration::setup` 已把 `bootstrap_chat` 的技术性启动准备拆成局部 helper；其中 `concurrency`、`permissions`、`model_runtime`、`provider_client`、`runtime_support` 已迁移到 `crates/runtime::bootstrap`，`prompt_bundle`、`tooling` 与 CLI logging/session adapter 暂留入口侧。这些模块只表达 runtime bootstrap 过渡边界，不等同于新的领域上下文；后续若要进一步下沉，应先处理 slash command registry 的持锁 await 与 Agent Runtime 边界。

## 7. COLA 工程分层规范

DDD 用于回答领域边界和统一语言是什么，COLA 用于约束代码如何分层落地。Aemeath 后续重构应把二者结合：先按 DDD 确定 Bounded Context，再用 COLA 风格组织入口、应用服务、领域模型和基础设施适配器。

### 7.1 分层定义

| COLA 层 | 职责 | Aemeath 对应 |
|---|---|---|
| Adapter | 接收外部输入，做协议转换和展示投影。 | `apps/cli` 的 CLI command、TUI event handler、REPL adapter；未来 HTTP endpoint、SDK adapter。 |
| Application | 编排用例流程，调用领域上下文和端口，不承载核心业务规则。 | `crates/runtime::api` 与 runtime application service：session/chat、resume、cancel、permission choice、runtime event stream。 |
| Domain | 表达领域模型、聚合、不变量、领域服务和端口定义。 | Runtime、Project、Policy、Prompt、Tools、Storage、Hook、Audit 的 domain 模块。 |
| Infrastructure | 实现外部系统适配和 I/O 细节。 | provider SDK、filesystem、git、shell、web、MCP、hook runner、session storage。 |
| Client / API | 定义对外契约和数据传输对象。 | `runtime::api`、各 support crate 的 `api` 模块、command/query/response DTO、runtime event DTO。 |

### 7.2 Aemeath 目标映射

```text
apps/cli
  → adapter

crates/runtime::api
  → client / api + application facade

crates/runtime/{application,domain}
  → 核心域 application / domain

crates/{project,policy,prompt,provider,tools,storage,hook,audit}/{api,application,domain,infrastructure}
  → supporting domains

crates/core
  → 最小共享内核
```

### 7.3 COLA 约束

1. Adapter 层必须薄，只处理协议、终端、UI、连接生命周期和结果展示。
2. Application 层负责编排 Chat、resume、cancel、permission choice、interaction answer 等用例。
3. Domain 层拥有业务规则和不变量，不依赖 HTTP、CLI、TUI、数据库、文件系统或 provider SDK。
4. Infrastructure 层只能通过 domain/application 定义的 port 或 gateway 接入。
5. Client / API 层只定义契约，不实现领域规则。
6. DTO / Command / Response 不应泄漏为领域实体。
7. 领域事件和 runtime event 应保持协议无关，再由不同 Adapter 投影为 TUI、CLI、HTTP SSE/WebSocket 或 SDK 输出。

## 8. PermissionDecision、HookDecision 与 Audit

`PermissionDecision` 和 `HookDecision` 必须分离。

原因：

| 维度 | PermissionDecision | HookDecision |
|---|---|---|
| 来源 | 内部安全模型 | 用户配置的外部脚本 |
| 规则归属 | Policy | Hook |
| 输入 | actor / action / resource / risk / grant | hook event JSON |
| 输出 | Allow / Ask / Deny | Continue / Block |
| 是否影响 capability | 是 | 否 |
| 是否用于权限继承 | 是 | 否 |

执行链路示意：

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

例如 AllowAll 下工具可以被 Security / Policy 允许，但 Stop hook 仍可以因架构检查失败而阻止完成。Audit 需要分别记录 policy decision、hook decision 和 final outcome。

## 9. 聚合草案

本节只定义高层聚合，详细字段和实现留给后续重构计划。

| Bounded Context | Aggregate Root |
|---|---|
| Runtime | `AgentDefinition`、`Session`、`Chat`、`TaskBoard` |
| Project | `ProjectContext` |
| Policy | `PermissionSession`、`GrantSet` |
| Prompt | `PromptProfile`、`SkillCatalog`、`GuidanceProfile` |
| Provider | `ModelInvocation` |
| Tools | `ToolCatalog`、`ToolExecutionBatch` |
| Storage | `SessionRecord`、`MemoryCollection` |
| Hook | `HookRun` |
| Audit | `AuditTrail` |

### Runtime 聚合

`AgentDefinition`：配置化 Agent 的领域定义，包含 role、model profile、guidance profile、capability set、permission envelope、memory scope 和 collaboration policy。

`Session`：Runtime 主聚合根，维护多个 Chat、全局 usage summary 和 recovery state 的一致性。

`Chat`：一次用户输入触发的完整处理聚合，维护 Chat 状态、Turn 列表、Tool batch 生命周期、Model invocation 生命周期、ask-user pause/resume、stop condition 和 final response。

`TaskBoard`：规划任务聚合，维护 task 状态流转、blocked_by / blocks 依赖、task list 完成条件和 continue/resume 恢复规则。

`ProjectContext`：Project 聚合根，维护 cwd、path_base、workspace root、worktree stack、git branch、项目级 config / instruction / skill 来源。其不变量包括：path_base 必须属于当前 project/worktree 语义边界，EnterWorktree / ExitWorktree 必须成对维护 stack，Bash 更新 cwd 后必须同步 path_base。

`PermissionSession` / `GrantSet`：Policy 聚合根，维护 grant scope、capability、expiration、actor inheritance、AskMe / Auto / Plan / AllowAll 不变量。

聚合根对外只能通过各 crate 的 `api` 接收 command/query；外部不得直接修改聚合内部状态。状态变化应产出 domain event，再由 runtime 编排 storage、audit、hook 或 adapter projection。

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
7. Project 是独立 supporting domain，目录名使用 `project`。
8. Tools 是独立 supporting domain，目录名使用 `tools`。
9. Policy 独立。
10. Audit 独立。
11. PermissionDecision 与 HookDecision 分离。
12. Prompt 独立，统一承载 skills、guidance、instruction 与 system prompt 组合规则。
13. Storage 统一承载 session history、memory、cost history 与 task persistence。
14. Provider 是 provider ACL，不是核心域。
15. Hook 是生命周期自动化适配，不是权限模型。
16. CLI/TUI/HTTP/SDK 入口必须保持薄，只作为 inbound adapter 接入 `runtime::api`。
17. `apps/cli` 严格只直接依赖 `runtime` 和纯技术库，不直接依赖 `core` 或 supporting domains。
18. Runtime 是唯一编排者，可以依赖 supporting domains。
19. Supporting domains 默认只依赖 `core`，横向依赖必须进入 architecture allowlist。
20. `core` 是最小共享内核，不能成为所有领域概念的混合仓库。
21. 目标 workspace 采用 `apps/`、`crates/` 两类顶层目录；crate 名不添加 `aemeath-` 前缀。
22. COLA 是 DDD 的工程落地参考，不替代领域建模。
23. Adapter / Application / Domain / Infrastructure / Client 的职责必须分离。
24. Cargo dependency graph、forbidden import、public API visibility 和 Stop hook 必须共同防止双向依赖与边界绕过。

## 11. 与既有 feature 的关系

| Feature | 关系 |
|---|---|
| #36 Multi-Agent 框架 | 只参考历史 DDD 设计，不恢复已移除的分布式 server/agents/proto/infra。 |
| #40 Claude 优先兼容 | 归入 Project、Prompt 的 source discovery / compatibility ACL；配置快照由 Runtime 通过 Project/Prompt API 获取。 |
| #42 权限管控系统 | Policy 的主要设计来源；Audit 独立后补足审计边界。 |
| #43 worktree cwd 同步 | 归入 Project 的 path_base / working_root / worktree 一致性规则。 |
| #45 EnterWorktree / ExitWorktree | 归入 Project 与 Tools 的上下文切换能力。 |
| #46 TUI status line | 归入 `apps/cli` 对 Project / Policy / Runtime 状态事件的 projection。 |

## 12. 后续工作

1. 用本设计评审现有 crate/module 边界。
2. 按 `apps/`、`crates/` 目标结构编写实施计划。
3. 标出现有类型属于哪个 Bounded Context 和哪个聚合。
4. 先建立 `runtime::api` 并收束 `apps/cli` 依赖，再按 checkpoint 迁移 workspace crate；每一步保持可编译。
5. 实施 cargo dependency graph、forbidden import、thin CLI、core upstream dependency 等 architecture guards。
6. 后续重构应保持 CLI/TUI 行为可验证，最终通过完整验收门禁。
