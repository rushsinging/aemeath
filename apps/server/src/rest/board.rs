use crate::model::app::{AppState, BoardUpdate, Chat, ChatMessage};
use axum::{
    Json, Router,
    extract::{Path, State, ws::WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use serde::Serialize;

#[derive(Serialize)]
struct WorkspaceInfo {
    id: String,
    name: String,
    provider: String,
    model: String,
}

#[derive(Serialize)]
struct BoardSnapshot {
    snapshot_id: String,
    workspace_id: String,
    workspace: WorkspaceInfo,
    chats: Vec<crate::model::app::Chat>,
    recent_messages: Vec<crate::model::app::ChatMessage>,
    is_full_snapshot: bool,
}

#[derive(Serialize)]
struct BoardSnapshotUpdate {
    snapshot_id: String,
    changed_workspace: Option<WorkspaceInfo>,
    is_full_snapshot: bool,
    timestamp: i64,
    changed_chats: Vec<Chat>,
    removed_chat_ids: Vec<String>,
    new_messages: Vec<ChatMessage>,
    updated_messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct BoardEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    payload: BoardSnapshotUpdate,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ws/workspaces/{workspace_id}/board", get(board_websocket))
        .route(
            "/api/workspaces/{workspace_id}/board/snapshot",
            get(get_board_snapshot),
        )
        .with_state(state)
}

async fn get_board_snapshot(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
) -> Result<Json<BoardSnapshot>, axum::http::StatusCode> {
    build_snapshot(&state, &workspace_id)
        .map(Json)
        .map_err(|_| axum::http::StatusCode::NOT_FOUND)
}

async fn board_websocket(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |mut socket| async move {
        let mut updates = state.subscribe_board_updates();
        if let Ok(snapshot) = build_snapshot(&state, &workspace_id) {
            let event = BoardEvent {
                event_type: "snapshot",
                payload: BoardSnapshotUpdate::from_snapshot(snapshot),
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
                event_type: "update",
                payload: BoardSnapshotUpdate::from_board_update(update),
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

fn build_snapshot(
    state: &AppState,
    workspace_id: &str,
) -> Result<BoardSnapshot, crate::model::app::StoreError> {
    let workspace = state.get_workspace(workspace_id)?;
    Ok(BoardSnapshot {
        snapshot_id: uuid_like_snapshot_id(),
        workspace_id: workspace_id.to_string(),
        workspace: WorkspaceInfo {
            id: workspace.id,
            name: workspace.name,
            provider: workspace.provider,
            model: workspace.model,
        },
        chats: state.list_chats(workspace_id),
        recent_messages: state.list_recent_messages(workspace_id),
        is_full_snapshot: true,
    })
}

impl BoardSnapshotUpdate {
    fn from_snapshot(snapshot: BoardSnapshot) -> Self {
        Self {
            snapshot_id: snapshot.snapshot_id,
            changed_workspace: Some(snapshot.workspace),
            is_full_snapshot: true,
            timestamp: now_millis(),
            changed_chats: snapshot.chats,
            removed_chat_ids: Vec::new(),
            new_messages: snapshot.recent_messages,
            updated_messages: Vec::new(),
        }
    }

    fn from_board_update(update: BoardUpdate) -> Self {
        Self {
            snapshot_id: uuid_like_snapshot_id(),
            changed_workspace: None,
            is_full_snapshot: false,
            timestamp: now_millis(),
            changed_chats: Vec::new(),
            removed_chat_ids: Vec::new(),
            new_messages: vec![update.message],
            updated_messages: Vec::new(),
        }
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64
}

fn uuid_like_snapshot_id() -> String {
    mongodb::bson::oid::ObjectId::new().to_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_snapshot_event_serializes_event_type() {
        let state = AppState::default();
        let workspace = state
            .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
            .expect("workspace created");
        let snapshot = build_snapshot(&state, &workspace.id).expect("snapshot built");

        let payload = serde_json::to_string(&BoardEvent {
            event_type: "snapshot",
            payload: BoardSnapshotUpdate::from_snapshot(snapshot),
        })
        .expect("event serialized");

        assert!(payload.contains("\"type\":\"snapshot\""));
        assert!(payload.contains("\"payload\""));
        assert!(payload.contains("\"is_full_snapshot\":true"));
        assert!(!payload.contains("event_type"));
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
            .add_message(
                &workspace.id,
                &chat.id,
                "user".into(),
                "hello".into(),
                "k1".into(),
            )
            .expect("message added");

        let payload = serde_json::to_string(&BoardEvent {
            event_type: "update",
            payload: BoardSnapshotUpdate::from_board_update(BoardUpdate {
                workspace_id: workspace.id.clone(),
                event_kind: crate::model::app::BoardEventKind::MessageAdded,
                message: result.message,
            }),
        })
        .expect("event serialized");

        assert!(payload.contains("\"type\":\"update\""));
        assert!(payload.contains("\"new_messages\""));
        assert!(payload.contains("\"content\":\"hello\""));
        assert!(payload.contains("\"is_full_snapshot\":false"));
        assert!(!payload.contains("event_type"));
    }

    #[test]
    fn test_build_snapshot_returns_workspace_chats_and_messages() {
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

        let snapshot = build_snapshot(&state, &workspace.id).expect("snapshot built");

        assert!(snapshot.is_full_snapshot);
        assert_eq!(snapshot.chats.len(), 1);
        assert_eq!(snapshot.recent_messages.len(), 1);
    }

    #[test]
    fn test_build_snapshot_rejects_missing_workspace() {
        let result = build_snapshot(&AppState::default(), "missing");

        assert!(result.is_err());
    }
}
