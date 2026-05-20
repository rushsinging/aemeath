# Feature #36 Sprint 0 基础设施实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 搭建 #36 多 Agent 框架的可编译、可启动、可验证空架子，为 Sprint 1 的 Workspace/Chat API 提供目录、配置、proto、MongoDB 和 Agent role 基础设施。

**Architecture:** Sprint 0 只建立骨架，不实现业务 CRUD。`server/` 提供 axum REST/WS 监听 `3000` 与 tonic gRPC 监听 `50051` 的启动框架；`share/proto/` 管理跨 server/agents/ui 共享 proto；`infra/` 管理 MongoDB replica set 与本地开发 compose；`agents/` 管理 Main Agent/Sub-Agent role 与配置装配器骨架。

**Tech Stack:** Rust workspace、tonic 0.12、prost 0.13、axum 0.8、mongodb 3.x、tokio、serde、toml、Docker Compose、MongoDB replica set。

---

## 对齐的 spec 决策

- 目录采用 `share/`、`server/`、`agents/`、`infra/`，不使用根目录 `proto/` 或 `aemeath-server/`。
- MVP 端口采用拆端口：REST/WS `0.0.0.0:3000`，gRPC `0.0.0.0:50051`。
- WebSocket / Board `snapshot_id` 统一使用 UUIDv7 字符串，不使用 ULID。
- Sprint 0 只做基础设施；Workspace/Chat CRUD、BoardSnapshot 数据组装、idempotency_records 去重属于 Sprint 1。
- 业务代码不得直接读取环境变量；环境变量只能在配置加载边界读取。

## 文件结构

### 新建/修改文件

| 路径 | 责任 |
|------|------|
| `Cargo.toml` | workspace 增加 `server`、`agents` 成员 |
| `share/proto/common.proto` | Common proto：Empty、EventType、AgentType、TokenScope、WatchRequest、IdempotencyOptions |
| `share/proto/workspace.proto` | Sprint 0 仅定义 WorkspaceService 空骨架所需最小消息 |
| `share/proto/agent.proto` | AgentRegistryService 骨架 proto |
| `share/openapi/sdk/ts/package.json` | TypeScript SDK 空壳 |
| `server/Cargo.toml` | server crate 依赖 |
| `server/build.rs` | 使用 `tonic-build` 编译 `share/proto/*.proto` |
| `server/src/lib.rs` | server crate 模块导出 |
| `server/src/config.rs` | ServerConfig 配置分层与测试 |
| `server/src/proto.rs` | `tonic::include_proto!` 统一导出 |
| `server/src/rest/mod.rs` | axum router 聚合 |
| `server/src/rest/health.rs` | `GET /healthz` |
| `server/src/grpc/mod.rs` | gRPC service 聚合骨架 |
| `server/src/grpc/agent.rs` | AgentRegistryService tonic 骨架 |
| `server/src/db.rs` | MongoDB client/database 初始化骨架 |
| `server/src/main.rs` | 并发启动 REST/WS 与 gRPC 两个 listener |
| `agents/Cargo.toml` | agents crate 依赖 |
| `agents/src/lib.rs` | agents crate 模块导出 |
| `agents/src/config.rs` | RoleConfig 与 TOML 装配器 |
| `agents/src/features/*/mod.rs` | Chat/Assistant/Scheduler/Executor/Evolver/Sub-Agent feature 空模块 |
| `agents/roles/*.toml` | 5 个 Main Agent + 5 个 Sub-Agent role 配置骨架 |
| `infra/mongodb/init.js` | collection 与 index 初始化，幂等可重复执行 |
| `infra/deploy/docker-compose.dev.yaml` | MongoDB replica set + server 本地开发编排 |
| `infra/deploy/Dockerfile.server` | server 开发镜像 |
| `docs/feature/specs/036-01-plan.md` | 保持 Sprint 0 决策一致：拆端口、share/proto |
| `docs/feature/specs/036-02-spec-architecture.md` | 保持端口、crate、proto 路径一致 |
| `docs/feature/specs/036-05-spec-api.md` | 保持 snapshot_id UUIDv7 一致 |

## Task 1: 清理偏离 spec 的早期骨架

**Files:**
- Delete: `aemeath-server/`
- Move: `proto/*.proto` → `share/proto/*.proto`
- Modify: `Cargo.toml`

- [ ] **Step 1: 检查当前偏离项**

