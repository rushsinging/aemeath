use crate::model::app::{AppState, Chat, StoreError, Workspace, analyze_message_type};
use crate::rest::dto::{
    AddMessageRequest, AddMessageResponse, AnalyzeMessageRequest, AnalyzeMessageResponse,
    CreateChatRequest, CreateWorkspaceRequest, ErrorResponse, ListChatsResponse, ListMessagesQuery,
    ListMessagesResponse, ListWorkspacesResponse, UpdateChatRequest, UpdateWorkspaceRequest,
};
use crate::rest::path::{ChatPath, WorkspacePath};
use aide::{
    OperationIo,
    axum::{
        ApiRouter,
        routing::{get_with, post_with},
    },
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub fn api_router() -> ApiRouter<AppState> {
    ApiRouter::new()
        .api_route(
            "/api/workspaces",
            post_with(create_workspace, |op| {
                op.id("createWorkspace")
                    .summary("Create workspace")
                    .response_with::<200, axum::Json<Workspace>, _>(|res| {
                        res.description("Workspace created")
                    })
                    .response_with::<400, axum::Json<ErrorResponse>, _>(|res| {
                        res.description("Invalid input")
                    })
            })
            .get_with(list_workspaces, |op| {
                op.id("listWorkspaces")
                    .summary("List workspaces")
                    .response_with::<200, axum::Json<ListWorkspacesResponse>, _>(|res| {
                        res.description("Workspaces listed")
                    })
            }),
        )
        .api_route(
            "/api/workspaces/{workspace_id}",
            get_with(get_workspace, |op| {
                op.id("getWorkspace")
                    .summary("Get workspace")
                    .response_with::<200, axum::Json<Workspace>, _>(|res| {
                        res.description("Workspace found")
                    })
                    .response_with::<404, axum::Json<ErrorResponse>, _>(|res| {
                        res.description("Workspace not found")
                    })
            })
            .patch_with(update_workspace, |op| {
                op.id("updateWorkspace")
                    .summary("Update workspace")
                    .response_with::<200, axum::Json<Workspace>, _>(|res| {
                        res.description("Workspace updated")
                    })
                    .response_with::<404, axum::Json<ErrorResponse>, _>(|res| {
                        res.description("Workspace not found")
                    })
            })
            .delete_with(delete_workspace, |op| {
                op.id("deleteWorkspace")
                    .summary("Delete workspace")
                    .response_with::<204, (), _>(|res| res.description("Workspace deleted"))
                    .response_with::<404, axum::Json<ErrorResponse>, _>(|res| {
                        res.description("Workspace not found")
                    })
            }),
        )
        .api_route(
            "/api/workspaces/{workspace_id}/chats",
            post_with(create_chat, |op| {
                op.id("createChat")
                    .summary("Create chat")
                    .response_with::<200, axum::Json<Chat>, _>(|res| {
                        res.description("Chat created")
                    })
                    .response_with::<404, axum::Json<ErrorResponse>, _>(|res| {
                        res.description("Workspace not found")
                    })
            })
            .get_with(list_chats, |op| {
                op.id("listChats")
                    .summary("List chats")
                    .response_with::<200, axum::Json<ListChatsResponse>, _>(|res| {
                        res.description("Chats listed")
                    })
            }),
        )
        .api_route(
            "/api/workspaces/{workspace_id}/chats/{chat_id}",
            get_with(get_chat, |op| {
                op.id("getChat")
                    .summary("Get chat")
                    .response_with::<200, axum::Json<Chat>, _>(|res| res.description("Chat found"))
                    .response_with::<404, axum::Json<ErrorResponse>, _>(|res| {
                        res.description("Chat not found")
                    })
            })
            .patch_with(update_chat, |op| {
                op.id("updateChat")
                    .summary("Update chat")
                    .response_with::<200, axum::Json<Chat>, _>(|res| {
                        res.description("Chat updated")
                    })
                    .response_with::<404, axum::Json<ErrorResponse>, _>(|res| {
                        res.description("Chat not found")
                    })
            })
            .delete_with(delete_chat, |op| {
                op.id("deleteChat")
                    .summary("Delete chat")
                    .response_with::<204, (), _>(|res| res.description("Chat deleted"))
                    .response_with::<404, axum::Json<ErrorResponse>, _>(|res| {
                        res.description("Chat not found")
                    })
            }),
        )
        .api_route(
            "/api/workspaces/{workspace_id}/chats/{chat_id}/analyze",
            post_with(analyze_message, |op| {
                op.id("analyzeMessage")
                    .summary("Analyze message")
                    .response_with::<200, axum::Json<AnalyzeMessageResponse>, _>(|res| {
                        res.description("Message analysis returned")
                    })
            }),
        )
        .api_route(
            "/api/workspaces/{workspace_id}/chats/{chat_id}/messages",
            post_with(add_message, |op| {
                op.id("addMessage")
                    .summary("Add message")
                    .response_with::<200, axum::Json<AddMessageResponse>, _>(|res| {
                        res.description("Message added or deduplicated")
                    })
                    .response_with::<404, axum::Json<ErrorResponse>, _>(|res| {
                        res.description("Chat not found")
                    })
            })
            .get_with(list_messages, |op| {
                op.id("listMessages")
                    .summary("List messages")
                    .response_with::<200, axum::Json<ListMessagesResponse>, _>(|res| {
                        res.description("Messages listed")
                    })
                    .response_with::<404, axum::Json<ErrorResponse>, _>(|res| {
                        res.description("Chat not found")
                    })
            }),
        )
}

pub fn router(state: AppState) -> Router {
    api_router().with_state(state).into()
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
    Path(path): Path<WorkspacePath>,
) -> Result<Json<Workspace>, ApiError> {
    Ok(Json(state.get_workspace(&path.workspace_id)?))
}

async fn update_workspace(
    State(state): State<AppState>,
    Path(path): Path<WorkspacePath>,
    Json(request): Json<UpdateWorkspaceRequest>,
) -> Result<Json<Workspace>, ApiError> {
    Ok(Json(state.update_workspace(
        &path.workspace_id,
        request.name,
        request.provider,
        request.model,
    )?))
}

async fn delete_workspace(
    State(state): State<AppState>,
    Path(path): Path<WorkspacePath>,
) -> Result<StatusCode, ApiError> {
    state.delete_workspace(&path.workspace_id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_chat(
    State(state): State<AppState>,
    Path(path): Path<WorkspacePath>,
    Json(request): Json<CreateChatRequest>,
) -> Result<Json<Chat>, ApiError> {
    Ok(Json(state.create_chat(&path.workspace_id, request.title)?))
}

async fn list_chats(
    State(state): State<AppState>,
    Path(path): Path<WorkspacePath>,
) -> Json<ListChatsResponse> {
    Json(ListChatsResponse {
        chats: state.list_chats(&path.workspace_id),
    })
}

async fn get_chat(
    State(state): State<AppState>,
    Path(path): Path<ChatPath>,
) -> Result<Json<Chat>, ApiError> {
    Ok(Json(state.get_chat(&path.workspace_id, &path.chat_id)?))
}

async fn update_chat(
    State(state): State<AppState>,
    Path(path): Path<ChatPath>,
    Json(request): Json<UpdateChatRequest>,
) -> Result<Json<Chat>, ApiError> {
    Ok(Json(state.update_chat(
        &path.workspace_id,
        &path.chat_id,
        request.title,
        request.status,
    )?))
}

async fn delete_chat(
    State(state): State<AppState>,
    Path(path): Path<ChatPath>,
) -> Result<StatusCode, ApiError> {
    state.delete_chat(&path.workspace_id, &path.chat_id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn add_message(
    State(state): State<AppState>,
    Path(path): Path<ChatPath>,
    Json(request): Json<AddMessageRequest>,
) -> Result<Json<AddMessageResponse>, ApiError> {
    let result = state.add_message(
        &path.workspace_id,
        &path.chat_id,
        request.role.unwrap_or_default(),
        request.content,
        request.idempotency_key,
    )?;
    Ok(Json(AddMessageResponse {
        message: result.message,
        deduplicated: result.deduplicated,
    }))
}

async fn list_messages(
    State(state): State<AppState>,
    Path(path): Path<ChatPath>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<ListMessagesResponse>, ApiError> {
    let page = state.list_chat_messages(
        &path.workspace_id,
        &path.chat_id,
        query.limit.unwrap_or(50),
        query.before.as_deref(),
    )?;
    Ok(Json(ListMessagesResponse {
        messages: page.messages,
        has_more: page.has_more,
        next_cursor: page.next_cursor,
    }))
}

async fn analyze_message(
    Path(_path): Path<ChatPath>,
    Json(request): Json<AnalyzeMessageRequest>,
) -> Json<AnalyzeMessageResponse> {
    let analysis = analyze_message_type(&request.content);
    Json(AnalyzeMessageResponse {
        message_type: analysis.message_type,
        reason: analysis.reason,
    })
}

#[derive(OperationIo)]
#[aide(output_with = "axum::Json<ErrorResponse>")]
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
#[path = "chat_tests.rs"]
mod tests;
