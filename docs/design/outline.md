# Aemeath 设计总纲

> 详细设计稿见 `snapshot/specs/` 与 `superpowers/specs/`。

## 核心域

Aemeath 的核心域是 **Agent Runtime**——把一次用户输入推进成完整的 Agent 协作过程：构造上下文、调用模型、执行工具、调度子代理、处理用户交互、维护任务进度、判断停止条件，并输出最终结果。

## 统一语言

| 术语 | 定义 |
|---|---|
| Agent | 由 `ConfigurationSnapshot` 解析出的配置化执行者实体 |
| Session | 用户与 Aemeath 的持续协作容器 |
| Chat | 一次用户输入触发的完整处理单元 |
| Agent Looping | Chat 内部的循环推进机制，协调模型调用、工具执行、子代理、用户交互、Task 更新和停止条件 |
| Turn | Agent Looping 中某个 Agent 针对一个目标的一次执行片段 |
| SubAgent | 由父 Turn 委托创建的子 Turn，使用不同 Agent 配置或 role |
| Task | 运行时规划和跟踪复杂工作的状态，由 Agent Looping 创建、推进和完成 |

## Bounded Context

### 核心域 — Agent Runtime

维护 Agent / Session / Chat / Turn / Task，在 Chat 内执行 Agent Looping，调用 Provider / Tool / Memory / Prompt，创建和调度 SubAgent。

### 支撑域

| 上下文 | 职责 |
|---|---|
| Config | 统一加载多来源配置，解析为不可变 `ConfigurationSnapshot` |
| Tool | 管理 ToolCatalog / SkillCatalog / SlashCommand catalog，将 ToolCall 转为受控执行 |
| Project Context | 维护 project root、worktree stack、git branch，提供路径和资源事实 |
| Policy | 权限和风险判断，支持 AskMe / Auto / Plan / AllowAll 语义 |
| Audit | 独立记录权限、hook、工具、模型调用和最终 outcome |
| Memory | 管理长期知识检索、沉淀、提醒，不依赖 Prompt |
| Prompt | 加载并合并 AGENTS.md / guidance / system prompt，管理 GuidanceProfile 和 PromptContract |

### 基础设施层

| 上下文 | 职责 |
|---|---|
| Provider | 将内部 ModelRequest 转为 provider request，将 streaming chunk 归一化为内部事件 |
| Hook | 将生命周期事件转为 hook input，执行外部 hook command，解析 HookDecision |
| Session History | 保存 Session / Chat / Turn / Task 等持久化投影 |
| External Adapters | provider / filesystem / git / shell / web / MCP / terminal 统一归 `shared/adapter/` |

## 六边形架构原则

核心域通过 **端口（Port）** 与外部世界交互，适配器（Adapter）实现端口契约：

```
              ┌─────────────────────────────────────┐
              │          Inbound Adapters            │
              │   CLI · TUI · Server(WsProxy)        │
              └──────────────┬──────────────────────┘
                             │  调用端口
              ┌──────────────▼──────────────────────┐
              │        Application Service           │
              │           (runtime)                  │
              │  ┌───────────────────────────────┐  │
              │  │       Domain Model             │  │
              │  │  Session · Chat · Turn · Task  │  │
              │  └───────────────────────────────┘  │
              │  ┌───────────────────────────────┐  │
              │  │       Domain Ports             │  │
              │  │  Provider · Tool · Memory ·    │  │
              │  │  Prompt · Policy · Storage     │  │
              │  └───────────────────────────────┘  │
              └──────────────┬──────────────────────┘
                             │  实现端口
              ┌──────────────▼──────────────────────┐
              │         Outbound Adapters            │
              │  LLM · Git · FS · MCP · Web · DB     │
              └─────────────────────────────────────┘
```

- **入站适配器**（Inbound）：CLI / TUI / Server，只负责协议转换，不承载业务逻辑。
- **出站适配器**（Outbound）：Provider / Git / FS / MCP 等，实现核心域定义的端口。
- **应用服务**（Application Service）：`runtime` feature，唯一编排者，所有入站适配器接入同一组 API。
- **领域模型**（Domain Model）：Session / Chat / Turn / Task / ToolCall 等实体和值对象。
- **领域端口**（Domain Port）：`AgentClient` trait / `Tool` trait / `Storage` trait 等核心域定义的抽象。

## COLA 工程分层

DDD 决定边界，COLA 负责每个 feature 内部的工程分层。每个 feature 是一个 Bounded Context，内部私有，只有 `contract` + `gateway`（经 `api.rs`）可跨边界访问。

```
agent/
  features/          # 业务 feature boundary（每个 = 一个 Bounded Context）
    runtime/         # 核心域：Agent Loop / turn / session state / compact / cost
    tools/           # 支撑域：Tool + Skill + Slash Command 注册与执行
    provider/        # 基础设施：LLM provider 协议归一化
    prompt/          # 支撑域：guidance / system prompt material
    project/         # 支撑域：cwd / paths / worktree / git facts
    storage/         # 基础设施：session / memory / task / history 持久化投影
    policy/          # 支撑域：permission / risk 判断
    hook/            # 基础设施：生命周期事件 → hook input → HookDecision
    audit/           # 支撑域：审计事件 / 操作轨迹
  shared/            # 横切基础设施：port / adapter / shared kernel（ids / errors / types / config schema）
  composition/       # 组合根：唯一生产装配入口

packages/
  sdk/               # AgentClient trait + 公共类型（CLI ↔ Runtime 通信契约）
  global/logging/    # 日志 projection 适配
```

Feature 内部分层：`contract/`（Published Language）→ `gateway/`（Open Host Service）→ `api.rs`（facade）→ `business/`（domain rules）→ `adapter/infra`。

## 依赖铁律

```
share       → ∅
project     → share
tools       → share, project, storage
runtime     → 全部 feature + share + sdk + logging
composition → runtime, tools, provider, project, sdk
```

- 无任何 feature 能依赖 runtime。
- git 进程调用不进 share（`check-share-minimal-kernel.sh` 禁止）。
- Config 不独立 feature：schema 归 `shared/config/`，加载编排归 composition root / runtime bootstrap。

## 关键约束

- **薄入口**：CLI / TUI / Server 等入站适配器只负责输入解析、事件展示、终端管理；不承载 Agent Looping、Task 状态机、PermissionDecision、Tool Execution 等核心逻辑。
- **统一应用服务**：所有入站适配器接入 runtime feature 暴露的同一组入口无关 API；runtime 是唯一编排者。
- **协议无关事件**：RuntimeEvent / InteractionRequest / PermissionPrompt / ToolExecutionEvent 等输出协议无关，TUI / CLI / HTTP 只是不同 projection。
- **PermissionDecision 与 HookDecision 分离**。
- **Memory 不依赖 Skill / Guidance**。

## 模块设计索引

| 模块 | 设计文档 | 角色 |
|---|---|---|
| Runtime | [runtime-design.md](runtime-design.md) | 核心域，Agent Looping 主循环 |
| TUI | [tui-design.md](tui-design.md) | 入站适配器，用户交互界面 |
| Server | [server-design.md](server-design.md) | 入站适配器，多租户远端服务 |