Run:
```bash
git status --short --untracked-files=all
```

Expected: 能看到当前未提交的 `aemeath-server/`、根目录 `proto/`、`Cargo.toml`、`Cargo.lock`。

- [ ] **Step 2: 创建目标目录并迁移 proto**

Run:
```bash
mkdir -p share/proto
mv proto/*.proto share/proto/
rmdir proto
```

Expected: `share/proto/common.proto` 等文件存在，根目录 `proto/` 不存在。

- [ ] **Step 3: 将 crate 目录从 `aemeath-server` 改为 `server`**

Run:
```bash
mv aemeath-server server
```

Expected: `server/Cargo.toml` 存在，`aemeath-server/` 不存在。

- [ ] **Step 4: 修改 workspace members**

Modify `Cargo.toml`：
```toml
members = [
    "aemeath-core",
    "aemeath-llm",
    "aemeath-tools",
    "aemeath-cli",
    "server",
    "agents",
]
```

Expected: workspace member 使用 `server`，不再引用 `aemeath-server`。

- [ ] **Step 5: 更新 server package 名称**

Modify `server/Cargo.toml`：
```toml
[package]
name = "server"
version = "0.1.0"
edition = "2024"
```

Expected: `cargo metadata --no-deps` 能识别 package `server`。

- [ ] **Step 6: 验证迁移后当前 server 测试仍可运行**

Run:
```bash
cargo test -p server
```

Expected: 现有 config/id/health/proto 测试通过。若 build.rs 仍引用 `../proto`，此步会失败，进入 Task 2 修正。

## Task 2: 修正 share/proto 与 proto 编译骨架

**Files:**
- Modify: `server/build.rs`
- Modify: `server/src/proto.rs`
- Modify: `share/proto/common.proto`
- Create/Modify: `share/proto/agent.proto`
- Modify: `share/proto/workspace.proto`

- [ ] **Step 1: 写/确认 `common.proto` 内容**

Modify `share/proto/common.proto`：
```protobuf
syntax = "proto3";

package aemeath.v1;

message Empty {}

enum EventType {
  EVENT_TYPE_UNSPECIFIED = 0;
  EVENT_TYPE_CREATED = 1;
  EVENT_TYPE_UPDATED = 2;
  EVENT_TYPE_DELETED = 3;
}

enum AgentType {
  AGENT_TYPE_UNSPECIFIED = 0;
  AGENT_TYPE_CHAT = 1;
  AGENT_TYPE_SCHEDULER = 2;
  AGENT_TYPE_EXECUTOR = 3;
  AGENT_TYPE_ASSISTANT = 4;
  AGENT_TYPE_EVOLVER = 5;
  AGENT_TYPE_SUB_AGENT = 6;
}

enum TokenScope {
  TOKEN_SCOPE_UNSPECIFIED = 0;
  TOKEN_SCOPE_BOARD_READ = 1;
  TOKEN_SCOPE_BOARD_WRITE = 2;
  TOKEN_SCOPE_AGENT_REGISTRY = 3;
}

message IdempotencyOptions {
  string key = 1;
  string scope = 2;
}

message WatchRequest {
  string workspace_id = 1;
  AgentType agent_type = 2;
  repeated string event_types = 3;
  optional string resume_snapshot_id = 4;
}
```

- [ ] **Step 2: 写 AgentRegistryService proto 骨架**

Create `share/proto/agent.proto`：
```protobuf
syntax = "proto3";

package aemeath.v1;

import "common.proto";

service AgentRegistryService {
  rpc Register(RegisterAgentRequest) returns (RegisterAgentResponse);
  rpc Heartbeat(HeartbeatRequest) returns (HeartbeatResponse);
  rpc Deregister(DeregisterAgentRequest) returns (Empty);
}

message RegisterAgentRequest {
  string workspace_id = 1;
  string role = 2;
  AgentType agent_type = 3;
}

message RegisterAgentResponse {
  string agent_id = 1;
  string token = 2;
}

message HeartbeatRequest {
  string agent_id = 1;
  string workspace_id = 2;
}

message HeartbeatResponse {
  bool ok = 1;
}

message DeregisterAgentRequest {
  string agent_id = 1;
  string workspace_id = 2;
}
```

- [ ] **Step 3: 写 WorkspaceService 最小 proto 骨架**

