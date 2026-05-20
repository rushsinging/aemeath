# Feature #36 MVP 实现计划

## 目标

最小可用实现：用户能通过 Web UI 创建 Workspace、发送聊天消息，前端通过 WebSocket
实时接收 BoardSnapshot 增量推送。MongoDB 持久化所有数据，幂等去重保证可靠投递。

## 产出范围（缩小版 Sprint 0 + Sprint 1）

| # | 模块 | 产出 | 对应 Spec |
|---|------|------|-----------|
| 1 | proto | 4 个 .proto 文件 | 036-05§gRPC |
| 2 | server 骨架 | cargo init + axum+tonic 共享端口 50051 | 036-02§技术栈 |
| 3 | MongoDB | 连接池 + 5 个 collection 初始化脚本 | 036-03§Data |
| 4 | Workspace | REST + gRPC CRUD | 036-05§WorkspaceService |
| 5 | Chat + ChatMessage | REST + gRPC CRUD + idempotency_key 去重 | 036-05§ChatService |
| 6 | WebSocket | BoardSnapshot 全量 + 增量推送 | 036-05§WebSocket / Board |
| 7 | 幂等基础设施 | idempotency_records 表 + TTL 索引 | 036-03§idempotency_records |

**不产出**（延后到后续 Sprint）：
- Agent 注册 / 心跳 / Token（Sprint 3）
- Requirement / Project / Task / Reflection（Sprint 2/4/5）
- RBAC 中间件（Sprint 3）
- Scheduler / Executor / Evolver（Sprint 4/5）
- Qdrant 向量存储（Sprint 5）
- Full scan / Reconcile（Sprint 5）

## 分步执行

### Step 1 — Proto 文件定义

新建 `proto/` 目录（与 Cargo workspace 平级），编写 4 个 .proto 文件：

| 文件 | 内容 | 关键 message |
|------|------|-------------|
| `proto/common.proto` | EventType 枚举、Empty、Workspace 基础类型 | EventType, Empty, Workspace |
| `proto/workspace.proto` | WorkspaceService CRUD | CreateWorkspaceRequest/Response, Get/List/Update/Delete |
| `proto/chat.proto` | ChatService + Chat/ChatMessage proto | Chat, ChatMessage, TokenUsage, ToolCall, ToolResult, SendMessageRequest/Response, ChatEvent |
| `proto/board.proto` | BoardService + BoardSnapshot/BoardSnapshotUpdate | BoardSnapshot, BoardSnapshotUpdate, WorkspaceInfo, ChatCard, BoardEvent |

Cargo workspace 新增成员 `aemeath-server/`，通过 `tonic-build` 在 build.rs 中编译 proto。

### Step 2 — Server 骨架

新建 `aemeath-server/` crate：

```
aemeath-server/
├── Cargo.toml       # tonic + axum + mongodb + tower + prost + tokio + tracing
├── build.rs         # tonic_build::configure() 编译 proto/*.proto
├── src/
│   ├── main.rs      # tokio::main: 启动 axum router + tonic server（共享端口 50051）
│   ├── db.rs        # MongoDB 连接池初始化、collection handle
│   ├── proto.rs     # tonic::include_proto!("...") 统一导出
│   ├── svc/         # gRPC Service 实现
│   │   ├── mod.rs
│   │   ├── workspace.rs   # WorkspaceService
│   │   ├── chat.rs        # ChatService
│   │   └── board.rs       # BoardService
│   └── rest/        # axum REST 路由
│       ├── mod.rs
│       ├── ws.rs           # WebSocket handler（BoardSnapshot 推送）
│       ├── workspace.rs    # REST Workspace CRUD
│       └── chat.rs         # REST Chat/Message CRUD
├── migrations/
│   └── init.js             # MongoDB collection + index 初始化脚本
├── docker-compose.yml      # MongoDB 5.0+ replica set
└── .env.example
```

