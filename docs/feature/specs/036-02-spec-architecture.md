# #36 多 Agent 框架 — Spec / 架构与 Agent 设计

> **DDD 设计参考**：[Multi-Agent 框架 DDD 设计](../../superpowers/specs/2026-05-20-multi-agent-ddd-design.md) — Bounded Context、Context Map、领域事件、Outbox、Redis 消息层与 Repository 归属以该文档为准。

## 概述

将当前单 Agent 架构升级为 **Server + Agent 完全分布式** 的多 Agent 协作框架：

- **API Server** — 无状态 REST API + WebSocket gateway；负责鉴权、命令/查询、MongoDB 写入、Outbox 写入、Redis-backed BoardEvent 转发；NEVER 直接调度或点名调用 Agent。
- **MongoDB** — 业务状态真相源：聚合状态、Outbox、幂等记录、AgentRun 审计、Board snapshot projection、Qdrant 引用。
- **Redis Streams** — 消息层：IntegrationEvent、WorkQueue、BoardEvent、control signal、consumer group ack/reclaim。
- **Redis TTL keys** — Agent presence 与短期心跳信号；长期 Agent 状态仍以 MongoDB `agent_instances` 为准。
- **Agent Runtime** — 所有 Main Agent 类型都可多实例部署；通过 Redis WorkQueue 消费 `WorkItem`，通过 MongoDB lease/状态机保证幂等与恢复。
- **Scheduler / Evolver** — 可多实例运行；通过 workspace-level `ControllerLease` 避免重复 reconcile。
- **Sub-Agent** — Executor 内部按需调用的一次性无状态执行单元，不注册为 AgentInstance，不直接访问白板或消息层。
- **白板 UI** — REST 拉取 snapshot，WebSocket 接收 Redis-backed BoardEvent；BoardEvent 是 UI 投影，不是领域真相。

P0 设计约束：Executor 崩溃恢复、WorkItem 重试、幂等性、Redis pending reclaim、Outbox 发布一致性、WebSocket 断线后基于 Redis stream id 补发或要求重拉 snapshot。

## DDD 架构

> 本节基于 [DDD 设计文档](../../superpowers/specs/2026-05-20-multi-agent-ddd-design.md)。

### Bounded Context 划分

系统分为六个 Bounded Context：

```text
┌─────────────────────────────────────────────────────────────────┐
│                        Core Domain                              │
│                                                                 │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────┐  │
│  │ Conversation │    │ Requirement  │    │    Project       │  │
│  │ Context      │───▶│ Context      │───▶│    Context       │  │
│  │ Chat Agent   │    │ Assistant    │    │ Project / Task   │  │
│  └──────────────┘    └──────────────┘    └────────┬─────────┘  │
│                                                   │             │
│                                          ┌────────▼──────────┐  │
│                                          │ Orchestration     │  │
│                                          │ Context           │  │
│                                          │ Scheduler         │  │
│                                          │ Executor          │  │
│                                          │ WorkItem / Lease  │  │
│                                          │ SubAgent          │  │
│                                          └───────────────────┘  │
└─────────────────────────────────────────────────────────────────┘

┌──────────────────┐        ┌───────────────────────────────────┐
│ Evolution        │        │ Platform Context                  │
│ Context          │        │ Workspace / Auth / API Server     │
│ Evolver Agent    │        │ MongoDB / Redis / REST / WS       │
└──────────────────┘        └───────────────────────────────────┘
```

| BC | 职责 | 主要 Agent / 组件 |
|---|---|---|
| Conversation Context | 维护 Conversation/Message，分类用户输入，向用户汇报状态 | Chat Agent |
| Requirement Context | 将用户意图结构化为 Requirement，等待用户确认后流向 Project Context | Assistant Agent |
| Project Context | 管理 Project / Task 完整生命周期，Task 是 Project 的内部实体 | — |
| Orchestration Context | Scheduler/Evolver lease、WorkItem、AgentRun、ExecutorAssignment、故障恢复与重试 | Scheduler / Executor / Assistant / Evolver |
| Evolution Context | 异步扫描已完成 Project，提炼 Skill（Supporting Subdomain） | Evolver Agent |
| Platform Context | 多租户隔离、MongoDB 持久化、Redis 消息层、REST、WebSocket gateway、Outbox publisher | API Server / Infra |

