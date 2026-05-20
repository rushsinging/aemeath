# #36 多 Agent 框架 — Spec / 架构与 Agent 设计

## 概述

将当前单 Agent 架构升级为多 Agent 协作框架，参考 K8s 控制面设计：
- **API Server**（数据面）— gRPC（Agent ↔ API Server）+ REST/WebSocket（前端），白板数据 CRUD + Watch
- **Scheduler**（调度面）— 管理 Agent Pool 生命周期，按需求量动态扩缩
- **Agent 角色**— 5 类 Main Agent（Chat / Scheduler / Executor / Assistant / Evolver）+ Sub-Agent（Executor 唤起，无状态）
- **对话/分析拆解**— Chat 分析消息类型，Scheduler 调度 Assistant 做需求深度拆解为 Project/Task，草案存入 `Requirement.draft`；Chat 面向用户多轮对话并汇报状态
- **白板**（呈现层）— 纯 UI，通过 REST/WS 调 Server 获取数据渲染
- **MVP 分级交付**— v0.1 单 Agent+白板 → v0.2 多 Agent 编排 → v0.3 自我进化
- **P0 设计约束**— Executor 崩溃恢复、Task 重试、幂等性、Watch 断线恢复


## 核心决策

| 维度 | 决策 |
|---|---|
| 通信模式 | 白板 SSOT，所有 Agent 通过 API Server 间接通信 |
| API 协议 | tonic gRPC（Agent ↔ API Server）+ REST/WebSocket（前端） |
| Watch 机制 | API Server 独占 MongoDB Change Streams；Agent / 前端通过 gRPC Server Streaming / WebSocket 间接消费 |
| 数据库 | MongoDB（文档型，无外键，数组存引用） |
| 调度层次 | 两条链路：Scheduler → Assistant；Scheduler → Executor → Sub-Agent |
| 租户隔离 | Workspace 归属租户（个人/团队） |
| Session 策略 | 5 类 Main Agent（Chat / Scheduler / Executor / Assistant / Evolver）各自管理上下文；Sub-Agent 无状态，上下文由 Executor 进程内调用传递 |
| 白板访问 | Chat / Scheduler / Executor / Assistant / Evolver 可访问；Sub-Agent 不可访问 |
| Agent 实现 | 一个通用模板 + 装配器（role → skill/MCP/prompt/权限），角色配置由 TOML 定义，支持用户自定义 Role |
| 需求分析 | Chat 接收用户消息 + 分析消息类型；Scheduler 调度 Assistant 做需求深度拆解 → Project/Task，草案存入 `Requirement.draft`，用户确认后写入 Project/Task |
| Executor 分配策略 | Executor 按 Project 独占绑定，一个 Project 同时只分配一个 Executor |
| Evolver | 独立后台进程，定期扫描白板：总结已完成项目 → 提炼可复用模式 → 生成/优化 Skills、MCP 配置，驱动系统自我进化 |
| P0 约束 | MongoDB replica set（Change Streams 必需）；Change Streams 由 API Server 独占订阅，Agent / 前端通过 gRPC stream / WebSocket 间接消费 |
| MVP 交付 | v0.1 单 Agent + 白板 → v0.2 多 Agent 编排 → v0.3 自我进化（Evolver + RAG） |
| P0 故障恢复 | Executor 崩溃 → 心跳超时释放 Project，并仅在崩溃恢复时将非终态 Task（InProgress/InReview/Retrying）回退 Pending；Task 重试 ≤3 次；gRPC 幂等写（idempotency_key）；Watch resume_token 断线续传 |


## 技术栈与框架

| 组件 | 选型 | 版本约束 | 说明 |
|------|------|---------|------|
| **gRPC** | **tonic** | 0.12+ | Rust 最成熟的 gRPC 框架，async/.await 原生支持，proto 驱动 9 个 Service 的代码生成 |
| **HTTP / REST** | **axum** | 0.8+ | 基于 tower + tokio，WebSocket 原生支持（`axum::extract::ws`） |
| **端口模型** | 拆端口部署 | — | REST/WebSocket 监听 `3000`，gRPC 监听 `50051`；避免 tonic + axum 共享端口分流增加 MVP 复杂度 |
| **MongoDB** | **mongodb** crate（官方） | 3.x | 支持 Change Streams + Transaction（依赖 MongoDB 5.0+ replica set） |
| **Qdrant** | **qdrant-client** crate（官方） | 1.x | 向量存储 + CRUD + Search |
| **Proto 管理** | tonic-build + prost | — | 编译期从 .proto 生成 Rust 代码；Sprint 0 使用 `share/proto/`，Sprint 0.5 后迁移到 `packages/proto/`，独立于 Rust workspace |
| **Cargo workspace** | apps + packages monorepo | — | 应用放 `apps/`（cli/server/agents），公共库与协议/SDK 放 `packages/`（core/llm/tools/proto/sdk） |

