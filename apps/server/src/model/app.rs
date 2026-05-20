use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Chat {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
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

#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    InvalidInput { field: &'static str },
    NotFound { entity: &'static str },
}

#[derive(Clone, Debug, Default)]
pub struct AppState {
    inner: Arc<Mutex<StoreInner>>,
}

#[derive(Debug, Default)]
struct StoreInner {
    workspaces: HashMap<String, Workspace>,
    chats: HashMap<String, Chat>,
    messages: HashMap<String, ChatMessage>,
    message_idempotency: HashMap<String, String>,
}

impl AppState {
    pub fn create_workspace(
        &self,
        tenant_id: String,
        name: String,
        provider: String,
        model: String,
    ) -> Result<Workspace, StoreError> {
        require_non_empty("name", &name)?;
        let now = now_millis();
        let workspace = Workspace {
            id: new_id(),
            tenant_id: default_if_empty(tenant_id, "default"),
            name,
            provider,
            model,
            created_at: now,
            updated_at: now,
            version: 1,
        };

        self.inner
            .lock()
            .expect("store mutex poisoned")
            .workspaces
            .insert(workspace.id.clone(), workspace.clone());
        Ok(workspace)
    }

    pub fn get_workspace(&self, workspace_id: &str) -> Result<Workspace, StoreError> {
        self.inner
            .lock()
            .expect("store mutex poisoned")
            .workspaces
            .get(workspace_id)
            .cloned()
            .ok_or(StoreError::NotFound {
                entity: "workspace",
            })
    }

    pub fn list_workspaces(&self, tenant_id: Option<&str>) -> Vec<Workspace> {
        let mut workspaces: Vec<_> = self
            .inner
            .lock()
            .expect("store mutex poisoned")
            .workspaces
            .values()
            .filter(|workspace| tenant_id.is_none_or(|tenant| workspace.tenant_id == tenant))
            .cloned()
            .collect();
        workspaces.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        workspaces
    }

    pub fn update_workspace(
        &self,
        workspace_id: &str,
        name: Option<String>,
        provider: Option<String>,
        model: Option<String>,
    ) -> Result<Workspace, StoreError> {
        let mut inner = self.inner.lock().expect("store mutex poisoned");
        let workspace = inner
            .workspaces
            .get_mut(workspace_id)
            .ok_or(StoreError::NotFound {
                entity: "workspace",
            })?;

        if let Some(name) = name {
            require_non_empty("name", &name)?;
            workspace.name = name;
        }
        if let Some(provider) = provider {
            workspace.provider = provider;
        }
        if let Some(model) = model {
            workspace.model = model;
        }
        workspace.updated_at = now_millis();
        workspace.version += 1;
        Ok(workspace.clone())
    }

    pub fn delete_workspace(&self, workspace_id: &str) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().expect("store mutex poisoned");
        if inner.workspaces.remove(workspace_id).is_none() {
            return Err(StoreError::NotFound {
                entity: "workspace",
            });
        }
        inner.chats.retain(|_, chat| chat.workspace_id != workspace_id);
        inner
            .messages
            .retain(|_, message| message.workspace_id != workspace_id);
        inner
            .message_idempotency
            .retain(|key, _| !key.starts_with(&format!("{workspace_id}:")));
        Ok(())
    }

    pub fn create_chat(&self, workspace_id: &str, title: String) -> Result<Chat, StoreError> {
        require_non_empty("title", &title)?;
        let mut inner = self.inner.lock().expect("store mutex poisoned");
        if !inner.workspaces.contains_key(workspace_id) {
            return Err(StoreError::NotFound {
                entity: "workspace",
            });
        }

        let now = now_millis();
        let chat = Chat {
            id: new_id(),
            workspace_id: workspace_id.to_string(),
            title,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            version: 1,
        };
        inner.chats.insert(chat.id.clone(), chat.clone());
        Ok(chat)
    }

    pub fn get_chat(&self, workspace_id: &str, chat_id: &str) -> Result<Chat, StoreError> {
        let chat = self
            .inner
            .lock()
            .expect("store mutex poisoned")
            .chats
            .get(chat_id)
            .cloned()
            .ok_or(StoreError::NotFound { entity: "chat" })?;
        if chat.workspace_id != workspace_id {
            return Err(StoreError::NotFound { entity: "chat" });
        }
        Ok(chat)
    }

    pub fn list_chats(&self, workspace_id: &str) -> Vec<Chat> {
        let mut chats: Vec<_> = self
            .inner
            .lock()
            .expect("store mutex poisoned")
            .chats
            .values()
            .filter(|chat| chat.workspace_id == workspace_id)
            .cloned()
            .collect();
        chats.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        chats
    }

    pub fn delete_chat(&self, workspace_id: &str, chat_id: &str) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().expect("store mutex poisoned");
        let chat = inner
            .chats
            .get(chat_id)
            .ok_or(StoreError::NotFound { entity: "chat" })?;
        if chat.workspace_id != workspace_id {
            return Err(StoreError::NotFound { entity: "chat" });
        }
        inner.chats.remove(chat_id);
        inner.messages.retain(|_, message| message.chat_id != chat_id);
        inner
            .message_idempotency
            .retain(|key, _| !key.starts_with(&format!("{workspace_id}:{chat_id}:")));
        Ok(())
    }

    pub fn add_message(
        &self,
        workspace_id: &str,
        chat_id: &str,
        role: String,
        content: String,
        idempotency_key: String,
    ) -> Result<AddMessageResult, StoreError> {
        require_non_empty("content", &content)?;
        require_non_empty("idempotency_key", &idempotency_key)?;
        let mut inner = self.inner.lock().expect("store mutex poisoned");
        let chat = inner
            .chats
            .get(chat_id)
            .ok_or(StoreError::NotFound { entity: "chat" })?;
        if chat.workspace_id != workspace_id {
            return Err(StoreError::NotFound { entity: "chat" });
        }

        let idempotency_scope = format!("{workspace_id}:{chat_id}:{idempotency_key}");
        if let Some(message_id) = inner.message_idempotency.get(&idempotency_scope) {
            let message = inner
                .messages
                .get(message_id)
                .expect("idempotency index references missing message")
                .clone();
            return Ok(AddMessageResult {
                message,
                deduplicated: true,
            });
        }

        let now = now_millis();
        let message = ChatMessage {
            id: new_id(),
            chat_id: chat_id.to_string(),
            workspace_id: workspace_id.to_string(),
            sender_type: "user".to_string(),
            role: default_if_empty(role, "user"),
            content,
            idempotency_key,
            created_at: now,
            updated_at: now,
            version: 1,
        };
        inner
            .message_idempotency
            .insert(idempotency_scope, message.id.clone());
        inner.messages.insert(message.id.clone(), message.clone());
        Ok(AddMessageResult {
            message,
            deduplicated: false,
        })
    }

    pub fn list_recent_messages(&self, workspace_id: &str) -> Vec<ChatMessage> {
        let mut messages: Vec<_> = self
            .inner
            .lock()
            .expect("store mutex poisoned")
            .messages
            .values()
            .filter(|message| message.workspace_id == workspace_id)
            .cloned()
            .collect();
        messages.sort_by(|left, right| left.created_at.cmp(&right.created_at));
        messages
    }
}

