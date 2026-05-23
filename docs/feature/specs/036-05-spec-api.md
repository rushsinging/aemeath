# #36 多 Agent 框架 — Spec / API & 项目结构

> **DDD 设计参考**：[Multi-Agent 框架 DDD 设计](../../superpowers/specs/2026-05-20-multi-agent-ddd-design.md) — API 端点归属遵循 Bounded Context；跨进程协作通过 Outbox + Redis Streams，不提供 Agent RPC / Watch 接口。

## API 边界

#36 的外部接口分为三类：

1. **REST Command / Query**：前端、CLI、调试工具访问 API Server。
2. **WebSocket BoardEvent Gateway**：API Server 从 Redis BoardEvent stream 读取事件并推送给 UI。
3. **Redis Streams 消息契约**：Agent Runtime 消费 WorkQueue / ControlSignal，OutboxPublisher 发布 IntegrationEvent / BoardEvent。

明确取消：

- 不提供 Agent RPC 调度接口。
- 不提供 Watch RPC / server streaming 接口。
- 不依赖 MongoDB Change Stream 作为跨进程通知机制。
- API Server 不点名调用某个 Agent。

## REST / WebSocket API 设计（前端接口）

Server 通过 REST + WebSocket 为前端白板提供数据。REST 返回当前资源状态；WebSocket 只推送 Redis-backed BoardEvent。

OpenAPI / SDK 约束：REST 接口 schema MUST 由 Rust server 代码导出，当前采用 `aide + schemars`；SDK MUST 由导出的 OpenAPI 自动生成，NEVER 手写业务 SDK client。所有新增 REST 接口 MUST 使用可导出的 API router/DTO 写法，并纳入 OpenAPI/SDK 一致性 stop hook。

### 认证

所有 REST/WS 端点携带 Bearer JWT token（`Authorization: Bearer <jwt>`）；WS 可在连接 query parameter 中传递 `?token=<jwt>` 或使用升级前认证上下文。REST/WS 中间件校验 token 的 `scope` 字段（如 `board_read`/`board_write`）。RBAC scope 完整定义见 `036-02-spec-architecture.md`。

Agent Runtime 不通过 Server RPC 调度。Agent 访问 MongoDB / Redis / Application API 的凭据由部署配置或本地配置注入，权限仍按 role scope 校验。

### REST 端点

```text
POST   /api/workspaces                                      # 创建 Workspace
GET    /api/workspaces                                      # Workspace 列表
DELETE /api/workspaces                                      # 批量删除 Workspace
GET    /api/workspaces/:ws_id                               # Workspace 详情
PATCH  /api/workspaces/:ws_id                               # 更新 Workspace
DELETE /api/workspaces/:ws_id                               # 删除单个 Workspace
GET    /api/workspaces/:ws_id/board                         # 白板聚合 snapshot

POST   /api/workspaces/:ws_id/chats                         # 创建 Conversation/Chat
GET    /api/workspaces/:ws_id/chats                         # Chat 列表
DELETE /api/workspaces/:ws_id/chats                         # 批量删除 Chat
GET    /api/workspaces/:ws_id/chats/:chat_id                # Chat 详情
PATCH  /api/workspaces/:ws_id/chats/:chat_id                # 更新 Chat title/status
DELETE /api/workspaces/:ws_id/chats/:chat_id                # 删除单个 Chat
GET    /api/workspaces/:ws_id/chats/:chat_id/messages       # Chat 消息分页
POST   /api/workspaces/:ws_id/chats/:chat_id/messages       # 创建 ChatMessage；写 Outbox(MessageAppended)
POST   /api/workspaces/:ws_id/chats/:chat_id/analyze        # 调试/前端辅助：同步预判 message_type，不替代异步 Agent 分析

GET    /api/workspaces/:ws_id/requirements                  # Requirement 列表
POST   /api/workspaces/:ws_id/requirements                  # 创建 Requirement；写 Outbox(RequirementCreated)
GET    /api/workspaces/:ws_id/requirements/:requirement_id  # Requirement 详情
PATCH  /api/workspaces/:ws_id/requirements/:requirement_id  # 更新 Requirement 草案/状态/关联
POST   /api/workspaces/:ws_id/requirements/:requirement_id/confirm  # 确认草案并创建 Project/Task
POST   /api/workspaces/:ws_id/requirements/:requirement_id/cancel   # 软取消 Requirement
POST   /api/workspaces/:ws_id/requirements/:requirement_id/reject   # 拒绝 Requirement
POST   /api/workspaces/:ws_id/requirements/:requirement_id/resubmit # 重新提交

GET    /api/workspaces/:ws_id/projects                      # Project 列表
GET    /api/workspaces/:ws_id/projects/:project_id          # Project 详情
GET    /api/workspaces/:ws_id/projects/:project_id/tasks    # Project 下 Task 列表
POST   /api/workspaces/:ws_id/projects/:project_id/resume   # 恢复 Blocked Project
POST   /api/workspaces/:ws_id/projects/:project_id/cancel   # 取消 Project；写 CancelRequested Outbox
POST   /api/workspaces/:ws_id/projects/:project_id/retry    # 重开 Project
GET    /api/workspaces/:ws_id/projects/:project_id/tasks/:task_id       # Task 详情
PATCH  /api/workspaces/:ws_id/projects/:project_id/tasks/:task_id       # 更新 Task 信息
POST   /api/workspaces/:ws_id/projects/:project_id/tasks/:task_id/cancel# 取消 Task
POST   /api/workspaces/:ws_id/projects/:project_id/tasks/:task_id/retry # 重开 Task

GET    /api/workspaces/:ws_id/agents                        # AgentInstance 列表（MongoDB 摘要 + Redis presence 派生状态）
GET    /api/workspaces/:ws_id/work-items                    # 调试/运维：WorkItem 列表
GET    /api/workspaces/:ws_id/agent-runs                    # 调试/运维：AgentRun 审计列表
```