关键依赖：
```toml
[dependencies]
tonic = "0.12"
prost = "0.13"
tonic-build = "0.12"
axum = { version = "0.8", features = ["ws"] }
tower = "0.5"
mongodb = "3.2"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
uuid = { version = "1", features = ["v7"] }    # ULID 格式 snapshot_id
```

main.rs 核心逻辑：
1. 读取环境变量 `MONGO_URI`（默认 `mongodb://localhost:27017`）→ 连接 MongoDB
2. 初始化 collection handle（workspaces, chats, chat_messages, idempotency_records, board_index）
3. 构建 axum `Router`（REST + WS 路由）
4. 构建 tonic `Router`（gRPC Service 注册）
5. 共享 `TcpListener::bind("0.0.0.0:50051")`，通过 accept header 分流

### Step 3 — MongoDB 连接与初始化

`db.rs`：
- `MongoClient` 连接池（默认 10 个连接）
- 返回 `Database` handle
- 导出 struct `AppState { db: Database }`（axum State + tonic service 共享）

`init.js`（Mongo Shell 脚本）：
```js
// 5 个 collection
db.getSiblingDB("aemeath");
db.createCollection("workspaces");
db.createCollection("chats");
db.createCollection("chat_messages");
db.createCollection("idempotency_records");
db.createCollection("board_index");  // snapshot_id 计数器

// 索引
db.workspaces.createIndex({ "created_at": -1 });
db.chats.createIndex({ "workspace_id": 1 });
db.chat_messages.createIndex({ "chat_id": 1, "created_at": -1 });
db.idempotency_records.createIndex({ "created_at": 1 }, { expireAfterSeconds: 3600 });
db.idempotency_records.createIndex({ "key": 1, "entity_type": 1, "scope": 1 }, { unique: true });
db.board_index.createIndex({ "snapshot_id": 1 }, { unique: true });
```

### Step 4 — Workspace CRUD

**gRPC**（`svc/workspace.rs`）：
- `CreateWorkspace`：写入 MongoDB `workspaces` collection，返回 Workspace
- `GetWorkspace`：按 id 查询
- `ListWorkspaces`：分页列表（默认 page_size=20）
- `UpdateWorkspace`：部分更新（name / provider / model）
- `DeleteWorkspace`：级联删除 chats / chat_messages

**REST**（`rest/workspace.rs`）：
- `POST /api/workspaces`
- `GET /api/workspaces/:id`
- `GET /api/workspaces`
- `PATCH /api/workspaces/:id`
- `DELETE /api/workspaces/:id`

Workspace 文档结构（对应 036-03）：
```json
{
  "_id": ObjectId,
  "name": "My Workspace",
  "provider": "anthropic",
  "model": "claude-sonnet-4-20250514",
  "description": "",
  "created_at": ISODate,
  "updated_at": ISODate
}
```

### Step 5 — Chat + ChatMessage CRUD

**gRPC**（`svc/chat.rs`）：
- `CreateChat`：创建会话，关联 workspace_id
- `SendMessage`：写入 chat_messages，含 idempotency_key 去重检查
- `ListMessages`：按 chat_id 分页查询
- `WatchChat`：ServerStreaming ChatEvent（后端用，MVP 仅定义 proto）

**REST**（`rest/chat.rs`）：
- `POST /api/workspaces/:workspace_id/chats`
- `GET /api/workspaces/:workspace_id/chats`
- `POST /api/chats/:chat_id/messages`（payload 含 idempotency_key）
- `GET /api/chats/:chat_id/messages`

**去重逻辑**（幂等写）：
1. 客户端生成 ULID 作为 idempotency_key
2. 服务端 `insert_one` 时唯一索引冲突 → 直接返回已有文档（不报错）
3. `idempotency_records` collection TTL 1 小时自动清理

Chat 文档结构：
```json
{
  "_id": ObjectId,
  "workspace_id": ObjectId,
  "title": "新年贺卡设计",
  "created_at": ISODate,
  "updated_at": ISODate,
  "agent_instance_id": null  // Sprint 3 填充
}
```

