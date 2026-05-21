#[path = "app_helpers.rs"]
mod app_helpers;
#[path = "app_types.rs"]
mod app_types;
use app_helpers::*;
pub use app_types::*;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

impl Default for AppState {
    fn default() -> Self {
        let (board_events, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(Mutex::new(StoreInner::default())),
            board_events,
        }
    }
}

impl AppState {
    pub fn subscribe_board_updates(&self) -> broadcast::Receiver<BoardUpdate> {
        self.board_events.subscribe()
    }

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
        inner
            .chats
            .retain(|_, chat| chat.workspace_id != workspace_id);
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

    pub fn update_chat(
        &self,
        workspace_id: &str,
        chat_id: &str,
        title: Option<String>,
        status: Option<String>,
    ) -> Result<Chat, StoreError> {
        let mut inner = self.inner.lock().expect("store mutex poisoned");
        let chat = inner
            .chats
            .get_mut(chat_id)
            .ok_or(StoreError::NotFound { entity: "chat" })?;
        if chat.workspace_id != workspace_id {
            return Err(StoreError::NotFound { entity: "chat" });
        }
        if let Some(title) = title {
            require_non_empty("title", &title)?;
            chat.title = title;
        }
        if let Some(status) = status {
            require_non_empty("status", &status)?;
            chat.status = status;
        }
        chat.updated_at = now_millis();
        chat.version += 1;
        Ok(chat.clone())
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
        inner
            .messages
            .retain(|_, message| message.chat_id != chat_id);
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
        let _ = self.board_events.send(BoardUpdate {
            workspace_id: workspace_id.to_string(),
            event_kind: BoardEventKind::MessageAdded,
            message: message.clone(),
        });
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

    pub fn list_chat_messages(
        &self,
        workspace_id: &str,
        chat_id: &str,
        limit: usize,
        before: Option<&str>,
    ) -> Result<MessagePage, StoreError> {
        let inner = self.inner.lock().expect("store mutex poisoned");
        let chat = inner
            .chats
            .get(chat_id)
            .ok_or(StoreError::NotFound { entity: "chat" })?;
        if chat.workspace_id != workspace_id {
            return Err(StoreError::NotFound { entity: "chat" });
        }

        let mut messages: Vec<_> = inner
            .messages
            .values()
            .filter(|message| message.workspace_id == workspace_id && message.chat_id == chat_id)
            .cloned()
            .collect();
        messages.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.id.cmp(&left.id))
        });

        if let Some(before) = before {
            let before_position = messages
                .iter()
                .position(|message| message.id == before)
                .ok_or(StoreError::NotFound { entity: "message" })?;
            messages = messages.into_iter().skip(before_position + 1).collect();
        }

        let limit = limit.max(1);
        let has_more = messages.len() > limit;
        let mut page_messages: Vec<_> = messages.into_iter().take(limit).collect();
        let next_cursor = if has_more {
            page_messages.last().map(|message| message.id.clone())
        } else {
            None
        };

        Ok(MessagePage {
            messages: std::mem::take(&mut page_messages),
            has_more,
            next_cursor,
        })
    }
}

pub fn analyze_message_type(content: &str) -> MessageAnalysis {
    if contains_any(content, &["bug", "错误", "修复", "失败"]) {
        MessageAnalysis {
            message_type: "feedback".to_string(),
            reason: "消息包含 bug/错误/修复/失败 等反馈关键词".to_string(),
        }
    } else if contains_any(content, &["实现", "新增", "创建", "需求", "feature"]) {
        MessageAnalysis {
            message_type: "requirement".to_string(),
            reason: "消息包含 实现/新增/创建/需求/feature 等需求关键词".to_string(),
        }
    } else {
        MessageAnalysis {
            message_type: "chitchat".to_string(),
            reason: "消息未命中需求或反馈关键词".to_string(),
        }
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
