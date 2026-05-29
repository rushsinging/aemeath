# Feature #47：以 DDD 思路重新设计 Aemeath 架构

## 修订历史

| 日期 | 修订 | 说明 |
|------|------|------|
| 2026-05-25 | §6.5 只读视图修正 | 从 `&Session` 引用改为快照（`SessionSnapshot`、`CostInfo`、`Vec<TaskSummary>`） + `watch::Receiver<ChangeSet>` 变更通道。只推标记不推数据，CLI 按需 pull。RuntimeHandle 加入 `change_tx`。快照均无锁，不影响 TUI 帧率。 |
| 2026-05-25 | 新增 §6.6 CLI 边界重新设计 | 分析 fat CLI 现状（run_orchestration 438 行手动编排），定义薄边界（只做 parse args + load config + 启动），AgentClient::new() 吞掉全部 build_*、系统块组装和散落 API 调用。CLI 残留仅 args.rs + run.rs，TuiApp 从 18 参数改为单 AgentClient 注入。附四步实施路径。 |
| 2026-05-25 | 合并 #50、#51 → 回退 UI 为核心域 | **§2**：UI / Interface 回归支撑域（代码量大是因为迁移未完成，不是业务复杂度）。**§3**：移除 3.3 节 UI Domain 术语。**§4**：移除 UI Domain + AgentClient SDK 章节，§4.2 维持 Interface 为薄入口适配层。**§5**：恢复 Interface → Agent Runtime 的原始 Context Map，插入 packages/sdk 作为桥接层。**§6**：恢复 6.1 薄入口，新增 6.5 AgentClient SDK（packages/sdk，4 个只读视图），更新依赖图和 allowlist 为 `cli → sdk`。**§11**：更新 #50/#51 说明为支撑域定位。**§12**：Phase 2 从 UI Domain 物理改回目录整理，Phase 3 守卫从 U1-U7 简化。 |
| 2026-05-25 | 合并 #50、#51 | 核心域扩展为双核、新增 AgentClient SDK、4 个 Bounded Context、U1-U7 守卫。后被同日修订回退。 |
| 2026-05-24 | DeepSeek review | 合并 GLM review 与 DeepSeek review 修正意见。 |
| 2026-05-23 | GLM review | 合并 review-by-glm 的修正意见。 |
| 2026-05-22 | 初稿 | DDD 架构设计初稿完成。 |
| 2026-05-27 | P16-P18 实施归档 | P16：core/ 层端口隔离，消除外部 crate 直接引用。P17：share/core 瘦身（63→55 文件），业务逻辑迁入对应 domain。P18：架构守卫固化（8 守卫）+#47 完成归档。 |
| 2026-05-29 | 现状校订 | 对齐实现态并消除文档脱节：①最小共享内核 crate 实际命名为 `share`（替换原计划的 `core`，`core` crate 不存在），share 当前臃肿过界、瘦身列为显式技术债；②顶层目录为 `agent/`（非 `crates/`）；③COLA crate 内部分层命名统一为 `core/business/utils`（对齐 `agent/runtime` 现状）；④`audit`、`policy` 当前**未实现**完整职责，仅需支撑 allow-all 模式，相关章节降级标注；⑤guard 清单同步为实际接入的 6 个；⑥重排 §6.5/§6.6 重复编号、清理 packages/sdk 自相矛盾句。 |

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
| UI / Interface | CLI/TUI/REPL 适配层。代码量大是因为迁移未完成（当前约 40 个源文件），不是业务复杂度。目标：维持薄入口，通过 AgentClient SDK（`packages/sdk`）与 Agent Runtime 通信。 |

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

> **实现状态（2026-05-29）**：`agent/policy` 当前仅含 `security` 内容扫描（`scan_content`/`SecurityWarning`，约 150 行），**完整权限模型（PermissionRequest/Decision/Grant/Mode/Capability/RiskAssessment）尚未实现**。当前运行时按 allow-all 处理，本节为目标设计（依赖 #42 落地），不代表已成型职责。

#### Audit

职责：

- 独立记录 Agent、Chat、Turn、ToolExecution、PermissionDecision、HookDecision、ModelInvocation 和 final outcome。
- 提供 correlation id，把 Session / Chat / Turn / Agent / Tool / Resource 串起来。

关键原则：Audit 只记录事实，不做权限判断，也不阻止执行。

> **实现状态（2026-05-29）**：`agent/audit` 当前为空骨架（仅 `AuditApiMarker`，约 14 行），**本节职责尚未实现**。当前依赖 allow-all 模式，审计链路（correlation id、policy/hook/outcome 三分记录）为目标设计，待后续落地。

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
  → packages/sdk (AgentClient trait)
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

所有入口都应该接入 `agent/runtime` 暴露的同一组入口无关 API，例如：

- start session
- handle chat
- resume chat
- cancel chat
- stream runtime events
- answer interaction request
- apply permission choice