### 端口与服务映射

| 端口 | 协议 | 处理方 | 内容 |
|------|------|--------|------|
| 50051 | gRPC（tonic） | API Server | 9 个 Service 的全部 RPC |
| 3000 | HTTP/1.1 + WS（axum） | API Server | REST CRUD 端点 + WebSocket（BoardSnapshot / Chat） |
| — | — | Agent | agent 不监听端口，作为 gRPC client 通过 AgentRegistryService.Heartbeat 单向上报 |

> MVP 采用拆端口部署：REST/WS 与 gRPC 分别绑定 listener。共享端口可作为后续运维收敛项评估，但不是 #36 v0.1 的默认实现。


## 架构概览

### 仓库目录布局

#36 从 Sprint 0.5 起采用 apps + packages 的 monorepo 布局：

```text
apps/
  cli/        # 原 aemeath-cli，终端入口、TUI、REPL
  server/     # #36 API Server：REST/WS + gRPC + DB access
  agents/     # #36 Agent runtime、role config、Main/Sub-Agent features
  ui/         # 后续 Sprint 2 引入的白板 Web UI
packages/
  core/       # 原 aemeath-core，公共核心库
  llm/        # 原 aemeath-llm，LLM client 公共库
  tools/      # 原 aemeath-tools，Tool 实现公共库
  proto/      # 共享 proto 定义，替代 share/proto
  sdk/        # REST/WS/gRPC SDK，替代 share/openapi/sdk
infra/
  mongodb/
  deploy/
docs/
```

约束：`share/` 在 Sprint 0.5 后不再保留；Rust package 名为兼容现有代码可继续使用 `aemeath-core`、`aemeath-llm`、`aemeath-tools`、`aemeath-cli`。

```
                    白板 UI（Vue + Element Plus，apps/ui/）
                         │  REST / WebSocket
                         ▼
┌──────────────────────────────────────────────────────────┐
│              API Server（apps/server/）                    │
│                                                            │
│  ┌──────────────────┐   ┌──────────────────────────┐      │
│  │  REST / WS 网关   │   │   gRPC Service（Agent 接入）│      │
│  │  （前端接口）      │   │                          │      │
│  └────────┬─────────┘   └──────────┬───────────────┘      │
│           │                        │                       │
│  ┌────────┴────────────────────────┴──────────────────┐    │
│  │ Chat Svc │Workspace│ Req Svc │ Proj Svc │ Task Svc │ Board Svc │Reflection│ Agent Reg │   │
│  │          │  Svc    │         │          │          │           │   Svc    │           │   │
│  └────────────────────────────────────────────────────┘    │
│                          │                                  │
│                    MongoDB（文档存储）                       │
│                                                              │
│                    Qdrant（向量存储，RAG）                     │
└──────────────────────────────────────────────────────────┘

> **注**：MongoDB 和 Qdrant 是独立部署的外部数据服务，不运行于 API Server 进程内。API Server 通过 MongoDB Driver 和 Qdrant Client 分别连接。Qdrant 向量写入由 API Server 的异步 embedding task 完成，MongoDB 与 Qdrant 之间无直接通信。

        ▲                      ▲
        │ gRPC Watch / CRUD    │ gRPC Watch / CRUD
        │                      │
┌───────┴──────┐          ┌────┴──────────────────┐
│  Scheduler   │          │  Chat（随连绑定）       │
│  （单例）     │          │  接收用户消息            │
│              │          │  写 ChatMessage         │
│ Watch        │          │  Watch BoardSnapshot   │
│ Project      │          │  Watch Requirement     │
│ Requirement  │          │  汇报用户              │
│              │          └───────────────────────┘
└──────┬───────┘
       │
       ├── 创建/调度 Assistant Pool 执行 Requirement 分析 ──► ┌─────────────────────┐
       │                                                   │  Assistant Pool     │
       │                                                   │  后台分析/拆解/汇总   │
       │                                                   └─────────────────────┘
       │ 分派 Project (gRPC)
       ▼
┌─────────────────────┐
│  Executor Pool      │
│  #1, #2, ...        │
│  （持有 Session，   │
│   访问白板）         │
└──────────┬──────────┘
           │ 唤起 Sub-Agent（进程内调用；Sub-Agent 与 Executor 同进程，异步调用）
           ▼
     ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐
     │ Planner  │  │  Coder   │  │  Tester  │  │ Reviewer │  │ Designer │
     │ (无状态)  │  │ (无状态)  │  │ (无状态)  │  │ (无状态)  │  │ (无状态)  │
     └──────────┘  └──────────┘  └──────────┘  └──────────┘  └──────────┘

     ┌───────────────────────────────────┐
     │  Evolver（独立后台，单例）         │
     │  定期扫描白板 → 提炼模式           │
     │  生成 / 优化 Skills、MCP          │
     └───────────────────────────────────┘
             │ gRPC Watch / CRUD
             └───────────────────────────────► API Server
```


