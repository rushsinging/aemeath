use crate::model::app::AppState;
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
struct BoardEvent {
    snapshot: BoardSnapshot,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/ws/workspaces/{workspace_id}/board",
            get(board_websocket),
        )
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
        if let Ok(snapshot) = build_snapshot(&state, &workspace_id) {
            let event = BoardEvent { snapshot };
            if let Ok(payload) = serde_json::to_string(&event) {
                let _ = socket.send(axum::extract::ws::Message::Text(payload.into())).await;
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

fn uuid_like_snapshot_id() -> String {
    mongodb::bson::oid::ObjectId::new().to_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

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