说明：

- DELETE 批量删除端点的 IDs 通过 query params 传递。
- Requirement 和 Project 不暴露 REST DELETE；使用 cancel 进行软取消。
- 本文中 `Conversation` 是 DDD 语义名；API 路径和当前 Rust 类型 MAY 继续沿用 `Chat` / `ChatMessage`，二者在 #36 v0.1 中等价。
- REST command 写入 MongoDB 聚合状态与 OutboxEvent 后即可返回 `accepted` 或当前资源状态；Agent 执行结果异步到达。

## WebSocket BoardEvent Gateway

```text
WS /ws/workspaces/:ws_id/board?last_stream_id=<redis_stream_id>
```

连接行为：

1. Server 校验 token 与 `board_read` scope。
2. 若携带 `last_stream_id`，Server 从 Redis BoardEvent stream 尝试补发 missed events。
3. 若未携带 cursor、cursor 过旧、stream 已裁剪或检测到 gap，Server 发送 `snapshot_required`。
4. Client 收到 `snapshot_required` 后调用 `GET /api/workspaces/:ws_id/board` 拉取当前 snapshot，再继续接收增量 BoardEvent。
5. 每个 API Server 副本只维护自己连接上的 WebSocket client；跨副本同步依赖 Redis stream，不依赖进程内 EventBus。

### WS 消息类型

```jsonc
// 心跳
{
  "type": "heartbeat",
  "stream_id": "1700000000000-0",
  "timestamp": 1700000000000
}

// 需要客户端重拉 snapshot
{
  "type": "snapshot_required",
  "reason": "cursor_too_old | gap_detected | initial_connect",
  "latest_stream_id": "1700000000000-0"
}

// 增量 BoardEvent
{
  "type": "board_event",
  "stream_id": "1700000000001-0",
  "payload": {
    "workspace_id": "...",
    "event_type": "updated",
    "snapshot_id": "...",
    "timestamp": 1700000000001,
    "changed_requirements": [],
    "changed_projects": [],
    "changed_tasks": [],
    "new_messages": [],
    "changed_agents": []
  }
}

// 错误
{
  "type": "error",
  "code": "UNAUTHORIZED | RATE_LIMITED | INTERNAL",
  "message": "..."
}
```

