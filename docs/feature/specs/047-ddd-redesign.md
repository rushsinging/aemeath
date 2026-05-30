# Feature #47：以 DDD 思路重新设计 Aemeath 架构

> **文档范围**：本 spec 是 **#47（DDD 基线）/ #61（架构债务收口）/ #62（audit/policy 落地）** 的共同架构基线，#47 已归档但基线随后续 feature 持续演进。
>
> **阅读约定**：**正文（§1–§12）只描述目标态约束**，是判断"是否合规"的依据；**实现态、迁移进展、历史债务一律收敛到文末「附录 A：实现进展」**，带日期、随代码演进，不构成约束。若正文与附录冲突，正文为准（附录滞后则更新附录）。

## 修订历史

> 按时间倒序（最新在上）。早期 #50/#51 的合并与同日回退已合并为一条历史摘要。

| 日期 | 修订 | 说明 |
|------|------|------|
| 2026-05-30 | 正文/附录分离 + 多处对齐实现态 | **结构**：正文收敛为纯目标态约束，所有"现状/进展/债务"迁入新增「附录 A：实现进展」。**§6.7**：删除已作废的引用版 `AgentClient` trait 与其 impl 示例，只保留快照版，明确 `async_trait` + dyn-safety/Mock 结论。**§5**：删除 Context Map 中实现里不存在的虚边（`Config→Project`、`Tool→Policy/Hook/Audit`），gate/config 编排归 Runtime。**Config 定位（§4.2/§6.5）**：明确"概念是 supporting domain、契约 `ConfigurationSnapshot` 下沉 `share`、加载分层归 bootstrap"三分法，撤销"config 必须独立 crate / share 违规"的旧判断。**§6.4.3/§7.2**：COLA"统一三层"改为"按职责分层，无内容的层不建"（对齐 #61 D4 务实裁定）。**事实更新**：§2 TUI 文件数、§6.4.4 guard 清单按实测修正。**勘误**：修订历史重排、Phase 编号消歧、§9 聚合表对齐术语表。 |
| 2026-05-29 | 现状校订 | 对齐实现态并消除文档脱节：①最小共享内核 crate 实际命名为 `share`（`core` crate 不存在）；②顶层目录为 `agent/`（非 `crates/`）；③COLA 内部分层命名统一为 `core/business/utils`；④`audit`、`policy` 当时未实现完整职责；⑤guard 清单同步；⑥重排 §6.5/§6.6 编号、清理 packages/sdk 自相矛盾句。 |
| 2026-05-27 | P16-P18 实施归档 | P16：core/ 层端口隔离。P17：share/core 瘦身（63→55 文件）。P18：架构守卫固化 +#47 完成归档。 |
| 2026-05-25 | #50/#51 合并后同日回退 | 一度将核心域扩展为双核、新增 4 个 UI Bounded Context、U1-U7 守卫；同日讨论后回退——UI/Interface 回归支撑域（薄入口），AgentClient SDK 保留并纳入 §6.5/§6.7。详见 §11。 |
| 2026-05-25 | §6.5/§6.6 SDK 边界设计 | 只读视图从 `&Session` 引用改为快照 + `watch::Receiver<ChangeSet>` 变更通道；新增 CLI 薄边界设计与 AgentClient::new() 编排归位。 |
| 2026-05-24 | DeepSeek review | 合并 GLM 与 DeepSeek review 修正意见。 |
| 2026-05-23 | GLM review | 合并 review-by-glm 的修正意见。 |
| 2026-05-22 | 初稿 | DDD 架构设计初稿完成。 |
| 2026-05-27 | P16-P18 实施归档 | P16：core/ 层端口隔离，消除外部 crate 直接引用。P17：share/core 瘦身（63→55 文件），业务逻辑迁入对应 domain。P18：架构守卫固化（8 守卫）+#47 完成归档。 |
| 2026-05-29 | 现状校订 | 对齐实现态并消除文档脱节：①最小共享内核 crate 实际命名为 `share`（替换原计划的 `core`，`core` crate 不存在），share 当前臃肿过界、瘦身列为显式技术债；②顶层目录为 `agent/`（非 `crates/`）；③COLA crate 内部分层命名统一为 `core/business/utils`（对齐 `agent/runtime` 现状）；④`audit`、`policy` 当前**未实现**完整职责，仅需支撑 allow-all 模式，相关章节降级标注；⑤guard 清单同步为实际接入的 6 个；⑥重排 §6.5/§6.6 重复编号、清理 packages/sdk 自相矛盾句。 |
| 2026-05-30 | Agent 目录 feature-boundary 重设计 | 采用 Wanaka 风格 `features/` + `shared/` + `composition/`：feature 内部以 `contract`/`gateway` 表达 Published Language 与 OHS，`shared` 承载横切基础设施、横切 port 与外部 adapter，`composition` 作为唯一生产组合根。 |

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
| UI / Interface | CLI/TUI/REPL 适配层。定位为薄入口：通过 AgentClient SDK（`packages/sdk`）与 Agent Runtime 通信，不承载领域规则。（TUI 当前体量见附录 A） |

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
| SessionRecord | Storage（Session History 子域） | Session 的持久化投影。注：Session History 是 Storage crate 内的子域，不是独立顶层 crate（§6.5 rule7）。 |

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

