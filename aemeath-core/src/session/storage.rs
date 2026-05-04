//! 会话持久层 — 保存 / 加载 / 列表 / 删除

use crate::session::types::*;

/// Save a session to disk
pub async fn save_session(session: &Session) -> Result<(), String> {
    validate_session_id(&session.id)?;

    let dir = sessions_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("failed to create sessions dir: {e}"))?;

    let path = dir.join(format!("{}.json", session.id));
    let json = serde_json::to_string_pretty(session)
        .map_err(|e| format!("failed to serialize session: {e}"))?;
    tokio::fs::write(&path, json)
        .await
        .map_err(|e| format!("failed to write session: {e}"))?;
    Ok(())
}

/// Load a session from disk by ID
pub async fn load_session(id: &str) -> Result<Session, String> {
    validate_session_id(id)?;

    let path = sessions_dir().join(format!("{id}.json"));
    if !path.exists() {
        return Err(format!("session not found: {id}"));
    }
    let json = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("failed to read session: {e}"))?;
    serde_json::from_str(&json).map_err(|e| format!("failed to parse session: {e}"))
}

/// List all saved sessions, sorted by updated_at descending
pub async fn list_sessions() -> Vec<Session> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Vec::new();
    }

    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut sessions = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(json) = tokio::fs::read_to_string(&path).await {
                if let Ok(session) = serde_json::from_str::<Session>(&json) {
                    sessions.push(session);
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
}

/// Delete a session by ID
pub async fn delete_session(id: &str) -> Result<(), String> {
    validate_session_id(id)?;

    let path = sessions_dir().join(format!("{id}.json"));
    if !path.exists() {
        return Err(format!("session not found: {id}"));
    }
    tokio::fs::remove_file(&path)
        .await
        .map_err(|e| format!("failed to delete session: {e}"))
}

/// Update session metadata
pub async fn update_session_metadata(
    id: &str,
    title: Option<String>,
    tags: Option<Vec<String>>,
    notes: Option<String>,
    is_favorite: Option<bool>,
) -> Result<Session, String> {
    let mut session = load_session(id).await?;

    if let Some(t) = title {
        session.set_title(t);
    }
    if let Some(new_tags) = tags {
        session.metadata.tags = new_tags;
        session.updated_at = now_iso();
    }
    if let Some(n) = notes {
        session.set_notes(n);
    }
    if let Some(fav) = is_favorite {
        session.set_favorite(fav);
    }

    save_session(&session).await?;
    Ok(session)
}