`agent/runtime` 是唯一编排者，负责把入口命令编排到 Project、Policy、Prompt、Provider、Tools、Storage、Hook、Audit 等 supporting domains。HTTP、CLI、TUI 不应各自复制一套核心流程，也不应直接依赖 supporting domain crate。

当前 Phase 4 的 `ChatApplicationService` 仍是过渡形态：它只做薄校验与分发，通过 `ChatRuntimePort` 调用现有 REPL/TUI adapter，以避免在一次重构中改写 agent loop 或 Tool Execution pipeline。目标形态会逐步把 CLI/TUI 初始化之外的 use case 编排上移到 `agent/runtime`；本阶段先收束启动 DTO、运行上下文和 bootstrap 边界，为后续迁移铺路。

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
sdk
project
policy
prompt
provider
tools
storage
hook
audit
share        # 最小共享内核（原计划命名为 core，实际落地为 share）
```

这要求：

1. 入口层不得承载核心领域逻辑。
2. `apps/cli` 只通过 `packages/sdk` 的 `AgentClient` 契约与 Runtime 交互；由于当前是单二进制 Rust workspace，CLI 仍需要一个运行时装配 crate 提供真实实现入口，但该依赖只能用于构造 `AgentClient`，不得泄漏到 TUI/业务代码。
3. `packages/sdk` 定义 AgentClient trait 及公共类型（ChatStream, SessionSummary, ProjectContext 等），由 `agent/runtime` 实现。
4. `agent/runtime` 是 Agent Runtime 核心域和唯一编排者，可以依赖 supporting domain crate。通过 `runtime::api` 暴露事件和 DTO，供 AgentClient 内部使用。
5. supporting domain crate 不反向依赖 `runtime` 或 `apps/cli`，必要协作通过 runtime 编排或通过 `share` 中稳定共享类型表达。
6. `share` 是最小共享内核（替换原计划的 `core` 命名），只能放 Result、错误、基础 value object、协议无关 DTO；不能承载 Chat、Tool pipeline、配置加载、权限评估或 hook 执行流程。**实现现状偏差**：当前 `share` 已超出此约束（含 `config`、`tool`(ToolRegistry/ToolContext)、`task`(TaskStore)、`memory`(MemoryStore) 等带行为类型与 IO 依赖），与本约束不符，**瘦身列为显式技术债**——后续应把带行为/带 IO 的类型移出 share，回归纯共享内核。
7. 技术分层可以存在，但不能压过领域边界。
8. 依赖方向应保持：Interface → packages/sdk contract；composition root → agent/runtime implementation → domain context → outbound port → external adapter。
9. 禁止 domain context 反向依赖 HTTP、CLI、TUI 等入口层。

目标依赖图：

```text
apps/cli
  → packages/sdk
  → agent/runtime（composition root only）
agent/runtime → packages/sdk (implements)
    → agent/project
    → agent/policy
    → agent/prompt
    → agent/provider
    → agent/tools
    → agent/storage
    → agent/hook
    → agent/audit
    （+ packages/logging）