> **物理归属（三分法）**：Config 在领域上是一个 supporting domain，但工程落地拆三处，**不设独立 `agent/config` crate**：
> 1. **契约 `ConfigurationSnapshot` 及各 typed view（schema）下沉 `share`**——它们稳定、协议无关、无编排逻辑、被几乎所有域消费，恰好符合最小共享内核的定义（§6.4.3 rule5）。
> 2. **加载分层编排（CLI/env/项目/全局/默认 的发现与合并）归 Runtime/CLI bootstrap**——这才是"配置加载流程"，§6.4 rule6 禁止它留在 share。
> 3. 因此 `share` 持有 config **schema** 不违反 rule6（rule6 禁的是"加载流程"而非"配置数据契约"）。Config 也**不依赖 Project**——加载所需的项目路径由 bootstrap 在编排时提供，不构成 `share→project` 反向边。

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

> ⚠️ **目标设计，未实现**：完整权限模型属 #62（依赖 #42），当前运行时按 allow-all 处理。本节描述目标职责。实现现状见附录 A。

#### Audit

职责：

- 独立记录 Agent、Chat、Turn、ToolExecution、PermissionDecision、HookDecision、ModelInvocation 和 final outcome。
- 提供 correlation id，把 Session / Chat / Turn / Agent / Tool / Resource 串起来。

关键原则：Audit 只记录事实，不做权限判断，也不阻止执行。

> ⚠️ **目标设计，未实现**：审计链路（correlation id、policy/hook/outcome 三分记录）属 #62，当前为空骨架。本节描述目标职责。实现现状见附录 A。

#### Memory

职责：

- 管理 MemoryEntry、Reminder、ReflectionSuggestion。
- 支持长期知识检索、沉淀、置顶、完成提醒和范围隔离。

关键原则：Memory 不依赖 Prompt；由 Agent Runtime 编排二者。

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

核心关系（依赖方向 = crate 依赖图 §6.4.1 的领域视图，二者必须一致）：

```text
Interface
  → packages/sdk (AgentClient trait)
    → Agent Runtime                 # 唯一编排者
        → Config
        → Prompt
        → Provider
        → Tool
            → Project Context       # tools→project（worktree 上下文）
            → Storage               # tools→storage（memory 持久化）
        → Policy                    # 权限/hook gate 由 Runtime 在 Tool 执行前后编排，
        → Hook                      #   不是 Tool 直接依赖 Policy/Hook（见 §8）
        → Audit
        → Storage                   # Session History / Memory / Task / Cost 持久化
```

> **gate 归属说明**：Tool 的领域职责包含"权限 gate / hook gate"（§4.2），但 gate 的*编排*由 Agent Runtime 完成——Runtime 在调用 Tool 前后串联 Policy/Hook/Audit。因此依赖图里 `tools` 只依赖 `project`/`storage`，**不依赖** `policy`/`hook`/`audit`（与 §6.4.1 allowlist 一致）。早期 Context Map 把这几条边画在 Tool 节点下是错误的，已上移到 Runtime。

补充关系：

