use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use storage::api::{
    AtomicDatasetPort, DatasetKey, DatasetMember, DatasetReadOutcome, Generation, SafePathSegment,
    StorageError,
};

use crate::adapters::{
    AtomicBlobSessionStore, DatasetCanonicalSessionWriter, LegacySessionDecoder,
};
use crate::application::SessionPersistenceService;
use crate::domain::session::{
    CanonicalSession, CommittedRunSlice, SessionGenerationCodec, SessionGenerationManifest,
    SessionGenerationWireError, SessionHistory, SessionStepMember,
};

pub struct DatasetSessionReader {
    dataset: Arc<dyn AtomicDatasetPort>,
    legacy_blob: Option<Arc<dyn storage::api::AtomicBlobPort>>,
}

impl DatasetSessionReader {
    pub fn new(
        dataset: Arc<dyn AtomicDatasetPort>,
        legacy_blob: Option<Arc<dyn storage::api::AtomicBlobPort>>,
    ) -> Self {
        Self {
            dataset,
            legacy_blob,
        }
    }

    pub async fn load(
        &self,
        session_id: &str,
    ) -> Result<CanonicalSession, SessionGenerationWireError> {
        let dataset_key = session_dataset_key(session_id)?;
        let manifest_name = safe_member_name(SessionGenerationManifest::manifest_member_name())?;
        let primary_manifest = match self
            .dataset
            .read_consistent(&dataset_key, std::slice::from_ref(&manifest_name))
            .await
        {
            Ok(outcome) => outcome,
            Err(error) => return Err(storage_error(error)),
        };
        if matches!(primary_manifest, DatasetReadOutcome::NotFound) {
            return self.load_and_migrate_legacy(session_id).await;
        }

        match self
            .decode_generation(&dataset_key, Generation::Primary, primary_manifest)
            .await
        {
            Ok(session) => Ok(session),
            Err(primary_error @ SessionGenerationWireError::UnsupportedFutureVersion { .. }) => {
                Err(primary_error)
            }
            Err(primary_error) => {
                let previous_manifest = self
                    .dataset
                    .read_previous(&dataset_key, &[manifest_name])
                    .await
                    .map_err(storage_error)?;
                match self
                    .decode_generation(&dataset_key, Generation::Previous, previous_manifest)
                    .await
                {
                    Ok(session) => Ok(session),
                    Err(_) => Err(primary_error),
                }
            }
        }
    }

    async fn load_and_migrate_legacy(
        &self,
        session_id: &str,
    ) -> Result<CanonicalSession, SessionGenerationWireError> {
        let Some(blob) = &self.legacy_blob else {
            return Err(SessionGenerationWireError::InvalidManifest(
                "Session generation 不存在".to_string(),
            ));
        };
        let store = AtomicBlobSessionStore::new(Arc::clone(blob), session_id)
            .map(Arc::new)
            .map_err(|error| SessionGenerationWireError::InvalidManifest(error.to_string()))?;
        let persistence = SessionPersistenceService::new(store, Arc::new(LegacySessionDecoder));
        let session = persistence
            .load()
            .await
            .map_err(|error| SessionGenerationWireError::InvalidManifest(error.to_string()))?;
        DatasetCanonicalSessionWriter::new(Arc::clone(&self.dataset))
            .save_initial(&session)
            .await
            .map_err(SessionGenerationWireError::InvalidManifest)?;
        Ok(session)
    }