### Context Map

| 上游 BC | 下游 BC | 集成模式 | 说明 |
|---|---|---|---|
| Conversation | Requirement | **Partnership** | 发布 `UserIntentCaptured` DomainEvent，经 Outbox 转为 IntegrationEvent |
| Requirement | Project | **Customer/Supplier** | 用户确认后发布 `RequirementConfirmed`，Project BC 创建 Project/Task |
| Project | Orchestration | **Published Language** | 发布 `TaskReadyForExecution`，Orchestration 创建 WorkItem 并投递 WorkQueue |
| Orchestration | Project | **Customer/Supplier（写回）** | Agent 执行完成后通过应用服务写回 Task 状态 |
| Orchestration | Platform | **Published Language + Infrastructure Service** | 使用 MessageBus / WorkQueue / PresenceStore 端口，不直接依赖 Redis client |
| Orchestration | LLM（packages/llm） | **Anticorruption Layer** | 隔离 provider 接口变动 |
| Evolution | Project | **Conformist** | 单向读取已完成 Project 数据 |

### 事件分层

跨 BC 业务流转通过事件驱动，但 MUST 区分三类事件：

| 类型 | 所在层 | 用途 |
|---|---|---|
| DomainEvent | Domain / Application | 聚合产生的业务事实，不包含 Redis stream id |
| IntegrationEvent | Infrastructure message contract | 经 Outbox 发布到 Redis Streams，供跨进程消费 |
| BoardEvent | UI projection event | 面向 WebSocket UI，可由 MongoDB Board snapshot 重建 |

事件转换链路：

```text
DomainEvent
  → OutboxEvent（MongoDB）
  → IntegrationEvent（Redis Streams）
  → BoardEvent / WorkQueue message / ControlSignal
```

典型 DomainEvent：

| 发布方 BC | 事件 | 下游动作 |
|---|---|---|
| Conversation | `UserIntentCaptured { conversation_id, intent, captured_at }` | Requirement 创建/分析 |
| Requirement | `RequirementDraftCreated { requirement_id, conversation_id, draft }` | BoardEvent 通知 UI |
| Requirement | `RequirementConfirmed { requirement_id, workspace_id, confirmed_at }` | Project 创建 Project/Task |
| Project | `TaskReadyForExecution { project_id, task_id, spec }` | Orchestration 创建 WorkItem |
| Orchestration | `WorkItemRequested { work_item_id, required_agent_type, kind }` | Outbox publisher 投递 Redis WorkQueue |
| Orchestration | `WorkItemCompleted { work_item_id, result_ref }` | Project 写回 Task 结果 |
| Orchestration | `AgentLost { agent_instance_id, last_heartbeat_at }` | 触发 lease/retry/reconcile |
| Evolution | `SkillPatternDiscovered { reflection_id, workspace_id }` | BoardEvent 通知 UI |

## 核心决策

| 维度 | 决策 |
|---|---|
| 通信模式 | MongoDB 保存业务状态，Redis Streams 作为消息层；Server 与 Agent 不通过 RPC 直接协作 |
| API 协议 | 前端使用 REST + WebSocket；Agent 使用 Redis WorkQueue + MongoDB repository，不暴露 Agent RPC 接口 |
| 消息机制 | Redis Streams + consumer group；Pub/Sub 仅 MAY 用于非关键短通知，NEVER 用于核心任务派发 |
| 数据库 | MongoDB（文档型，无外键，数组存引用），聚合状态与 Outbox 的真相源 |
| 调度层次 | Scheduler reconcile workspace 后创建 WorkItem；Assistant/Executor/Evolver 消费各自 WorkQueue |
| 租户隔离 | Workspace 归属租户（个人/团队），所有 stream/key/message MUST 携带 workspace_id 或租户隔离前缀 |
| Session 策略 | 5 类 MainAgent 各持有独立 LlmContext；Chat Agent 管理 Conversation；SubAgent 无状态，LlmContext 由调用方传递 |
| 白板访问 | Main Agent 通过 Application Service / Repository 访问业务数据；Sub-Agent 不可访问白板 |
| Agent 实现 | 一个通用 runtime + role 装配器；角色配置由 TOML 定义，支持用户自定义 Role |
| 需求分析 | Chat 接收用户消息并产生意图事件；Assistant 消费 WorkItem 做深度拆解，草案写入 Requirement.draft |
| Executor 分配策略 | Redis 派发 WorkItem；MongoDB WorkItem lease + Project/Task 状态机保证同一工作单元不会被重复执行 |
| Evolver | 可多实例后台 Agent；通过 ControllerLease 或 WorkItem shard 处理反思与 RAG，总结已完成项目并产出 Skill/MCP 建议 |
| P0 约束 | Outbox 必须保证 MongoDB 状态写入与 Redis 发布最终一致；Redis pending reclaim 后必须重新校验 MongoDB lease |
| MVP 交付 | v0.1 REST/WS + Redis/MongoDB 基础消息链路 → v0.2 多 Agent 编排 → v0.3 自我进化（Evolver + RAG） |
| P0 故障恢复 | Executor 崩溃 → lease/heartbeat 超时释放 WorkItem/Assignment；Task 重试 ≤3 次；idempotency_key 由 MongoDB unique index 保证；UI reconnect 使用 Redis stream id 补发或重拉 snapshot |

