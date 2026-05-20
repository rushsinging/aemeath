use crate::model::app::{Chat, ChatMessage, Workspace};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateWorkspaceRequest {
    pub tenant_id: Option<String>,
    pub name: String,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateWorkspaceRequest {
    pub name: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateChatRequest {
    pub title: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateChatRequest {
    pub title: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddMessageRequest {
    pub role: Option<String>,
    pub content: String,
    pub idempotency_key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListMessagesQuery {
    pub limit: Option<usize>,
    pub before: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeMessageRequest {
    pub content: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListWorkspacesResponse {
    pub workspaces: Vec<Workspace>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListChatsResponse {
    pub chats: Vec<Chat>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AddMessageResponse {
    pub message: ChatMessage,
    pub deduplicated: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListMessagesResponse {
    pub messages: Vec<ChatMessage>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AnalyzeMessageResponse {
    pub message_type: String,
    pub reason: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ErrorResponse {
    pub error: String,
}
