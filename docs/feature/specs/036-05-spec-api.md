# #36 多 Agent 框架 — Spec / API & 项目结构

## REST / WebSocket API 设计（前端接口）

Server 通过 REST + WebSocket 为前端白板提供数据。

### 认证

所有 REST/WS 端点携带 Bearer JWT token（`Authorization: Bearer <jwt>`）；WS 在连接 query parameter 中传递 `?token=<jwt>`。gRPC 中间件校验 token 的 `scope` 字段（如 `board_read`/`board_write`/`agent_registry`），REST/WS 中间件做相同校验。RBAC scope 完整定义见 036-02-spec-architecture.md ## RBAC Scope 定义 节。

### REST 端点
```
POST   /api/workspaces                           # 创建 Workspace
GET    /api/workspaces                           # Workspace 列表
DELETE /api/workspaces                           # 批量删除 Workspace
GET    /api/workspaces/:ws_id                    # Workspace 详情
PATCH  /api/workspaces/:ws_id                    # 更新 Workspace
DELETE /api/workspaces/:ws_id                    # 删除单个 Workspace
GET    /api/workspaces/:ws_id/board              # 白板聚合数据（一次性返回全部区块）
POST   /api/workspaces/:ws_id/chats              # 创建 Chat
GET    /api/workspaces/:ws_id/chats              # Chat 列表
DELETE /api/workspaces/:ws_id/chats              # 批量删除 Chat
GET    /api/workspaces/:ws_id/chats/:chat_id     # Chat 详情
PATCH  /api/workspaces/:ws_id/chats/:chat_id     # 更新 Chat title/status（归档等）
DELETE /api/workspaces/:ws_id/chats/:chat_id     # 删除单个 Chat
GET    /api/workspaces/:ws_id/chats/:chat_id/messages  # Chat 消息列表（?limit=50&before=<msg_id> 光标分页；response: { messages, has_more, next_cursor }）
GET    /api/workspaces/:ws_id/requirements       # Requirement 列表（支持 ?status=... 过滤）
GET    /api/workspaces/:ws_id/requirements/:requirement_id   # Requirement 详情
POST   /api/workspaces/:ws_id/requirements       # 创建 Requirement
PATCH  /api/workspaces/:ws_id/requirements/:requirement_id   # 更新 Requirement（草案/状态/关联）
POST   /api/workspaces/:ws_id/requirements/:requirement_id/cancel  # 软取消 Requirement，状态→Cancelled；不硬删除
POST   /api/workspaces/:ws_id/requirements/:requirement_id/reject   # 拒绝 Requirement → Rejected
POST   /api/workspaces/:ws_id/requirements/:requirement_id/resubmit # 重新提交（Rejected → Analyzing）
GET    /api/workspaces/:ws_id/projects           # Project 列表（支持 ?status=in_progress&requirement_id=... 过滤）
GET    /api/workspaces/:ws_id/projects/:project_id  # Project 详情
GET    /api/workspaces/:ws_id/projects/:project_id/tasks # 某个 Project 的 Task 列表
POST   /api/workspaces/:ws_id/projects/:project_id/resume # 用户反馈已写入 ChatMessage(message_type=feedback) 并关联 Project/Task 后，恢复 Blocked Project
POST   /api/workspaces/:ws_id/projects/:project_id/cancel  # 取消 Project
POST   /api/workspaces/:ws_id/projects/:project_id/retry   # 重开 Project（Failed → Pending）
GET    /api/workspaces/:ws_id/projects/:project_id/tasks/:task_id  # Task 详情
PATCH  /api/workspaces/:ws_id/projects/:project_id/tasks/:task_id  # 更新 Task 信息
POST   /api/workspaces/:ws_id/projects/:project_id/tasks/:task_id/cancel   # 取消 Task（对应 gRPC Cancel）
POST   /api/workspaces/:ws_id/projects/:project_id/tasks/:task_id/retry    # 重开 Task（Failed → Pending，保留 retry_count）
GET    /api/workspaces/:ws_id/agents             # Agent 实例列表
POST   /api/workspaces/:ws_id/chats/:chat_id/messages  # 创建 ChatMessage（Chat/用户调用）
POST   /api/workspaces/:ws_id/requirements/:id/confirm  # 确认草案并创建 Project/Task
                                                          # Request: {}（空 body — 确认当前 draft）
                                                          # Response: { requirement, created_projects: Vec<ProjectWithTasks> }
POST   /api/workspaces/:ws_id/requirements/:id/reject   # 驳回草案
```

说明：DELETE 批量删除端点的 IDs 通过 query params 传递。

说明：Requirement 和 Project 不暴露 REST DELETE 端点；使用 POST `.../cancel` 进行软取消（status → Cancelled）。仅 Workspace 和 Chat 支持 REST DELETE。

### WebSocket
```
WS /ws/workspaces/:ws_id/board
  → 实时推送 BoardSnapshot / BoardSnapshotUpdate 事件
  → 首次连接：全量 BoardSnapshot（is_full_snapshot=true）
  → 后续：增量 BoardSnapshotUpdate（各 changed/removed 字段列表）
  → snapshot_id 格式：ULID（单调递增，断线后比对判断是否需全量重拉）

应用层消息类型：
- `{"type": "Heartbeat", "snapshot_id": "01AR..."}` — Server 每 30s 发送；客户端无需回复，仅用于保活和断线超时检测
- `{"type": "Error", "code": "UNAUTHORIZED", "message": "..."}` — WS 级错误（鉴权失败、限流等）
- BoardSnapshot 和 BoardSnapshotUpdate 的 type 字段分别为 `"snapshot"` 和 `"update"`
```