## 技术栈与框架

| 组件 | 选型 | 版本约束 | 说明 |
|------|------|---------|------|
| **HTTP / REST / WebSocket** | **axum** | 0.8+ | API Server 对前端暴露 REST 与 WebSocket gateway |
| **MongoDB** | **mongodb** crate（官方） | 3.x | 聚合状态、Outbox、幂等记录、AgentRun 审计；Transaction 依赖 MongoDB 5.0+ replica set |
| **Redis** | **redis** crate / async connection manager | 0.25+ | Streams、consumer group、TTL presence、control signal |
| **Qdrant** | **qdrant-client** crate（官方） | 1.x | 向量存储 + CRUD + Search |
| **OpenAPI / SDK** | aide + schemars + TS SDK 生成 | — | REST schema 从 Rust server 导出，SDK 自动生成 |
| **Cargo workspace** | apps + packages monorepo | — | 应用放 `apps/`，公共库与 SDK 放 `packages/` |

### 端口与服务映射

| 端口 | 协议 | 处理方 | 内容 |
|------|------|--------|------|
| 3000 | HTTP/1.1 + WS（axum） | API Server | REST command/query + WebSocket BoardEvent gateway |
| — | Redis Streams | Redis | Agent WorkQueue、IntegrationEvent、BoardEvent、control signal |
| — | MongoDB Driver | API Server / Agent Runtime | 聚合状态、Outbox、幂等记录、lease、AgentRun |
| — | — | Agent | Agent 不监听端口，不提供 RPC 服务；作为 Redis worker 运行 |

## 架构概览

### 仓库目录布局

#36 从 Sprint 0.5 起采用 apps + packages 的 monorepo 布局：

```text
apps/
  cli/        # 原 aemeath-cli，终端入口、TUI、REPL
  server/     # #36 API Server：REST/WS gateway + MongoDB/Redis infra
  agents/     # #36 Agent runtime、Redis worker、role config、Main/Sub-Agent features
  ui/         # 白板 Web UI
packages/
  core/       # 原 aemeath-core，公共核心库与 Domain/Application 抽象
  llm/        # 原 aemeath-llm，LLM client 公共库
  tools/      # 原 aemeath-tools，Tool 实现公共库
  proto/      # 暂停新增 RPC proto；后续仅保留兼容/内部 schema 时再评估
  sdk/        # REST/WS SDK，OpenAPI 自动生成
infra/
  mongodb/
  redis/
  deploy/
docs/
```

约束：`share/` 在 Sprint 0.5 后不再保留；Rust package 名为兼容现有代码可继续使用 `aemeath-core`、`aemeath-llm`、`aemeath-tools`、`aemeath-cli`。