Modify `share/proto/workspace.proto`：
```protobuf
syntax = "proto3";

package aemeath.v1;

import "common.proto";

service WorkspaceService {
  rpc Watch(WatchRequest) returns (stream WorkspaceEvent);
}

message Workspace {
  string id = 1;
  string tenant_id = 2;
  string name = 3;
  string provider = 4;
  string model = 5;
  int64 created_at = 6;
  int64 updated_at = 7;
  uint64 version = 8;
}

message WorkspaceEvent {
  EventType event_type = 1;
  optional Workspace entity = 2;
}
```

- [ ] **Step 4: 修正 `server/build.rs`**

Modify `server/build.rs`：
```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure().build_server(true).compile_protos(
        &[
            "../share/proto/common.proto",
            "../share/proto/workspace.proto",
            "../share/proto/agent.proto",
        ],
        &["../share/proto"],
    )?;
    Ok(())
}
```

- [ ] **Step 5: 编译验证**

Run:
```bash
cargo check -p server
```

Expected: proto 编译通过，无 `No such file or directory`。

## Task 3: Server 配置与双 listener 启动骨架

**Files:**
- Modify: `server/src/config.rs`
- Create: `server/src/main.rs`
- Create: `server/src/grpc/mod.rs`
- Create: `server/src/grpc/agent.rs`
- Modify: `server/src/rest/mod.rs`
- Modify: `server/src/rest/health.rs`

- [ ] **Step 1: 扩展配置测试覆盖拆端口**

Add tests in `server/src/config.rs`:
```rust
#[test]
fn test_server_config_defaults_use_split_ports() {
    let config = ServerConfig::default();

    assert_eq!(config.http_addr, "0.0.0.0:3000");
    assert_eq!(config.grpc_addr, "0.0.0.0:50051");
}
```

Run:
```bash
cargo test -p server test_server_config_defaults_use_split_ports -- --nocapture
```

Expected: PASS。

- [ ] **Step 2: 写 gRPC AgentRegistryService 骨架**

Create `server/src/grpc/mod.rs`：
```rust
pub mod agent;
```

Create `server/src/grpc/agent.rs`：
```rust
use crate::proto::aemeath::v1::agent_registry_service_server::AgentRegistryService;
use crate::proto::aemeath::v1::{
    DeregisterAgentRequest, Empty, HeartbeatRequest, HeartbeatResponse, RegisterAgentRequest,
    RegisterAgentResponse,
};
use tonic::{Request, Response, Status};

#[derive(Debug, Default)]
pub struct AgentRegistryGrpc;

#[tonic::async_trait]
impl AgentRegistryService for AgentRegistryGrpc {
    async fn register(
        &self,
        _request: Request<RegisterAgentRequest>,
    ) -> Result<Response<RegisterAgentResponse>, Status> {
        Err(Status::unimplemented("AgentRegistryService.Register 尚未实现"))
    }

    async fn heartbeat(
        &self,
        _request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        Err(Status::unimplemented("AgentRegistryService.Heartbeat 尚未实现"))
    }

    async fn deregister(
        &self,
        _request: Request<DeregisterAgentRequest>,
    ) -> Result<Response<Empty>, Status> {
        Err(Status::unimplemented("AgentRegistryService.Deregister 尚未实现"))
    }
}
```

- [ ] **Step 3: 导出 grpc 模块**

Modify `server/src/lib.rs`：
```rust
pub mod config;
pub mod grpc;
pub mod model;
pub mod proto;
pub mod rest;
```

- [ ] **Step 4: 写 main 双 listener 骨架**

Create `server/src/main.rs`：
```rust
use server::config::ServerConfig;
use server::grpc::agent::AgentRegistryGrpc;
use server::proto::aemeath::v1::agent_registry_service_server::AgentRegistryServiceServer;
use std::net::SocketAddr;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::load()?;
    let http_addr: SocketAddr = config.http_addr.parse()?;
    let grpc_addr: SocketAddr = config.grpc_addr.parse()?;

    let rest = axum::serve(
        tokio::net::TcpListener::bind(http_addr).await?,
        server::rest::router(),
    );

    let grpc = Server::builder()
        .add_service(AgentRegistryServiceServer::new(AgentRegistryGrpc::default()))
        .serve(grpc_addr);

    tokio::try_join!(rest, grpc)?;
    Ok(())
}
```

- [ ] **Step 5: 编译验证**

