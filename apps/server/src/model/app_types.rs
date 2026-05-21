use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct Workspace {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct Chat {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct ChatMessage {
    pub id: String,
    pub chat_id: String,
    pub workspace_id: String,
    pub sender_type: String,
    pub role: String,
    pub content: String,
    pub idempotency_key: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddMessageResult {
    pub message: ChatMessage,
    pub deduplicated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageAnalysis {
    pub message_type: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessagePage {
    pub messages: Vec<ChatMessage>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

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

#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    InvalidInput { field: &'static str },
    NotFound { entity: &'static str },
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub(super) inner: Arc<Mutex<StoreInner>>,
    pub(super) board_events: broadcast::Sender<BoardUpdate>,
}

#[derive(Debug, Default)]
pub(super) struct StoreInner {
    pub(super) workspaces: HashMap<String, Workspace>,
    pub(super) chats: HashMap<String, Chat>,
    pub(super) messages: HashMap<String, ChatMessage>,
    pub(super) message_idempotency: HashMap<String, String>,
}