上述每个 supporting domain 均只依赖 → agent/share（最小共享内核）
```

实际 Cargo 依赖应以 architecture guard 固化：`apps/cli/Cargo.toml` 只应声明 `packages/sdk`、`agent/runtime`（真实实现装配）和纯技术库依赖，不得声明 supporting domain、`agent/share` 或其他业务 crate 依赖。

#### 6.4.1 Cargo 依赖图守卫

必须新增基于 `cargo metadata` 的依赖图检查，而不是只依赖人工约定。守卫使用显式 allowlist，默认拒绝未声明的业务 crate 依赖。

目标 allowlist：

| Crate | 允许直接依赖的业务 crate |
|---|---|
| `cli` | `sdk`, `runtime`（仅 composition root 装配真实实现） |
| `sdk` | （接口 crate，只含 trait + 公共类型，无内部依赖） |
| `runtime` | `sdk`, `logging`, `share`, `project`, `policy`, `prompt`, `provider`, `tools`, `storage`, `hook`, `audit` |
| `project` | `share` |
| `policy` | `share` |
| `prompt` | `share` |
| `provider` | `share` |
| `tools` | `share`, `project` |
| `storage` | `share` |
| `hook` | `share` |
| `audit` | `share` |
| `share` | 无 |

> **横向依赖说明（refs #61 D2，rule4）**：`tools → project` 为已批准的横向依赖。原因：worktree 进入/退出/上下文恢复属带 git 子进程 + 文件系统 IO 的行为，不应留在 `share` 共享内核。该逻辑原在 `share::worktree_ops` 与 `project::worktree` 重复（DRY 违规）。瘦身将 `share::worktree_ops` 删除，归位 `project::worktree`（worktree 是 project domain 的天然职责），`tools` 复用之。方向 `tools → project → share`，`project` 不反依赖 `tools`，无环。替代方案（移入 tools / 复制副本）会再生 DRY 违规或语义错位，故采用横向依赖。

规则：

1. `apps/cli` 只能直接依赖 `sdk`、`runtime` 和纯技术库；`runtime` 依赖仅限 composition root 构造真实 `AgentClient`，禁止 TUI/业务代码直接使用 runtime 内部 API。
2. `sdk` 是纯接口 crate，只含 trait + 公共类型，无业务依赖。
3. `runtime` 是唯一编排者，可以依赖所有 supporting domains 和 `share`，并实现 `sdk` 的 AgentClient trait。
4. supporting domain 默认只能依赖 `share`，不能互相横向依赖；如果确实需要横向依赖，必须先进入 architecture allowlist，并在 spec 中说明原因、方向和替代方案。
5. `share` 不能依赖任何业务 crate。
6. 任何业务 crate 都不能依赖 `cli`。
7. 任何 supporting domain 都不能依赖 `runtime`。
8. 检查应覆盖 package name，而不是目录字符串，避免移动目录后规则失效。

需要阻断的例子：

```text
cli -> runtime 内部 API      ← TUI/业务代码应走 packages/sdk AgentClient；runtime 只允许在 composition root 装配
cli -> tools
cli -> share
tools -> policy
policy -> provider
audit -> storage
share -> project
tools -> runtime
provider -> cli
```

#### 6.4.2 Rust import 守卫

Cargo 依赖图之外，还必须检查源码 import，防止代码绕过 `runtime::api` 或引入边界泄漏。

`apps/cli/src/**/*.rs` 禁止出现：

```text
use share::
use project::
use policy::
use prompt::
use provider::
use tools::
use storage::
use hook::
use audit::
```

`apps/cli` 只能通过 `sdk::AgentClient` 契约访问运行时能力；过渡期仅允许 `args.rs`、`main.rs`、`run_orchestration.rs` 在 composition root 使用 `runtime::api::bootstrap/client/command` 做装配。

supporting domain 的源码禁止出现：

```text
use runtime::
use cli::
```

除 `share` 外，supporting domain 之间的 `use <other_support>::` 也默认禁止。所有例外必须和 Cargo allowlist 同步维护。

#### 6.4.3 Public API 与可见性约束

每个业务 crate 对外只应暴露稳定 API 面：

```text
pub mod api;
```

内部实现按 COLA 分层组织，命名统一为 `core` / `business` / `utils`（对齐 `agent/runtime` 现状），默认保持 crate-private：

```text
mod core;       // 编排 + 端口定义（client / service / port）
mod business;   // 领域规则与不变量
mod utils;      // bootstrap / adapter / IO 实现
```

约束：

1. 外部 crate 只能使用 `<crate>::api::*`。
2. 聚合根、实体和值对象不应无选择地从 crate root 暴露。
3. `runtime::api` 是入口层唯一可见的业务 API，负责重新导出或映射 CLI/TUI/HTTP/SDK 需要的 request、command、event、interaction 和错误展示契约。
4. support domain 的 `api` 暴露 use case / query / command / DTO，不暴露内部 repository、adapter 或 provider SDK 细节。
5. 如果某个类型被多个 domain 共享，应优先判断它是否是真正稳定的共享 value object；只有满足稳定、协议无关、无编排逻辑时才下沉到 `share`。

> **实现现状偏差（2026-05-29）**：依赖图边界已硬性达标，但 Public API 收窄尚未真正落地——多数 supporting domain 的 `api.rs` 仍是 `XxxApiMarker` 占位 + `pub use` 内部模块（如 `project::api` 直接 `pub use worktree::*`），内部模块仍 `pub`；`runtime::api` 也直接 `pub use provider; pub use tools; …` 转发整个下游 crate。约束 1/2/4 待后续按 `core/business/utils` 分层与 api 收窄逐步落实。

#### 6.4.4 Hook 集成

`.agents/hooks/check-architecture-guards.sh` 最终必须聚合以下检查：

```text
check-cargo-dependency-graph.sh     # crate 依赖图 allowlist
check-cli-thin-entry.sh             # apps/cli 只依赖 runtime+sdk
check-share-no-upstream-deps.sh     # share 无上游依赖（原计划名 check-core-no-upstream-deps）
check-cola-layer-purity.sh          # 分层纯度（share 含 IO 警告 = 瘦身技术债）
check-forbidden-imports.sh          # cli/tui 源码 import 边界
check-rust-file-lines.sh            # 单文件 ≤400 行
```

以上为实现实际接入的 6 个架构 guard（命名按落地态：共享内核守卫为 `check-share-no-upstream-deps.sh`）。这些脚本必须在 Stop hook 中执行；任何依赖图违规、import 违规、`share` 上游依赖或 `apps/cli` 直接依赖 support/share 都应阻止完成。

### 6.5 目标 workspace 目录结构

目录结构调整采用一次性目标设计、分 checkpoint 实施的方式。最终 workspace 应让目录和 crate 名直接表达产品语义与 Bounded Context，同时避免 `contexts` / `shared` 这类顶层抽象词造成理解成本。

目标结构：

```text
apps/
  cli/                 # 薄入口：参数解析、TUI/REPL 事件适配、启动 runtime

