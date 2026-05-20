# Feature #36 Sprint 1 Complete API Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 补齐 Sprint 1 剩余后端 API 缺口，使 Workspace/Chat/Message/Board 的 REST、WS、gRPC 切片覆盖 plan 中列出的最小交付项。

**Architecture:** 继续使用当前内存 `AppState` 作为 Sprint 1 可验证 store，不引入 MongoDB 持久化；将 Chat 更新、消息分类、board 增量事件先做成进程内可测试逻辑。Gateway 仅提供本地开发反向代理配置，不引入运行时鉴权。

**Spec alignment note:** 本计划已执行并落地。REST `analyze` 已补入 `036-05-spec-api.md` 作为调试/前端辅助端点，正式 Agent 间通信仍以 gRPC `ChatService.AnalyzeMessage` 为准。本文使用 `Chat` 命名；DDD 语义中的 `Conversation` 在 #36 v0.1 中与 `Chat` 等价。

**Tech Stack:** Rust、axum REST/WS、tonic gRPC、prost proto、tokio broadcast、nginx config、Docker Compose。

---

## Task 1: 补齐 Chat Update 与 AnalyzeMessage proto

**Files:**
- Modify: `packages/proto/chat.proto`
- Modify: `apps/server/build.rs`

- [ ] **Step 1: 修改 chat.proto**

Add RPCs to `service ChatService`:
```proto
  rpc UpdateChat(UpdateChatRequest) returns (UpdateChatResponse);
  rpc AnalyzeMessage(AnalyzeMessageRequest) returns (AnalyzeMessageResponse);
```

Add messages:
```proto
message UpdateChatRequest {
  string workspace_id = 1;
  string chat_id = 2;
  optional string title = 3;
  optional string status = 4;
}

message UpdateChatResponse {
  Chat chat = 1;
}

message AnalyzeMessageRequest {
  string workspace_id = 1;
  string chat_id = 2;
  string content = 3;
}

message AnalyzeMessageResponse {
  string message_type = 1;
  string reason = 2;
}
```

Expected: generated server trait requires `update_chat` and `analyze_message` implementations.

- [ ] **Step 2: Verify proto compilation fails until implementation**

Run:
```bash
cargo check -p server
```

Expected: FAIL because `ChatGrpc` does not implement new RPCs.

## Task 2: 实现 AppState Chat 更新、消息分类和 board event bus

**Files:**
- Modify: `apps/server/src/model/app.rs`

- [ ] **Step 1: Add tests first**

Add tests at bottom of `apps/server/src/model/app.rs`:
```rust
#[test]
fn test_update_chat_changes_title_and_version() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "Old".into())
        .expect("chat created");

    let updated = state
        .update_chat(&workspace.id, &chat.id, Some("New".into()), None)
        .expect("chat updated");

    assert_eq!(updated.title, "New");
    assert_eq!(updated.version, 2);
}

#[test]
fn test_analyze_message_classifies_requirement() {
    let analysis = analyze_message_type("请实现一个新功能");

    assert_eq!(analysis.message_type, "requirement");
}

#[test]
fn test_analyze_message_classifies_feedback() {
    let analysis = analyze_message_type("这里有个 bug 需要修复");

    assert_eq!(analysis.message_type, "feedback");
}
```

Run:
```bash
cargo test -p server model::app::tests::test_update_chat_changes_title_and_version model::app::tests::test_analyze_message_classifies_requirement model::app::tests::test_analyze_message_classifies_feedback
```

Expected: FAIL because functions do not exist.

- [ ] **Step 2: Implement minimal logic**

Add public types/methods:
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageAnalysis {
    pub message_type: String,
    pub reason: String,
}

impl AppState {
    pub fn update_chat(
        &self,
        workspace_id: &str,
        chat_id: &str,
        title: Option<String>,
        status: Option<String>,
    ) -> Result<Chat, StoreError> { ... }
}