```text
                 白板 UI（Vue + Element Plus，apps/ui/）
                  │ REST snapshot/query
                  │ WebSocket BoardEvent stream
                  ▼
┌──────────────────────────────────────────────────────────────┐
│                 API Server（无状态，多副本）                   │
│                                                              │
│  REST Command/Query     WebSocket Gateway     Outbox Publisher│
│        │                    │                    │            │
│        ▼                    ▼                    ▼            │
│     MongoDB             Redis BoardEvent     Redis Streams     │
│  Aggregates/Outbox      Stream tailing       publish/ack       │
└──────────────────────────────────────────────────────────────┘
          ▲                      ▲                    ▲
          │                      │                    │
          │ MongoDB Repository   │ Redis Streams      │
          │                      │                    │
┌─────────┴──────────────────────┴────────────────────┴────────┐
│                  Agent Runtime（可多实例）                     │
│                                                              │
│ Chat / Assistant / Scheduler / Executor / Evolver             │
│ - 写 AgentInstance MongoDB 摘要 + Redis presence TTL           │
│ - XREADGROUP 消费 WorkQueue / ControlSignal                    │
│ - MongoDB claim lease / 写 AgentRun / 写业务状态 / 写 Outbox    │
│ - Scheduler/Evolver 先抢 ControllerLease 再 reconcile           │
│ - Executor 内部按需唤起 Sub-Agent                              │
└──────────────────────────────────────────────────────────────┘
```

> **注**：MongoDB、Redis 和 Qdrant 是独立部署的外部服务，不运行于 API Server 进程内。MongoDB 与 Redis 之间无直接通信；应用通过 OutboxPublisher 将 MongoDB outbox_events 发布到 Redis Streams。Qdrant 向量写入由异步 worker 完成，MongoDB 只保存 `embedding_ref` 与 `embedding_status`。
>
> **DDD 限界上下文**：Project 上下文的聚合根只有 `Project`，`ProjectTask` 是 Project 聚合的子实体。ProjectTask 的生命周期管理通过 Project Context 应用服务执行。执行队列中的 `WorkItem` 属于 Orchestration Context，不是 ProjectTask。

## Redis 消息层设计

### Stream 命名

```text
aemeath:{tenant_id}:integration                 # 跨 BC IntegrationEvent
aemeath:{tenant_id}:board:{workspace_id}        # UI BoardEvent stream
aemeath:{tenant_id}:work:{agent_type}           # Agent WorkQueue
aemeath:{tenant_id}:control:{controller_type}   # scheduler/evolver/cancel 等控制信号
aemeath:{tenant_id}:presence:{agent_instance_id}# TTL key，短期在线状态
```

### WorkQueue message

Redis WorkQueue message 只放路由与引用字段：

```text
work_item_id
workspace_id
required_agent_type
kind
idempotency_key
payload_ref
created_at
schema_version
```

完整 payload 存 MongoDB `work_items` 或其引用对象中。Agent 消费 Redis message 后 MUST 重新加载 MongoDB WorkItem 并执行原子 claim/start；NEVER 仅凭 Redis message 执行业务副作用。

### Consumer group

```text
stream: aemeath:{tenant_id}:work:{agent_type}
group: agents:{agent_type}
consumer: {agent_instance_id}
```

失败恢复：

1. Agent 崩溃后 Redis message 留在 pending。
2. 其他同类型 Agent 使用 `XAUTOCLAIM` 接管超时 pending message。
3. 接管者加载 MongoDB WorkItem，校验 status/lease_owner/lease_expires_at/attempt。
4. 若 WorkItem 可重试则续租并执行；否则 XACK 或投递 dead-letter。

### Outbox 发布

所有会触发 Redis 消息的写操作 MUST 先写 MongoDB OutboxEvent。OutboxPublisher 可运行在 API Server 或独立 worker 中，可多实例部署；通过 MongoDB 原子 claim 保证同一 OutboxEvent 只由一个 publisher 发布。

## AgentRole（动态角色标识，支持内置 + 用户自定义）

角色不分固定枚举，而是字符串标识 + 角色配置文件。内置角色按生命周期分为两组：

| Main Agent（长期运行） | Sub-Agent（按需唤起） | 调用发起方 / 实际调度关系 |
|---|---|---|
| chat | - | WebSocket 连接层或 Chat WorkItem 触发；可多实例 |
| assistant | planner | 消费 assistant WorkQueue；Executor 内部可调用 planner |
| scheduler | coder | 多实例运行，按 workspace ControllerLease reconcile；Executor 内部可调用 coder |
| executor | tester | 消费 executor WorkQueue；内部调用 tester |
| evolver | reviewer | 多实例运行，按 ControllerLease 或 WorkItem shard 处理反思；Executor 内部可调用 reviewer |
|  | designer | Executor 内部调用 designer |