agent/                 # crate 顶层目录（落地态，非 crates/）
  runtime/             # 核心域：Agent Runtime，编排 Chat / Turn / Tool / Model / Task
  project/             # Project Context：cwd、path_base、worktree、项目配置和指令来源发现
  policy/              # Permission / capability / risk / approval（现仅 security 扫描，完整权限模型未实现）
  prompt/              # guidance、skills、system prompt、prompt bundle
  provider/            # LLM provider 防腐层
  tools/               # tool catalog、tool execution、MCP tool adapter
  storage/             # session history、memory、cost history、task persistence
  hook/                # hook event、runner、decision
  audit/               # audit event、correlation id、审计日志（现为空骨架，未实现）
  share/               # 最小共享内核：错误、基础消息类型、通用 value object（原计划命名 core；当前臃肿待瘦身）

packages/
  sdk/                 # AgentClient trait + 公共类型（CLI 与 Runtime 唯一通信契约）
  logging/             # 日志 projection 适配
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
share        # 原计划命名为 core
```

命名约束：

1. `apps/cli` 是唯一当前可执行入口，保持薄入口；不设置 `agent/interface`，TUI/REPL adapter 暂留 `apps/cli`，后续如需多入口共享 projection，再从 runtime API 抽公共 adapter 类型。
2. `runtime` 表达核心域 Agent Runtime；不使用 `agent-runtime`，避免 crate 名过长。
3. `project` 表达项目上下文；不使用 `project-context`，避免目录名重复 context 概念。
4. `provider` 表达模型 provider 防腐层；不使用 `model-gateway`。
5. `tools` 使用复数，表达工具集合、执行管线和 MCP adapter；不使用 `tool-execution`。
6. `prompt` 统一承载 skills、guidance、CLAUDE/AGENTS instruction 与 system prompt 组合规则。
7. `storage` 统一承载 session history、memory、cost history、task persistence 等持久化投影；不再拆 `session-history` / `memory` 顶层 crate。
8. `hook` 使用短名；不使用 `hook-automation`。
9. `audit` 独立记录运行事实和 correlation id。
10. `share`（原计划命名 `core`）必须保持小而稳定，禁止变成新的大杂烩。**现状**：已变成大杂烩，瘦身列为技术债。

迁移约束：

1. 不恢复 #36：不创建 `apps/server`、`apps/agents`、`packages/proto`、`infra`。（修正：`packages/sdk` 作为 AgentClient 契约桥接层**保留并已创建**，见 §6.7；同时新增 `packages/logging`。早期"不创建 packages/sdk"的表述源于已回退的 #50/#51 合并，此处更正。）
2. 已完成：原 `shared/kernel`、`contexts/provider`、`contexts/tool` 过渡结构已收束到 `agent/share`、`agent/provider`、`agent/tools`，并创建其余 `agent/*` 目标 crate；`contexts/`、`shared/` 顶层目录已移除。
3. 允许重命名 crate 和公开 API，但必须保持 CLI/TUI 行为不变。
4. 每个 crate 内部再按 COLA 分层组织，命名统一为 `core`（编排+端口）、`business`（领域规则）、`utils`（bootstrap/adapter/IO）；但顶层只表达产品/领域语义。
5. `apps/cli` 只能依赖 `agent/runtime`（composition root）+ `packages/sdk` 和纯技术库；不能直接依赖 supporting domains 或 `share`。（已在首轮实施中通过 Cargo 依赖收束和 architecture guards 固化）
6. supporting domains 之间依赖必须按 Context Map 方向收敛，禁止 domain 反向依赖 `apps/cli`、TUI 或 REPL。
7. 实施必须按 checkpoint 保持可编译：每个 checkpoint 至少运行 `cargo check`，最终运行完整验收门禁。

建议 checkpoint：

1. 建立 `agent/share`（最小共享内核）和 `agent/runtime`，先由 `runtime::api` re-export 或包装 CLI 当前需要的启动 DTO，使 `apps/cli` 依赖逐步收束到 runtime。
2. 将 `contexts/provider` 迁移为 `agent/provider`，保持 provider API、streaming、pricing、model pool 行为不变。
3. 将 `contexts/tool` 迁移为 `agent/tools`，保持 tool schema、registry、MCP 生命周期、权限/hook gate 行为不变。
4. 从 `shared/kernel` 拆出 `agent/project`、`agent/policy`、`agent/prompt`、`agent/storage`、`agent/hook`、`agent/audit` 的低耦合类型和端口；剩余稳定共享类型进入 `agent/share`。
5. 让 `agent/runtime` 成为唯一编排者，逐步接管 Chat、Turn、Task、Tool batch、Model invocation、Permission prompt、Hook、Audit 的 use case 编排。（Phase 2 checkpoint：已迁移低 UI 耦合的 chat application contract、agent_runner，以及 runtime bootstrap 中的 concurrency、permissions、model_runtime、provider_client、runtime_support 到 runtime；Guidance 已从 core 拆入 prompt 并通过 runtime adapter 接入 HookRunner；TUI/REPL adapter、prompt/tooling adapter、logging adapter 仍暂留 apps/cli）
6. 增加 architecture guard：禁止 `apps/cli` 直接依赖 `share` 和 supporting domain crate；禁止 supporting domain 反向依赖 `runtime` / `apps/cli`。
7. 移除 `contexts/`、`shared/` 过渡目录，更新 `.agents/aemeath.json` 与 `.agents/hooks/*` 中的旧路径、旧 package 名和架构守卫，再运行完整验收。（首轮实施已完成，后续继续拆分 support domain 内部职责）

`.agents` 迁移要求：

1. `build_cli.sh` 与 `.agents/aemeath.json` 的 Stop hooks 必须在 CLI 迁移到 `apps/cli` 后持续匹配目标 workspace 路径。
2. `check-unit-tests.sh` 必须从旧 package 名 `kernel` / `provider` / `tool` / `cli` 迁移到目标 crate 名 `share`、`runtime`、`project`、`policy`、`prompt`、`provider`、`tools`、`storage`、`hook`、`audit`、`cli`。
3. `check-architecture-guards.sh` 应继续聚合所有架构守卫，但守卫目标需从 `contexts/`、`shared/` 切换到 `apps/`、`agent/`。
4. `check-rust-file-lines.sh` 的扫描范围最终应限定为 `apps/`、`agent/`，并继续排除 `target`、`.git`、`.worktrees`。
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
10. `run_orchestration::setup` 已把 `bootstrap_chat` 的技术性启动准备拆成局部 helper；其中 `concurrency`、`permissions`、`model_runtime`、`provider_client`、`runtime_support` 已迁移到 `agent/runtime::bootstrap`，`prompt_bundle`、`tooling` 与 CLI logging/session adapter 暂留入口侧。这些模块只表达 runtime bootstrap 过渡边界，不等同于新的领域上下文；后续若要进一步下沉，应先处理 slash command registry 的持锁 await 与 Agent Runtime 边界。

### 6.7 AgentClient SDK

AgentClient 是 Agent Runtime 对外暴露的统一客户端 SDK，定义在 `packages/sdk/`，实现在 `agent/runtime/`。它是 CLI（薄入口）与 Agent Runtime 之间的唯一通信契约。

**trait/impl 分层**：

`packages/sdk` 只放 trait + 公共类型，零业务依赖：

```rust
// packages/sdk/src/lib.rs
pub trait AgentClient: Send + Sync + Clone + 'static {
    fn session(&self) -> &Session;
    fn cost(&self) -> &CostTracker;
    fn tasks(&self) -> &TaskStore;
    fn project(&self) -> ProjectContext;
    async fn chat(&self, input: ChatInput) -> Result<ChatStream>;
    fn cancel(&self);
    async fn save_session(&self) -> Result<()>;
    async fn load_session(&self, id: &SessionId) -> Result<Session>;
    async fn list_sessions(&self) -> Result<Vec<SessionSummary>>;
    async fn delete_session(&self, id: &SessionId) -> Result<()>;
    async fn compact(&self, session: &mut Session) -> Result<CompactResult>;
}
```

`new()` 不在 trait 里——不同部署模式（真实 Runtime vs Mock）需要不同构造签名，trait 只管运行时行为。

`agent/runtime` 提供具体实现，AgentClientImpl 只是 RuntimeHandle 的薄代理：

```rust
// agent/runtime/src/client.rs
#[derive(Clone)]
pub struct AgentClientImpl {
    handle: Arc<RuntimeHandle>,
}

impl AgentClientImpl {
    pub async fn new(config: &Config, args: &CliArgs) -> Result<Self> {
        let handle = Runtime::initialize(config, args).await?;
        Ok(Self { handle: Arc::new(handle) })
    }
}

impl AgentClient for AgentClientImpl {
    fn session(&self) -> &Session  { self.handle.session() }
    fn cost(&self) -> &CostTracker { self.handle.cost() }
    fn tasks(&self) -> &TaskStore  { self.handle.tasks() }
    fn project(&self) -> ProjectContext { self.handle.project() }
    async fn chat(&self, input: ChatInput) -> Result<ChatStream> {
        self.handle.chat(input).await
    }
    fn cancel(&self) { self.handle.cancel() }
    // ... 全部委托给 RuntimeHandle
}
```

**初始化编排归 Runtime**：

```rust
// agent/runtime/src/runtime.rs
pub struct Runtime;
impl Runtime {
    pub async fn initialize(config: &Config, args: &CliArgs) -> Result<RuntimeHandle> {
        let logger = init_logging(config);
        let hooks = HookRunner::new(config)?;
        let model = select_model(config, args);
        let provider = build_provider_client(config, &model)?;
        let llm = build_llm_client(provider, &model)?;
        let tools = build_chat_tooling(config)?;
        let session = resolve_session(config, args)?;
        let prompt = build_system_prompt(config, &session)?;
        RuntimeHandle::new(/* 组装好的内部对象 */)
    }
}