pub fn analyze_message_type(content: &str) -> MessageAnalysis { ... }
```

Classification rules:
- content contains `bug` / `错误` / `修复` / `失败` => `feedback`
- content contains `实现` / `新增` / `创建` / `需求` / `feature` => `requirement`
- otherwise => `chitchat`

Run tests again. Expected: PASS.

## Task 3: 补齐 REST PATCH Chat 与 analyze endpoint

**Files:**
- Modify: `apps/server/src/rest/chat.rs`

- [ ] **Step 1: Add REST tests**

Add tests for:
- `PATCH /api/workspaces/:workspace_id/chats/:chat_id` updates title
- `POST /api/workspaces/:workspace_id/chats/:chat_id/analyze` returns `message_type`

Expected failing tests before route implementation.

- [ ] **Step 2: Implement routes**

Add request types:
```rust
#[derive(Deserialize)]
struct UpdateChatRequest {
    title: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct AnalyzeMessageRequest {
    content: String,
}

#[derive(Serialize)]
struct AnalyzeMessageResponse {
    message_type: String,
    reason: String,
}
```

Update route:
```rust
.route(
    "/api/workspaces/{workspace_id}/chats/{chat_id}",
    get(get_chat).patch(update_chat).delete(delete_chat),
)
.route(
    "/api/workspaces/{workspace_id}/chats/{chat_id}/analyze",
    post(analyze_message),
)
```

Run:
```bash
cargo test -p server rest::chat
```

Expected: PASS.

## Task 4: 实现 gRPC UpdateChat 与 AnalyzeMessage

**Files:**
- Modify: `apps/server/src/grpc/chat.rs`

- [ ] **Step 1: Implement required trait methods**

Add imports for `UpdateChatRequest`, `UpdateChatResponse`, `AnalyzeMessageRequest`, `AnalyzeMessageResponse`.

Implement:
```rust
async fn update_chat(...)
async fn analyze_message(...)
```

Run:
```bash
cargo test -p server grpc::chat
```

Expected: PASS.

## Task 5: 增加 nginx gateway 配置

**Files:**
- Create: `infra/gateway/nginx.conf`
- Modify: `infra/deploy/docker-compose.dev.yaml`

- [ ] **Step 1: Add nginx.conf**

Content:
```nginx
events {}

http {
  upstream aemeath_http {
    server server:3000;
  }

  upstream aemeath_grpc {
    server server:50051;
  }

  server {
    listen 8080;

    location /api/ {
      proxy_pass http://aemeath_http;
      proxy_http_version 1.1;
      proxy_set_header Host $host;
      proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws/ {
      proxy_pass http://aemeath_http;
      proxy_http_version 1.1;
      proxy_set_header Upgrade $http_upgrade;
      proxy_set_header Connection "upgrade";
      proxy_set_header Host $host;
    }

    location /grpc/ {
      grpc_pass grpc://aemeath_grpc;
    }
  }
}
```

- [ ] **Step 2: Add gateway service to compose**

Add service:
```yaml
  gateway:
    image: nginx:1.27-alpine
    depends_on:
      - server
    ports:
      - "8080:8080"
    volumes:
      - ../gateway/nginx.conf:/etc/nginx/nginx.conf:ro
```

Run:
```bash
docker compose -f infra/deploy/docker-compose.dev.yaml config >/tmp/aemeath-sprint1-gateway-config.out
```

Expected: PASS.

## Task 6: Update docs, verify, commit

**Files:**
- Modify: `docs/feature/active.md`
- Modify: `docs/feature/specs/036-01-plan.md`

- [ ] **Step 1: Update feature status**

Mention Sprint 1 now includes PATCH Chat, AnalyzeMessage and gateway config.

- [ ] **Step 2: Full verification**

Run:
```bash
cargo test -p server
cargo test -p agents
cargo check
docker compose -f infra/deploy/docker-compose.dev.yaml config >/tmp/aemeath-sprint1-gateway-config.out
```

Expected: PASS.

- [ ] **Step 3: Commit**

Run:
```bash
git add -A
git commit -m "feat(#36): complete sprint 1 chat api surface"
```

Expected: commit succeeds.