### AgentRole（动态角色标识，支持内置 + 用户自定义）

角色不分固定枚举，而是字符串标识 + 角色配置文件。内置角色按生命周期分为两组：

| Main Agent（长期运行） | Sub-Agent（按需唤起） | 调用发起方 / 实际调度关系 |
|---|---|---|
| chat | - | 连接层按用户连接创建/复用 |
| assistant | planner | Scheduler 调度 Assistant；Executor 进程内异步调用 planner |
| scheduler | coder | Scheduler 自身单例；Executor 进程内异步调用 coder |
| executor | tester | Scheduler 创建/分派 Executor；Executor 进程内异步调用 tester |
| evolver | reviewer | Evolver 自身单例；Executor 进程内异步调用 reviewer |
|  | designer | Executor 进程内异步调用 designer |

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

/// 角色配置：TOML 文件反序列化。
/// 角色配置文件应填充所有字段；`#[serde(default)]` 用于兼容用户自定义角色或旧配置缺省字段，避免缺字段导致反序列化必败。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RoleConfig {
    pub name: String,                    // 角色标识，如 "security-auditor"
    pub description: String,
    pub pool_size: usize,                 // Pool 期望实例数；0 表示无 Pool/随连绑定或单例
    pub system_prompt: String,
    pub models: Vec<RoleModelConfig>,    // 模型列表，按优先级排列
    pub permissions: RolePermissions,     // Agent runtime 工具权限；API Server 资源权限由 token scope 控制
    pub skills: Vec<String>,
    pub mcp: McpConfig,                  // 继承现有 McpConfig 定义
    // 用户自定义角色可额外扩展字段
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