pub struct RuntimeHandle {
    session: Arc<RwLock<Session>>,
    cost_tracker: Arc<CostTracker>,
    task_store: Arc<RwLock<TaskStore>>,
    project: RwLock<ProjectContext>,
    change_tx: watch::Sender<ChangeSet>,
    chat_tx: mpsc::UnboundedSender<ChatEvent>,
    cancel_token: Arc<AtomicBool>,
}
```

核心原则：

```
CLI TUI/业务代码只知道 AgentClient（trait）
AgentClientImpl 只是 RuntimeHandle 的薄代理
Runtime 拥有全部初始化逻辑和编排能力
```

**只读视图：快照 + 变更通道**

`&Session` 做不到无锁——`RwLock::read()` 被写方持有 3ms 就会卡 TUI 帧。TUI 需要的是**即时快照 + 变更通道**，不是引用。

```rust
pub trait AgentClient: Send + Sync + Clone + 'static {
  // ─── 快照（无锁，永不阻塞） ───
  fn session_snapshot(&self) -> SessionSnapshot;      // cheap clone
  fn cost(&self) -> CostInfo;                         // Atomic 读取
  fn task_list(&self) -> Vec<TaskSummary>;            // 快照
  fn project(&self) -> ProjectContext;                // Copy 值类型

  // ─── 变更通道 ───
  fn changes(&self) -> watch::Receiver<ChangeSet>;

  // ─── 写操作 ───
  async fn chat(&self, input: ChatInput) -> Result<ChatStream>;
  fn cancel(&self);
  // ... session 管理
}
```

变更通道只推送标记，不推送数据：

```rust
// packages/sdk/src/lib.rs
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

