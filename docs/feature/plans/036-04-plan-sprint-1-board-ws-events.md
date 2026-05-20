# Feature #36 Sprint 1 Board WebSocket Events Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Sprint 1 的 Board WebSocket 在初始全量快照之后，能在发送新 ChatMessage 时推送增量事件，满足 Sprint 1 验收中的“新增消息更新”。

**Architecture:** 继续使用内存 `AppState`，在 `AppState` 内增加 `tokio::sync::broadcast` 事件总线；`add_message` 在非幂等重复写入时发布 `BoardEventKind::MessageAdded`。`rest::board` WebSocket 连接先发送全量 `BoardSnapshot`，随后订阅当前 workspace 的 board event 并发送增量 JSON。

**Spec alignment note:** 本计划已执行并落地，但 WS payload 使用的是 Sprint 1 临时格式：`event_type="full_snapshot" | "message_added"`。最新 `036-05-spec-api.md` 已将正式协议定为 `type="snapshot"|"update" + payload: BoardSnapshotUpdate`；后续 Sprint 1 收尾切片 MUST 迁移到正式协议，避免 Sprint 2 前端绑定临时格式。

**Tech Stack:** Rust、axum WebSocket、tokio broadcast、serde JSON、tower/axum test utilities。

---

## File Structure

- Modify `apps/server/src/model/app.rs`
  - 增加 `BoardEventKind` 与 `BoardUpdate` 类型。
  - `AppState` 持有 broadcast sender。
  - `add_message` 成功新建消息时发布 `MessageAdded`；幂等重复请求不重复发布。
  - 提供 `subscribe_board_updates()` 给 WebSocket 使用。
- Modify `apps/server/src/rest/board.rs`
  - `BoardEvent` 增加 `event_type` 与可选 `message` 字段。
  - WebSocket 初始发送 full snapshot 后，循环转发当前 workspace 的增量事件。
  - 添加测试覆盖 JSON 结构和事件过滤逻辑。
- Modify `docs/feature/active.md`
  - 标记 Sprint 1 已包含 Board WebSocket 新消息增量推送。
- Modify `docs/feature/specs/036-01-plan.md`
  - 更新 Sprint 1 当前落地状态。

---

## Task 1: AppState board event bus

**Files:**
- Modify: `apps/server/src/model/app.rs`

- [ ] **Step 1: Add event bus types and sender field**

Add imports:
```rust
use tokio::sync::broadcast;
```