Rust 侧保留字符串常量引用：

```rust
/// 内置角色常量（代码引用用）
pub mod builtin_roles {
    pub const CHAT: &str = "chat";
    pub const ASSISTANT: &str = "assistant";
    pub const SCHEDULER: &str = "scheduler";
    pub const EXECUTOR: &str = "executor";
    pub const EVOLVER: &str = "evolver";
    pub const PLANNER: &str = "planner";
    pub const CODER: &str = "coder";
    pub const TESTER: &str = "tester";
    pub const REVIEWER: &str = "reviewer";
    pub const DESIGNER: &str = "designer";
}
```

## Agent 生命周期管控

所有 Main Agent 类型都可多实例部署。Agent 启动时：

1. 生成 `agent_instance_id`。
2. 写入或刷新 MongoDB `agent_instances`：agent_type、role、capabilities、max_concurrency、status、last_heartbeat_at。
3. 周期性刷新 Redis presence TTL key。
4. 按 `agent_type` 加入对应 Redis WorkQueue consumer group。
5. 执行期间写 `AgentRun` 与 WorkItem lease。
6. 收到 drain/cancel control signal 后停止接新任务，完成或释放当前 lease。

| Agent | 模式 | 生命周期 | 管控者 |
|---|---|---|---|
| **Chat** | 可多实例 | WS 连接或 Chat WorkItem 触发；连接断开后释放短期 presence | 连接层 / WorkQueue |
| **Assistant** | Worker Pool | 消费 assistant WorkQueue，空闲可常驻或由部署平台缩容 | 部署平台 + WorkQueue |
| **Scheduler** | 多实例 Controller | 抢 workspace-level ControllerLease 后 reconcile | ControllerLease |
| **Executor** | Worker Pool | 消费 executor WorkQueue，按 WorkItem lease 执行 Project/Task | WorkQueue + MongoDB lease |
| **Evolver** | 多实例 Controller/Worker | 抢 ControllerLease 或消费 evolver WorkQueue | ControllerLease / WorkQueue |

### 管控边界

- **部署平台**（本地进程、systemd、Docker Compose、K8s 等）负责进程数量与重启；Scheduler 不负责创建/销毁 OS 进程。
- **Scheduler** 负责 reconcile：根据 Project/Requirement/Task 状态创建 WorkItem、修复异常 lease、投递 control signal。
- **Agent Runtime** 负责消费 WorkQueue、续租、执行、写回结果。
- **Sub-Agent** 由 Executor 在进程内按需创建、异步调用，执行完毕即释放；不创建 AgentInstance 文档，不写 Redis presence。
- **Server MUST NOT 点名调用某个 Agent**；Agent 间协作通过 WorkItem、ControlSignal、Outbox/IntegrationEvent 完成。

### 健康与 presence

- Redis presence TTL 表示短期在线信号，例如 `aemeath:{tenant_id}:presence:{agent_instance_id}`。
- MongoDB `agent_instances.last_heartbeat_at` 是可查询摘要，由 Agent 定期刷新，或由 reconciler 根据 Redis presence 写回 Lost/Offline。
- Busy 超时、lease 超时、token 过期都通过 Application Service 与 MongoDB 状态机处理；Redis TTL 只是触发信号。

## Scheduler 设计

### 职责

- 多实例运行，通过 `ControllerLease(workspace_id, scheduler)` 获得某个 workspace 的 reconcile 权。
- 监听 Redis control stream 或定期扫描 MongoDB，发现需要处理的 Requirement / Project / Task。
- 创建 `WorkItem` 并写 Outbox，由 OutboxPublisher 投递到对应 Redis WorkQueue。
- 对账 WorkItem lease、AgentRun、Project/Task 非终态、cancel 信号与 retry policy。
- NEVER 直接创建/销毁进程，NEVER 直接调用 Executor/Assistant。

### ControllerLease

```text
controller_leases
- workspace_id
- controller_type: scheduler | evolver
- owner_agent_id
- lease_expires_at
generation
```

抢占规则：