**Runtime 侧触发**——每次 session / cost / tasks / project 变更时：

```rust
// chat loop 中
self.session.write().await.push(msg);
let _ = self.change_tx.send(ChangeSet::SESSION | ChangeSet::COST);

// tool result 后
self.task_store.write().await.update(task_id, status);
let _ = self.change_tx.send(ChangeSet::TASKS);

// worktree 切换后
*self.project.write().unwrap() = new_project;
let _ = self.change_tx.send(ChangeSet::PROJECT);
```

**CLI 侧消费**：

```rust
let mut changes = client.changes();
loop {
  select! {
      Ok(()) = changes.changed() => {
          let set = *changes.borrow_and_update();
          if set.contains(ChangeSet::SESSION) {
              model.messages = client.session_snapshot();
          }
          if set.contains(ChangeSet::COST) {
              model.cost = client.cost();
          }
          if set.contains(ChangeSet::TASKS) {
              model.tasks = client.task_list();
          }
          if set.contains(ChangeSet::PROJECT) {
              model.project = client.project();
          }
          model.dirty = true;
      }
      Some(event) = input.next() => { /* key/mouse */ }
      _ = tick.tick() => { /* 强制渲染 */ }
  }
}
```

**为什么不用 Arc 直接共享？**

| 方案 | 问题 |
|------|------|
| `Arc<RwLock<Session>>` 传 CLI | 打破 trait 边界——CLI 知道内部数据布局 |
| `&Session` 引用 | 需要持有锁，阻塞渲染 |
| `SessionSnapshot` + `watch` | 快照无锁、CLI 不知道内存布局、变更精确 |

快照开销：

| 快照类型 | 内部实现 | 开销 |
|---------|---------|------|
| `SessionSnapshot` | clone 消息列表（底层 Vec 已 Arc 共享） | 低 |
| `CostInfo` | 两个 `AtomicU64` 的 read | 纳秒级 |
| `Vec<TaskSummary>` | clone 若干摘要结构体 | 低 |
| `ProjectContext` | Copy | 零 |

**ChatStream 设计**：

```rust
// packages/sdk/src/stream.rs
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

pub struct ChatStream {
    rx: tokio::sync::mpsc::UnboundedReceiver<ChatEvent>,
}
impl ChatStream {
    pub async fn recv(&mut self) -> Option<ChatEvent> { self.rx.recv().await }
}
```

mpsc 而非 Stream trait：TUI 需要 `recv().await` 阻塞等待——终端事件循环是轮询模型。

**cancel 机制**：

```rust
fn cancel(&self) {
    self.cancel_token.store(true, Ordering::Release);
}
```

chat() 内部每处理一个 ChatEvent 检查 AtomicBool，无需 AbortHandle。

CLI 的 TUI/业务代码只依赖 `packages/sdk` 契约，不直接依赖 `agent/` 的任何 supporting crate；单二进制部署下，composition root 仍直接依赖 `agent/runtime` 以获得真实 `AgentClientImpl`。

### 6.8 CLI 边界重新设计

当前 CLI 承担了大量不该由入口层承担的初始化编排工作。`run_orchestration/` 三文件共 438 行，其中 `setup.rs` (184 行) 手动组装 Runtime 依赖，`runtime.rs` (163 行) 持有散落的 Runtime API 调用，`prompt.rs` (91 行) 在入口层组装 system block。

