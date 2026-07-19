use std::sync::Arc;

use crate::adapters::{AtomicBlobSessionStore, LegacySessionDecoder};
use crate::application::{SessionLoadError, SessionPersistenceService};
use crate::domain::session::{
    now_iso, CanonicalSession, SessionCodec, SessionListEntry, SessionManagementError,
    SessionMetadataUpdate,
};

fn blob() -> Result<Arc<dyn storage::api::AtomicBlobPort>, SessionManagementError> {
    storage::api::file_system_blob(share::config::paths::global_agents_dir())
        .map_err(|error| SessionManagementError::Storage(error.to_string()))
}

fn store(
    blob: Arc<dyn storage::api::AtomicBlobPort>,
    id: &str,
) -> Result<Arc<AtomicBlobSessionStore>, SessionManagementError> {
    AtomicBlobSessionStore::new(blob, id)
        .map(Arc::new)
        .map_err(|error| SessionManagementError::Storage(error.to_string()))
}

fn map_load(id: &str, error: SessionLoadError) -> SessionManagementError {
    match error {
        SessionLoadError::NotFound => SessionManagementError::NotFound(id.to_string()),
        SessionLoadError::NoDecodableGeneration => SessionManagementError::Corrupt(id.to_string()),
        SessionLoadError::UnsupportedFutureVersion { version, .. } => {
            SessionManagementError::UnsupportedFutureVersion(version)
        }
        other => SessionManagementError::Storage(other.to_string()),
    }
}

pub async fn load_canonical(id: &str) -> Result<CanonicalSession, SessionManagementError> {
    let store = store(blob()?, id)?;
    SessionPersistenceService::new(store, Arc::new(LegacySessionDecoder))
        .load()
        .await
        .map_err(|error| map_load(id, error))
}

pub async fn list() -> Result<Vec<SessionListEntry>, SessionManagementError> {
    let directory = share::config::paths::global_agents_dir().join("session");
    let mut entries = match tokio::fs::read_dir(&directory).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(SessionManagementError::Storage(error.to_string())),
    };
    let blob = blob()?;
    let mut sessions = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| SessionManagementError::Storage(error.to_string()))?
    {
        let path = entry.path();
        if path.extension().is_some() || !path.is_file() {
            continue;
        }
        let Some(id) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let service = SessionPersistenceService::new(
            store(Arc::clone(&blob), id)?,
            Arc::new(LegacySessionDecoder),
        );
        if let Ok(session) = service.load().await {
            sessions.push(SessionListEntry::from_canonical(&session));
        }
    }
    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(sessions)
}

pub async fn export(id: &str) -> Result<Vec<u8>, SessionManagementError> {
    let session = load_canonical(id).await?;
    SessionCodec::encode(&session)
        .map_err(|error| SessionManagementError::Storage(error.to_string()))
}

pub async fn import(bytes: &[u8]) -> Result<SessionListEntry, SessionManagementError> {
    let decoded = crate::adapters::decode_session(bytes).map_err(|error| match error {
        crate::domain::session::SessionCodecError::UnsupportedFutureVersion { version, .. } => {
            SessionManagementError::UnsupportedFutureVersion(version)
        }
        other => SessionManagementError::Corrupt(other.to_string()),
    })?;
    let session = decoded.session;
    let service = SessionPersistenceService::new(
        store(blob()?, &session.id)?,
        Arc::new(LegacySessionDecoder),
    );
    service
        .save(&session)
        .await
        .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
    Ok(SessionListEntry::from_canonical(&session))
}

pub async fn update_metadata(
    id: &str,
    update: SessionMetadataUpdate,
) -> Result<SessionListEntry, SessionManagementError> {
    let mut session = load_canonical(id).await?;
    update.apply(&mut session.metadata);
    session.updated_at = now_iso();
    let service =
        SessionPersistenceService::new(store(blob()?, id)?, Arc::new(LegacySessionDecoder));
    service
        .save(&session)
        .await
        .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
    Ok(SessionListEntry::from_canonical(&session))
}

pub async fn delete(id: &str) -> Result<(), SessionManagementError> {
    let store = store(blob()?, id)?;
    let outcome = store
        .delete_all()
        .await
        .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
    if !outcome.deleted_primary() && !outcome.deleted_previous() {
        return Err(SessionManagementError::NotFound(id.to_string()));
    }
    let tool_results = share::config::paths::session_tool_results_dir(id);
    if tool_results.exists() {
        tokio::fs::remove_dir_all(tool_results)
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
    }
    Ok(())
}