Add types after `MessageAnalysis`:
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoardEventKind {
    MessageAdded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardUpdate {
    pub workspace_id: String,
    pub event_kind: BoardEventKind,
    pub message: ChatMessage,
}
```

Change `AppState` and `Default`:
```rust
#[derive(Clone, Debug)]
pub struct AppState {
    inner: Arc<Mutex<StoreInner>>,
    board_events: broadcast::Sender<BoardUpdate>,
}

impl Default for AppState {
    fn default() -> Self {
        let (board_events, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(Mutex::new(StoreInner::default())),
            board_events,
        }
    }
}
```

- [ ] **Step 2: Add failing tests**

Add tests in `#[cfg(test)] mod tests`:
```rust
#[test]
fn test_add_message_publishes_board_update() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "General".into())
        .expect("chat created");
    let mut updates = state.subscribe_board_updates();

    let result = state
        .add_message(
            &workspace.id,
            &chat.id,
            "user".into(),
            "hello".into(),
            "k1".into(),
        )
        .expect("message added");

    let update = updates.try_recv().expect("update published");
    assert_eq!(update.workspace_id, workspace.id);
    assert_eq!(update.event_kind, BoardEventKind::MessageAdded);
    assert_eq!(update.message.id, result.message.id);
}

#[test]
fn test_add_message_deduplicated_request_does_not_publish_board_update() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "General".into())
        .expect("chat created");

    state
        .add_message(
            &workspace.id,
            &chat.id,
            "user".into(),
            "hello".into(),
            "k1".into(),
        )
        .expect("message added");
    let mut updates = state.subscribe_board_updates();

    let result = state
        .add_message(
            &workspace.id,
            &chat.id,
            "user".into(),
            "hello again".into(),
            "k1".into(),
        )
        .expect("message deduplicated");

    assert!(result.deduplicated);
    assert!(updates.try_recv().is_err());
}
```

Run:
```bash
cargo test -p server model::app::tests::test_add_message_publishes_board_update model::app::tests::test_add_message_deduplicated_request_does_not_publish_board_update
```

Expected: FAIL because `subscribe_board_updates` does not exist and `add_message` does not publish.

- [ ] **Step 3: Implement subscription and publishing**

Add method in `impl AppState`:
```rust
pub fn subscribe_board_updates(&self) -> broadcast::Receiver<BoardUpdate> {
    self.board_events.subscribe()
}
```

In `add_message`, after inserting the message and before return:
```rust
let _ = self.board_events.send(BoardUpdate {
    workspace_id: workspace_id.to_string(),
    event_kind: BoardEventKind::MessageAdded,
    message: message.clone(),
});
```

Do not send in the idempotency branch.

Run:
```bash
cargo test -p server model::app::tests
```

Expected: PASS.

---

## Task 2: Board WebSocket incremental event payload

**Files:**
- Modify: `apps/server/src/rest/board.rs`

- [ ] **Step 1: Import update types and add payload fields**

Change import:
```rust
use crate::model::app::{AppState, BoardEventKind, BoardUpdate, ChatMessage};
```

Change `BoardEvent`:
```rust
#[derive(Serialize)]
struct BoardEvent {
    event_type: &'static str,
    snapshot: Option<BoardSnapshot>,
    message: Option<ChatMessage>,
}
```

Add helper:
```rust
fn event_type(update: &BoardUpdate) -> &'static str {
    match update.event_kind {
        BoardEventKind::MessageAdded => "message_added",
    }
}
```

- [ ] **Step 2: Add JSON shape tests**

Add tests:
```rust
#[test]
fn test_full_snapshot_event_serializes_event_type() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let snapshot = build_snapshot(&state, &workspace.id).expect("snapshot built");

    let payload = serde_json::to_string(&BoardEvent {
        event_type: "full_snapshot",
        snapshot: Some(snapshot),
        message: None,
    })
    .expect("event serialized");

    assert!(payload.contains("\"event_type\":\"full_snapshot\""));
    assert!(payload.contains("\"snapshot\""));
}

#[test]
fn test_message_added_event_serializes_message() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "General".into())
        .expect("chat created");
    let result = state
        .add_message(&workspace.id, &chat.id, "user".into(), "hello".into(), "k1".into())
        .expect("message added");

    let payload = serde_json::to_string(&BoardEvent {
        event_type: "message_added",
        snapshot: None,
        message: Some(result.message),
    })
    .expect("event serialized");

    assert!(payload.contains("\"event_type\":\"message_added\""));
    assert!(payload.contains("\"content\":\"hello\""));
}
```

Run:
```bash
cargo test -p server rest::board::tests::test_full_snapshot_event_serializes_event_type rest::board::tests::test_message_added_event_serializes_message
```

Expected: PASS after payload type update.

- [ ] **Step 3: Update websocket handler to forward updates**

Replace `board_websocket` body with:
```rust
async fn board_websocket(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |mut socket| async move {
        let mut updates = state.subscribe_board_updates();
        if let Ok(snapshot) = build_snapshot(&state, &workspace_id) {
            let event = BoardEvent {
                event_type: "full_snapshot",
                snapshot: Some(snapshot),
                message: None,
            };
            if let Ok(payload) = serde_json::to_string(&event) {
                if socket
                    .send(axum::extract::ws::Message::Text(payload.into()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }

        while let Ok(update) = updates.recv().await {
            if update.workspace_id != workspace_id {
                continue;
            }
            let event = BoardEvent {
                event_type: event_type(&update),
                snapshot: None,
                message: Some(update.message),
            };
            if let Ok(payload) = serde_json::to_string(&event) {
                if socket
                    .send(axum::extract::ws::Message::Text(payload.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    })
}
```

Run:
```bash
cargo test -p server rest::board
```

Expected: PASS.

---

## Task 3: Documentation, verification and commit

**Files:**
- Modify: `docs/feature/active.md`
- Modify: `docs/feature/specs/036-01-plan.md`

- [ ] **Step 1: Update docs**

In `docs/feature/active.md`, update #36 row to mention `Board WebSocket 新消息增量推送`.

In `docs/feature/specs/036-01-plan.md`, update Sprint 1 current status sentence:
```markdown
**当前落地状态**：已完成内存版 Workspace/Chat/ChatMessage store、REST Workspace/Chat/Message、PATCH Chat、AnalyzeMessage、WS 初始 BoardSnapshot 与新消息增量推送、ChatService/BoardService gRPC 骨架、gateway compose 配置与 idempotency_key 去重；MongoDB 持久化和 Watch 断线恢复语义留到后续切片。
```

- [ ] **Step 2: Full verification**

Run:
```bash
cargo test -p server
cargo test -p agents
cargo check
docker compose -f infra/deploy/docker-compose.dev.yaml config >/tmp/aemeath-sprint1-board-events-compose.out
```

Expected: PASS. Existing CLI MCP unused warnings are acceptable if unchanged.

- [ ] **Step 3: Commit**

Run:
```bash
git add -A
git commit -m "feat(#36): stream board message updates"
```

Expected: commit succeeds and `git status --short --branch` is clean.