**CLI 的薄边界**：CLI 只做三件事——解析启动参数、加载配置、在 composition root 构造 AgentClient 并启动 TUI/REPL 循环。所有 Runtime 初始化逻辑移入 `AgentClientImpl::from_args()`，所有 TUI/业务 Runtime 调用逐步走 AgentClient 方法。

```
当前（fat）:                             目标（thin）:

CLI                                      CLI               AgentClient
├── parse args                           ├── parse args    AgentClient::new(config, args)
├── load config                          ├── load config   │ ├── build_provider_client()
├── build_provider_client()  ←不该       └── run_tui(client)│ ├── build_model_config()
├── build_model_config()     ←不该            │            │ ├── build_llm_client()
├── build_llm_client()       ←不该            │ session()  │ ├── build_chat_tooling()
├── build_chat_tooling()     ←不该            │ cost()     │ ├── build_agent_runner()
├── build_agent_runner()     ←不该            │ tasks()    │ ├── init hooks, logging
├── assemble context         ←不该            │ project()  │ └── return AgentClient
├── create TuiApp(18 params) ←不该            │ chat(input)
└── run event loop                           ▼
```

**AgentClient::new() 吞掉的职责**（从 CLI 移入 agent/runtime）：

| 当前位置 | 职责 | 移入 AgentClient |
|---------|------|-----------------|
| `setup.rs` | build_chat_application() | `new()` 内部调用 |
| `setup.rs` | build_chat_tooling() | `new()` 内部调用 |
| `setup.rs` | build_provider_client() | `new()` 内部调用 |
| `setup.rs` | build_model_config() | `new()` 内部调用 |
| `setup.rs` | build_llm_client() | `new()` 内部调用 |
| `setup.rs` | build_agent_runner() | `new()` 内部调用 |
| `setup.rs` | 日志初始化、session 创建/恢复、hook 初始化 | `new()` 内部调用 |
| `prompt.rs` | system block / system_prompt_text 组装 | `new()` 内部调用 |
| `runtime.rs` | chat(), cancel(), save_session(), compact() | 成为 AgentClient 方法 |

**CLI 残留边界**（after AgentClient::new() 实施后）：

```
apps/cli/src/
├── args.rs              ← 参数解析 + ChatBootstrapArgs DTO 映射
├── main.rs              ← panic hook + tokio main
├── run_orchestration.rs ← composition root：init commands → runtime::client::from_args() → TUI/REPL
└── tui/**               ← 只应逐步依赖 sdk::AgentClient 契约，不再直接展开 runtime 内部对象
```

**TuiApp 构造函数变更**：

```
// 之前：18 个独立参数注入
App::run(state, session, messages, tool_registry, hook_runner, ...)

// 之后：只注入 AgentClient
App::run(state, client: AgentClient)
```

**四步实施路径**：

| 步 | 内容 | 验证 |
|----|------|------|
| 1 | 在 `packages/sdk` 定义 `AgentClient` trait | `cargo check -p sdk` |
| 2 | 在 `agent/runtime` 实现 `AgentClient::new()`，吞掉 setup.rs 的全部 build_* | `cargo check -p agent/runtime` |
| 3 | CLI 的 `run_orchestration.rs` 改为 composition root：`runtime::api::client::from_args(args.into()).await` 后只把 AgentClient/SDK 投影传入 TUI | `cargo check -p cli` |
| 4 | 删除 setup.rs、runtime.rs、prompt.rs，TuiApp 逐步改用 AgentClient；直连 runtime 内部对象仅作为过渡债务保留在文档中 | `cargo check -p cli`，TUI 跑起来验证 |

## 7. COLA 工程分层规范

DDD 用于回答领域边界和统一语言是什么，COLA 用于约束代码如何分层落地。Aemeath 后续重构应把二者结合：先按 DDD 确定 Bounded Context，再用 COLA 风格组织入口、应用服务、领域模型和基础设施适配器。

### 7.1 分层定义

| COLA 层 | 职责 | Aemeath 对应 |
|---|---|---|
| Adapter | 接收外部输入，做协议转换和展示投影。 | `apps/cli` 的 CLI command、TUI event handler、REPL adapter；未来 HTTP endpoint、SDK adapter。 |
| Application | 编排用例流程，调用领域上下文和端口，不承载核心业务规则。 | `agent/runtime::api` 与 runtime application service：session/chat、resume、cancel、permission choice、runtime event stream。 |
| Domain | 表达领域模型、聚合、不变量、领域服务和端口定义。 | Runtime、Project、Policy、Prompt、Tools、Storage、Hook、Audit 的 domain 模块。 |
| Infrastructure | 实现外部系统适配和 I/O 细节。 | provider SDK、filesystem、git、shell、web、MCP、hook runner、session storage。 |
| Client / API | 定义对外契约和数据传输对象。 | `runtime::api`、各 support crate 的 `api` 模块、command/query/response DTO、runtime event DTO。 |