### Board 聚合响应结构
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
// WorkspaceInfo 对齐 data spec，BoardSnapshot 同时显式携带 workspace_id 便于订阅/路由校验
pub struct BoardSnapshot {
    pub snapshot_id: String,                     // 当前快照 ID，用于增量订阅一致性校验
    pub workspace_id: ObjectId,                  // 当前 Workspace ID
    pub workspace: WorkspaceInfo,
    pub chats: Vec<Chat>,                         // Chat 会话
    pub recent_messages: Vec<ChatMessage>,        // 近期 Chat 消息（默认最近 50 条）
    pub requirements: Vec<Requirement>,           // Requirement 记录与草案
    pub projects: Vec<ProjectWithTasks>,           // Project & Tasks
    pub agent_instances: Vec<AgentInstance>,       // Agent 状态
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSnapshotUpdate {
    pub snapshot_id: String,        // 基于哪个快照做 diff
    pub changed_workspace: Option<WorkspaceInfo>,  // Workspace 元信息变更（name/provider/model 等）；首次消息（is_full_snapshot=true）时必须为 Some；None = 本次无变更
    pub is_full_snapshot: bool,     // 首次 Watch 消息为 true，表示本消息携带完整快照语义；后续为 false，仅推送增量
    pub timestamp: i64,
    pub changed_requirements: Vec<Requirement>,   // 新增/变更的 Requirement
    pub removed_requirement_ids: Vec<ObjectId>,   // 删除/移除的 Requirement ID
    pub changed_projects: Vec<ProjectWithTasks>,  // 新增/变更的 Project；tasks 仅 Project 首次出现时填充全部；增量更新时 tasks 为空，Task 变更走 changed_tasks
    pub changed_tasks: Vec<ProjectTask>,          // 新增/变更的 ProjectTask（含 status/results 等运行时变更）
    pub removed_project_ids: Vec<ObjectId>,       // 删除/移除的 Project ID
    pub removed_task_ids: Vec<ObjectId>,          // 删除/移除的 ProjectTask ID
    pub changed_chats: Vec<Chat>,                 // 新增/变更的 Chat 会话
    pub removed_chat_ids: Vec<ObjectId>,          // 删除/移除的 Chat ID
    pub new_messages: Vec<ChatMessage>,           // 新增 Chat 消息（首次出现）
    pub updated_messages: Vec<ChatMessage>,       // 已有消息的任何字段变更（content/message_type/元数据等；异步写入的 message_type 也在此）
    pub changed_agents: Vec<AgentInstance>,       // 新增/变更的 Agent 状态
    pub removed_agent_ids: Vec<ObjectId>,         // 删除/移除的 Agent ID
}

/// Workspace 文档的核心子集，嵌入 BoardSnapshot；字段与 data spec 一致
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct ProjectWithTasks {
    pub project: Project,
    pub tasks: Vec<ProjectTask>,
}
```

## gRPC API 设计（Agent 间通信）

详细 message 字段定义见 plan 各 Sprint。

### Common Types（common.proto）

```protobuf
// AgentType 仅用于内置角色标识；自定义角色不扩展固定枚举，使用 agent_role: string 字段承载。
enum AgentType {
  AGENT_TYPE_UNSPECIFIED = 0;
  CHAT = 1;
  SCHEDULER = 2;
  EXECUTOR = 3;
  ASSISTANT = 4;
  EVOLVER = 5;
  // SUB_AGENT 仅用于日志/追踪标识；Sub-Agent 不直连 API Server，不出现在 Watch/注册 RPC 中。
  SUB_AGENT = 6;
}

message WatchRequest {
  string workspace_id = 1;
  AgentType agent_type = 2;
  repeated string event_types = 3;
  optional string resume_snapshot_id = 4;  // 断线重连时携带上次 snapshot_id，用于去重（不保证不丢事件；断线恢复一致性依赖全量扫描）
}

message Empty {}
```

### ChatService（chat.proto）

```protobuf
service ChatService {
  rpc Create(CreateChatRequest) returns (Chat);
  rpc UpdateChat(UpdateChatRequest) returns (Chat); // 更新 title / status（归档等）。REST: PATCH /api/workspaces/:ws_id/chats/:chat_id
  rpc AddMessage(AddMessageRequest) returns (ChatMessage);
  rpc AnalyzeMessage(AnalyzeMessageRequest) returns (AnalyzeMessageResponse);  // Chat 分析消息类型
  rpc Get(GetChatRequest) returns (Chat);
  rpc List(ListChatsRequest) returns (ListChatsResponse);
  rpc DeleteChat(DeleteChatRequest) returns (Empty);
  rpc Watch(WatchRequest) returns (stream ChatEvent);
}
```

### WorkspaceService（workspace.proto）

```protobuf
service WorkspaceService {
  rpc Create(CreateWorkspaceRequest) returns (Workspace);
  rpc Update(UpdateWorkspaceRequest) returns (Workspace);        // 仅更新 provider/model 等可修改字段；用户侧通过 REST PATCH 触发
  rpc Get(GetWorkspaceRequest) returns (Workspace);
  rpc List(ListWorkspacesRequest) returns (ListWorkspacesResponse);
  rpc Delete(DeleteWorkspaceRequest) returns (Empty); // REST 触发，Agent 一般不直接调用
  rpc Watch(WatchRequest) returns (stream WorkspaceEvent);
}
```

### RequirementService（requirement.proto）

```protobuf
service RequirementService {
  rpc Create(CreateRequirementRequest) returns (Requirement);
  rpc Update(UpdateRequirementRequest) returns (Requirement);
  rpc Get(GetRequirementRequest) returns (Requirement);
  rpc List(ListRequirementsRequest) returns (ListRequirementsResponse);
  rpc Analyze(AnalyzeRequirementRequest) returns (Requirement);   // Assistant 原子抢占 Pending→Analyzing；Rejected 可重新提交→Analyzing（驳回后可重新分析）
  rpc Confirm(ConfirmRequirementRequest) returns (Requirement);   // 用户确认草案
  rpc Reject(RejectRequirementRequest) returns (Requirement);     // 用户驳回草案
  rpc Cancel(CancelRequirementRequest) returns (CancelRequirementResponse); // 软取消，状态→Cancelled
  rpc Watch(WatchRequest) returns (stream RequirementEvent);
}

message CancelRequirementResponse {
  Requirement requirement = 1;
  RequirementStatus previous_status = 2;
}
```

### ProjectService（project.proto）

```protobuf
service ProjectService {
  rpc Create(CreateProjectRequest) returns (Project);
  rpc Update(UpdateProjectRequest) returns (Project);
  rpc Get(GetProjectRequest) returns (Project);
  rpc List(ListProjectsRequest) returns (ListProjectsResponse);
  rpc Assign(AssignProjectRequest) returns (Project);             // Scheduler 分配→Executor
  rpc Accept(AcceptProjectRequest) returns (Project);             // Executor 接受
  rpc Resume(ResumeProjectRequest) returns (ResumeProjectResponse);
  rpc Retry(RetryProjectRequest) returns (Project);                // Failed → Pending 重开
  rpc Complete(CompleteProjectRequest) returns (Project);
  rpc Block(BlockProjectRequest) returns (Project);
  rpc Fail(FailProjectRequest) returns (Project);
  rpc Cancel(CancelProjectRequest) returns (CancelProjectResponse);
  rpc Watch(WatchRequest) returns (stream ProjectEvent);
}

message CancelProjectResponse {
  Project project = 1;
  ProjectStatus previous_status = 2;
}

message ResumeProjectResponse {
  Project project = 1;
  ProjectStatus previous_status = 2;  // Blocked
}
```

### ProjectTaskService（project_task.proto）

```protobuf
service ProjectTaskService {
  rpc Create(CreateTaskRequest) returns (ProjectTask);
  rpc Get(GetTaskRequest) returns (ProjectTask);
  rpc Update(UpdateTaskRequest) returns (ProjectTask);
  rpc Complete(CompleteTaskRequest) returns (ProjectTask);
  rpc List(ListTasksRequest) returns (ListTasksResponse);
  rpc Cancel(CancelTaskRequest) returns (CancelTaskResponse);
  rpc Fail(FailTaskRequest) returns (FailTaskResponse);
  rpc Retry(RetryTaskRequest) returns (ProjectTask);               // Failed → Pending 重开
  rpc Watch(WatchRequest) returns (stream ProjectTaskEvent);
}
// Task 状态流转 InProgress→InReview、InReview→InProgress（返工）、InProgress→Retrying 通过 Update RPC 实现。
```

### AgentRegistryService（agent.proto）

```protobuf
service AgentRegistryService {
  rpc Register(RegisterAgentRequest) returns (AgentInstance);      // agent_role: string 承载自定义角色；SUB_AGENT 不注册
  rpc Heartbeat(HeartbeatRequest) returns (HeartbeatResponse);  // Executor / Assistant / Chat Agent（Chat 仅在 Busy 时被监控）
  rpc Deregister(DeregisterAgentRequest) returns (Empty);
  rpc List(ListAgentsRequest) returns (ListAgentsResponse);
  rpc Watch(WatchRequest) returns (stream AgentEvent);             // WatchRequest.agent_type 不使用 SUB_AGENT
  rpc RefreshToken(RefreshTokenRequest) returns (RefreshTokenResponse);
}

// ===== ReflectionService =====
// 独立 proto 文件：share/proto/reflection.proto
```

```protobuf
// share/proto/reflection.proto
service ReflectionService {
  rpc Create(CreateReflectionRequest) returns (Reflection);        // Evolver 写入 Reflection
  rpc Get(GetReflectionRequest) returns (Reflection);
  rpc List(ListReflectionsRequest) returns (ListReflectionsResponse);
  rpc TriggerReflection(TriggerReflectionRequest) returns (Reflection);  // 手动触发 Project 反思
  rpc Watch(WatchRequest) returns (stream ReflectionEvent);
}

message Reflection {
  string reflection_id = 1;
  string title = 2;
  string content = 3;              // Markdown 格式反思描述
  string project_id = 4;           // 关联 Project（可选）
  string workspace_id = 5;
  repeated string related_entity_ids = 6;  // 关联实体 ID（Project / Task / Requirement）
  string embedding_status = 7;     // pending | indexed | failed
  string embedding_ref = 8;        // Qdrant point id（indexed 时非空）
  int64 created_at = 9;
  int64 reflected_at = 10;         // 反思来源 Project 的 completed 时间
}

message CreateReflectionRequest {
  string title = 1;
  string content = 2;
  string project_id = 3;
  string workspace_id = 4;
  repeated string related_entity_ids = 5;
  string idempotency_key = 6;
}

message GetReflectionRequest {
  string reflection_id = 1;
}

message ListReflectionsRequest {
  string workspace_id = 1;
  optional string project_id = 2;
  optional string embedding_status = 3;
  int32 page_size = 4;
  string page_token = 5;
}

message ListReflectionsResponse {
  repeated Reflection reflections = 1;
  string next_page_token = 2;
}

message TriggerReflectionRequest {
  string project_id = 1;
  string workspace_id = 2;
  string idempotency_key = 3;
}

message ReflectionEvent {
  enum EventType { CREATED = 0; UPDATED = 1; DELETED = 2; }
  EventType event_type = 1;
  Reflection reflection = 2;
}
```

### BoardService（board.proto）

```protobuf
service BoardService {
  rpc Watch(WatchRequest) returns (stream BoardSnapshotUpdate);   // 首次消息为全量快照（is_full_snapshot=true），之后按 snapshot_id 连续增量推送
  rpc GetBoardSnapshot(GetBoardRequest) returns (BoardSnapshot);
}
```


## 项目结构变更

### Crate 依赖关系
```
ui/              # 纯 UI ──HTTP/WS──▶ server
                  #   ──依赖──▶ share/openapi/sdk/ts
server/          # API Server ──依赖──▶ share
agents/          # Agent 运行时（独立部署）──依赖──▶ share
                  #   ──gRPC──▶ server
infra/           # 基建与部署 ──不依赖──▶ 其他模块
cli/             # CLI（保留）──依赖──▶ share
share/           # 共享层
  ├── core       #   共享核心库（类型、错误、工具抽象）
  ├── llm        #   LLM 客户端
  ├── tools      #   工具注册
  ├── proto/     #   gRPC protobuf 定义
  │   └── sdk/   #   生成的 gRPC SDK（rust + ts）
  └── openapi/   #   OpenAPI 3 schema（REST + WS）
      └── sdk/   #   生成的 API SDK（rust + ts）
```

### 目录结构

```
aemeath/
├── share/                        # ★ 共享层
│   ├── aemeath-core/             #   核心库（不变）
│   ├── aemeath-llm/              #   LLM 客户端（不变）
│   ├── aemeath-tools/            #   工具注册（不变）
│   ├── proto/                    #   gRPC protobuf 定义
│   │   ├── chat.proto
│   │   ├── workspace.proto
│   │   ├── requirement.proto
│   │   ├── project.proto
│   │   ├── project_task.proto
│   │   ├── agent.proto
│   │   ├── common.proto           #   共享枚举/类型（如 CostTier）
│   │   ├── reflection.proto
│   │   ├── board.proto            #   BoardService
│   │   └── sdk/                  #   proto 生成的 SDK
│   │       ├── rust/             #     tonic 生成
│   │       └── ts/               #     protobuf-ts 生成
│   └── openapi/                  #   OpenAPI 3 schema
│       ├── spec.yaml             #     REST + WS 接口定义
│       └── sdk/                  #     OpenAPI 生成的 SDK
│           ├── rust/             #       Rust SDK（reqwest）
│           └── ts/               #       TypeScript SDK（fetch）
├── cli/                          # CLI（保留）
│   └── src/main.rs
├── server/                       # ★ API Server（按 feature 组织）
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs               #   服务入口（组装 feature）
│   │   ├── share/                #   server 内部共享层（feature 间通信接口）
│   │   │   ├── mod.rs
│   │   │   ├── types.rs          #     共享类型（WorkspaceId, ProjectId 等）
│   │   │   ├── repo_traits.rs    #     各 feature repository 暴露的 trait
│   │   │   └── event_bus.rs      #     内部事件总线（feature 间解耦通知）
│   │   └── features/             #   feature 模块
│   │       ├── chat/             #     chat feature
│   │       │   ├── mod.rs        #       对外暴露 pub 模块声明
│   │       │   ├── grpc.rs
│   │       │   ├── rest.rs
│   │       │   └── repository.rs
│   │       ├── workspace/        #     workspace feature
│   │       │   ├── mod.rs
│   │       │   ├── grpc.rs
│   │       │   ├── rest.rs
│   │       │   └── repository.rs
│   │       ├── requirement/      #     requirement feature
│   │       │   ├── mod.rs
│   │       │   ├── grpc.rs
│   │       │   ├── rest.rs
│   │       │   └── repository.rs
│   │       ├── project/          #     project feature
│   │       │   ├── mod.rs
│   │       │   ├── grpc.rs
│   │       │   ├── rest.rs
│   │       │   └── repository.rs
│   │       ├── project_task/     #     project_task feature
│   │       │   ├── mod.rs
│   │       │   ├── grpc.rs
│   │       │   ├── rest.rs
│   │       │   └── repository.rs
│   │       ├── agent/            #     agent feature（Agent Registry，非独立 crate）
│   │       │   ├── mod.rs
│   │       │   ├── grpc.rs
│   │       │   ├── rest.rs
│   │       │   └── repository.rs
│   │       ├── board/            #     board feature（白板聚合）
│   │       │   ├── mod.rs
│   │       │   ├── grpc.rs       #       BoardService gRPC handler (Watch + GetBoardSnapshot)
│   │       │   ├── rest.rs       #       GET /api/workspaces/:ws_id/board
│   │       │   └── aggregator.rs #       跨 feature 聚合逻辑
│   │       └── ws/               #     WebSocket feature
│   │           ├── mod.rs
│   │           └── handler.rs    #       WS 连接管理 + BoardSnapshot 推送
│   │       └── reflection/        #     reflection feature（Evolver 写入反思）
│   │           ├── mod.rs
│   │           ├── grpc.rs
│   │           ├── rest.rs
│   │           └── repository.rs
├── agents/                       # ★ Agent 运行时（独立部署，按 role 组织）
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs               #   Agent 进程入口
│   │   ├── share/                #   agents 内部共享层（role 间通信接口）
│   │   │   ├── mod.rs
│   │   │   ├── types.rs          #     共享类型（AgentId / ProjectId / TaskContext 等）
│   │   │   ├── template.rs       #     通用 Agent 模板 trait
│   │   │   └── pool.rs           #     Pool trait（Executor 实现）
│   │   └── features/             #   role feature 模块
│   │       ├── scheduler/        #     Scheduler（调度 + Watch + 对账）
│   │       │   ├── mod.rs
│   │       │   └── scheduler.rs
│   │       ├── executor/         #     Executor（任务执行 + Pool 管理）
│   │       │   ├── mod.rs
│   │       │   └── executor.rs
│   │       ├── evolver/          #     Evolver（反思引擎 + 知识优化）
│   │       │   ├── mod.rs
│   │       │   └── evolver.rs
│   │       ├── chat/             #     Chat（用户对话 + 汇报）
│   │       │   ├── mod.rs
│   │       │   └── chat.rs
│   │       ├── assistant/        #     Assistant（后台需求分析/草案 Worker，由 Scheduler 调度）
│   │       │   ├── mod.rs
│   │       │   └── assistant.rs
│   │       └── sub_agent/        #     Sub-Agent（进程内 tokio task 执行）
│   │           ├── mod.rs
│   │           └── sub_agent.rs
│   └── roles/                    #   角色配置（TOML）
│       ├── chat.toml
│       ├── assistant.toml
│       ├── scheduler.toml
│       ├── executor.toml
│       ├── evolver.toml
│       ├── planner.toml
│       ├── coder.toml
│       ├── tester.toml
│       ├── reviewer.toml
│       ├── designer.toml
│       └── custom/
├── infra/                          # ★ 基建与部署（开发环境）
│   ├── mongodb/                    #   MongoDB 初始化脚本
│   │   ├── init-collections.js    #     collection + 索引创建
│   │   └── seed.js                #     开发环境种子数据
│   ├── gateway/                    #   反向代理（NGINX）
│   │   └── nginx.conf
│   └── deploy/                     #   部署编排
│       ├── docker-compose.dev.yaml     #     本地开发（Server + MongoDB + Gateway）
│       └── Dockerfile             #     开发镜像
├── ui/                           # ★ 纯 Web 前端（Vue 3 + Element Plus，Vite 构建）
│   ├── package.json
│   ├── tsconfig.json
│   ├── vite.config.ts
│   └── src/
│       ├── views/                # 页面：会话 / 白板 / 需求详情 / 项目管理
│       ├── components/           # 组件：ChatMessage / BoardCard / StatusBadge / DAGView
│       ├── composables/          # useBoardSnapshot / useWS / useChat
│       ├── stores/               # Pinia 状态管理
│       └── lib/                  # 工具函数、类型定义
├── CLAUDE.md
├── TODO.md
└── docs/
```

## 实现前必须定稿的架构决策

以下关键设计项如果在 spec 阶段不定稿，实现时必定返工。每项给出当前决策、影响范围、未决问题和默认方案。

---

### 1. 状态机与合法迁移

#### 当前决策

```
  Requirement（关联方式：1 Requirement N:N Project，完成判定看 ProjectTask）:
    Pending ──▶ Analyzing ──▶ Draft ──▶ InProgress ──▶ Completed
                   ▲              │         │
                   │              ▼         ▼
                   └────────── Rejected ◀── (用户驳回)

