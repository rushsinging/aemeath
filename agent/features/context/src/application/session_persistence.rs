use std::sync::Arc;

use crate::domain::session::{CanonicalSession, SessionCodec, SessionCodecError};
use crate::ports::{SessionGeneration, SessionSnapshotStore, SessionStoreError};

pub struct SessionPersistenceService {
    store: Arc<dyn SessionSnapshotStore>,
}

impl SessionPersistenceService {
    pub fn new(store: Arc<dyn SessionSnapshotStore>) -> Self {
        Self { store }
    }

    pub async fn load(&self) -> Result<CanonicalSession, SessionLoadError> {
        let primary = self.store.read(SessionGeneration::Primary).await?;
        let Some(primary) = primary else {
            return Err(SessionLoadError::NotFound);
        };
        match SessionCodec::decode(&primary) {
            Ok(decoded) => Ok(decoded.session),
            Err(SessionCodecError::UnsupportedFutureVersion {
                version,
                original_bytes,
            }) => Err(SessionLoadError::UnsupportedFutureVersion {
                version,
                original_bytes,
            }),
            Err(_) => self.recover_previous().await,
        }
    }

    pub async fn save(&self, session: &CanonicalSession) -> Result<(), SessionLoadError> {
        let bytes = SessionCodec::encode(session)?;
        self.store.write(&bytes).await?;
        Ok(())
    }

    async fn recover_previous(&self) -> Result<CanonicalSession, SessionLoadError> {
        let previous = self.store.read(SessionGeneration::Previous).await?;
        let Some(previous) = previous else {
            self.store.quarantine(SessionGeneration::Primary).await?;
            return Err(SessionLoadError::NoDecodableGeneration);
        };
        match SessionCodec::decode(&previous) {
            Ok(decoded) => {
                self.store.quarantine(SessionGeneration::Primary).await?;
                self.store.promote_previous().await?;
                Ok(decoded.session)
            }
            Err(SessionCodecError::UnsupportedFutureVersion {
                version,
                original_bytes,
            }) => Err(SessionLoadError::UnsupportedFutureVersion {
                version,
                original_bytes,
            }),
            Err(_) => {
                self.store.quarantine(SessionGeneration::Primary).await?;
                self.store.quarantine(SessionGeneration::Previous).await?;
                Err(SessionLoadError::NoDecodableGeneration)
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionLoadError {
    #[error("Session snapshot not found")]
    NotFound,
    #[error("No decodable Session generation")]
    NoDecodableGeneration,
    #[error("Session schema version {version} is newer than supported")]
    UnsupportedFutureVersion {
        version: u32,
        original_bytes: Vec<u8>,
    },
    #[error(transparent)]
    Codec(#[from] SessionCodecError),
    #[error(transparent)]
    Store(#[from] SessionStoreError),
}