## Board 聚合响应结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSnapshot {
    pub snapshot_id: String,
    pub workspace_id: ObjectId,
    pub workspace: WorkspaceInfo,
    pub chats: Vec<Chat>,
    pub recent_messages: Vec<ChatMessage>,
    pub requirements: Vec<Requirement>,
    pub projects: Vec<ProjectWithTasks>,
    pub agent_instances: Vec<AgentInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardEvent {
    pub stream_id: Option<String>,
    pub workspace_id: ObjectId,
    pub event_type: String,
    pub snapshot_id: Option<String>,
    pub timestamp: i64,
    pub changed_workspace: Option<WorkspaceInfo>,
    pub changed_requirements: Vec<Requirement>,
    pub removed_requirement_ids: Vec<ObjectId>,
    pub changed_projects: Vec<ProjectWithTasks>,
    pub changed_tasks: Vec<ProjectTask>,
    pub removed_project_ids: Vec<ObjectId>,
    pub removed_task_ids: Vec<ObjectId>,
    pub changed_chats: Vec<Chat>,
    pub removed_chat_ids: Vec<ObjectId>,
    pub new_messages: Vec<ChatMessage>,
    pub updated_messages: Vec<ChatMessage>,
    pub changed_agents: Vec<AgentInstance>,
    pub removed_agent_ids: Vec<ObjectId>,
}
```

BoardEvent 是 UI projection，不是 DomainEvent。客户端不得把 BoardEvent 当作业务命令确认；业务状态以 REST query 返回的 MongoDB snapshot 为准。

## Redis Streams 契约

### Stream 命名

```text
aemeath:{tenant_id}:integration
aemeath:{tenant_id}:board:{workspace_id}
aemeath:{tenant_id}:work:{agent_type}
aemeath:{tenant_id}:control:{controller_type}
aemeath:{tenant_id}:dead-letter:{agent_type}
```

### IntegrationEvent message

```jsonc
{
  "schema_version": "1",
  "event_id": "outbox_event_object_id",
  "workspace_id": "...",
  "event_type": "TaskReadyForExecution",
  "aggregate_type": "Project",
  "aggregate_id": "...",
  "payload_ref": {
    "collection": "outbox_events",
    "id": "..."
  },
  "idempotency_key": "...",
  "created_at": 1700000000000
}
```

### WorkQueue message

```jsonc
{
  "schema_version": "1",
  "work_item_id": "...",
  "workspace_id": "...",
  "required_agent_type": "executor",
  "kind": "execute_project",
  "payload_ref": {
    "collection": "work_items",
    "id": "..."
  },
  "idempotency_key": "...",
  "created_at": 1700000000000
}
```

约束：

- Redis message 只放路由字段与引用，完整 payload 存 MongoDB。
- Agent 消费 message 后 MUST 重新加载 MongoDB WorkItem 并执行原子 claim。
- Redis `stream_id` 是传输 cursor，不是领域版本。
- 消费者 MUST 用 `idempotency_key` 和 MongoDB 状态去重。

### BoardEvent message

```jsonc
{
  "schema_version": "1",
  "workspace_id": "...",
  "event_type": "updated",
  "snapshot_id": "...",
  "payload_ref": {
    "collection": "board_events",
    "id": "..."
  },
  "created_at": 1700000000000
}
```

BoardEvent payload MAY 直接放在 Redis 中，也 MAY 只放 `payload_ref` 指向 MongoDB projection/event 记录。若事件体较大，SHOULD 使用引用以避免 Redis stream 膨胀。

### ControlSignal message

```jsonc
{
  "schema_version": "1",
  "workspace_id": "...",
  "signal_type": "cancel_work_item | drain_agent | reconcile_workspace",
  "target_agent_id": "optional",
  "work_item_id": "optional",
  "reason": "...",
  "created_at": 1700000000000
}
```

ControlSignal 是加速通知，真实状态仍在 MongoDB。Agent 收到 cancel signal 后 MUST 读取 MongoDB cancel_requested_at / WorkItem status 再执行副作用。

## Agent Runtime 消费协议

### WorkQueue consumer group

```text
stream: aemeath:{tenant_id}:work:{agent_type}
group: agents:{agent_type}
consumer: {agent_instance_id}
```

消费流程：

```text
1. XREADGROUP 读取 WorkQueue
2. 根据 work_item_id 加载 MongoDB WorkItem
3. 条件更新 WorkItem: Pending/expired → Leased
4. 创建 AgentRun，WorkItem → Running
5. 执行 LLM/tool/Sub-Agent
6. 通过 Application Service 写业务结果和 OutboxEvent
7. WorkItem → Succeeded / Failed / Cancelled
8. XACK Redis message
```

失败恢复：

- Agent 崩溃后 Redis pending message 由同组 consumer `XAUTOCLAIM`。
- 接管者 MUST 重新校验 MongoDB lease，不得仅凭 pending message 执行。
- WorkItem 终态后重复 message 直接 XACK。
- 重试耗尽后写 dead-letter stream，并将 WorkItem 标记 Failed。

## OutboxPublisher 协议

OutboxPublisher 可运行在 API Server 或独立 worker 中，支持多实例部署。

流程：

```text
1. MongoDB 原子 claim outbox_events(status=pending)
2. 根据 domain_event_type 转换为 IntegrationEvent / BoardEvent / WorkQueue message
3. Redis XADD 到目标 stream
4. 回写 published_stream / published_stream_id / status=published
5. 失败则 publish_attempt+1，未耗尽时回 pending，耗尽后 failed
```

发布可能重复，消费者必须幂等。

## 项目结构

```text
apps/
  server/
    src/
      interfaces/
        rest/
        websocket/
      application/
      infrastructure/
        mongo/
        redis/
        outbox/
  agents/
    src/
      runtime/
        worker.rs
        heartbeat.rs
        consumer.rs
      application/
      infrastructure/
        mongo/
        redis/
        llm/
  ui/
packages/
  core/
    src/
      domain/
      events/
      ids/
      error/
  llm/
  tools/
  sdk/        # REST/WS SDK，OpenAPI 自动生成
infra/
  mongodb/
  redis/
```

## SDK 约束

- SDK MUST 从 REST OpenAPI 自动生成。
- SDK MAY 提供 WebSocket helper，负责 `last_stream_id` 保存、自动重连、`snapshot_required` 回调。
- SDK MUST NOT 暴露 Agent RPC client。
- SDK MUST NOT 把 Redis Streams 暴露给浏览器端；Redis 只由 server/agent/worker 后端组件访问。