                   Analyzing ──(超时 120s 无产出)──▶ Pending
    - 多方并发控制：Analyzing 标记由 Assistant 原子抢占
    - Draft 允许多轮修改（Draft → Draft），draft_history 追加入口（revision++）
    - Draft 内有 projects/tasks 草案
    - ConfirmRequirement 事务中一次性完成（v0.1，无异步）：
      1. 创建 Project（含 Task 草案）
      2. 创建 ProjectTask
      3. 回填 `Requirement.project_ids`（append 新 Project ID）
      4. 回填 `Requirement.task_ids`（append 新 Task ID）
      5. `Requirement.status → InProgress`
      6. `Requirement.version++`
    - InProgress→Completed：所有 task_ids 中的 ProjectTask 为 Completed 或 Cancelled
    - Rejected → Analyzing：用户重新提交后重启分析流程
    - 任意状态 → Cancelled：用户主动取消（级联取消关联 Project 和 Task）
    - **不使用 Confirmed 状态**（v0.1 简化：Confirm RPC 同步完成所有操作，直接进 InProgress）

Project:
    Pending ──▶ Assigned ──▶ InProgress ──▶ Completed
      │          │             │  │
      │          │             │  ├──▶ Failed（终态，需人工干预重开）
      │          │             │  │
      │          │             │  ├──▶ Blocked ──▶ InProgress (用户反馈)
      │          │             │  │       │
      │          │             │  │       └──▶ Cancelled
      │          │             │  │
      │          │             │  ├──▶ Pending（崩溃恢复）
      │          │             │  │
      │          │             │  └──▶ Cancelled
      │          │
      │          ├── (超时) ──▶ Pending
      │          └──▶ Cancelled
      │
      └──▶ Cancelled

