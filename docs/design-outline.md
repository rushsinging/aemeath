# Aemeath 设计总纲

> 来源：[DDD 架构设计](snapshot/specs/047-ddd-redesign.md) · [TUI Model/View](superpowers/specs/2026-05-27-tui-model-view-architecture.md) · [TUI SDK DTO](snapshot/specs/047-tui-sdk-dto-boundary-design.md) · [Runtime UUIDv7](superpowers/specs/2026-06-13-runtime-tui-uuidv7-id-design.md) · [Context 所有权](superpowers/specs/2026-06-07-agent-context-ownership-redesign.md) · [Server MVP](superpowers/specs/2026-06-01-server-foundation-mvp-design.md)

---

## 一、架构总纲

### 核心域

Agent Runtime——把一次用户输入推进成完整的 Agent 协作过程：构造上下文、调用模型、执行工具、调度 SubAgent、处理用户交互、维护任务进度、判断停止条件，并输出最终结果。

### 统一语言

| 术语 | 定义 |
|---|---|
| Agent | 由 `ConfigurationSnapshot` 解析出的配置化执行者实体 |
| Session | 用户与 Aemeath 的持续协作容器 |
| Chat | 一次用户输入触发的完整处理单元 |
| Agent Looping | Chat 内部的循环推进机制，协调模型调用、工具执行、SubAgent、用户交互、Task 更新和停止条件 |
| Turn | Agent Looping 中某个 Agent 针对一个目标的一次执行片段 |
| SubAgent | 由父 Turn 委托创建的 child Turn，使用不同 Agent 配置或 role |
| Task | 运行时规划和跟踪复杂工作的状态，由 Agent Looping 创建、推进和完成 |

### Bounded Context

**Core Domain — Agent Runtime**：维护 Agent/Session/Chat/Turn/Task，在 Chat 内执行 Agent Looping，调用 Provider/Tool/Memory/Prompt，创建和调度 SubAgent。

**Supporting Domains**：

| Context | 职责 |
|---|---|
| Config | 统一加载多来源配置，解析为不可变 `ConfigurationSnapshot` |
| Tool | 管理 ToolCatalog/SkillCatalog/SlashCommand catalog，将 ToolCall 转为受控执行 |
| Project Context | 维护 project root、worktree stack、git branch，提供路径和资源事实 |
| Policy | 权限和风险判断，支持 AskMe/Auto/Plan/AllowAll 语义 |
| Audit | 独立记录权限、hook、工具、模型调用和最终 outcome |
| Memory | 管理长期知识检索、沉淀、提醒，不依赖 Prompt |
| Prompt | 加载并合并 AGENTS.md / guidance / system prompt，管理 GuidanceProfile 和 PromptContract |

**ACL / Infrastructure**：

| Context | 职责 |
|---|---|
| Provider | 将内部 ModelRequest 转为 provider request，将 streaming chunk 归一化为内部事件 |
| Hook | 将生命周期事件转为 hook input，执行外部 hook command，解析 HookDecision |
| Session History | 保存 Session/Chat/Turn/Task 等持久化投影 |
| External Adapters | provider / filesystem / git / shell / web / MCP / terminal 统一归 `shared/adapter/` |

### COLA 分层

`agent/` 采用 **feature-boundary 纵向切分**：DDD feature boundary 决定外部边界，COLA 负责每个 feature 内部的工程分层。每个 feature 是一个 Bounded Context，内部私有，只有 `contract` + `gateway`（经 `api.rs`）可跨边界访问。

```
agent/
  features/          # 业务 feature boundary（每个 = 一个 Bounded Context）
    runtime/         # Agent Loop / turn / session state / compact / cost
    tools/           # Tool + Skill + Slash Command 能力注册与执行
    provider/        # LLM provider gateway（协议归一化）
    prompt/          # guidance / system prompt material
    project/         # cwd / paths / worktree / git facts
    storage/         # session / memory / task / history 持久化投影
    policy/          # permission / risk 判断
    hook/            # 生命周期事件 → hook input → HookDecision
    audit/           # 审计事件 / 操作轨迹
  shared/            # 横切基础设施、port、adapter、shared kernel
  composition/       # 组合根：唯一生产装配入口

packages/
  sdk/               # AgentClient trait + 公共类型（CLI ↔ Runtime 通信契约）
  global/logging/    # 日志 projection 适配
```