```text
Project Context
  → discovers project roots / config sources / instruction sources / skill-hook paths

Config
  → 加载分层（CLI/env/项目/全局/默认）由 Runtime/CLI bootstrap 编排
  → produces ConfigurationSnapshot（不可变快照，作为共享契约下沉 share，被各域消费）

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

所有入口都应该接入 `agent/runtime` 暴露的同一组入口无关 API，例如：

- start session
- handle chat
- resume chat
- cancel chat
- stream runtime events
- answer interaction request
- apply permission choice

`agent/runtime` 是唯一编排者，负责把入口命令编排到 Project、Policy、Prompt、Provider、Tools、Storage、Hook、Audit 等 supporting domains。HTTP、CLI、TUI 不应各自复制一套核心流程，也不应直接依赖 supporting domain crate。

目标形态：CLI/TUI 初始化之外的 use case 编排全部上移到 `agent/runtime`，`ChatApplicationService` 只做薄校验与分发。当前过渡形态（经 `ChatRuntimePort` 调用现有 REPL/TUI adapter、尚未改写 agent loop）的进展见附录 A。

### 6.3 协议无关事件模型

Agent Runtime 和相关上下文应输出协议无关事件，例如：

- RuntimeEvent
- InteractionRequest
- PermissionPrompt
- ToolExecutionEvent
- AuditEvent

TUI 渲染、CLI stdout、HTTP SSE/WebSocket 都只是这些事件的不同 projection。

### 6.4 上下文驱动包边界（现行 crate 约束）

> **2026-05-30 说明**：本节记录当前已落地的 `agent/<crate>` 形态和既有 guard 约束，用于解释现状与迁移前门禁。后续目标结构以 §6.5 的 `agent/features` + `agent/shared` + `agent/composition` 为准；迁移完成后，本节的 `share` 最小共享内核表述应由 §6.5 的 Wanaka 风格 `shared` 基础设施语义替代。

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
6. `share` 是最小共享内核（替换原计划的 `core` 命名），只能放 Result、错误、基础 value object、协议无关 DTO（含 `ConfigurationSnapshot` 等配置**数据契约**）；**不能承载行为或流程**——Chat、Tool pipeline、配置**加载流程**、权限评估、hook 执行、有状态 registry/store（ToolRegistry/TaskStore/MemoryStore）一律不得驻留 share。区分要点：**配置 schema（数据）允许，配置加载（流程）不允许**。该约束由 `check-share-minimal-kernel.sh` 守护（#61 已将 store 类迁出，share 现为纯数据契约层，详见附录 A）。
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
    （+ packages/global/logging）
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
| `tools` | `share`, `project`, `storage` |
| `storage` | `share` |
| `hook` | `share` |
| `audit` | `share` |
| `share` | 无 |

> **横向依赖说明（refs #61 D2，rule4）**：`tools → project` 为已批准的横向依赖。原因：worktree 进入/退出/上下文恢复属带 git 子进程 + 文件系统 IO 的行为，不应留在 `share` 共享内核。该逻辑原在 `share::worktree_ops` 与 `project::worktree` 重复（DRY 违规）。瘦身将 `share::worktree_ops` 删除，归位 `project::worktree`（worktree 是 project domain 的天然职责），`tools` 复用之。方向 `tools → project → share`，`project` 不反依赖 `tools`，无环。替代方案（移入 tools / 复制副本）会再生 DRY 违规或语义错位，故采用横向依赖。
>
> **横向依赖说明（refs #61 D2 第二批，rule4）**：`tools → storage` 为已批准的横向依赖。原因：memory 持久化（`MemoryStore` 的 fs read/write/create_dir、`path` 的 `canonicalize`）属带文件系统 IO 的行为，不应留在 `share` 共享内核；按 §13 rule7（storage 统一承载 memory persistence），其天然归位 `storage::memory`。`memory_tool` 与 runtime 双方消费 `MemoryStore`，runtime 已依赖 storage，`tools` 复用同一实现避免重复实现/复制副本（DRY）。memory 的 DTO / 枚举 / error / 纯函数（`MemoryEntry`/`MemoryCategory`/`MemoryLayer`/`MemorySource`/`MemoryError`/`AddResult`/`CompactResult`/`MemoryStats`/scoring/dedup/format/`SessionReminders`）属协议无关共享内核，保留 `share::memory`。方向 `tools → storage → share`，`storage` 不反依赖 `tools`，无环。替代方案（port 注入 `dyn MemoryStorePort`）需 13 方法 1:1 透传 trait + per-call 工厂 + 跨 19 处 ToolContext 构造点注入，抽象收益为零而改造面巨大，故采用横向依赖（与 `tools → project` 同范式）。

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

内部实现按 COLA 分层组织，层命名固定取自 `core` / `business` / `utils`，默认保持 crate-private：

```text
mod core;       // 编排 + 端口定义（client / service / port）
mod business;   // 领域规则与不变量
mod utils;      // bootstrap / adapter / IO 实现
```

**按职责分层，不强制凑满三层**：只建实际有职责的层，无内容的层不建空目录。简单 domain 可能只有 `business`（如 project/policy/prompt/storage/hook），骨架 domain 可平铺（如 audit），完整三层仅 runtime/tools 等真正需要的 crate。层命名一旦出现必须取自上述三者，但"出现几层"由职责决定。`check-cola-layer-purity.sh` 守护的是层间纯度，不强制层数。

约束：

1. 外部 crate 只能使用 `<crate>::api::*`。
2. 聚合根、实体和值对象不应无选择地从 crate root 暴露。
3. `runtime::api` 是入口层唯一可见的业务 API，负责重新导出或映射 CLI/TUI/HTTP/SDK 需要的 request、command、event、interaction 和错误展示契约。
4. support domain 的 `api` 暴露 use case / query / command / DTO，不暴露内部 repository、adapter 或 provider SDK 细节。
5. 如果某个类型被多个 domain 共享，应优先判断它是否是真正稳定的共享 value object；只有满足稳定、协议无关、无编排逻辑时才下沉到 `share`。

> 实现现状（api 收窄落地程度、各 crate 当前分层）见附录 A。

#### 6.4.4 Hook 集成

`.agents/hooks/check-architecture-guards.sh` 最终必须聚合以下检查：

```text
check-cargo-dependency-graph.sh     # crate 依赖图 allowlist
check-cli-thin-entry.sh             # apps/cli 只依赖 runtime+sdk
check-share-no-upstream-deps.sh     # share 无上游依赖
check-cola-layer-purity.sh          # 分层纯度
check-forbidden-imports.sh          # cli/tui 源码 import 边界
check-rust-file-lines.sh            # 单文件 ≤400 行
check-crate-api-boundary.sh         # 跨 domain 仅经 <crate>::api::*（#61 D1/D3）
check-share-minimal-kernel.sh       # share 不得回归 store/IO/行为（#61 D2）
```

这些脚本必须在 Stop hook 中执行；任何依赖图违规、import 违规、`share` 上游依赖、绕过 `<crate>::api`、`share` 回归 store/IO 或 `apps/cli` 直接依赖 support/share 都应阻止完成。**清单随实现演进**——当前实际接入的完整集合以 `check-architecture-guards.sh` 为准（见附录 A）。

### 6.5 Agent 目录 feature-boundary 目标结构

> **2026-05-30 修订**：后续 `agent/` 目录采用 Wanaka 风格 feature-boundary，而不是继续把每个 bounded context 直接铺在 `agent/` 顶层。DDD feature boundary 决定外部边界；COLA 只负责每个 feature 内部的工程分层。

目标结构：

```text
agent/
  features/             # 业务 feature boundary，按能力纵向切分
    runtime/            # Agent Loop / turn / session state / compact / cost
    tools/              # Tool + Skill + Slash Command 能力注册与执行
    provider/           # LLM provider gateway
    prompt/             # AGENTS.md / CLAUDE.md / guidance / system prompt material
    project/            # cwd / paths / worktree / git facts
    storage/            # session / memory / task / history 持久化投影
    policy/             # permission / risk；当前只实现 AllowAll
    audit/              # 审计事件 / 操作轨迹；独立 feature
  shared/               # 横切基础设施、横切 port、外部 adapter
  composition/          # 组合根，负责生产依赖装配