```text
findOneAndUpdate(
  { workspace_id, controller_type, $or: [ { lease_expires_at: { $lt: now } }, { owner_agent_id } ] },
  { $set: { owner_agent_id, lease_expires_at: now + ttl }, $inc: { generation: 1 } }
)
```

同一个 `workspace_id + controller_type` 同时只能有一个有效 owner。owner 必须周期性续租；续租失败后立即停止该 workspace 的 reconcile。

### 调度流程

#### Requirement → Assistant 分析链路

```text
1. Scheduler 获得 workspace ControllerLease
2. 扫描 Requirement(status=pending/analyzing 且无活跃 WorkItem)
3. 创建 WorkItem(kind=AnalyzeRequirement, required_agent_type=assistant, payload_ref=requirement_id)
4. 写 OutboxEvent(WorkItemRequested)
5. OutboxPublisher 投递 aemeath:{tenant}:work:assistant
6. Assistant Agent XREADGROUP 消费并 claim WorkItem
7. Assistant 写 Requirement.draft / 状态 / OutboxEvent
```

#### Project / Task → Executor 执行链路

```text
1. Scheduler 获得 workspace ControllerLease
2. 扫描 Project/Task 可执行状态
3. 创建 WorkItem(kind=ExecuteProject 或 ExecuteTask, required_agent_type=executor)
4. OutboxPublisher 投递 executor WorkQueue
5. Executor claim WorkItem，创建 AgentRun
6. Executor 编排 Sub-Agent 执行 Task
7. Executor 通过 Project Context 应用服务写回 TaskResult / TaskStatusChanged
8. 完成后 XACK Redis message，释放 lease
```

### 可配置参数

```jsonc
{
  "scheduler": {
    "controller_lease_ttl_sec": 30,
    "reconcile_interval_sec": 5,
    "full_scan_rate_limit_sec": 60,
    "work_item_lease_ttl_sec": 120,
    "max_work_item_retries": 3,
    "blocked_timeout_sec": 3600,
    "cancel_timeout_sec": 60
  },
  "redis": {
    "stream_read_count": 10,
    "stream_block_ms": 5000,
    "pending_idle_timeout_ms": 60000,
    "stream_retention_max_len": 10000
  },
  "agent": {
    "heartbeat_interval_sec": 15,
    "presence_ttl_sec": 45,
    "drain_timeout_sec": 60
  }
}
```

## Evolver 设计（含 RAG）

Evolver 是对系统元认知的出口——它观察已完成的工作，提炼可复用模式，产出新的 Skills 和 MCP 配置，驱动系统自我进化。

Evolver 可多实例运行，处理方式二选一：

1. 抢 `ControllerLease(workspace_id, evolver)` 后扫描近期已完成 Project。
2. 消费 `evolver` WorkQueue 中的反思 WorkItem。

### Embedding 写入时机

API Server 或后台 worker 在写入以下文档时异步生成 embedding，并将向量与检索 payload 写入 Qdrant。MongoDB 只保存业务文档和 `embedding_ref`，不承担向量检索职责。

| 文档 | embedding 内容 | 触发时机 |
|---|---|---|
| ChatMessage | 用户消息 / Agent 回复的可检索摘要（仅 message_type=requirement；普通消息不向量化，embedding_status 为 not_applicable） | ChatMessage 写入时 |
| Requirement | 需求文本提炼 | Requirement 写入时 |
| Project（状态=completed） | project name + summary + 关键决策描述 | Project 完成时 |
| ProjectTask（状态=completed） | task name + Executor 产出摘要 + 遇到的坑/解法 | Task 完成时 |
| Reflection | 模式总结 + Skill/MCP 产出描述 | Reflection 写入时 |

### 反思流程

```text
1. Evolver 获得 workspace lease 或消费 ReflectProject WorkItem
2. 扫描近期已完成但未反思的 Project
3. 对 Project/Task/Requirement 做 Qdrant 检索
4. LLM 综合上下文产出模式总结、Skill 优化、MCP 建议
5. 写 Reflection / Project.reflected_at / OutboxEvent
6. OutboxPublisher 发布 BoardEvent，UI 显示反思结果
```

## Agent 实现：模板 + 装配器

### 通用模板（`agents/src/template.rs`）

Agent 运行时核心：

