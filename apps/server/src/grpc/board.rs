use crate::model::app::AppState;
use crate::proto::aemeath::v1::board_service_server::BoardService;
use crate::proto::aemeath::v1::{
    BoardEvent, BoardSnapshot, Chat, ChatMessage, GetBoardSnapshotRequest,
    GetBoardSnapshotResponse, WatchRequest, WorkspaceInfo,
};
use std::pin::Pin;
use tokio_stream::{Stream, empty};
use tonic::{Request, Response, Status};

#[derive(Debug, Clone)]
pub struct BoardGrpc {
    state: AppState,
}

impl BoardGrpc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl BoardService for BoardGrpc {
    async fn get_board_snapshot(
        &self,
        request: Request<GetBoardSnapshotRequest>,
    ) -> Result<Response<GetBoardSnapshotResponse>, Status> {
        let request = request.into_inner();
        let snapshot = build_snapshot(&self.state, &request.workspace_id)?;
        Ok(Response::new(GetBoardSnapshotResponse {
            snapshot: Some(snapshot),
        }))
    }

    type WatchStream = Pin<Box<dyn Stream<Item = Result<BoardEvent, Status>> + Send + 'static>>;

    async fn watch(
        &self,
        _request: Request<WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        Ok(Response::new(Box::pin(empty()) as Self::WatchStream))
    }
}

fn build_snapshot(state: &AppState, workspace_id: &str) -> Result<BoardSnapshot, Status> {
    let workspace = state
        .get_workspace(workspace_id)
        .map_err(|_| Status::not_found("workspace 不存在"))?;
    Ok(BoardSnapshot {
        snapshot_id: mongodb::bson::oid::ObjectId::new().to_hex(),
        workspace_id: workspace_id.to_string(),
        workspace: Some(WorkspaceInfo {
            id: workspace.id,
            name: workspace.name,
            provider: workspace.provider,
            model: workspace.model,
        }),
        chats: state
            .list_chats(workspace_id)
            .into_iter()
            .map(Chat::from)
            .collect(),
        recent_messages: state
            .list_recent_messages(workspace_id)
            .into_iter()
            .map(ChatMessage::from)
            .collect(),
        is_full_snapshot: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_board_grpc_returns_snapshot() {
        let state = AppState::default();
        let workspace = state
            .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
            .expect("workspace created");
        let service = BoardGrpc::new(state);

        let response = service
            .get_board_snapshot(Request::new(GetBoardSnapshotRequest {
                workspace_id: workspace.id,
            }))
            .await
            .expect("snapshot returned")
            .into_inner();

        assert!(response.snapshot.expect("snapshot exists").is_full_snapshot);
    }
}