ChatMessage 文档结构：
```json
{
  "_id": ObjectId,
  "chat_id": ObjectId,
  "workspace_id": ObjectId,
  "sender_type": "user",
  "sender_id": ObjectId,
  "role": "user",
  "content": "你好",
  "tool_calls": [],
  "tool_results": [],
  "idempotency_key": "01AR...",
  "token_usage": null,
  "created_at": ISODate,
  "version": 1
}
```

### Step 6 — WebSocket BoardSnapshot 推送

`rest/ws.rs`：
- 端点：`GET /ws/{workspace_id}`
- 连接时：查询 MongoDB 组装全量 BoardSnapshot（is_full_snapshot=true），channel 设为 Snapshot variant
- 后续：Watch `chats` / `chat_messages` collection 的 ChangeStream → 组装 BoardSnapshotUpdate → channel 推送
- 断开时：不丢弃任何事件，由前端 snack 队列缓存（30s 超时）重连后续传
- Heartbeat：每 30s 发送 `{"type": "Heartbeat", "snapshot_id": "...."}`

BoardSnapshot 结构（对齐 036-05 L71-82）：
```rust
struct BoardSnapshot {
    snapshot_id: String,
    workspace_id: ObjectId,
    workspace: WorkspaceInfo,
    chats: Vec<Chat>,
    recent_messages: Vec<ChatMessage>,
    requirements: Vec<Requirement>,      // MVP 为空
    projects: Vec<ProjectWithTasks>,     // MVP 为空
    agent_instances: Vec<AgentInstance>, // MVP 为空
}
```

snapshot_id 生成算法：查询 `board_index` collection 自增 ULID（或直接 `uuid::Uuid::now_v7()`）。

增量推送策略：
- MongoDB ChangeStream 监听 `chats` / `chat_messages` 的 insert/update/delete
- 组装为 BoardSnapshotUpdate（changed_chats / new_messages / updated_messages / removed_chat_ids）
- `is_full_snapshot=false`，只包含变更字段
- removed 事件仅送 id 列表

### Step 7 — 幂等基础设施

`idempotency_records` collection 用于：
- ChatMessage 发送去重（当前 Sprint 直接使用）
- 后续 Sprint 2-5 的 Project/ProjectTask 分配、状态变更去重

TTL 索引 1 小时自动过期，防止记录堆积。

唯一复合索引：`{ key, entity_type, scope }` 保证原子去重。

## 测试策略

每个 Step 完成后编写对应测试：

| Step | 测试类型 | 覆盖点 |
|------|---------|--------|
| 1 | build.rs 编译检查 | proto 编译通过 |
| 2 | 集成测试 | server 启动、健康检查 |
| 3 | 集成测试 | MongoDB 连接、CRUD |
| 4 | 集成测试 | Workspace CRUD 全路径 |
| 5 | 集成测试 | Chat + Message CRUD + 幂等 |
| 6 | 集成测试 | WS 连接、全量快照、增量推送 |
| 7 | 单元测试 | 唯一索引去重、TTL 过期 |

## 验证门禁

- `cargo build -p aemeath-server` — 编译通过
- `cargo test -p aemeath-server` — 全部测试通过
- `docker-compose up -d && sleep 3 && cargo run -p aemeath-server` — 服务启动
- `curl http://localhost:50051/api/workspaces` — REST 健康检查
- `websocat ws://localhost:50051/ws/test-workspace` — WS 连接验证

## 风险

| 风险 | 缓解 |
|------|------|
| MongoDB ChangeStream 需要 replica set | docker-compose 配置单节点 replica set（`rs.initiate()`） |
| tonic + axum 共用端口 TcpListener 类型不匹配 | 使用 `tokio::net::TcpListener` + `tower::make::Shared` 叠加两个 service |
| proto 字段与 MongoDB BSON ObjectId 转换 | 自定义 Serialize/Deserialize，ObjectId ↔ hex string |
| WS 广播：多个 WS 连接共享一个 ChangeStream | 使用 `tokio::sync::broadcast` channel 分发事件 |