packages/
  sdk/                  # AgentClient trait + 公共类型（CLI 与 Runtime 通信契约）
  gloabal/
    logging/           # 日志 projection 适配（现有拼写保持不动）
```

语义：

1. `features/` 是业务 feature boundary。每个 feature 拥有自己的对外语言、对外服务入口、内部编排与领域规则。
2. `shared/` 不是 Minimal Shared Kernel；它是跨 feature 共享的基础设施、横切能力 port 与外部系统 adapter 层。
3. `composition/` 是 composition root，负责把 `features/*`、`shared/*`、`shared/adapter/*` 装配成可运行应用。
4. `packages/sdk` 仍保留为入口层与 Runtime 的外部通信契约；它不是业务 feature。

#### 6.5.1 Feature 内部模板

每个 feature 内部统一使用：

```text
agent/features/<feature>/src/
  contract/             # Published Language：DTO / Event / Command / Query
  gateway/              # Open Host Service：该 feature 对外服务入口
  core/                 # 内部编排 / use case / port
  business/             # 内部规则 / 领域模型 / 状态机
  utils/                # feature 私有工具
  api.rs                # 只 re-export contract + gateway
  lib.rs
```

约束：

1. `contract` 是 Published Language。
2. `gateway` 是 Open Host Service（OHS），即 feature 对外开放的稳定服务入口。
3. `api.rs` 是跨 feature 的统一出口，只允许 re-export `contract` 与 `gateway`。
4. 跨 feature 禁止直接依赖对方的 `core`、`business`、`utils`，也禁止绕过 `api.rs` 直接访问对方 `contract` 或 `gateway` 路径。
5. Feature 内部不设置 `acl/` 目录；外部协议、旧模型和第三方系统适配统一进入 `shared/adapter/*`。

允许：

```text
runtime -> tools::api::{ToolGateway, ToolCall}
tools   -> policy::api::{PermissionGateway, PermissionRequest}
prompt  -> project::api::{ProjectGateway, ProjectContext}
```

禁止：

```text
runtime -> tools::core::Dispatcher
runtime -> tools::business::BuiltinTool
runtime -> tools::utils::PathSecurity
runtime -> tools::gateway::ToolGateway   # 必须统一经 tools::api
```

#### 6.5.2 Feature 边界职责

```text
runtime
  负责 Agent Loop / Chat Loop / turn 编排 / session state / context window / compact / cost / reflection / interrupt / resume。
  不负责 tool、skill、command 注册，不负责 provider 协议适配，不负责 prompt 文件扫描。

tools
  负责 built-in tools、MCP tools、skills、slash commands 的注册、发现、metadata 与执行。
  执行前调用 policy gateway，执行后写 audit gateway。

provider
  负责 LLM provider 访问、streaming response 解析、model profile、provider pool / fallback / retry、usage 解析。
  不负责 Agent Loop、Prompt 组装、Tool 执行或最终成本规则。

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
  负责 PermissionRequest -> PermissionDecision。当前阶段只实现 AllowAll；后续再扩展 risk / confirmation / deny / inherited permission。
  不执行 tool，不写 audit，不修改 runtime 状态。

audit
  负责记录操作事实、permission decision、tool/command/skill 执行事件。当前先实现最小事件模型。
  不判断 allow/deny，不执行工具，不修改 runtime 状态。
```

#### 6.5.3 Shared 语义

`shared/` 是跨 feature 共享的基础设施层，包含横切基础能力 port 与所有外部 adapter：

```text
agent/shared/src/
  adapter/
    provider/
    mcp/
    filesystem/
    process/
    git/
    storage/
    hook/
    telemetry/
  logger/
  telemetry/
  config/
  filesystem/
  process/
  git/
  http/
  json/
  ids.rs
  errors.rs
  types.rs
  lib.rs
```

规则：

1. 横切能力 port 放 `shared/<capability>/`。
2. 具体 adapter 一律放 `shared/adapter/<capability>/`。
3. 除 `shared/adapter/**` 和必要的 `shared/types.rs` 例外外，`shared` 不反向依赖 `features/**`。
4. Feature 代码不能直接 import `shared/adapter/**`。
5. 生产代码中只有 `composition` 可以 import `shared/adapter/**`；测试可按需使用 fake 或 test adapter。

Port 放置标准：

| 类型 | port 位置 | adapter 位置 |
|---|---|---|
| 横切基础能力 | `shared/<capability>/` | `shared/adapter/<capability>/` |
| 聚合自有能力 | `features/<feature>/src/core/` 或 `features/<feature>/src/context/` | `shared/adapter/<capability>/` |

判断标准：这个能力属于某个业务聚合吗？是，则 port 归该 feature；否，若它是日志、配置、文件系统、进程、git、HTTP、clock、id generator 等横切基础能力，则 port 归 `shared/<capability>/`。

#### 6.5.4 Composition Root

`composition/` 负责生产依赖装配：

```text
agent/composition/src/
  app.rs
  runtime.rs
  tools.rs
  provider.rs
  prompt.rs
  project.rs
  storage.rs
  policy.rs
  audit.rs
  context.rs
  lib.rs
```

职责：

1. 创建 shared 基础设施实现。
2. 创建 shared adapter。
3. 创建各 feature gateway / service。
4. 注入依赖并组装 AppContext / RuntimeContext。
5. 为 CLI/TUI/server 等入口暴露启动入口。

`composition` 不承载业务规则、provider 协议转换细节、工具执行细节、权限判断规则或 prompt 合并规则。

启动方向：

```text
apps/cli
  -> agent/composition
      -> features/*/api
      -> shared/*
      -> shared/adapter/*
```

#### 6.5.5 依赖规则

```text
features/* 可以依赖 shared 横切 port。
features/* 可以单向依赖其他 feature 的 api.rs，但禁止循环依赖。
features/* 不能直接依赖其他 feature 的 core / business / utils / gateway / contract 路径。
features/* 不能直接依赖 shared/adapter/**。
shared/<capability> 原则上不依赖 features/**。
shared/adapter/** 可以依赖它实现的 feature-owned port。
composition 可以依赖 features/*/api、shared/*、shared/adapter/*。
features/* 和 shared/* 都不能依赖 composition。
```

推荐 feature 依赖层级：

```text
runtime
  -> tools::api
  -> provider::api
  -> prompt::api
  -> project::api
  -> storage::api
  -> policy::api
  -> audit::api

tools
  -> project::api
  -> storage::api
  -> policy::api
  -> audit::api

prompt
  -> project::api
  -> storage::api

provider
  -> shared only

project / storage / policy / audit
  -> shared only
```

#### 6.5.6 架构守卫

后续架构守卫应逐步覆盖：

1. `api.rs` 只允许 re-export `contract` + `gateway`。
2. 跨 feature 禁止访问对方 `core` / `business` / `utils` / `gateway` / `contract` 直接路径，只能访问 `<feature>::api::*`。
3. 禁止 feature dependency cycle。
4. 禁止 feature 直接 import `shared/adapter/**`。
5. `shared` 除 `shared/adapter/**` 和必要 `shared/types.rs` 例外外，禁止 import `features/**`。
6. `composition` 是唯一生产装配入口，生产代码中只有它可以 import `shared/adapter/**`。

#### 6.5.7 迁移计划

采用渐进迁移，不一次性搬完：

1. **P1 skeleton**：建立 `agent/features/`、`agent/shared/`、`agent/composition/` 骨架与最小 re-export，不迁移业务逻辑。
2. **P2 shared**：迁移横切能力，如 errors、ids、logger、config、filesystem、process、git、json、telemetry；横切 port 进入 `shared/<capability>/`，adapter 进入 `shared/adapter/<capability>/`。
3. **P3 support features**：优先迁移低依赖 feature：audit、policy、project、storage、prompt。
4. **P4 capability features**：迁移 provider、tools。
5. **P5 runtime**：最后迁移 runtime，让其通过其他 feature gateway 编排完整 Agent Loop。
6. **P6 guard**：补齐 feature API、dependency cycle、shared adapter、composition root 等架构守卫。

迁移约束：

1. 不恢复 #36：不创建 `apps/server`、`apps/agents`、`packages/proto`、`infra`。
2. 允许重命名 crate 和公开 API，但必须保持 CLI/TUI 行为不变。
3. 每个 checkpoint 必须保持可编译，至少运行 `cargo check`；最终运行完整验收门禁。
4. 目录迁移和 `.agents/hooks/*` 架构守卫更新必须处于同一 checkpoint，避免 hook 与源码结构脱节。

`.agents` 迁移要求：

1. `build_cli.sh` 与 `.agents/aemeath.json` 的 Stop hooks 必须在 CLI 迁移到 `apps/cli` 后持续匹配目标 workspace 路径。
2. `check-unit-tests.sh` 必须从旧 package 名 `kernel` / `provider` / `tool` / `cli` 迁移到目标 crate 名 `share`、`runtime`、`project`、`policy`、`prompt`、`provider`、`tools`、`storage`、`hook`、`audit`、`cli`。
3. `check-architecture-guards.sh` 应继续聚合所有架构守卫，但守卫目标需从 `contexts/`、`shared/` 切换到 `apps/`、`agent/`。
4. `check-rust-file-lines.sh` 的扫描范围最终应限定为 `apps/`、`agent/`，并继续排除 `target`、`.git`、`.worktrees`。
5. `check-tui-tea-purity.sh` 与 `check-unsafe-text-ops.sh` 的 TUI 路径保持 `apps/cli/src/tui`。
6. 新增 architecture guard 必须检查 `apps/cli` 只直接依赖 `runtime` 和纯技术库，且 Rust import 不绕过 `runtime::api`。
7. hook 脚本调整应和对应目录迁移处于同一 checkpoint，保证每个 checkpoint 的 hook 与源码结构一致。

### 6.6 Chat 启动边界对象化

> 本节原为分阶段重构的"实施进展附注"（ChatRuntimeContext / ChatLaunchOptions / NoTuiChatLaunch / TuiChatLaunch 的过渡拆分），属实现态而非目标态约束，已迁至**附录 A**。目标态约束只有一条：入口启动依赖应表达为稳定的 application 边界对象（context + options + mode-specific launch DTO），HTTP/SDK 接入时复用同一组对象，不复制 CLI/TUI 专属参数结构；`ChatApplicationService` 只校验和分发，不直接调用 `repl`/`tui::App`。

### 6.7 AgentClient SDK

AgentClient 是 Agent Runtime 对外暴露的统一客户端 SDK，定义在 `packages/sdk/`，实现在 `agent/runtime/`。它是 CLI（薄入口）与 Agent Runtime 之间的唯一通信契约。

**trait/impl 分层**：

`packages/sdk` 只放 trait + 公共类型，零业务依赖：

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

关键约束（均已落定，不再变动）：

1. **不暴露内部业务类型引用**。trait 只返回 sdk 自有的快照/值类型（`SessionSnapshot`、`CostInfo`、`TaskSummary`、`ProjectContext`、`ChangeSet`）；这些类型定义在 `packages/sdk` 内，由 runtime 投影填充。这样 sdk 保持**零业务依赖**（§6.4.1 rule2）——若返回 `&Session`/`&TaskStore`，sdk 就被迫依赖 storage/runtime，违反约束且 `&` 引用还要持锁阻塞 TUI 帧。
2. **异步方法 MUST 用 `#[async_trait]`**（对齐项目编码规范；裸 `async fn` in trait 不满足 dyn-safe）。trait 去掉 `Clone` bound——`dyn AgentClient` 不能要求 `Clone`；需要多态共享时用 `Arc<dyn AgentClient>`，真实实现与 Mock 都按此装配。
3. **`new()` 不在 trait 里**——不同部署模式（真实 Runtime vs Mock）需要不同构造签名，trait 只管运行时行为。

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

#[async_trait]
impl AgentClient for AgentClientImpl {
    fn session_snapshot(&self) -> SessionSnapshot { self.handle.session_snapshot() }
    fn cost(&self) -> CostInfo { self.handle.cost() }
    fn task_list(&self) -> Vec<TaskSummary> { self.handle.task_list() }
    fn project(&self) -> ProjectContext { self.handle.project() }
    fn changes(&self) -> watch::Receiver<ChangeSet> { self.handle.changes() }
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

**为什么只读视图用快照而非引用**

`&Session` 做不到无锁——`RwLock::read()` 被写方持有 3ms 就会卡 TUI 帧。TUI 需要的是**即时快照 + 变更通道**，不是引用。这正是上面 trait 用 `session_snapshot()`/`cost()`/`task_list()` 返回值类型、并配 `changes()` 变更通道的原因。

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

### 6.8 CLI 薄边界（目标态）

CLI 只做三件事——**解析启动参数、加载配置、在 composition root 构造 AgentClient 并启动 TUI/REPL 循环**。所有 Runtime 初始化编排（build_provider_client / build_llm_client / build_chat_tooling / build_agent_runner / hooks / logging / session 创建恢复 / system prompt 组装）归 `AgentClientImpl::new()`（在 `agent/runtime`），CLI 不得承载。TuiApp 只注入单个 `AgentClient`（或 `Arc<dyn AgentClient>`），不再注入散落的 runtime 内部对象。

```text
CLI                          AgentClient (agent/runtime)
├── parse args               AgentClient::new(config, args)
├── load config              │  ├── build provider / llm / tooling / agent_runner
└── run_tui(client)          │  ├── init hooks, logging, session
                             │  └── return AgentClient
   client.session_snapshot() / cost() / task_list() / project() / changes() / chat(input)
```

> 本节原含"当前(fat) `run_orchestration/` 三文件 438 行"的现状对比、职责迁移表与四步实施路径——属实现态进展，已迁至**附录 A**（注：该 fat CLI 已重构完成，`setup.rs`/`prompt.rs` 等已删）。

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

agent/{project,policy,prompt,provider,tools,storage,hook,audit}/api + 按需的 {core,business,utils}
  → supporting domains（按职责分层，无内容的层不建；层名取自 core/business/utils）

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

> ⚠️ **目标设计**：Policy（`PermissionDecision`/`PermissionSession`/`GrantSet`）与 Audit（`AuditTrail`/correlation id）属 #62，当前未实现，运行时按 allow-all 处理。**HookDecision 路径已实现**，Stop hook 仍可阻止完成。本节描述目标分离模型，实现现状见附录 A。

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

本节只定义高层聚合，详细字段和实现留给后续重构计划。聚合根名与 §3 术语表对齐；下表新引入而 §3.2 未列的术语在「术语补充」中给出定义。

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

## 12. 后续工作（SDK 重构路线）

> 本节阶段编号是 SDK 重构路线（SDK-Phase）的内部序号，与 §6 行文中其它"阶段"无对应关系。各阶段当前落地进度见附录 A。

1. **SDK-Phase 0**（SDK）：在 `packages/sdk` 中定义 `AgentClient` trait 及公共类型（ChatStream, SessionSummary, ProjectContext），在 `agent/runtime` 中实现。
2. **SDK-Phase 1**（初始化）：将 build_* 编排委托给 `AgentClientImpl::new()`，CLI 瘦身为 composition root，零散 runtime API 调用迁移至 AgentClient。
3. **SDK-Phase 2**（目录整理）：完成 CLI 目录整理（#50），收拢碎片文件，mod.rs 标注模块职责。
4. **SDK-Phase 3**（守卫）：实施架构守卫，确保 CLI 只依赖 `packages/sdk`、`agent/runtime`（composition root）和纯技术库。
5. **SDK-Phase 4**（SDK-first 解耦，**最大未结缺口**）：在 SDK 中补齐 TUI 所需的只读投影/命令事件，让 `tui/**` 完全从 `runtime::api::*` 迁移到 `sdk::*`，`runtime` 直连只留在 composition root 装配层。**目标终态**：`AgentClient` 是 TUI 与 runtime 的唯一通道——`ChatRequest` 只含 TUI 拥有的 `messages`，`cwd`/`workspace_context`/`read_files`/`session_reminders`/`CancellationToken`/`TaskStore`/`LlmClient`/`ToolRegistry`/system prompt/并发限制/hook/logger 均为 runtime 内部状态，不进入 SDK request；TUI 只把 `ChatStreamEvent` 映射为 `UiEvent`、经 `QueueDrainPort` 提供排队输入、经 `ChatHandle` 取消、经 `task_status()` 渲染任务、经 `save_current_session()` 保存。**此阶段尚未完成（TUI 仍直连 runtime 内部对象），是目标态与实现态最大的落地缺口，建议拆独立 feature + 配 guard 推进。**当前已落地的第一轮投影见附录 A。
6. 用本设计评审现有 crate/module 边界。
7. 标出现有类型属于哪个 Bounded Context 和哪个聚合。
8. 后续重构应保持 CLI/TUI 行为可验证，最终通过完整验收门禁。

---

## 附录 A：实现进展（截至 2026-05-30）

> 本附录收纳从正文剥离的"实现态/迁移进展/历史债务"。它带日期、随代码演进，**不构成约束**；与正文冲突时以正文为准。各条标注与之对应的正文小节。

### A.1 crate 物理结构与分层（§6.4.3 / §7.2）

- 顶层目录：`apps/cli`、`agent/{runtime,project,policy,prompt,provider,tools,storage,hook,audit,share}`、`packages/{sdk,gloabal/logging}`。
- COLA 内部分层现状（**按需分层，已对齐 #61 D4**）：
  - 完整三层 `core/business/utils`：`runtime`、`tools`
  - 两层 `core/business`：`provider`
  - 单层 `business`：`project`、`policy`、`prompt`、`storage`、`hook`
  - 平铺（仅 `api`）：`audit`（骨架）
- Public API 收窄（§6.4.3）：`prompt`/`provider`/`tools`/`storage`/`hook` 的 `api.rs` 已是收窄门面（精确 re-export + 内部 crate-private）；`project::api` 仍含 `pub use worktree::*` 通配 + `ProjectApiMarker`；`policy::api` 仅暴露 `security`；`audit::api` 仍是 `AuditApiMarker`。跨 crate 边界由 `check-crate-api-boundary.sh` 守护。

### A.2 share 瘦身（§6.4 rule6，#61 D2 完成）

- ToolRegistry→`tools::api`、TaskStore/task→`storage::api`、MemoryStore→`storage::api`、worktree 行为→`project::api`、skill loader→`prompt::skill` 均已迁出。
- `share` 现存：`error`、`message`、`session_types`、`string_idx`、`config`（schema/typed view，**无加载流程**）、`memory`/`task`（DTO/纯函数）、`tool.rs`（ToolContext DTO）、`skill_ops`（Skill DTO + parser）。
- Config 现状（§4.2 三分法）：`agent/share/src/config/`（约 21 文件）几乎全是 schema + serde 反序列化，被 7 个域（hook/project/prompt/provider/runtime/storage/tools）共享，不依赖 project；加载分层编排在 CLI/runtime bootstrap。结论：留在 share 合理，**不需独立 `agent/config` crate**。
- 守护：`check-share-minimal-kernel.sh`（禁止回归 store/IO/行为）。

### A.3 架构 guard 实际集合（§6.4.4）

`check-architecture-guards.sh` 当前聚合（>6，以脚本为准）：`check-cargo-dependency-graph.sh`、`check-cli-thin-entry.sh`、`check-share-no-upstream-deps.sh`、`check-cola-layer-purity.sh`、`check-forbidden-imports.sh`、`check-rust-file-lines.sh`、`check-crate-api-boundary.sh`、`check-share-minimal-kernel.sh`，以及多个 TUI 单源守卫（#55–#59 系列）。

### A.4 Chat 启动边界拆分进展（§6.6）

过渡对象 `ChatRuntimeContext`（client/registry/system_blocks/system_prompt_text/user_context/agent_runner/task_store/skills_map/hook_runner/memory_config/json_logger/agent_semaphore）、`ChatLaunchOptions`（cwd/verbose/markdown/context_size/resume/allow_all/max_tool_concurrency）、`NoTuiChatLaunch`/`TuiChatLaunch` 已定义；`ChatApplicationService` 经 `ChatRuntimePort` 分发到现有 REPL/TUI adapter，尚未改写 agent loop。`run_orchestration::setup` 的 `concurrency`/`permissions`/`model_runtime`/`provider_client`/`runtime_support` 已迁入 `agent/runtime::bootstrap`，`prompt_bundle`/`tooling`/CLI logging/session adapter 暂留入口侧。

### A.5 CLI 薄边界进展（§6.8）

原 fat CLI（`run_orchestration/` 三文件，曾约 438 行）已重构：`setup.rs`、`prompt.rs` 等已删，build_* 编排已归 `AgentClientImpl`。

### A.6 SDK 重构路线进展（§12）

- SDK-Phase 0–3：基本落地（sdk trait、composition root 瘦身、目录整理、架构守卫）。
- SDK-Phase 4：**第一轮投影已落地**（`ChatBootstrapArgs`、`ModelSummary`、扩展 `SessionSummary`，`models`/`sessions` 子命令改走 `sdk::AgentClient`，`TuiLaunchContext` 集中 TUI 启动对象）。**TUI 仍直连 runtime 内部对象，SDK-first 解耦未完成**——目标态与实现态最大缺口。
- `packages/sdk` 实际依赖：`tokio`/`serde`/`async-trait`/`serde_json`/`bitflags`（零业务依赖，符合 §6.4.1 rule2）。

### A.7 未实现域（→ #62）

- `agent/policy`：仅 `security` 内容扫描（`scan_content`/`SecurityWarning`），完整权限模型（PermissionRequest/Decision/Grant/Mode/Capability/RiskAssessment）未实现，运行时 allow-all。
- `agent/audit`：仅 `AuditApiMarker` 骨架，审计链路（correlation id、policy/hook/outcome 三分记录）未实现。
- 两者均强关联 #42 权限管控系统，实施前应与 #42 spec 对齐。

### A.8 待办小项

- `packages/gloabal/` → `packages/global/` 改名（涉及 workspace path + guard 路径，单独小 PR）。
- `project::api` 收窄（去 `pub use worktree::*` 通配）。
