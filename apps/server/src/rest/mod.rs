use crate::model::app::AppState;
use aide::{
    axum::{ApiRouter, IntoApiResponse},
    openapi::{Info, OpenApi},
};
use axum::{Extension, Json, Router, routing::get};

pub mod board;
pub mod board_dto;
pub mod chat;
pub mod dto;
pub mod health;
pub mod path;

pub fn router(state: AppState) -> Router {
    let (router, api) = router_with_openapi(state);
    router.layer(Extension(api))
}

pub fn router_with_openapi(state: AppState) -> (Router, OpenApi) {
    let mut api = build_openapi();
    let router = api_router()
        .finish_api(&mut api)
        .route("/openapi.json", get(serve_openapi))
        .with_state(state);
    (router, api)
}

pub fn export_openapi_json() -> Result<String, serde_json::Error> {
    let mut api = build_openapi();
    let _ = api_router().finish_api(&mut api);
    serde_json::to_string_pretty(&api)
}

fn api_router() -> ApiRouter<AppState> {
    ApiRouter::new()
        .merge(chat::api_router())
        .merge(board::api_router())
}

fn build_openapi() -> OpenApi {
    OpenApi {
        info: Info {
            title: "Aemeath API".to_string(),
            version: "0.1.0".to_string(),
            description: Some("#36 REST API contract generated from Rust server code.".to_string()),
            ..Info::default()
        },
        ..OpenApi::default()
    }
}

async fn serve_openapi(Extension(api): Extension<OpenApi>) -> impl IntoApiResponse {
    Json(api)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_openapi_json_contains_sprint1_paths() {
        let json = export_openapi_json().expect("openapi serializes");

        assert!(json.contains("/api/workspaces"));
        assert!(json.contains("/api/workspaces/{workspace_id}/chats/{chat_id}/messages"));
        assert!(json.contains("/api/workspaces/{workspace_id}/board/snapshot"));
    }

    #[test]
    fn test_export_openapi_json_contains_operation_ids() {
        let json = export_openapi_json().expect("openapi serializes");

        assert!(json.contains("createWorkspace"));
        assert!(json.contains("addMessage"));
        assert!(json.contains("getBoardSnapshot"));
    }
}