    - Assigned 超时门限：assign_timeout_sec（60s），超时回退 Pending
    - Pending → Cancelled：用户取消或 Requirement 级联取消
    - Assigned → Cancelled：用户取消，需通知 Executor
    - InProgress → Cancelled：用户取消；Executor 采用 cooperative cancel，停止当前执行并释放 worktree / merge_lock
    - Project 进入 Blocked 后由 Agent 主动提醒用户；无系统自动超时，用户通过反馈解锁或手动取消
    - Blocked → InProgress：用户反馈通过 `ChatMessage(message_type=feedback)` 写入并关联 Project/Task，然后调用 ProjectService.Resume / REST resume 入口恢复执行
    - Blocked → Cancelled：用户主动取消，适用于长时间无法解决的阻塞
    - Failed 为普通执行失败终态之一，不自动回退 Pending；只有显式人工重试/重开才可创建新的分配流程
    - Executor 崩溃恢复时，Scheduler 可将 InProgress Project 清空 assigned_executor_id 并回退 Pending 以重新分配
    - Completed 后冻结

ProjectTask:
  Pending ──▶ InProgress ──▶ InReview ──▶ Completed
                ▲   │            │
                │   │            └──▶ InProgress (Review 不通过返工)
                │   │
                │   └── retry_needed / 可重试失败 ──▶ Retrying
                │                                      │
                └──────────────────────────────────────┘
                  （下一次 attempt 开始：Retrying→InProgress）

  Pending ──▶ Cancelled
  InProgress / InReview / Retrying ──(Executor 崩溃恢复)──▶ Pending
  InProgress / Retrying ──(超过 max_task_retries 或不可重试失败)──▶ Failed

  - InProgress→Retrying：Sub-Agent 返回 retry_needed 或 Executor 判定本次 attempt 可重试时；写 intermediate attempt result，不写 final result
  - Retrying→InProgress：下一次 attempt 开始执行时
  - InProgress/Retrying→Failed：超过 max_task_retries（默认 3）或不可重试失败时，写 final failed result；普通失败不自动回退 Pending
  - InReview→InProgress：Reviewer 返回修改意见时
  - InProgress/InReview/Retrying→Pending：仅 Executor 崩溃恢复时由 Scheduler 回退并清空 assigned_executor_id
  - Pending→Cancelled：用户取消 / Project 被取消时级联
  - InProgress/InReview/Retrying 均可直接通过 `CancelTask` → Cancelled；Executor 采用 cooperative cancel，收到取消信号后尽快停止当前 attempt、释放资源并写回最终取消状态

AgentInstance:
  Idle ──▶ Busy ──▶ Idle
    │         │
    ▼         ▼
  Error ──▶ Idle (恢复) / 销毁 (Scheduler 回收)

