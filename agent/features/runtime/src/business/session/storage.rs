//! 会话持久层 — 保存 / 加载 / 列表 / 删除
//!
//! ## 原子写与损坏兜底
//!
//! `save_session` 采用 **tmp → fsync → rename** 原子写策略（POSIX 同目录
//! rename 原子），读者永远只看到「旧完整版」或「新完整版」，消除
//! truncate-then-write 导致的 0 字节/半截 JSON 截断窗口。
//!
//! `load_session` 在主文件解析失败时依次尝试 `.bak` 回退、`.corrupt`
//! 转存，确保损坏的会话文件不会被静默丢弃。

use crate::business::session::types::*;
use crate::LOG_TARGET;
use tokio::io::AsyncWriteExt;

/// 旧格式迁移：若 `chats` 为空且 `messages` 非空，把扁平 messages 包装为
/// 单个 `ChatSegment::normal(None)`，存入 `chats`，清空 `messages`。
fn migrate_legacy_messages(session: &mut Session) {
    if session.chats.is_empty() && !session.messages.is_empty() {
        log::info!(
            target: LOG_TARGET,
            "session {} migrating legacy flat messages ({}) to chat chain",
            session.id,
            session.messages.len()
        );
        let messages = std::mem::take(&mut session.messages);
        let mut seg = super::chat_chain::ChatSegment::normal(None);
        seg.messages = messages;
        session.chats.push(seg);
        session.messages = Vec::new();
    }
}

/// Save a session to disk (atomic: tmp → fsync → rename).
///
/// Before replacing `{id}.json`, the previous version is preserved as
/// `{id}.json.bak` for one-level rollback on corruption.
pub async fn save_session(session: &Session) -> Result<(), String> {
    validate_session_id(&session.id)?;

    let dir = sessions_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("failed to create sessions dir: {e}"))?;

    let path = dir.join(format!("{}.json", session.id));
    let tmp_path = dir.join(format!("{}.json.tmp", session.id));
    let bak_path = dir.join(format!("{}.json.bak", session.id));

    let json = serde_json::to_string_pretty(session)
        .map_err(|e| format!("failed to serialize session: {e}"))?;

    // 1. 写入临时文件 + fsync（保证数据落盘后再 rename）
    {
        let mut file = tokio::fs::File::create(&tmp_path)
            .await
            .map_err(|e| format!("failed to create temp session file: {e}"))?;
        file.write_all(json.as_bytes())
            .await
            .map_err(|e| format!("failed to write session: {e}"))?;
        file.sync_all()
            .await
            .map_err(|e| format!("failed to sync session file: {e}"))?;
    }

    // 2. 备份旧版本（若存在）为 .bak
    if path.exists() {
        let _ = tokio::fs::rename(&path, &bak_path).await;
    }

    // 3. 原子 rename：同目录 rename 在 POSIX 下是原子的，
    //    读者永远只看到完整的旧版本或完整的新版本。
    tokio::fs::rename(&tmp_path, &path)
        .await
        .map_err(|e| format!("failed to rename session file: {e}"))?;

    Ok(())
}

/// Load a session from disk by ID.
///
/// Falls back to `.bak` if the primary file is corrupted; if no valid backup
/// exists, moves the corrupted file to `.corrupt` and returns an error so the
/// caller can surface it to the user instead of silently starting fresh.
pub async fn load_session(id: &str) -> Result<Session, String> {
    validate_session_id(id)?;

    let dir = sessions_dir();
    let path = dir.join(format!("{id}.json"));
    let bak_path = dir.join(format!("{id}.json.bak"));
    let corrupt_path = dir.join(format!("{id}.json.corrupt"));

    if !path.exists() {
        return Err(format!("session not found: {id}"));
    }
    let json = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("failed to read session: {e}"))?;

    match serde_json::from_str::<Session>(&json) {
        Ok(mut session) => {
            migrate_legacy_messages(&mut session);
            Ok(session)
        }
        Err(parse_err) => {
            log::warn!(
                target: LOG_TARGET,
                "session {} JSON corrupted ({}), attempting .bak fallback",
                id,
                parse_err
            );
            // 尝试 .bak 回退
            if bak_path.exists() {
                if let Ok(bak_json) = tokio::fs::read_to_string(&bak_path).await {
                    if let Ok(mut session) = serde_json::from_str::<Session>(&bak_json) {
                        log::info!(
                            target: LOG_TARGET,
                            "session {} recovered from .bak backup",
                            id
                        );
                        migrate_legacy_messages(&mut session);
                        return Ok(session);
                    }
                }
            }
            // .bak 不存在或也损坏：转存 .corrupt 保留原始数据供手工抢救
            let _ = tokio::fs::rename(&path, &corrupt_path).await;
            let corrupt_display = corrupt_path.display();
            Err(format!(
                "session {id} is corrupted and no valid .bak exists. \
                 Corrupted file moved to {corrupt_display}. Parse error: {parse_err}"
            ))
        }
    }
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
        .map_err(|e| format!("failed to delete session: {e}"))?;

    // 清理该 session 的 tool_outputs 子目录（生命周期与 session 绑定）
    let tool_outputs_dir = share::config::paths::session_tool_outputs_dir(id);
    if tool_outputs_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&tool_outputs_dir).await;
    }

    Ok(())
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