fn default_if_empty(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

fn new_id() -> String {
    ObjectId::new().to_hex()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() {
        Err(StoreError::InvalidInput { field })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_workspace_stores_workspace() {
        let state = AppState::default();

        let workspace = state
            .create_workspace(
                "tenant-a".to_string(),
                "Main".to_string(),
                "anthropic".to_string(),
                "claude".to_string(),
            )
            .expect("workspace created");

        assert_eq!(workspace.name, "Main");
        assert_eq!(state.get_workspace(&workspace.id), Ok(workspace));
    }

    #[test]
    fn test_create_workspace_rejects_empty_name() {
        let state = AppState::default();

        let result = state.create_workspace(
            "tenant-a".to_string(),
            " ".to_string(),
            "anthropic".to_string(),
            "claude".to_string(),
        );

        assert!(matches!(
            result,
            Err(StoreError::InvalidInput { field }) if field == "name"
        ));
    }

    #[test]
    fn test_add_message_deduplicates_by_idempotency_key() {
        let state = AppState::default();
        let workspace = state
            .create_workspace(
                "tenant-a".to_string(),
                "Main".to_string(),
                "anthropic".to_string(),
                "claude".to_string(),
            )
            .expect("workspace created");
        let chat = state
            .create_chat(&workspace.id, "General".to_string())
            .expect("chat created");

        let first = state
            .add_message(
                &workspace.id,
                &chat.id,
                "user".to_string(),
                "hello".to_string(),
                "same-key".to_string(),
            )
            .expect("message added");
        let second = state
            .add_message(
                &workspace.id,
                &chat.id,
                "user".to_string(),
                "changed".to_string(),
                "same-key".to_string(),
            )
            .expect("message deduplicated");

        assert!(!first.deduplicated);
        assert!(second.deduplicated);
        assert_eq!(first.message.id, second.message.id);
        assert_eq!(second.message.content, "hello");
    }

    #[test]
    fn test_add_message_rejects_missing_chat() {
        let state = AppState::default();

        let result = state.add_message(
            "workspace-a",
            "missing-chat",
            "user".to_string(),
            "hello".to_string(),
            "key".to_string(),
        );

        assert!(matches!(
            result,
            Err(StoreError::NotFound { entity }) if entity == "chat"
        ));
    }
}