Feature 内部统一 COLA 分层：`contract/`（Published Language）→ `gateway/`（Open Host Service）→ `api.rs`（facade）→ `business/`（domain rules）→ `adapter/infra`。

### 依赖铁律

```
share   → ∅
project → share
tools   → share, project, storage
runtime → 全部 feature + share + sdk + logging
composition → runtime, tools, provider, project, sdk
```

无任何 feature 能依赖 runtime。git 进程调用不进 share。

### 关键约束

- **薄入口**：CLI/TUI/REPL 等 inbound adapter 只负责输入解析、事件展示、终端管理；不承载核心逻辑。
- **统一应用服务**：所有入口接入 `runtime` feature 暴露的同一组入口无关 API；`runtime` 是唯一编排者。
- **协议无关事件**：RuntimeEvent / InteractionRequest / PermissionPrompt / ToolExecutionEvent 等输出协议无关，TUI/CLI/HTTP 只是不同 projection。
- **Config 不独立 feature**：schema 归 `shared/config/`，加载编排归 composition root / runtime bootstrap。
- **PermissionDecision 与 HookDecision 分离**。
- **Memory 不依赖 Skill/Guidance**。

---

## 二、Runtime

### Agent Looping

Agent Looping 是 Chat 内部的循环推进机制：

```
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

### Tool 执行编排

LLM 返回 tool_use → Agent 收集 → 并发执行 → 结果注入回消息。`Tool` trait 与 `ToolRegistry` 定义在 `agent/features/tools`；runtime 只负责循环里的调度与结果回填。

### 内部 ID 体系（UUIDv7）

内部实体 ID 与 provider 协议 ID 严格分离：

| 层 | ID 类型 | 来源 | 用途 |
|---|---|---|---|
| 内部 | `ChatId` / `ChatTurnId` / `ToolCallId`（UUIDv7 newtype） | runtime/TUI 生成 | 跨 chat/turn/tool join |
| Provider 协议 | `provider_id: String` | provider stream 返回 | 回填给 LLM 时使用 |
| 持久化 | 内部 UUIDv7 serde 为字符串 | 会话落盘 | 旧非 UUIDv7 id 加载时临时重新生成 |

**ToolCall 双 ID**：`id: ToolCallId`（内部 UUIDv7）+ `provider_id: String`（provider 返回）。

核心规则：
- 新会话中所有内部 ID **MUST** 为 UUIDv7（`new_v7()`）。
- Provider 返回的 tool id **MUST NOT** 作为内部 `ToolCallId`。
- 回填给 LLM 时 **MUST** 使用 `provider_id`，通过内部 `ToolCallId` 查找。
- 旧历史非 UUIDv7 id **MUST** 临时重新生成，不持久化兼容映射。

ID 类型定义在 `packages/sdk`，由 runtime/TUI 复用。

### Agent Context 所有权

**project 拥有 workspace 的类型与规则，runtime 仅持有实例生命周期。**

| 类型 | 所在 crate | 职责 |
|---|---|---|
| `WorkspaceService` | `agent/features/project` | enter/exit worktree、git 校验，单一可变状态源 |
| `WorkspaceFrame` | `agent/features/project`（内部） | 替代原 `WorkingContext`，不跨 crate |
| `PersistedWorkspaceContext` | `agent/shared` | 纯 serde DTO，仅用于会话持久化 |

收益：消除撕裂读（单一 Mutex）、子 agent 工作目录隔离、六边形合规（domain 不直接调 git）。

---

## 三、TUI

### Model/View 分离

TUI 的核心模型是"用户与 Agent 的交互会话"，不是屏幕区域。

1. 业务真相只存在于 Model State；View State 只服务显示，不能反向决定业务状态。
2. 保留 TEA 外壳，用 Model Context 重构 TEA Model 内部边界。
3. Agent/SDK/runtime 事件进入 TUI 后必须先被适配为内部意图，不能直接修改输出行。
4. Render 只消费 ViewModel 和 ViewState，不匹配 tool id、不修改模型、不根据文本反推状态。

### 分层数据流

```
External Event → Msg → Application Coordinator / update → Model → ViewAssembler → ViewModel → Render → Effect
```

| 层 | 职责 |
|---|---|
| Msg | TEA update loop 统一入口，包装 terminal / Agent / timer / hook 等外部输入 |
| Model | 按业务能力拆分的 Context（Conversation / Input / Runtime / Diagnostic），保存业务真相和状态转换规则 |
| Intent | Application Coordinator 发给某个 Model Context 的处理意图 |
| Change | Model Context 处理 Intent 后产生的状态变化事实 |
| ViewAssembler | 从 Model + ViewState 组装 ViewModel |
| ViewState | 纯显示交互状态（scroll / collapse / selection / animation / render cache） |
| Render | 把 ViewModel + ViewState 画到 terminal 的 ratatui 层 |
| Effect | update 后需要 runtime 执行的副作用描述 |

### Model Context

| Context | 职责 |
|---|---|
| Conversation | 消息列表、tool call 状态、agent progress，维护对话真相 |
| Input | 用户输入编辑、历史导航、自动补全、slash 命令识别 |
| Runtime | 会话状态（idle / processing / waiting）、连接状态、cancel 信号 |
| Diagnostic | 内部诊断信息（token 使用、成本、调试日志） |

### SDK DTO 边界

- `apps/cli/src/tui/**` 不出现 `runtime::api` 或 `::runtime` 类型依赖。
- `sdk::ChatEvent` 使用强类型 SDK DTO（`ToolResultImage` / `AgentProgressEventView` / `WorkspaceContextView` 等）。
- TUI 内部事件和渲染状态只使用 SDK DTO 或 TUI 私有 view model。
- runtime 类型与 SDK DTO 的转换集中在 `agent/runtime` 的 `AgentClientImpl` 及 composition root。

### 关键约束

- `ToolCall.status` 是 tool 标题图标和颜色的唯一来源。
- TUI 作为 CLI Adapter 不定义 Domain Model，通过 `AgentClient` trait（`packages/sdk`）与 Runtime 通信。

---

## 四、Server（后续）

### 概述

将 Aemeath 从单机 CLI 扩展为**多租户、硬隔离**的 agent server。MVP 只证整条管道——控制面、worker 协议、CLI 双模式——用最小实现让它真能跑、能 dogfood。

### 进程拓扑

```
CLI（双模式，TUI 不变）
 ├─ 直连:   AgentClientImpl（本地 runtime，进程内直调）
 └─ server: ServerSessionClient ──WS(TCP)──┐
                                            ▼
              控制面进程  aemeath serve（常驻）
              WsProxy: 终结 client WS、auth/路由（不解析帧内容）
              SessionManager: session_id → WorkerHandle
              WorkerLauncher（LocalProcess）
                    │
                    ├── worker 进程 A（会话A，uds WS）
                    ├── worker 进程 B（会话B，uds WS）
                    └── ...
```

同一个 `aemeath` 二进制，三种角色：默认（CLI）/ `serve`（控制面）/ `worker`。

### 核心决策

| 决策 | 内容 |
|---|---|
| 硬隔离 | 每会话独立 worker 进程/沙箱 |
| 单一协议 | `AgentClient`-over-WS（`Call`/`Resp`/`Frame`），前门（TCP）与后轴（uds）同一套协议 |
| 控制面薄代理 | 只做路由/调度/隔离/代理，帧内容一律透传不反序列化，**NEVER 承载领域实体** |
| worker 自托管 WS | worker = 现有 runtime + WS server，runtime 一行不改 |
| CLI 双模式 | `--server <url>` 连远端 / 缺省本地直连，composition 注入切换 |
| 契约预留多 agent | `ChatEvent`/`SessionSnapshot` 带 `AgentId`，Single 模式退化为 `"main"` |

### 架构边界

| scope | 实体 | 归属 |
|---|---|---|
| session 级 | 对话/Turn/Agent Loop/workspace | worker + session 存储 |
| 账户/项目级 | Requirement/Project/Task/团队 | 独立"协作域"BC（新服务，自有 DB） |
| 基础设施级 | session 注册表/worker 调度/配额 | 控制面 |

### 非目标（defer）

认证/多租户隔离、中心 DB、真沙箱（容器/microVM）、跨机/控制面 HA、资源治理、swarm。
