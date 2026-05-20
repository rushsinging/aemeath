use crate::model::app::{
    analyze_message_type, AppState, Chat, ChatMessage, StoreError, Workspace,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct CreateWorkspaceRequest {
    tenant_id: Option<String>,
    name: String,
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct UpdateWorkspaceRequest {
    name: Option<String>,
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct CreateChatRequest {
    title: String,
}

#[derive(Deserialize)]
struct UpdateChatRequest {
    title: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct AddMessageRequest {
    role: Option<String>,
    content: String,
    idempotency_key: String,
}

#[derive(Deserialize)]
struct AnalyzeMessageRequest {
    content: String,
}

#[derive(Serialize)]
struct ListWorkspacesResponse {
    workspaces: Vec<Workspace>,
}

#[derive(Serialize)]
struct ListChatsResponse {
    chats: Vec<Chat>,
}

#[derive(Serialize)]
struct AddMessageResponse {
    message: ChatMessage,
    deduplicated: bool,
}

#[derive(Serialize)]
struct AnalyzeMessageResponse {
    message_type: String,
    reason: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/workspaces", post(create_workspace).get(list_workspaces))
        .route(
            "/api/workspaces/{workspace_id}",
            get(get_workspace)
                .patch(update_workspace)
                .delete(delete_workspace),
        )
        .route(
            "/api/workspaces/{workspace_id}/chats",
            post(create_chat).get(list_chats),
        )
        .route(
            "/api/workspaces/{workspace_id}/chats/{chat_id}",
            get(get_chat).patch(update_chat).delete(delete_chat),
        )
        .route(
            "/api/workspaces/{workspace_id}/chats/{chat_id}/analyze",
            post(analyze_message),
        )
        .route(
            "/api/workspaces/{workspace_id}/chats/{chat_id}/messages",
            post(add_message),
        )
        .with_state(state)
}

async fn create_workspace(
    State(state): State<AppState>,
    Json(request): Json<CreateWorkspaceRequest>,
) -> Result<Json<Workspace>, ApiError> {
    let workspace = state.create_workspace(
        request.tenant_id.unwrap_or_default(),
        request.name,
        request.provider.unwrap_or_default(),
        request.model.unwrap_or_default(),
    )?;
    Ok(Json(workspace))
}

async fn list_workspaces(State(state): State<AppState>) -> Json<ListWorkspacesResponse> {
    Json(ListWorkspacesResponse {
        workspaces: state.list_workspaces(None),
    })
}

async fn get_workspace(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
) -> Result<Json<Workspace>, ApiError> {
    Ok(Json(state.get_workspace(&workspace_id)?))
}

async fn update_workspace(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
    Json(request): Json<UpdateWorkspaceRequest>,
) -> Result<Json<Workspace>, ApiError> {
    Ok(Json(state.update_workspace(
        &workspace_id,
        request.name,
        request.provider,
        request.model,
    )?))
}

async fn delete_workspace(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.delete_workspace(&workspace_id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_chat(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
    Json(request): Json<CreateChatRequest>,
) -> Result<Json<Chat>, ApiError> {
    Ok(Json(state.create_chat(&workspace_id, request.title)?))
}

async fn list_chats(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
) -> Json<ListChatsResponse> {
    Json(ListChatsResponse {
        chats: state.list_chats(&workspace_id),
    })
}

async fn get_chat(
    State(state): State<AppState>,
    Path((workspace_id, chat_id)): Path<(String, String)>,
) -> Result<Json<Chat>, ApiError> {
    Ok(Json(state.get_chat(&workspace_id, &chat_id)?))
}

async fn update_chat(
    State(state): State<AppState>,
    Path((workspace_id, chat_id)): Path<(String, String)>,
    Json(request): Json<UpdateChatRequest>,
) -> Result<Json<Chat>, ApiError> {
    Ok(Json(state.update_chat(
        &workspace_id,
        &chat_id,
        request.title,
        request.status,
    )?))
}

async fn delete_chat(
    State(state): State<AppState>,
    Path((workspace_id, chat_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    state.delete_chat(&workspace_id, &chat_id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn add_message(
    State(state): State<AppState>,
    Path((workspace_id, chat_id)): Path<(String, String)>,
    Json(request): Json<AddMessageRequest>,
) -> Result<Json<AddMessageResponse>, ApiError> {
    let result = state.add_message(
        &workspace_id,
        &chat_id,
        request.role.unwrap_or_default(),
        request.content,
        request.idempotency_key,
    )?;
    Ok(Json(AddMessageResponse {
        message: result.message,
        deduplicated: result.deduplicated,
    }))
}

async fn analyze_message(
    Path((_workspace_id, _chat_id)): Path<(String, String)>,
    Json(request): Json<AnalyzeMessageRequest>,
) -> Json<AnalyzeMessageResponse> {
    let analysis = analyze_message_type(&request.content);
    Json(AnalyzeMessageResponse {
        message_type: analysis.message_type,
        reason: analysis.reason,
    })
}

struct ApiError(StoreError);

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error) = match self.0 {
            StoreError::InvalidInput { field } => {
                (StatusCode::BAD_REQUEST, format!("字段 {field} 不能为空"))
            }
            StoreError::NotFound { entity } => (StatusCode::NOT_FOUND, format!("{entity} 不存在")),
        };
        (status, Json(ErrorResponse { error })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_workspace_router_creates_workspace() {
        let response = router(AppState::default())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/workspaces")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"tenant_id":"t1","name":"Main","provider":"p","model":"m"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .expect("request succeeds");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_workspace_router_rejects_empty_workspace_name() {
        let response = router(AppState::default())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/workspaces")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":" "}"#))
                    .unwrap(),
            )
            .await
            .expect("request succeeds");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_message_router_deduplicates_message() {
        let state = AppState::default();
        let workspace = state
            .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
            .expect("workspace created");
        let chat = state
            .create_chat(&workspace.id, "General".into())
            .expect("chat created");
        let app = router(state);
        let uri = format!(
            "/api/workspaces/{}/chats/{}/messages",
            workspace.id, chat.id
        );

        let first = post_message(app.clone(), &uri).await;
        let second = post_message(app, &uri).await;

        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(second.status(), StatusCode::OK);
        let body = axum::body::to_bytes(second.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("\"deduplicated\":true"));
    }

    #[tokio::test]
    async fn test_chat_router_updates_chat_title() {
        let state = AppState::default();
        let workspace = state
            .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
            .expect("workspace created");
        let chat = state
            .create_chat(&workspace.id, "Old".into())
            .expect("chat created");
        let app = router(state);
        let uri = format!("/api/workspaces/{}/chats/{}", workspace.id, chat.id);

        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"title":"New"}"#))
                    .unwrap(),
            )
            .await
            .expect("request succeeds");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("\"title\":\"New\""));
    }

    #[tokio::test]
    async fn test_chat_router_analyzes_message() {
        let state = AppState::default();
        let workspace = state
            .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
            .expect("workspace created");
        let chat = state
            .create_chat(&workspace.id, "General".into())
            .expect("chat created");
        let app = router(state);
        let uri = format!(
            "/api/workspaces/{}/chats/{}/analyze",
            workspace.id, chat.id
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"content":"请实现一个新功能"}"#))
                    .unwrap(),
            )
            .await
            .expect("request succeeds");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("\"message_type\":\"requirement\""));
    }

    async fn post_message(app: Router, uri: &str) -> axum::response::Response {
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"role":"user","content":"hello","idempotency_key":"k1"}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("request succeeds")
    }
}