    async fn decode_generation(
        &self,
        dataset_key: &DatasetKey,
        generation: Generation,
        manifest_read: DatasetReadOutcome,
    ) -> Result<CanonicalSession, SessionGenerationWireError> {
        let DatasetReadOutcome::Found(manifest_read) = manifest_read else {
            return Err(SessionGenerationWireError::InvalidManifest(
                "Session generation 不存在".to_string(),
            ));
        };
        let manifest_bytes = only_member_bytes(manifest_read.members())?;
        let manifest = SessionGenerationCodec::decode_manifest(manifest_bytes)?;
        if format!("{}.dataset", manifest.session_id())
            != dataset_key
                .segments()
                .first()
                .map(SafePathSegment::as_str)
                .unwrap_or_default()
        {
            return Err(SessionGenerationWireError::InvalidManifest(
                "Session generation identity 与数据集键不一致".to_string(),
            ));
        }

        let requested_names = std::iter::once(manifest.state_member_name())
            .chain(std::iter::once(manifest.metadata_member_name()))
            .chain(manifest.steps().iter().map(|step| step.member_name()))
            .map(safe_member_name)
            .collect::<Result<Vec<_>, _>>()?;
        let outcome = match generation {
            Generation::Primary => {
                self.dataset
                    .read_consistent(dataset_key, &requested_names)
                    .await
            }
            Generation::Previous => {
                self.dataset
                    .read_previous(dataset_key, &requested_names)
                    .await
            }
        }
        .map_err(storage_error)?;
        let DatasetReadOutcome::Found(read) = outcome else {
            return Err(SessionGenerationWireError::InvalidManifest(
                "Session generation 引用成员缺失".to_string(),
            ));
        };
        assemble_session(&manifest, read.members())
    }
}

fn assemble_session(
    manifest: &SessionGenerationManifest,
    members: &[DatasetMember],
) -> Result<CanonicalSession, SessionGenerationWireError> {
    let members_by_name = members
        .iter()
        .map(|member| (member.name().as_str(), member.bytes()))
        .collect::<HashMap<_, _>>();
    let state_bytes = members_by_name
        .get(manifest.state_member_name())
        .ok_or_else(|| {
            SessionGenerationWireError::InvalidManifest("Session state member 缺失".to_string())
        })?;
    let metadata_bytes = members_by_name
        .get(manifest.metadata_member_name())
        .ok_or_else(|| {
            SessionGenerationWireError::InvalidManifest("Session metadata member 缺失".to_string())
        })?;
    let metadata = SessionGenerationCodec::decode_metadata(metadata_bytes)?;

    let state = SessionGenerationCodec::decode_state(state_bytes)?;
    if state.session_id() != manifest.session_id()
        || metadata.session_id() != manifest.session_id()
        || metadata.revision() != manifest.revision()
    {
        return Err(SessionGenerationWireError::InvalidManifest(
            "Session state 与 generation manifest 不一致".to_string(),
        ));
    }

    let mut slices = Vec::<CommittedRunSlice>::new();
    for reference in manifest.steps() {
        let bytes = members_by_name
            .get(reference.member_name())
            .ok_or_else(|| {
                SessionGenerationWireError::InvalidManifest(format!(
                    "Session step member 缺失：{}",
                    reference.member_name()
                ))
            })?;
        let member: SessionStepMember = SessionGenerationCodec::decode_step(bytes)?;
        if member.cursor() != reference.cursor() {
            return Err(SessionGenerationWireError::InvalidManifest(
                "Session step reference 与 member identity 不一致".to_string(),
            ));
        }
        if let Some(slice) = slices
            .last_mut()
            .filter(|slice| slice.run_id == member.cursor().run_id)
        {
            slice.steps.push(member.step().clone());
        } else {
            slices.push(CommittedRunSlice::new(
                member.cursor().run_id.clone(),
                vec![member.step().clone()],
            ));
        }
    }
    Ok(state.into_session(metadata, SessionHistory::from_slices(slices)))
}

fn only_member_bytes(members: &[DatasetMember]) -> Result<&[u8], SessionGenerationWireError> {
    if members.len() != 1 {
        return Err(SessionGenerationWireError::InvalidManifest(
            "Session generation manifest member 缺失".to_string(),
        ));
    }
    Ok(members[0].bytes())
}

fn session_dataset_key(session_id: &str) -> Result<DatasetKey, SessionGenerationWireError> {
    super::dataset_session_writer::session_dataset_key(session_id).map_err(storage_error)
}

fn safe_member_name(name: &str) -> Result<SafePathSegment, SessionGenerationWireError> {
    SafePathSegment::from_str(name).map_err(storage_error)
}

fn storage_error(error: StorageError) -> SessionGenerationWireError {
    SessionGenerationWireError::InvalidManifest(error.to_string())
}