- Redis WorkQueue consumer loop
- MongoDB WorkItem claim / lease renew / result writeback
- LLM 对话循环
- 工具调用执行
- Sub-Agent 调用（仅 Executor）
- 上下文管理（压缩、token 估算）
- AgentRun 审计记录

### 装配器（`agents/src/assembler.rs`）

根据角色配置组装 Agent：

```text
assembler.assemble(role: RoleConfig) -> ConfiguredAgent {
    system_prompt: role.system_prompt,
    skills: role.skills,
    mcp_servers: role.mcp.servers,
    can_call_roles: role.permissions.can_call_roles,
    model_selector: ModelSelector::from(role.models),
    tools: role.permissions.allowed_tools,
    work_queue: WorkQueue::for_role(role.name),
}
```

### 角色配置（`agents/roles/`）

Main Agent 和 Sub-Agent 均通过 `RoleConfig` + 装配器创建。每个角色使用独立 TOML 配置文件；Main Agent 包括 `chat.toml`、`assistant.toml`、`evolver.toml`、`scheduler.toml`、`executor.toml`，Sub-Agent 包括 `planner.toml`、`coder.toml`、`tester.toml`、`reviewer.toml`、`designer.toml`。

```toml
# scheduler.toml
name = "scheduler"
description = "多实例 Scheduler Controller，负责 workspace reconcile 与 WorkItem 创建"
pool_size = 1
system_prompt = "你是 Scheduler Agent，负责 reconcile Project/Requirement，创建 WorkItem 并处理恢复。"
skills = []
mcp = { servers = [] }

[[models]]
model = "deepseek/deepseek-chat"
cost_tier = "low"

[permissions]
allowed_tools = []
scope = ["board_read", "board_write", "work_item_write"]
can_create_agents = false
can_call_roles = []
max_subagents = 0
```

```toml
# executor.toml
name = "executor"
description = "消费 WorkItem，编排 Sub-Agent 执行 Project/Task，写回结果"
pool_size = 3
system_prompt = "你是 Executor Agent，负责消费执行类 WorkItem、编排 Sub-Agent 执行 Task，并将结果写回。"
skills = ["task-management"]
mcp = { servers = [] }

[[models]]
model = "anthropic/claude-sonnet-4-20250514"
cost_tier = "high"

[permissions]
allowed_tools = ["agent_call"]
scope = ["board_read", "board_write", "work_item_claim", "work_item_complete"]
max_subagents = 5
can_call_roles = ["planner", "coder", "tester", "reviewer", "designer"]
can_create_agents = false
```

## RBAC Scope 定义

scope 由 REST/WS 中间件、Application Service 与 Agent runtime 共同校验：

| scope | 说明 | 允许的调用方 |
|-------|------|-------------|
| `board_read` | 读取白板 snapshot、Project、Requirement、Agent 列表 | UI 客户端、Main Agent |
| `board_write` | 写入白板相关业务状态 | UI 客户端、Chat、Assistant、Executor、Evolver |
| `work_item_read` | 读取 WorkItem | Scheduler、Assistant、Executor、Evolver |
| `work_item_write` | 创建 WorkItem | Scheduler、Application Service |
| `work_item_claim` | claim / renew WorkItem lease | Assistant、Executor、Evolver |
| `work_item_complete` | 完成 WorkItem / 写 AgentRun | Assistant、Executor、Evolver |
| `presence_write` | 写 Redis presence / MongoDB heartbeat 摘要 | Agent Runtime |

### Token 生命周期

1. **签发**：Agent 启动时从本地配置或部署平台获得启动凭据；API Server 可通过 REST 管理 token，也 MAY 由离线配置提供。
2. **传递**：REST/WS 使用 `Authorization: Bearer <jwt>`；Agent runtime 访问 MongoDB/Redis/Application API 时使用同一授权模型或部署侧 service credential。
3. **刷新**：优先由部署平台或 API Server REST token endpoint 刷新；不依赖 Agent RPC。
4. **过期**：鉴权失败后 Agent 进入 Draining/Error，释放或等待 lease 超时；Scheduler/reconciler 后续恢复 WorkItem。
5. **Scope 更新**：scope 在签发时确定，不支持热更新。角色变更需重启对应 AgentInstance。