fn default_max_subagents() -> usize {
    0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleModelConfig {
    pub model: String,
    pub cost_tier: CostTier,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RolePermissions {
    #[serde(default)]
    pub allowed_tools: Vec<String>,       // Agent runtime 工具白名单；不包含 board_read/board_write 等 API scope
    #[serde(default)]
    pub scope: Vec<String>,               // API Server 资源权限 scope，如 board_read/board_write/agent_registry
    #[serde(default = "default_max_subagents")]
    pub max_subagents: usize,
    #[serde(default)]
    pub can_call_roles: Vec<String>,      // 可唤起/通信的目标角色列表（含 Main Agent 和 Sub-Agent）
    #[serde(default)]
    pub can_create_agents: bool,          // 是否允许创建 Agent（仅 scheduler 为 true）
}
```


## Agent 生命周期管控

Scheduler 管理 Executor Pool 和 Assistant Pool。Chat 和 Evolver 独立管控，不与 Scheduler 耦合；Assistant 受 Scheduler 管辖。

| Agent | 模式 | 生命周期 | 管控者 |
|---|---|---|---|
| **Executor** | Pool（动态伸缩） | Scheduler 按 pending Project 数创建/销毁 | Scheduler |
| **Assistant** | 受 Scheduler 管辖（Pool） | Scheduler 按需创建/回收 | Scheduler |
| **Chat** | 随连绑定（无 Pool） | 用户连接 → 创建/复用；断连 + 超时 → 释放 | 连接层 |
| **Scheduler** | 单例 | 启动即注册，常驻 | — |
| **Evolver** | 单例 | 启动即注册，常驻后台，定时循环 | 自身 |

```
Chat —── 随连绑定（无 Pool）───
  Workspace 有用户连接 → 创建 / 复用 Chat
  用户断连 + 超时       → 释放资源

Assistant —── Pool（Scheduler Watch 驱动）───
  Requirement pending/analyzing → Scheduler 创建/调度 Assistant → 分析/草案
  Requirement 分析完成 / 空闲超时 → Scheduler 销毁

Evolver —── 单例 + 定时器 ───
  启动时注册为单例
  循环: sleep(interval) → 扫描已完成 Project → 提炼模式 → 产出 Skills/MCP → sleep(interval)

Executor —── Pool（Scheduler Watch 驱动）───
  Project pending → Scheduler 创建 Executor → 绑定分配
  Project 完成 / 空闲超时 → Scheduler 销毁
```

### 管控边界
- **Scheduler** 是唯一能做 Agent 创建/销毁动态决策的组件，管辖 Executor Pool 和 Assistant Pool
- Chat 生命周期由连接层（gRPC session）管理，Scheduler 不参与
- **Assistant** 作为后台 worker 由 Scheduler 调度，处理 Requirement 分析、Project/Task 拆解和结果汇总
- **Evolver** 自身维护定时循环，不受 Scheduler 调度
- **Sub-Agent**（planner/coder/tester/reviewer/designer）无独立生命周期：由 Executor 在进程内按需创建、异步调用，执行完毕即释放；不创建 AgentInstance 文档，不注册到 AgentRegistry

**Chat Agent 生命周期特殊说明**：Chat 创建 AgentInstance 文档（由连接层注册，非 Scheduler 分配），遵循 AgentStatus 状态机：Idle = 等待用户消息，Busy = 处理消息并写入白板。

Chat Agent 使用**双通道健康模型**（详见 State spec Chat Agent 特殊说明）：
- **WS keepalive（连接通道）**：WS 断开后在短暂容忍窗口（如 5s WS graceful close）后触发 AgentInstance doc 直接删除，跳过 HeartbeatLost→Error 路径。适用于崩溃/网络断开场景。
- **gRPC 逻辑心跳（Scheduler 通道）**：Chat Agent 通过 `AgentRegistryService.Heartbeat` RPC 定期向 `agent_heartbeats` 写入心跳。Scheduler 仅在 Chat Agent 为 **Busy** 时监控此心跳，用于检测"WS 活跃但内部卡死"场景（默认 `busy_timeout_sec=600s`）。
- **两通道互不冲突**：WS 断连走 WS keepalive 通道（即时删除），内部卡死走 gRPC 逻辑心跳通道（超时→Error）。Chat Agent 不使用 HeartbeatLost 中间状态。

### 模型健康状态流转
```
状态转移表:

| 源状态     | 事件           | 目标状态   | 说明                              |
|-----------|----------------|-----------|-----------------------------------|
| Healthy   | 1 次请求失败    | Degraded  | 首次失败，进入降级                   |
| Degraded  | 1 次请求成功    | Healthy   | 单次成功即恢复；阈值 `recovery_threshold` 默认 1，可配置为 >1 防止抖动 |
| Degraded  | 连续 2 次请求失败 | Unhealthy | 从 Degraded 起连续失败；失败计数器在进入 Degraded 时重置 |
| Unhealthy | 冷却 60s + 1 次请求成功 | Degraded | 冷却后重试成功，回到降级             |

降级路径（按 RoleModelConfig 列表优先级）:
  同 tier 逐项降级 → 下一 tier（high → medium → low）→ 全部 Unhealthy → Scheduler 告警
```


## Scheduler 设计

### 职责
- v0.1 已定稿：Scheduler 单例；多 Scheduler/主备不作为开放问题，放到 v0.2 再评估
- 管理 Executor Pool + Assistant Pool（Chat 和 Evolver 不受 Scheduler 管辖）
- 根据 pending Project 数量动态创建/销毁 Executor 实例
- **将 Project 分派给 idle 状态的 Executor**（Project 粒度分配，非 Task 粒度）
- 分配前检查目标 Project 是否已有 assigned_executor_id，防止两个 Executor 分配到同一 Project
- Scheduler 对账负责 assigned 超时检测和回退：`status=assigned && assigned_at < now - assign_timeout_sec → pending`（assign_timeout_sec 默认 60s，可配置）

### Project 独占分配机制
```
分配前提:
  - Project._id 天然保证单文档原子更新（_id 唯一）
  - AgentInstance.current_project_id 部分唯一索引用于防止多个 Executor 同时声明同一个 Project
  - 单个 Executor 同时只能处理一个 Project，由 AgentInstance 单文档字段和状态机保证

分配流程（必须在 MongoDB transaction 中执行）:
  1. 条件更新 Project:
     db.projects.updateOne(
       { _id: project_id, assigned_executor_id: { $exists: false }, status: "pending" },
       { $set: { assigned_executor_id: executor_id, status: "assigned" } }
     )
     若 matched_count=0 → Project 已被其他 Executor 抢占或状态不再 pending，事务回滚

  2. 条件更新 Executor:
     db.agent_instances.updateOne(
       { _id: executor_id, current_project_id: { $exists: false }, status: "idle" },
       { $set: { current_project_id: project_id, status: "busy" } }
     )
     若 matched_count=0 → Executor 忙或不存在，事务回滚

  3. Executor 收到分配并接受后，通过 ProjectService.Accept 将 Project status 从 assigned 改为 in_progress。

MongoDB 约束:
  - AgentInstance 部分唯一索引:
    db.agent_instances.createIndex(
      { current_project_id: 1 },
      { unique: true, partialFilterExpression: { current_project_id: { $exists: true } } }
    )
  - 不需要在 Project.assigned_executor_id 上建唯一索引（_id + 条件更新 + transaction 已保证 Project 分配互斥）
```

### 扩缩策略

Scheduler 管理 Executor Pool 和 Assistant Pool。Chat 随用户连接绑定；Assistant 由 Scheduler 调度（Pool），Evolver 为单例：

```
Executor Pool 大小 = min(max_concurrent_executor, max(min_concurrent_executor, pending_project_count))
Assistant Pool 大小 = min(max_concurrent_assistant, max(min_concurrent_assistant, pending_requirement_count))
```

默认值：`min_concurrent_executor=1`, `max_concurrent_executor=10`, `min_concurrent_assistant=1`, `max_concurrent_assistant=5`。`pool_size` 在 RoleConfig 表示期望/初始实例数，实际扩缩在 [min, max] 区间内自动调整。min/max 通过全局 Scheduler 配置段（`~/.aemeath/config.json` 中 `scheduler.pool` ）覆盖，各 Role TOML 不含这些参数。

其他可配置参数（`scheduler.*`）:
- `assign_timeout_sec`（默认 60）：Scheduler 分配 Executor 后等待其 Accept 的超时。超时后 Assigned→Pending 回退。
- `idle_timeout_sec`（默认 300）：Agent 空闲超过此时间后缩容销毁。低于此值保留实例避免频繁创建/销毁。
- `agent_init_timeout_sec`（默认 30）：Agent 注册初始化超时。超时后 AgentStatus → Error。
- `reconcile_interval_sec`（默认 5）：Scheduler 对账扫描间隔（Change Stream 事件驱动 + 定期全量对账）。每 `reconcile_interval_sec` 扫描一次，每次扫描内全量处理。
- `full_scan_rate_limit_sec`（默认 60）：全量对账最大频率限制（两次相邻全量扫描的最小间隔，防止高频对账雪崩）。
- `heartbeat_interval_sec`（默认 15）：Agent 心跳发送间隔。
- `heartbeat_timeout_sec`（默认 30）：心跳超时判定阈值（连续无心跳超过此值触发 HeartbeatLost）。
- `blocked_timeout_sec`（默认 3600，即 1 小时）：Project 最长 Blocked 时长（见 State spec Blocked 说明）。超时后自动 Blocked→Failed 并释放 merge_lock。
- `cancel_timeout_sec`（默认 60）：Cooperative Cancel 等待 Executor 确认的超时。超时后 Scheduler 按崩溃恢复处理。
- `busy_timeout_sec`（默认 600，即 10 分钟）：Chat Agent Busy 超时（见 State spec Chat 双通道健康模型）。超时后 Scheduler 标记 AgentStatus→Error。
- `token_ttl_sec`（默认 3600，即 1 小时）：Agent Token 有效期。Agent 在剩余 < 5min 时主动刷新。

配置示例（`~/.aemeath/config.json` 中 `scheduler.pool` 段）:
```jsonc
{
  "scheduler": {
    "pool": {
      "min_concurrent_executor": 1,
      "max_concurrent_executor": 10,
      "min_concurrent_assistant": 1,
      "max_concurrent_assistant": 5,
      "assign_timeout_sec": 60,
      "idle_timeout_sec": 300,
      "agent_init_timeout_sec": 30,
      "reconcile_interval_sec": 5,
      "full_scan_rate_limit_sec": 60,
      "heartbeat_interval_sec": 15,
      "heartbeat_timeout_sec": 30,
      "blocked_timeout_sec": 3600,
      "cancel_timeout_sec": 60,
      "busy_timeout_sec": 600,
      "token_ttl_sec": 3600
    }
  }
}
```

### 调度流程

Scheduler 同时 Watch Project 与 Requirement，两条链路并行处理，互不阻塞。

#### Project → Executor 调度链路
```
1. 收到 Project 变更事件（status=pending 且无 assigned_executor_id）
2. 查询空闲 Executor
3. 无空闲 Executor 且未达 max → 创建新 Executor
3a. 无空闲 Executor 且已达 max → Project 维持 pending，Scheduler 下次对账周期重试；pending_project_count 持续计入池扩容需求（若扩容后仍有空闲 worker slot 则在下一周期创建）
4. 在 MongoDB transaction 中条件绑定 Project → Executor
5. 更新 Project status → assigned
6. 更新 Executor status → busy
7. Executor 接受分配后调用 ProjectService.Accept，Project status → in_progress
```

#### Requirement → Assistant 分析链路
```
1. 收到 Requirement 变更事件（status=pending）
2. 查询空闲 Assistant
3. 无空闲 Assistant 且未达 max → 创建新 Assistant
3a. 无空闲 Assistant 且已达 max → Requirement 维持 pending，Scheduler 下次对账周期重试
4. 分配 Requirement 分析任务给 Assistant
5. Assistant 深度分析 Requirement，生成 Project/Task 草案
6. 将分析结果写回 Requirement.draft，并更新 Requirement status
```


## Evolver 设计（含 RAG）

Evolver 是对系统元认知的出口——它观察已完成的工作，提炼可复用模式，产出新的 Skills 和 MCP 配置，驱动系统自我进化。

### Embedding 写入时机

API Server 在写入以下文档时异步生成 embedding，并将向量与检索 payload 写入 Qdrant。MongoDB 只保存业务文档和 `embedding_ref`（Qdrant point id / collection 名），不承担向量检索职责。

| 文档 | embedding 内容 | 触发时机 |
|---|---|---|
| ChatMessage | 用户消息 / Agent 回复的可检索摘要（仅 message_type=requirement；普通消息不向量化，embedding_status 为 not_applicable） | ChatMessage 写入时（message_type=requirement） |
| Requirement | 需求文本提炼（LLM 将长文本压缩为 512 token 摘要后向量化） | Requirement 写入时 |
| Project（状态=completed） | project name + 生成的 summary + 关键决策描述 | Project 完成时 |
| ProjectTask（状态=completed） | task name + Executor 产出摘要 + 遇到的坑/解法 | Task 完成时 |
| Reflection | 模式总结 + Skill/MCP 产出描述 | Reflection 写入时 |

### 反思流程
```
Evolver 定时循环（interval 可配，默认 24h）:

1. 扫描近期（~7d）已完成但未反思的 Project
2. 对每个 Project 做 embedding 检索:
   - 用"有哪些之前做过的类似项目？"查询 Project embedding
   - 用"有哪些反复出现的问题？"查询 ProjectTask embedding
   - 用"用户最近关注什么方向？"查询 Requirement embedding
3. LLM 综合检索到的上下文，产出:
   a. 模式总结（如"XXX 类需求推荐用 YYY 技术方案"）
   b. Skill 生成/优化（如自动生成 react-form 模板 Skill）
   c. MCP 建议（如检测到重复 API 调用 → 建议配置对应 MCP server）
4. 反思结果写入白板（reflections collection）
5. 标记 Project 为已反思
```

**Evolver ↔ API Server gRPC 交互**：Evolver 通过以下 gRPC Service 与 API Server 通信：

| Service | RPC | 用途 |
|---------|-----|------|
| `ProjectService` | `Watch` / `List` / `Update` | 扫描已完成且未反思的 Project；反思完成后标记 `reflected_at` |
| `ReflectionService` | `Create` / `List` / `Get` | 写入反思结果（含 embedding_ref）；查询已有 Reflection 去重 |
| `RequirementService` | `Watch` / `List` | 查询近期 Requirement 趋势 |
| `ProjectTaskService` | `Watch` | 获取已完成 Task 的执行摘要 |

### Qdrant RAG 存储边界

RAG 不属于 P0 的硬依赖。向量数据统一存入 Qdrant，MongoDB 仅保存业务数据和向量引用：
- Qdrant collections：`chat_messages`、`requirements`、`projects`、`project_tasks`、`reflections`（同 shape，但不同 collection 可配置不同向量维度和 payload index；`reflections` 对应 reflections schema 中 `embedding_ref.collection = "reflections"`）。
- Reflections 写入时异步写入 `reflections` collection，不覆盖 `chat_messages` embedding；若反思引用了聊天内容，仅在 payload 中保存引用的 `chat_message_ids`。
- Point id：使用对应 MongoDB 文档 `_id` 的字符串形式，便于回查业务文档。
- Payload：保存 `workspace_id`、`document_type`、`source_id`、`status`、`created_at`、`updated_at` 等过滤字段。
- 本地开发：通过 Docker 启动 Qdrant；未配置 Qdrant 时禁用 RAG，Evolver 跳过相似检索步骤，只做规则化总结。
- 一致性：MongoDB 写入成功后异步写 Qdrant；Qdrant 写失败不回滚业务写入，记录 `embedding_status=failed` 并由后台 worker 重试。

### Qdrant Collection 配置
```jsonc
{
  "collection_name": "project_tasks",
  "vectors": {
    "size": 1536,
    "distance": "Cosine"
  },
  "payload_indexes": [
    "workspace_id",
    "document_type",
    "status",
    "updated_at"
  ]
}
```

同样的 collection shape 也适用于 `chat_messages`、`requirements`、`projects`、`project_tasks`、`reflections`。


## Agent 实现：模板 + 装配器

### 通用模板（`agents/src/template.rs`）
Agent 运行时核心：
- LLM 对话循环（当前 agent_runner 提取/抽象）
- 工具调用执行
- 上下文管理（压缩、token 估算）
- 结果汇总

### 装配器（`agents/src/assembler.rs`）
根据角色配置组装 Agent：
```
assembler.assemble(role: RoleConfig) -> ConfiguredAgent {
    system_prompt: role.system_prompt,
    skills: role.skills,
    mcp_servers: role.mcp.servers,
    can_call_roles: role.permissions.can_call_roles,
    model_selector: ModelSelector::from(role.models),
    tools: role.permissions.allowed_tools,
}
```

### 角色配置（`agents/roles/`）

Main Agent 和 Sub-Agent 均通过 `RoleConfig` + 装配器创建。每个角色使用独立 TOML 配置文件；Main Agent 包括 `chat.toml`、`assistant.toml`、`evolver.toml`、`scheduler.toml`、`executor.toml`，Sub-Agent 包括 `planner.toml`、`coder.toml`、`tester.toml`、`reviewer.toml`、`designer.toml`。所有 TOML 示例采用顶层扁平格式：`name`、`description`、`pool_size`、`system_prompt`、`skills`、`mcp` 均位于顶层，模型使用 `[[models]]`，权限使用 `[permissions]`，不使用 `[role]` 嵌套段。

```toml
# chat.toml（面向用户的对话 Agent）
name = "chat"
description = "面向用户的对话 Agent"
pool_size = 0               # 随连绑定，无 Pool
system_prompt = "你是面向用户的 Chat Agent，负责理解用户消息、写入白板并汇报状态。"
skills = ["conversation", "requirement-triage"]
mcp = { servers = [] }

[[models]]
model = "anthropic/claude-sonnet-4-20250514"
cost_tier = "high"

[permissions]
allowed_tools = ["read", "write", "web_search", "web_fetch"]
scope = ["board_read", "board_write"]
can_call_roles = []
max_subagents = 0              # Chat 不唤起 Sub-Agent
can_create_agents = false
```

```toml
# assistant.toml（后台需求分析/草案 Worker）
name = "assistant"
description = "后台需求分析/草案 Worker"
pool_size = 3               # Scheduler 管理 Pool
system_prompt = "你是 Assistant Agent，负责深度分析 Requirement，拆解 Project/Task 草案并写回白板。"
skills = ["requirement-analysis", "task-breakdown"]
mcp = { servers = [] }

[[models]]
model = "deepseek/deepseek-chat"
cost_tier = "low"

[permissions]
allowed_tools = ["read", "write", "grep", "glob"]
scope = ["board_read", "board_write", "agent_registry"]
can_call_roles = []
max_subagents = 0
can_create_agents = false
```

```toml
# evolver.toml
name = "evolver"
description = "定期扫描白板，提炼模式，生成/优化 Skills 和 MCP"
pool_size = 0               # 单例，无 Pool
system_prompt = "你是 Evolver Agent，负责从已完成项目中提炼可复用模式，生成或优化 Skills 和 MCP 配置建议。"
skills = ["analysis", "summarization"]
mcp = { servers = [] }

[[models]]
model = "deepseek/deepseek-chat"
cost_tier = "low"

[permissions]
allowed_tools = ["web_search"]
scope = ["board_read", "board_write"]     # Evolver 不含 agent_registry — 启动时自身注册，注册后不调 AgentRegistryService 其他 RPC
max_subagents = 0
can_call_roles = []
can_create_agents = false
```
  
```toml
# scheduler.toml
name = "scheduler"
description = "管理 Agent Pool 生命周期，分派任务"
pool_size = 0               # 单例，无 Pool
system_prompt = "你是 Scheduler Agent，负责 Watch Project/Requirement，管理 Assistant/Executor Pool 并执行调度。"
skills = []
mcp = { servers = [] }

[[models]]
model = "deepseek/deepseek-chat"
cost_tier = "low"

[permissions]
allowed_tools = []
scope = ["agent_registry", "board_read", "board_write"]
can_create_agents = true
can_call_roles = ["assistant", "executor"]
max_subagents = 0
```

```toml
# executor.toml
name = "executor"
description = "领取 Project，编排 Sub-Agent 执行 Tasks，写回白板"
pool_size = 3               # Scheduler 管理 Pool，可按需扩缩
system_prompt = "你是 Executor Agent，负责领取 Project、编排 Sub-Agent 执行 Task，并将结果写回白板。"
skills = ["task-management"]
mcp = { servers = [] }

[[models]]
model = "anthropic/claude-sonnet-4-20250514"
cost_tier = "high"

[[models]]
model = "openai/gpt-5"
cost_tier = "high"

[[models]]
model = "deepseek/deepseek-v4-pro"
cost_tier = "medium"

[permissions]
allowed_tools = ["agent_call"]
# agent_call 是 allowed_tools（runtime 工具），不属于 scope
scope = ["agent_registry", "board_read", "board_write"]
max_subagents = 5
can_call_roles = ["planner", "coder", "tester", "reviewer", "designer"]
can_create_agents = false
```

## RBAC Scope 定义

三类 scope，经由中间件校验：

| scope | 说明 | 允许的调用方 |
|-------|------|-------------|
| `board_read` | 读取白板（Chat、BoardSnapshot、Agent 列表） | UI 客户端、Chat Agent、Scheduler |
| `board_write` | 写入白板（创建/变更 Project、Task、Requirement） | UI 客户端、Chat Agent、Assistant、Executor |
| `agent_registry` | Agent 注册/心跳/注销 | Scheduler、Executor、Assistant、Chat Agent（连接层内部调用） |

TokenScope 枚举（`share/proto/common.proto`）：
```protobuf
enum TokenScope {
  SCOPE_UNSPECIFIED = 0;
  SCOPE_BOARD_READ = 1;
  SCOPE_BOARD_WRITE = 2;
  SCOPE_AGENT_REGISTRY = 3;
}
```

**Chat Agent 注册例外**：Chat 的连接层注册绕过 Scheduler-only 约束，由服务端内部完成 Register（无外部 API）。

### Token 生命周期

1. **签发**：`AgentRegistryService.Register` 成功后，API Server 签发 JWT（HS256，密钥服务端持有）。Payload：`{ agent_id, role, workspace_id, scope[], aud, iss, iat, exp }`。默认有效期 1h（可配 `token_ttl_sec`）。
2. **传递**：gRPC metadata key `authorization`，值格式 `Bearer <jwt>`（与 REST 统一）。
3. **刷新**：调用方为 Agent 自身——在剩余有效期 < 5min 时主动调用 `RefreshToken` RPC。服务端用 metadata 中旧 token 鉴权后签发新 token。
4. **过期**：gRPC 拦截器拒绝过期 token → `UNAUTHENTICATED`。Agent 心跳使用 token，过期则心跳失败 → 心跳超时 → HeartbeatLost。Scheduler 通过心跳超时间接感知 token 过期。
5. **Scope 更新**：v0.1 scope 在签发时确定，不支持热更新。角色变更需 Deregister + 重新 Register。