  - Error→Idle：暂时性错误（模型超时/熔断），冷却后恢复
  - Error→销毁（删除 AgentInstance 文档）：Scheduler 心跳检测超时 → 释放 Project 绑定
  - Initializing：Scheduler 创建后到 Idle 之间的过渡（已在 AgentStatus 枚举中定义）
```

#### 影响范围
- `Requirement.draft` 字段（草案 JSON、draft_history、revision）
- Project / ProjectTask MongoDB schema（status 字段枚举值 + 条件更新的 filter）
- gRPC Request / Response 的字段（`status` + `version`）
- Scheduler 决策逻辑（超时检测、崩溃恢复时的重新分配条件）
- Executor 编排流程（Task 状态流转、失败重试）
- WebSocket 推送事件（状态变更通知前端渲染）

#### 已定稿决策
- `draft_history.revision` 必须存在；每次 Draft→Draft 追加历史时 `revision++`，`Requirement.version` 仅用于乐观锁并发控制并随更新 `$inc`。
- `InProgress` / `InReview` / `Retrying` 均可直接 `CancelTask → Cancelled`，由 Executor cooperative cancel 停止当前 attempt 并释放资源。

#### 默认方案
- 文档中所有枚举值的 status 字段使用上述状态机
- `version: u64` 字段每次更新 `$inc`
- 草案保留修改历史在 `Requirement.draft_history: [{ revision, draft, timestamp }]`，其中 `revision` 为必填字段并单调递增；`Requirement.version` 用于乐观锁并发控制

---

### 2. Executor 编排模型

#### 当前决策

Executor 接收 Project 后，按 DAG（有向无环图）编排 Task 执行：

```
执行模式: DAG
  - 按 ProjectTask.depends_on 拓扑排序
  - 无依赖 Task 进入就绪队列，受 max_concurrent_tasks 限制
  - Task 完成后检查下游依赖，满足条件的下游 Task 进入就绪队列

依赖声明（ProjectTask）:
  depends_on: [task_id]              // 前置 Task ID 列表
  depends_type: "all" | "any"        // all = 全部完成，any = 任一完成

Confirm 阶段对 depends_on 做拓扑校验，有环直接拒绝确认草案。

并发控制:
  max_concurrent_tasks: usize        // 最大同时执行数（可配，默认 5）
```

Sub-Agent 调用流程：
```
Executor 对每个就绪 Task:
  1. 查询 Task.executor_type（如 "coder" / "planner"）
  2. 通过本地角色配置目录（roles/）查可用的 Sub-Agent 角色列表
  3. 装配子 Agent 上下文（request context: task description + related_messages + project context）
  4. 进程内启动 tokio task，调用 SubAgent::run → Sub-Agent 执行
  5. 收集当前批次所有 Task 的结果
  6. 批次完成后统一写回白板（方便 Chat 一次性感知进度，避免碎片推送）
  7. 根据结果判定 Task 状态（completed / failed / retry_needed）
     - completed / failed：写 ProjectTaskResult 作为 final result，并更新 ProjectTask.status
     - retry_needed：不写 final result；Executor 写一条 ProjectTaskResult(status=retry_needed) 作为 intermediate attempt result（记录本次 attempt 的 output/error_message/artifacts/retry_count），将 ProjectTask.status 置为 retrying，然后按 retry 策略重新执行该 Task
  8. retry: 失败或 retry_needed 后最多 max_task_retries 次（可配，默认 3）；最终超过 max_task_retries 后写 ProjectTaskResult(status=failed) 作为最终结果，并将 ProjectTask.status 置为 failed
```

#### 影响范围
- `ProjectTask` schema（depends_on、depends_type、executor_type 字段）
- `agent.proto`（ExecuteTask RPC 定义）
- Executor 运行时（DAG 拓扑排序 + 批次收集）
- 前端渲染（Task DAG 可视化）

#### 当前决策
- Executor 内部 DAG 执行器使用 tokio JoinSet。
- `Task.result_type` 省略该字段。

#### 默认方案
- v0.1 直接实现 DAG 模式
- 所有依赖声明用 Task ID，不支持外部资源依赖
- 不引入 result_type，Sub-Agent 输出直接透传给下游 Task

---

### 3. Sub-Agent 调用协议

#### 部署模型（v0.1 定稿）

**Sub-Agent 是 Executor 进程内的 tokio task**，不是独立进程。

```
Executor 进程
  ├── main.rs              # Executor 主循环
  ├── assembler.rs         # 装配器
  └── sub_agent.rs         # Sub-Agent 执行（进程内调用）

调用链路:
  Executor.execute_task()
    → SubAgent::run(config, task_description, project_context)
    → 返回 TaskResult
```

核心决策：
- Sub-Agent **不注册、不心跳、不直接连 API Server**
- Executor 本地装配 Sub-Agent 上下文，直接调用 `SubAgent::run()`
- Executor 通过 gRPC 向 Server 写 TaskResult（事实层）

#### ExecuteTask（进程内函数签名）

```rust
// agents/src/sub_agent.rs
pub struct ExecuteTaskParams {
    pub task_id: String,
    pub project_id: String,
    pub role: String,                   // "coder" / "planner" / ...
    pub task_description: String,
    pub project_context: String,        // LLM 摘要的 Project 上下文
    pub retry_count: u32,
    pub last_error: Option<String>,
}

pub enum SubAgentTaskStatus {
    Completed,
    Failed,
    RetryNeeded,
}

pub struct ExecuteTaskResult {
    pub task_id: String,
    pub status: SubAgentTaskStatus,    // completed | failed | retry_needed；Sub-Agent 执行结果枚举，独立于 DB 层 ProjectTaskStatus
    pub output: String,                // Sub-Agent 产出；retry_needed 时为本次尝试的中间输出/诊断
    pub artifacts: Vec<String>,        // 产出物引用（file_path / snippet_id）
}
```

`ExecuteTaskResult.status` 使用独立枚举 `SubAgentTaskStatus = completed | failed | retry_needed`，与 DB 层 `ProjectTaskStatus` 分离。`retry_needed` 由 Executor 映射为 `ProjectTaskStatus::Retrying`。Sub-Agent 返回 `ExecuteTaskResult.status = retry_needed` 表示“本次 attempt 未形成最终结果，但建议 Executor 重试”。Executor 必须写入一条 `ProjectTaskResult(status=retry_needed)` 作为 intermediate attempt result，保留本次 attempt 的 `output` / `artifacts` / `error_message` / `retry_count`，同时将 `ProjectTask.status` 更新为 `retrying`；该记录不作为 final result，不触发下游依赖完成判定。随后 Executor 使用递增后的 `retry_count` 与 `last_error` 重新调用 Sub-Agent。若最终超过 `max_task_retries` 仍未成功，Executor 写入最终 `ProjectTaskResult(status=failed)`，并将 `ProjectTask.status` 置为 `failed`。

Sub-Agent 不持有 Session，每次 ExecuteTask 是独立调用：
- 系统提示由 Executor 装配（assembler.rs：注入 role config + tools 白名单）
- 上下文完全从 ExecuteTaskParams 构建，不查白板
- 执行完成后 Executor 决定：等批次收集统一写 DB / 重试 / 放弃
- Sub-Agent 通过角色配置中的 `allowed_tools` 白名单使用 tools，各角色白名单不同
  - e.g. `coder` 可用：Bash / Read / Write / Edit / Glob / LSP
  - e.g. `planner` 可用：Read / Glob / WebSearch
  - Main Agent（Chat / Executor / Assistant / Evolver）同样各自拥有不同 tools 列表，按职责区分
- 沙箱隔离为 TODO（v0.1 不实现）

#### Worktree 与 Merge 锁

Sub-Agent 执行代码修改时，走 git worktree 隔离，**编辑阶段无冲突**。但 merge 回 main 时必须串行化。

**执行流程**：

```
Executor 分配 Task 给 Sub-Agent
→ Executor 为 Task 创建 worktree（task_id 命名）
→ Executor 通过 Server 记录 worktree metadata / Task 状态（Server 不创建 worktree）
→ Sub-Agent 在 worktree 内自由编辑（Bash/Read/Write/Edit/Glob）
→ Sub-Agent 完成后提交 worktree 的变更
→ Executor 决定：merge 回 main 还是放弃

Merge 阶段：
→ Executor 获取 Project 的 merge_lock（MongoDB 乐观锁）
→ 切换到 main worktree，执行 git merge {task_worktree}
→ 若冲突：LLM 辅助解决 → 重试 merge（最多 3 次）
→ 若仍失败：Task 标记 failed，释放 merge_lock
→ merge 成功：推送 main → 释放 merge_lock
```

**Merge 锁设计**：

```
Project 文档增加 merge_lock 字段；创建 Project 时必须写入 `merge_lock: { locked_by_task_id: null, locked_by_executor: null, locked_at: null }`，避免新 Project 缺字段导致锁获取条件不匹配：
{
"merge_lock": {
  "locked_by_task_id": "task_xxx",   // 当前持有锁的 Task ID
  "locked_at": ISODate(...),         // 锁获取时间
  "locked_by_executor": "exec-1"      // 持有锁的 Executor
}
}

获取锁：db.projects.updateOne(
{ _id: project_id, "merge_lock.locked_by_task_id": null },
{ $set: { merge_lock: { locked_by_task_id: task_id, locked_by_executor: executor_id, locked_at: new Date() } } }
)
// matchedCount == 0 → 锁被占用，等待

释放锁：db.projects.updateOne(
{ _id: project_id, "merge_lock.locked_by_task_id": task_id },
{ $set: { "merge_lock.locked_by_task_id": null, "merge_lock.locked_by_executor": null, "merge_lock.locked_at": null } }
)
```

**Merge 锁 vs DAG depends_on**：

| 机制 | 作用 | 粒度 |
|------|------|------|
| `depends_on`（DAG） | 表达 Task 间的**逻辑依赖**（Task B 需要 Task A 的产出） | 业务层面 |
| `merge_lock` | 保证同一 main 分支的 merge 操作**物理串行** | git 层面 |

即使 DAG 表达了 depends_on，仍可能有多个并行 Task 同时完成并尝试 merge。Merge 锁保证同一时刻只有 1 个 Task 在 merge，避免 git 冲突。

**Executor 崩溃后的 Merge 锁释放**：
- Scheduler 检测 Executor 心跳超时 → 级联释放该 Executor 持有的所有 merge_lock

#### 影响范围
- `agents/src/sub_agent.rs` — Sub-Agent 实现（进程内调用）
- `agents/src/assembler.rs` — 装配器（role config + tools + 上下文注入）
- Sub-Agent 角色配置（TOML 中的 model / tools / system_prompt）
- `server/src/grpc/` 不需要 SubAgentExecutionService（v0.1 无 gRPC Sub-Agent）

#### 未决问题
- 无

#### 默认方案
- Sub-Agent 使用配置中声明的 allowed_tools
- project_context 由 Executor 用 LLM 摘要生成（包含 Project name + description + 已完成 Task 的关键产出）
- 沙箱隔离为后续 TODO

---

### 4. 故障恢复与 Checkpoint

#### 当前决策

```
恢复层级:
  1. Watch 断线    → resume_token 续传 → token 过期 → 全量扫描（周期性对账）
  2. Executor 崩溃 → Scheduler 心跳超时 → 释放 Project；仅将关联非终态 Task（InProgress/InReview/Retrying）回退 Pending → 分配给新 Executor
  3. Scheduler 崩溃 → 重启后全量扫描 assigned 超时、in_progress 且 assigned executor 心跳超时、以及 busy Executor 绑定异常 → 重建 Watch → 恢复调度
  4. API Server 崩溃 → 数据库已持久化所有状态 → 重启恢复监听 → Agent 自动重连

Scheduler 对账机制（两层，详见 036-02 Scheduler config 段）:
  - **增量对账**：每 `reconcile_interval_sec`（默认 5s）执行轻量扫描：assigned 超时 / executor 心跳超时 / Agent busy 异常
  - **全量对账**：受 `full_scan_rate_limit_sec`（默认 60s）限制频率上限，防止雪崩
  - assigned 超时的 Project → 清理分配信息并重置为 pending
  - in_progress 且 assigned executor 心跳超时的 Project → 按 Executor 崩溃恢复路径释放 Project、回退非终态 Task 并重置为 pending
  - busy Executor 但 current_project_id 不存在或指向 completed/failed/cancelled Project → 清理 Executor 绑定并回收/置 Idle
  - pending 超时且无 Executor → 扩展 Pool
  - 完成后写 checkpoint 到独立 `scheduler_state` collection（checkpoint_time, last_full_scan, processed_count）

Executor 崩溃后的完整恢复:
  1. Scheduler 检测 Executor 心跳超时（30s）
  2. 释放 Project：$unset assigned_executor_id + status→pending（仅崩溃恢复路径；普通执行失败不自动回退 Pending）
  3. 级联释放 ProjectTask：project_id 匹配 + status∈{in_progress,in_review,retrying} → pending + $unset assigned_executor_id
  4. 释放该 Executor 持有的所有 merge_lock：$set { "merge_lock.locked_by_task_id": null, "merge_lock.locked_by_executor": null, "merge_lock.locked_at": null }
  5. 新 Executor 分配后查询项目关联的 pending Task → 按编排策略重新执行
```

#### 影响范围
- `agent_heartbeats` collection 结构（Executor / Assistant 始终监控；Chat Agent 仅在 Busy 时由 Scheduler 监控，见 State spec 双通道健康模型）
- `scheduler_state` collection 结构（Scheduler 对账 checkpoint）
- Scheduler 核心逻辑（对账定时器、心跳检测）
- MongoDB Change Stream 的 resume_token 管理
- Agent 启动流程（断线重连 + 状态恢复）

#### 未决问题
- 无（已定稿：v0.1 单例 + crash-recovery，v0.2 主备；Watch 断线降级为对账全量扫描；Executor 崩溃级联释放 ProjectTask：仅将 InProgress/InReview/Retrying 回退 Pending；普通执行失败不自动回退 Pending）

#### 默认方案
- v0.1 Scheduler 单例 + crash-recovery（重启全量扫描），v0.2 考虑主备
- resume_token 过期 → 用 `db.collection.watch({ startAtOperationTime: last_known })` 替代，不可用时降级为全量扫描

---

### 5. 权限与 Workspace 隔离

#### 当前决策

```
两种 Token：

1. 用户 Token（账户系统签发，前端持有）:
   {
     "user_id": "uuid",
     "workspace_id": "uuid",
     "iat": ...,
     "exp": ...
   }
   用于 REST / WebSocket 请求，前端登录后获得。
   账户系统（登录 / 注册 / 会话管理）v0.1 简单实现，
   后续可扩为 OIDC。

2. Agent Token（API Server 签发，Agent 持有）:
   {
     "agent_id": "uuid",
     "role": "executor",         // 角色标识
     "workspace_id": "ws_uuid",  // 所属 Workspace
     "scope": ["board_read", "board_write"],
     "aud": "aemeath-api-server",
     "iss": "aemeath-api-server",
     "iat": ...,
     "exp": ...
   }
   用于 Agent ↔ API Server 的 gRPC 调用。
   API Server 在 Agent 注册成功后签发；服务端校验 Agent Token 时必须同时校验 `aud == "aemeath-api-server"` 与 `iss == "aemeath-api-server"`。

权限校验位置:
 1. API Server → 校验用户 Token（REST/WS）和 Agent Token（gRPC）；`scope` 控制 API Server 资源权限，如 `board_read` / `board_write` / `agent_registry`
 2. Agent 装配器 → 注入 token + 按 RoleConfig.allowed_tools 白名单过滤 Agent runtime 工具（如 Bash/Read/Write/WebSearch/Grep/Glob）
 3. Sub-Agent → 进程内 tokio task，**无 token**。权限由 Executor 的 `RoleConfig.allowed_tools` / `can_call_roles` 控制。DB 写入统一由 Executor 经 gRPC 调 Server。

`board_read` / `board_write` 属于 token `scope`，不属于 `allowed_tools`；`allowed_tools` 只描述 Agent runtime 可调用工具。Token scope 完全由 RoleConfig.scope 派生，签发时不做额外添加。

Workspace 隔离:
  - 所有 MongoDB 查询强制带 workspace_id filter（repository 层注入）
  - Sub-Agent 由 Executor 在进程内调用，workspace 上下文由 Executor 传递，无需独立校验
  - Workspace 之间数据完全隔离

Executor 产出写 DB，Server 聚合生成白板，Chat 感知汇报:
  - Executor 执行完成后写 project_result / project_task_result 到 DB
  - Server 自动聚合 DB 事实层 → 生成 / 更新 BoardSnapshot
  - Chat Watch BoardSnapshot + Requirement → 感知全局状态变化与需求状态变更 → 汇报用户
```

#### 影响范围
- 所有 gRPC Request message 需要 `workspace_id` 字段
- API Server 的 gRPC 拦截器链
- `agents/src/assembler.rs` 的 token 注入逻辑
- `repository/` 层所有查询（强制 workspace_id 添加）

#### 当前决策
- Token 签发：v0.1 由 API Server 直接签发。
- 租户 RBAC：v0.1 单租户。

#### 默认方案
- v0.1 API Server 自身签发 token（无独立 Auth Service），v0.2 可配置 OIDC
- v0.1 只支持个人 Workspace（is_personal=true），团队和 RBAC 放到 v0.2

---

### 6. 幂等与一致性策略

#### 当前决策

```
幂等 ID 存储:
  - MongoDB collection: idempotency_records
  - 文档结构: { _id: ObjectId, key: string, entity_type: string, entity_id: ObjectId, scope: string, created_at: ISODate }
  - 唯一索引: { key: 1, entity_type: 1, scope: 1 } — 幂等作用域复合唯一
  - entity_type 取值: requirement | project | task | chat_message
  - scope 取值: 对应 workspace_id 或 chat_id
  - entity_id：幂等操作产出的实体 ObjectId（创建时回填）
  - TTL index: created_at_1, expireAfterSeconds=86400（24h）
  - 写入流程: 先 insertOne（JSON）。若 `{ key, entity_type, scope }` 冲突则返回已有记录的 entity_id
  - 重试：相同 key 的请求直接返回已创建的 entity_id，不重复执行逻辑
完整 schema 见 036-03-spec-data.md idempotency_records 节。

MongoDB ↔ Qdrant 一致性:
  - MongoDB 是主存储，Qdrant 是派生索引
  - 写入顺序: MongoDB → success → async task: Qdrant upsert
  - Qdrant 写失败: 设置 document.embedding_status = "failed"，后台 retry worker 周期扫描重试
  - Qdrant 不可用: embedding_status 保持 "pending"，Evolver 跳过 RAG，降级为规则化总结
  - 无分布式事务：不要求 MongoDB 和 Qdrant 原子性

版本冲突（乐观锁）:
  - 所有实体文档加 `version: u64` 字段
  - 更新操作: filter 包含 { _id, version: expected }, update 包含 { $inc: { version: 1 } }
  - 版本冲突 → 返回 ABORTED 错误 → 调用方重新读取最新文档 → 重试
  - 使用者: Scheduler（分配 Project）、Executor（完成 Task）、Assistant（更新 Requirement.draft，由 Chat 触发）
API 重试的重复创建防护:
  - Create 操作: idempotency_key + unique constraint（如 Chat.name + workspace_id 复合唯一索引）
  - Assign 操作: 条件更新（status + assigned_executor_id + version）天然幂等
  - 前端重试: 相同 idempotency_key 的 POST /api/.../chats/:chat_id/messages 返回已有 ChatMessage
```

#### 影响范围
- 所有 MongoDB entity document + `version` 字段
- `idempotency_records` collection + TTL 索引
- repository 层所有 update 方法签名（加上 expected_version 参数）
- gRPC 所有写操作的 request message（+ idempotency_key）
- Qdrant 的 embedding_status 字段 + 后台 retry worker

#### 未决问题
- 无（已定稿：TTL 24h，版本冲突固定 3 次重试 100ms 间隔，Qdrant 后台 worker 重试）

#### 默认方案
- TTL 24h，足够覆盖绝大多数场景；超时需要重建 idempotency_key
- 版本冲突重试：固定 3 次，每次间隔 100ms

---

### 7. 数据流设计（UI / Server / Agents）

#### 核心约束
- **Agents 不直接与 UI 交互**。所有 UI 可见状态由 Server 维护，UI 通过 Server 获取。
- **Server 是唯一的数据权威**（single source of truth）。Agents 不写 UI，只写 DB（通过 gRPC 调 Server）。

#### 数据流图

```
┌─────────────────────────────────────────────────────────┐
│                        UI（浏览器）                       │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐               │
│  │ 白板视图  │  │ 聊天视图  │  │ 需求视图  │               │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘               │
│       │              │              │                     │
│       └──────────────┼──────────────┘                     │
│                      │                                    │
│          REST（查询）  │  WebSocket（推送）                 │
└──────────────────────┼────────────────────────────────────┘
                       │
┌──────────────────────┼────────────────────────────────────┐
│                   Server（API Server）                     │
│                      │                                    │
│  ┌───────────────────┼───────────────────┐               │
│  │     rest/         │     ws.rs         │               │
│  │  chat.rs          │  (状态推送)        │               │
│  │  requirement.rs   │                    │               │
│  │  project.rs       │                    │               │
│  │  board.rs         │                    │               │
│  └───────┬───────────┴───────┬───────────┘               │
│          │                   │                            │
│  ┌───────┴───────────────────┴───────────┐               │
│  │          repository/（MongoDB）         │               │
│  └───────────────────────────────────────┘               │
│          ▲                   ▲                   ▲        │
│          │ gRPC              │ gRPC              │ gRPC   │
└──────────┼───────────────────┼───────────────────┼────────┘
           │                   │                   │
 ┌─────────┴────────────────┐  ┌┴─────────────────┐  ┌┴────────────────┐
 │   Chat                   │  │   Scheduler      │  │   Executor      │
 │   (agents/)              │  │   (agents/)      │  │   (agents/)     │
 │ 接收用户消息             │  │ Watch DB 变更     │  │ 执行 ProjectTask│
 │ 写 ChatMessage           │  │ 分配 Project      │  │ 调用 SubAgent  │
 │ Chat Watch BoardSnapshot │  │ 调度 Assistant    │  │ 写 TaskResult  │
 │ 汇报用户                │  │                 │  │               │
 └──────────────────────────┘            │
                         ┌───────┴────────────────────────────────────────────┐
                         │   Assistant                                        │
                         │   (agents/)                                        │
                         │ Assistant 分析 Requirements/Project（Scheduler 调度） │
                         │ Project/Task 草案                                  │
                         └────────────────────────────────────────────────────┘
                         ┌───────┴──────────┐
                         │   Evolver        │
                         │   (agents/)      │
                         │                  │
                         │   反思 + 优化     │
                         └──────────────────┘
```

#### 各层交互协议

| 方向 | 协议 | 用途 | 方向 |
|------|------|------|------|
| UI → Server | REST（HTTP） | 查询、CRUD 操作 | 请求-响应 |
| Server → UI | WebSocket | 状态变更实时推送 | 服务端推送 |
| Agents → Server | gRPC | 读写 DB、注册/心跳 | 请求-响应 |
| Server → Agents | gRPC | 指令下发（仅 Agent Registry） | 请求-响应 |

#### UI 的数据来源

| 数据类型 | 获取方式 | 更新方式 |
|----------|----------|----------|
| 白板（board） | REST GET `/api/workspaces/:ws_id/board` | Server 聚合后 WebSocket 推送 |
| 需求列表 | REST GET `/requirements` | WebSocket 增量推送 |
| Project 状态 | REST GET `/projects/{project_id}` | WebSocket 推送 ProjectTask 完成 |
| 聊天消息 | REST GET `/chats/{chat_id}/messages` | WebSocket 推送新消息 |
| Agent 状态 | REST GET `/api/workspaces/:ws_id/agents` | WebSocket 推送心跳/状态变更 |

#### 白板数据聚合（board.rs）

响应：`BoardSnapshot`，结构见 [Board 聚合响应结构](#board-聚合响应结构)。REST 初始全量拉取 `GET /api/workspaces/:ws_id/board` 默认限制 `recent_messages` 为最近 50 条，避免首次加载拉取过多历史消息；后续更新仍通过 WebSocket 推送增量 diff。

Server 收到 DB 变更（Agent 通过 gRPC 写入）后：
1. 聚合受影响的白板数据
2. 通过 WebSocket 向 UI 推送增量 diff

#### Agent 如何"呈现"信息

Agent **不直接写白板**。流程：

```
Executor 完成任务
  → gRPC 调 Server：UpdateProjectTaskResult(task_id, result)
  → Server 写 MongoDB：project_task.status = completed
  → Server 触发 board 聚合
  → Server WebSocket 推送 board diff 到 UI
  → UI 重新渲染
```

#### 数据层次（两层模型）

白板数据分两层，各司其职：

| 层次 | 数据 | 写入者 | 说明 |
|------|------|--------|------|
| **原始事实层** | Project / ProjectTask / ProjectResult / ProjectTaskResult | Executor（通过 gRPC） | Agent 执行的原始产出，写入 MongoDB |
| **展示视图层** | BoardSnapshot | Server（自动聚合） | 由 `board.rs` 从事实层实时计算，不依赖 Chat/Assistant 整理 |

```
Executor 写 TaskResult（事实层）
  → Server repository 写 MongoDB
  → Server board.rs 增量计算 BoardSnapshot（视图层）
  → WebSocket 推送 BoardSnapshot diff 到 UI
  → UI 直接渲染，无需二次处理
```

**Assistant 总结 ≠ 白板视图**：
- Executor 所有 Task 完成后写的 `ProjectResult.summary` 是**事实层**的一条记录
- BoardSnapshot 包含 summary 字段，但它由 Server 聚合，不是 Assistant 直接生成
- WebSocket 推送的是 BoardSnapshot（视图层），不是单个 Task 的状态变更
- 前端始终 watch 聚合后的 BoardSnapshot，而非原始 Task 变更流

#### 未决问题
- 无（已定稿：Agents 不碰 UI，Server 是唯一数据权威，两层数据模型，WebSocket 推送 BoardSnapshot diff）

---

### 8. 日志系统设计

#### 设计原则
1. **三层独立日志**：UI / Server / Agents 各有自己的日志输出，互不干扰
2. **统一格式**：JSON Lines，每层加 `layer` 字段区分来源
3. **可聚合**：支持按 `trace_id` / `project_id` / `task_id` 串联三层日志
4. **Agent 审计日志**：所有 LLM 调用（请求/响应/token 用量）记录审计日志

#### 日志层级

| 层级 | 输出位置 | 包含内容 |
|------|----------|----------|
| UI | 浏览器 console + 可选远程上报 | 用户操作、前端错误、WS 断线 |
| Server | `~/.aemeath/server.log` | HTTP/gRPC 请求、DB 操作、白板聚合 |
| Agents | `~/.aemeath/agents/{role}.log` | Agent 生命周期、LLM 调用、任务执行 |

#### 统一日志格式（JSON Lines）

```jsonc
{
  "ts": "2026-05-17T10:30:00.123Z",   // ISO 8601 时间戳
  "layer": "server|agents|ui",         // 来源层
  "level": "INFO|WARN|ERROR|DEBUG",
  "module": "grpc::requirement",       // 模块路径
  "msg": "ProjectTask completed",      // 日志消息
  "trace_id": "req_abc123",            // 请求全链路追踪 ID
  "project_id": "proj_xyz",            // 可选：关联的业务 ID
  "task_id": "task_001",               // 可选
  "elapsed_ms": 1234,                  // 可选：耗时
  "extra": {}                          // 可选：上下文数据
}
```

#### trace_id 传递链

```
UI 发起请求（生成 trace_id）
  → HTTP Header: X-Trace-Id: xxx
    → Server 接收，注入到所有后续操作
      → gRPC Metadata: x-trace-id: xxx
        → Agent 接收，Logger 自动打 tag
```

全链路通过 `trace_id` 串联：`cat ~/.aemeath/*.log | jq 'select(.trace_id=="xxx")'` 即可看到完整调用链。

#### 审计日志（Agents）

| 文件 | 内容 |
|------|------|
| `~/.aemeath/audit/llm.log` | 每次 LLM 调用的请求/响应摘要、token 用量、耗时 |
| `~/.aemeath/audit/tool.log` | 每次工具调用的名称、参数摘要、结果大小、耗时、调用时的 `role` 与 `allowed_tools` 状态 |
| `~/.aemeath/audit/task.log` | ProjectTask 生命周期：pending → assigned → in_progress → in_review → completed / failed / retrying / cancelled |

审计日志按天轮转：`llm.2026-05-17.log`。

Sub-Agent 工具调用审计：v0.1 信任 Executor 在进程内按 RoleConfig 过滤并执行工具调用，审计日志记录事后证据（每次调用时的 role、allowed_tools 快照、工具名和参数摘要）；v0.2 可考虑由服务端对工具调用事件做独立校验。

#### 日志级别配置

```toml
# ~/.aemeath/logging.toml
[server]
level = "info"
modules = { grpc = "debug", rest = "info", repository = "warn" }

[agents]
level = "info"
modules = { scheduler = "debug", executor = "debug", evolver = "info" }

[ui]
level = "warn"         # 前端默认不输出 debug 到 console
remote_report = false  # 可选远程上报
```

#### 影响范围
- `server/src/` — 引入 `tracing` crate，所有 gRPC/rest handler 加 span
- `agents/src/` — 引入 `tracing`，所有 Agent 逻辑加 span
- `ui/src/` — 前端日志封装，支持 console + 可选远程上报
- `share/` — 定义统一的 `LogRecord` 结构体（OpenAPI schema 的一部分）

#### 未决问题
- 无（已定稿：JSON Lines 统一格式，trace_id 全链路，三层独立输出）

---

### 9. 长任务超时 / 取消 / Token 刷新 / gRPC 错误码

#### 9.1 长任务超时与取消

`ExecuteTaskParams` 增加 deadline：

```rust
pub struct ExecuteTaskParams {
    // ... 已有字段
    pub deadline: Option<Instant>,       // 绝对截止时间（None = 无限）
}
```

Sub-Agent 执行循环中每条 LLM 调用前检查 deadline。超时后 Executor 将任务状态置为 `ProjectTaskStatus::Failed`；失败原因记录在 `ProjectTaskResult` 的 `error_message` / `output` 字段中。

**gRPC 取消服务**：

```protobuf
message CancelTaskRequest {
  string task_id = 1;
  string reason = 2;              // "user_cancelled" | "project_cancelled" | "timeout"
  string cancelled_by = 3;        // 取消发起方（user_id / agent_id）
}

message CancelTaskResponse {
  ProjectTask task = 1;
  ProjectTaskStatus previous_status = 2;
}

message FailTaskResponse {
  ProjectTask task = 1;
  ProjectTaskStatus previous_status = 2;
}
```

**级联取消**：
- 用户取消 Project → 级联取消所有 `status != completed/failed` 的 Task
- 用户取消 Requirement → 级联取消所有关联 Project → 再级联 Task

#### 9.2 Agent Token 刷新

Agent Token 有 `exp` 字段，长任务执行期间可能过期。

```protobuf
message RefreshTokenRequest {
  string agent_id = 1;
  // 服务端通过 gRPC metadata 中的 Agent Token 鉴权后刷新；request body 只传 agent_id。
}

message RefreshTokenResponse {
  string new_token = 1;           // 新 JWT（续期 exp）
  uint64 expires_in = 2;          // 秒
}
```

Executor 在执行长任务前检查 token 剩余有效期：
- `< 5min` → 主动 RefreshToken
- RefreshToken 失败 → 心跳也即将失败 → 提前标记 Project 为 pending 并退出

#### 9.3 gRPC 错误码

所有 gRPC Service 统一使用标准 gRPC Status Code：

| 错误码 | 触发条件 | 调用方处理 |
|--------|----------|-----------|
| `NOT_FOUND` | Project / Task / Requirement 不存在 | 重试无意义，返回 error |
| `PERMISSION_DENIED` | 调用方无权访问该资源 | 检查 Token 权限 |
| `FAILED_PRECONDITION` | 状态机不允许该操作（如 completed → in_progress） | 刷新状态后重试或忽略 |
| `ALREADY_EXISTS` | 创建请求的 idempotency_key 冲突 | 返回已有结果 |
| `ABORTED` | 乐观锁版本冲突 | 重试（带新 version） |
| `UNAVAILABLE` | 服务暂时不可用 | 指数退避重试 |
| `RESOURCE_EXHAUSTED` | Executor Pool 满 / 并发超限 | 等待后重试 |
| `DEADLINE_EXCEEDED` | 操作超时 | 检查超时设置，必要时重试 |

#### 9.4 `Register / Deregister` 硬校验

`RolePermissions.can_create_agents` 仅作为配置提示，**不作为最终授权来源**。

API Server 在 Agent 注册/注销 gRPC handler 中硬编码校验：
- 调用 `AgentRegistryService.Register / Deregister` 的请求方 `role` 必须为内置 `scheduler`
- 非 scheduler role 直接返回 `PERMISSION_DENIED`
- Scheduler Token 通过启动时配置预置密钥签发，不可被其他 Agent 获取

#### 影响范围
- `proto/project_task.proto` — Cancel RPC
- `proto/agent.proto` — RefreshToken RPC + Register/Deregister 硬校验
- `agents/src/sub_agent.rs` — deadline 检查逻辑
- `server/src/grpc/` — 所有 handler 统一错误码
- `server/src/grpc/agent_registry.rs` — Register/Deregister 硬校验

#### 未决问题
- 无

---

### 10. 架构约束与 CI 门禁

#### 10.1 模块引用约束

```
规则 1（顶级隔离）:
  cli / server / agents / ui 之间不可直接引用，只能通过 share/ 层引用。

规则 2（server 内部隔离）:
  server/src/features/{A}/ 不可直接引用 server/src/features/{B}/，
  只能通过 server/src/share/ 中暴露的 trait 调用。

规则 3（agents 内部隔离）:
  agents/src/features/{role_A}/ 不可直接引用 agents/src/features/{role_B}/，
  只能通过 agents/src/share/ 中暴露的 trait 调用。
```

#### 10.2 CI Stop Hook（每次提交前强制执行）

```
Stop Hook 1 — 单元测试:
  cargo test --workspace
  必须全绿。

Stop Hook 2 — 顶级隔离检查:
  检查 cli / server / agents / ui 的 Cargo.toml 中 [dependencies]，
  禁止出现同级模块的直接引用。
  允许的引用：share/aemeath-core、share/aemeath-llm、share/aemeath-tools、share/proto、share/openapi。

Stop Hook 3 — server features 隔离检查:
  检查 server/src/features/{feature}/ 中所有 .rs 文件，
  禁止 use crate::features::{other_feature}:: 形式的跨 feature 引用。
  允许的引用：crate::share:: 下的 trait。

Stop Hook 4 — agents features 隔离检查:
  检查 agents/src/features/{role}/ 中所有 .rs 文件，
  禁止 use crate::features::{other_role}:: 形式的跨 role 引用。
  允许的引用：crate::share:: 下的 trait。
```

#### 10.3 影响范围
- `share/proto/` — CI 脚本：`buf lint` + `buf breaking`
- `Cargo.toml`（workspace 及各 crate）— Stop Hook 2 检查源
- `server/Cargo.toml` / `agents/Cargo.toml` — 不引入跨 feature 依赖

#### 未决问题
- 无

---