Run:
```bash
cargo check -p server
```

Expected: 编译通过。若 `ServerConfig::load` 缺失，则实现只在 config 模块读取环境变量。

## Task 4: MongoDB infra 与 db 骨架

**Files:**
- Create: `server/src/db.rs`
- Modify: `server/src/lib.rs`
- Create: `infra/mongodb/init.js`
- Create: `infra/deploy/docker-compose.dev.yaml`
- Create: `infra/deploy/Dockerfile.server`

- [ ] **Step 1: 写 MongoDB collection/index 初始化脚本**

Create `infra/mongodb/init.js`：
```js
const database = db.getSiblingDB("aemeath");

const collections = [
  "workspaces",
  "chats",
  "chat_messages",
  "requirements",
  "projects",
  "project_tasks",
  "project_task_results",
  "agent_instances",
  "agent_heartbeats",
  "idempotency_records",
  "board_index",
  "reflections"
];

for (const name of collections) {
  if (!database.getCollectionNames().includes(name)) {
    database.createCollection(name);
  }
}

database.workspaces.createIndex({ tenant_id: 1, created_at: -1 });
database.chats.createIndex({ workspace_id: 1, created_at: -1 });
database.chat_messages.createIndex({ workspace_id: 1, chat_id: 1, created_at: -1 });
database.requirements.createIndex({ workspace_id: 1, status: 1, updated_at: -1 });
database.projects.createIndex({ workspace_id: 1, status: 1, updated_at: -1 });
database.project_tasks.createIndex({ workspace_id: 1, project_id: 1, status: 1 });
database.agent_instances.createIndex({ workspace_id: 1, role: 1, status: 1 });
database.agent_heartbeats.createIndex({ agent_id: 1 }, { unique: true });
database.idempotency_records.createIndex({ created_at: 1 }, { expireAfterSeconds: 3600 });
database.idempotency_records.createIndex({ key: 1, entity_type: 1, scope: 1 }, { unique: true });
database.board_index.createIndex({ snapshot_id: 1 }, { unique: true });
```

- [ ] **Step 2: 写 docker-compose.dev.yaml**

Create `infra/deploy/docker-compose.dev.yaml`：
```yaml
services:
  mongodb:
    image: mongo:7
    command: ["mongod", "--replSet", "rs0", "--bind_ip_all"]
    ports:
      - "27017:27017"
    volumes:
      - mongodb_data:/data/db
      - ../mongodb/init.js:/docker-entrypoint-initdb.d/init.js:ro
    healthcheck:
      test: ["CMD", "mongosh", "--quiet", "--eval", "try { rs.status().ok } catch (e) { rs.initiate({_id:'rs0', members:[{_id:0, host:'localhost:27017'}]}).ok }"]
      interval: 5s
      timeout: 5s
      retries: 20

  server:
    build:
      context: ../..
      dockerfile: infra/deploy/Dockerfile.server
    environment:
      AEMEATH_SERVER_MONGO_URI: mongodb://mongodb:27017/?replicaSet=rs0
      AEMEATH_SERVER_DB: aemeath
      AEMEATH_SERVER_HTTP_ADDR: 0.0.0.0:3000
      AEMEATH_SERVER_GRPC_ADDR: 0.0.0.0:50051
    ports:
      - "3000:3000"
      - "50051:50051"
    depends_on:
      mongodb:
        condition: service_healthy

volumes:
  mongodb_data:
```

- [ ] **Step 3: 写 server Dockerfile**

Create `infra/deploy/Dockerfile.server`：
```dockerfile
FROM rust:1.85-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build -p server

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/debug/server /usr/local/bin/server
EXPOSE 3000 50051
CMD ["server"]
```

- [ ] **Step 4: 写 db 骨架**

Create `server/src/db.rs`：
```rust
use crate::config::ServerConfig;
use mongodb::{Client, Database, options::ClientOptions};

pub async fn connect(config: &ServerConfig) -> mongodb::error::Result<Database> {
    let options = ClientOptions::parse(&config.mongo_uri).await?;
    let client = Client::with_options(options)?;
    Ok(client.database(&config.mongo_database))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connect_function_is_exposed_for_server_startup() {
        let _: fn(&ServerConfig) -> _ = connect;
    }
}
```

- [ ] **Step 5: 导出 db 模块并验证**

