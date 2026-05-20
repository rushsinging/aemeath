use crate::model::app::{Chat, ChatMessage};
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BoardSnapshot {
    pub snapshot_id: String,
    pub workspace_id: String,
    pub workspace: WorkspaceInfo,
    pub chats: Vec<Chat>,
    pub recent_messages: Vec<ChatMessage>,
    pub is_full_snapshot: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BoardSnapshotUpdate {
    pub snapshot_id: String,
    pub changed_workspace: Option<WorkspaceInfo>,
    pub is_full_snapshot: bool,
    pub timestamp: i64,
    pub changed_chats: Vec<Chat>,
    pub removed_chat_ids: Vec<String>,
    pub new_messages: Vec<ChatMessage>,
    pub updated_messages: Vec<ChatMessage>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BoardEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: BoardSnapshotUpdate,
}