### 7.2 Aemeath 目标映射

```text
apps/cli
  → adapter

agent/runtime::api
  → client / api 对外契约 + application facade

agent/runtime/{core,business,utils}
  → 核心域：core(编排+端口) / business(领域规则) / utils(bootstrap+adapter+IO)

agent/{project,policy,prompt,provider,tools,storage,hook,audit}/{api,core,business,utils}
  → supporting domains（内部分层命名统一为 core/business/utils）

agent/share
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

> **实现状态（2026-05-29）**：本节为目标设计。`PermissionDecision`/`PermissionSession`/`GrantSet`（Policy）与 `AuditTrail`/correlation id（Audit）**当前均未实现**，运行时按 allow-all 处理，Stop hook 仍可阻止完成（见 `check-architecture-guards.sh`）。HookDecision 路径已实现。

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
16. CLI/TUI/HTTP/SDK 入口必须保持薄，优先通过 `packages/sdk::AgentClient` 契约接入 Runtime；单二进制 CLI 的 composition root 可保留 `agent/runtime` 装配真实实现。
17. `apps/cli` 严格只直接依赖 `packages/sdk`、`agent/runtime`（composition root 装配）和纯技术库，不直接依赖 `share` 或 supporting domains。
18. Runtime 是唯一编排者，可以依赖 supporting domains。
19. Supporting domains 默认只依赖 `share`，横向依赖必须进入 architecture allowlist。
20. `share`（原计划命名 `core`）是最小共享内核，不能成为所有领域概念的混合仓库；**当前已超界，瘦身为技术债**。
21. 目标 workspace 采用 `apps/`、`agent/`、`packages/` 顶层目录；crate 名不添加 `aemeath-` 前缀。
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
| #46 TUI status line | 归入 UI Domain 的 ProjectContext 视图。 |
| #50 CLI TUI 目录整理 | 已并入本设计。物理目录结构为 AgentClient SDK 实施提供物理基础。 |
| #51 UI Domain DDD 设计 | 已并入本设计。UI Domain 经讨论回归支撑域（薄入口），AgentClient SDK 保留并纳入 §6.5。 |

## 12. 后续工作

1. **Phase 0**（SDK）：在 `packages/sdk` 中定义 `AgentClient` trait 及公共类型（ChatStream, SessionSummary, ProjectContext），在 `agent/runtime` 中实现。
2. **Phase 1**（初始化）：将 `setup.rs` 的 build_* 编排委托给 `AgentClientImpl::from_args()`，CLI 瘦身为 composition root。将零散 runtime API 调用逐步迁移至 AgentClient。
3. **Phase 2**（目录整理）：完成 CLI 目录整理（#50），收拢碎片文件。在 mod.rs 中添加 doc comment 标注模块职责。
4. **Phase 3**（守卫）：实施架构守卫脚本（包依赖检查），确保 CLI 只依赖 `packages/sdk`、`agent/runtime`（composition root）和纯技术库，不直接依赖任何 supporting domain 或 share/core。
5. **Phase 4**（SDK 投影）：在 SDK 中补齐 TUI 需要的只读投影/命令事件，让 `tui/**` 逐步从 `runtime::api::*` 迁移到 `sdk::*`；`runtime` 直连只保留在 `run_orchestration.rs` / `runtime_adapter.rs` 装配层。当前已完成第一轮投影：`ChatBootstrapArgs`、`ModelSummary`、扩展 `SessionSummary`，`models` / `sessions` 子命令改走 `sdk::AgentClient`，并用 `TuiLaunchContext` 把 TUI 启动所需 runtime 对象集中为过渡投影。下一轮执行 SDK-first TUI/runtime 解耦：`packages/sdk::AgentClient` 增加 `chat(ChatRequest, ChatEventSink, QueueDrainPort) -> ChatHandle`、`cancel_chat(ChatHandle)`、`save_current_session()`、`task_status() -> TaskStatusView`、`launch_context() -> TuiLaunchContext`。`ChatRequest` 只包含 TUI 拥有的 `messages`；`cwd` 来自 CLI bootstrap args 并由 runtime client 持有；`workspace_context`、`read_files`、`session_reminders`、`CancellationToken`、`TaskStore`、`LlmClient`、`ToolRegistry`、system prompt、并发限制、hook/json logger 均为 runtime 内部状态，不进入 SDK request。TUI 只把 SDK `ChatStreamEvent` 映射成 `UiEvent`、通过 `QueueDrainPort` 提供排队输入、通过 `ChatHandle` 请求取消、通过 `task_status()` 渲染任务状态；session 保存由 runtime 基于已处理 messages 和当前 session state 执行，TUI 只调用 `save_current_session()`。
6. 用本设计评审现有 crate/module 边界。
7. 标出现有类型属于哪个 Bounded Context 和哪个聚合。
8. 后续重构应保持 CLI/TUI 行为可验证，最终通过完整验收门禁。