Modify `server/src/lib.rs`：
```rust
pub mod config;
pub mod db;
pub mod grpc;
pub mod model;
pub mod proto;
pub mod rest;
```

Run:
```bash
cargo test -p server test_connect_function_is_exposed_for_server_startup -- --nocapture
```

Expected: PASS。

## Task 5: agents crate 与 RoleConfig 装配器骨架

**Files:**
- Create: `agents/Cargo.toml`
- Create: `agents/src/lib.rs`
- Create: `agents/src/config.rs`
- Create: `agents/src/features/{chat,assistant,scheduler,executor,evolver,sub_agent}/mod.rs`
- Create: `agents/roles/{chat,assistant,scheduler,executor,evolver,planner,coder,tester,reviewer,designer}.toml`

- [ ] **Step 1: 创建 agents crate**

Run:
```bash
cargo new agents --lib
```

Expected: `agents/Cargo.toml` 和 `agents/src/lib.rs` 存在。

- [ ] **Step 2: 写 RoleConfig 解析失败测试**

Create `agents/src/config.rs`：
```rust
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct RoleConfig {
    pub name: String,
    pub description: String,
    pub pool_size: usize,
    pub system_prompt: String,
    pub skills: Vec<String>,
    pub models: Vec<RoleModelConfig>,
    pub permissions: RolePermissions,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct RoleModelConfig {
    pub model: String,
    pub cost_tier: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct RolePermissions {
    pub allowed_tools: Vec<String>,
    pub scope: Vec<String>,
    pub max_subagents: usize,
    pub can_call_roles: Vec<String>,
    pub can_create_agents: bool,
}

pub fn parse_role_config(content: &str) -> Result<RoleConfig, toml::de::Error> {
    toml::from_str(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_role_config_reads_flat_fields_and_permissions() {
        let config = parse_role_config(
            r#"
name = "scheduler"
description = "管理 Agent Pool 生命周期"
pool_size = 0
system_prompt = "调度 Agent"
skills = []

[[models]]
model = "deepseek/deepseek-chat"
cost_tier = "low"

[permissions]
allowed_tools = []
scope = ["agent_registry", "board_read", "board_write"]
max_subagents = 0
can_call_roles = ["assistant", "executor"]
can_create_agents = true
"#,
        )
        .expect("role config parses");

        assert_eq!(config.name, "scheduler");
        assert_eq!(config.models[0].cost_tier, "low");
        assert!(config.permissions.can_create_agents);
        assert_eq!(config.permissions.can_call_roles, vec!["assistant", "executor"]);
    }
}
```

- [ ] **Step 3: 配置 agents 依赖**

Modify `agents/Cargo.toml`：
```toml
[package]
name = "agents"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { workspace = true }
toml = "0.8"
```

- [ ] **Step 4: 写 feature 模块骨架**

Create files:
```rust
// agents/src/features/chat/mod.rs
pub const ROLE: &str = "chat";
```
```rust
// agents/src/features/assistant/mod.rs
pub const ROLE: &str = "assistant";
```
```rust
// agents/src/features/scheduler/mod.rs
pub const ROLE: &str = "scheduler";
```
```rust
// agents/src/features/executor/mod.rs
pub const ROLE: &str = "executor";
```
```rust
// agents/src/features/evolver/mod.rs
pub const ROLE: &str = "evolver";
```
```rust
// agents/src/features/sub_agent/mod.rs
pub const PLANNER: &str = "planner";
pub const CODER: &str = "coder";
pub const TESTER: &str = "tester";
pub const REVIEWER: &str = "reviewer";
pub const DESIGNER: &str = "designer";
```

Modify `agents/src/lib.rs`:
```rust
pub mod config;
pub mod features {
    pub mod assistant;
    pub mod chat;
    pub mod evolver;
    pub mod executor;
    pub mod scheduler;
    pub mod sub_agent;
}
```

- [ ] **Step 5: 写 role TOML 骨架**

Create 10 files under `agents/roles/`。每个文件必须包含顶层字段 `name`、`description`、`pool_size`、`system_prompt`、`skills`、`[[models]]`、`[permissions]`。

Example `agents/roles/scheduler.toml`:
```toml
name = "scheduler"
description = "管理 Agent Pool 生命周期，分派任务"
pool_size = 0
system_prompt = "你是 Scheduler Agent，负责 Watch Project/Requirement，管理 Assistant/Executor Pool 并执行调度。"
skills = []

[[models]]
model = "deepseek/deepseek-chat"
cost_tier = "low"

[permissions]
allowed_tools = []
scope = ["agent_registry", "board_read", "board_write"]
max_subagents = 0
can_call_roles = ["assistant", "executor"]
can_create_agents = true
```

- [ ] **Step 6: 验证 agents crate**

Run:
```bash
cargo test -p agents
```

Expected: RoleConfig 解析测试通过。

## Task 6: TypeScript SDK 空壳与日志基建占位

**Files:**
- Create: `share/openapi/sdk/ts/package.json`
- Create: `server/src/logging.rs`
- Modify: `server/src/lib.rs`

- [ ] **Step 1: 创建 SDK package 空壳**

Create `share/openapi/sdk/ts/package.json`：
```json
{
  "name": "@aemeath/sdk",
  "version": "0.0.0",
  "private": true,
  "type": "module",
  "main": "dist/index.js",
  "types": "dist/index.d.ts",
  "scripts": {
    "build": "tsc --noEmit"
  }
}
```

- [ ] **Step 2: 写 logging 配置骨架测试**

Create `server/src/logging.rs`：
```rust
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogPaths {
    pub app_log: PathBuf,
    pub panic_log: PathBuf,
    pub agent_log: PathBuf,
}

pub fn default_log_paths(home: impl Into<PathBuf>) -> LogPaths {
    let base = home.into().join(".aemeath");
    LogPaths {
        app_log: base.join("aemeath.log"),
        panic_log: base.join("panic.log"),
        agent_log: base.join("agent.log"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_log_paths_use_aemeath_directory() {
        let paths = default_log_paths("/tmp/user");

        assert_eq!(paths.app_log, PathBuf::from("/tmp/user/.aemeath/aemeath.log"));
        assert_eq!(paths.panic_log, PathBuf::from("/tmp/user/.aemeath/panic.log"));
        assert_eq!(paths.agent_log, PathBuf::from("/tmp/user/.aemeath/agent.log"));
    }
}
```

- [ ] **Step 3: 导出 logging 模块并验证**

Modify `server/src/lib.rs`：
```rust
pub mod config;
pub mod db;
pub mod grpc;
pub mod logging;
pub mod model;
pub mod proto;
pub mod rest;
```

Run:
```bash
cargo test -p server test_default_log_paths_use_aemeath_directory -- --nocapture
```

Expected: PASS。

## Task 7: Sprint 0 验证与提交

**Files:**
- All changed files

- [ ] **Step 1: 格式化**

Run:
```bash
cargo fmt
```

Expected: Rust 文件格式化成功。

- [ ] **Step 2: 默认测试**

Run:
```bash
cargo test -p server
cargo test -p agents
```

Expected: 全部通过；默认测试不依赖外部 MongoDB。

- [ ] **Step 3: 全量检查**

Run:
```bash
cargo check
```

Expected: workspace 编译通过。

- [ ] **Step 4: Docker compose 配置静态检查**

Run:
```bash
docker compose -f infra/deploy/docker-compose.dev.yaml config
```

Expected: compose 文件解析成功。若本机无 Docker，记录为未执行，不阻塞 Rust 编译提交。

- [ ] **Step 5: 检查 spec/plan 关键词一致性**

Run:
```bash
git grep -n "共享端口\|ULID\|aemeath-server/\|proto/ 目录" docs/feature/specs docs/feature/plans || true
```

Expected: 无结果，或只出现说明历史决策的上下文。

- [ ] **Step 6: 提交**

Run:
```bash
git add Cargo.toml Cargo.lock share server agents infra docs/feature/specs docs/feature/plans
git commit -m "feat(#36): scaffold sprint 0 infrastructure"
```

Expected: commit 成功。

## 自检

- Spec coverage：Sprint 0 的 `share/proto`、`server`、`infra/mongodb`、`infra/deploy`、`agents`、AgentRegistryService 骨架、SDK 空壳、TOML 装配器、Sub-Agent roles、日志路径均有对应 task。
- Placeholder scan：本文不使用 TODO/TBD；未实现的业务 RPC 明确返回 tonic `unimplemented`，属于 Sprint 0 骨架行为。
- Type consistency：server crate 使用 package `server`；proto 路径统一 `share/proto`；REST/WS 端口 `3000`，gRPC 端口 `50051`；snapshot_id 使用 UUIDv7。
