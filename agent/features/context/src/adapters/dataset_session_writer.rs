use std::str::FromStr;
use std::sync::Arc;

use storage::api::{
    AtomicDatasetPort, DatasetChangeSet, DatasetKey, DatasetMember, DatasetMemberChange,
    DatasetMemberReference, Durability, SafePathSegment, StorageError, StorageErrorKind,
    StorageNamespace, WriteOptions,
};

use crate::domain::session::{
    CanonicalSession, SessionChangeSet, SessionGenerationCodec, SessionGenerationManifest,
};

pub struct DatasetCanonicalSessionWriter {
    dataset: Arc<dyn AtomicDatasetPort>,
}

#[async_trait::async_trait]
impl crate::adapters::CanonicalSessionWriter for DatasetCanonicalSessionWriter {
    async fn save(
        &self,
        before: &CanonicalSession,
        after: &CanonicalSession,
    ) -> Result<(), String> {
        self.save_incremental(before, after).await
    }
}

impl DatasetCanonicalSessionWriter {
    pub fn new(dataset: Arc<dyn AtomicDatasetPort>) -> Self {
        Self { dataset }
    }

    pub async fn save_initial(&self, session: &CanonicalSession) -> Result<(), String> {
        let changes = SessionChangeSet::initial(session).map_err(|error| error.to_string())?;
        self.commit(session.id.as_str(), changes).await
    }

    pub async fn save_incremental(
        &self,
        before: &CanonicalSession,
        after: &CanonicalSession,
    ) -> Result<(), String> {
        let changes =
            SessionChangeSet::between(before, after).map_err(|error| error.to_string())?;
        let dataset_key =
            session_dataset_key(after.id.as_str()).map_err(|error| error.to_string())?;
        let manifest = self
            .dataset
            .read_manifest(&dataset_key)
            .await
            .map_err(|error| error.to_string())?;
        if manifest.members().is_empty() {
            return self.save_initial(after).await;
        }
        verify_expected_generation(&manifest, before).map_err(|error| error.to_string())?;
        self.commit_with_manifest(&dataset_key, &manifest, changes)
            .await
    }

    async fn commit(&self, session_id: &str, changes: SessionChangeSet) -> Result<(), String> {
        let dataset_key = session_dataset_key(session_id).map_err(|error| error.to_string())?;
        let manifest = self
            .dataset
            .read_manifest(&dataset_key)
            .await
            .map_err(|error| error.to_string())?;
        self.commit_with_manifest(&dataset_key, &manifest, changes)
            .await
    }

    async fn commit_with_manifest(
        &self,
        dataset_key: &DatasetKey,
        manifest: &storage::api::DatasetManifest,
        changes: SessionChangeSet,
    ) -> Result<(), String> {
        let dataset_changes =
            map_session_changes(manifest, changes).map_err(|error| error.to_string())?;
        self.dataset
            .commit_incremental(
                dataset_key,
                &dataset_changes,
                WriteOptions::new(Durability::ProcessCrashSafe),
            )
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

fn verify_expected_generation(
    manifest: &storage::api::DatasetManifest,
    expected: &CanonicalSession,
) -> Result<(), StorageError> {
    let manifest_name = SafePathSegment::from_str("manifest.json")?;
    let Some(manifest_member) = manifest.member_evidence(&manifest_name) else {
        return Err(StorageError::new(
            StorageErrorKind::ConcurrentWrite,
            "Session 当前数据集代缺少领域 manifest",
        ));
    };
    let expected_steps = expected
        .run_slices
        .iter()
        .flat_map(|run_slice| {
            run_slice.steps.iter().cloned().map(|step| {
                crate::domain::session::SessionStepMember::new(
                    crate::domain::session::RunStepCursor {
                        run_id: run_slice.run_id.clone(),
                        step_id: step.step_id.clone(),
                    },
                    step,
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| StorageError::new(StorageErrorKind::InvalidKey, error.to_string()))?;
    let expected_manifest = SessionGenerationManifest::new(
        expected.id.clone(),
        expected.revision,
        expected_steps
            .iter()
            .map(|step| step.cursor().clone())
            .collect(),
    )
    .and_then(|manifest| manifest.with_step_metadata(&expected_steps))
    .and_then(|manifest| SessionGenerationCodec::encode_manifest(&manifest))
    .map_err(|error| StorageError::new(StorageErrorKind::InvalidKey, error.to_string()))?;
    if manifest_member.byte_len() != expected_manifest.len() as u64
        || !manifest_member.matches_bytes(&expected_manifest)
    {
        return Err(StorageError::new(
            StorageErrorKind::ConcurrentWrite,
            "Session 数据集修订号已变更，增量提交被拒绝",
        ));
    }
    Ok(())
}

pub(super) fn session_dataset_key(session_id: &str) -> Result<DatasetKey, StorageError> {
    DatasetKey::new(
        StorageNamespace::Session,
        vec![SafePathSegment::from_str(&format!("{session_id}.dataset"))?],
    )
}

fn map_session_changes(
    manifest: &storage::api::DatasetManifest,
    changes: SessionChangeSet,
) -> Result<DatasetChangeSet, StorageError> {
    let changed_members = changes
        .changed_members()
        .iter()
        .map(|member| {
            Ok(DatasetMemberChange::Replace(DatasetMember::new(
                SafePathSegment::from_str(member.name())?,
                member.bytes().to_vec(),
            )))
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    let reused_members = changes
        .reused_members()
        .iter()
        .map(|name| {
            let safe_name = SafePathSegment::from_str(name)?;
            manifest
                .member_evidence(&safe_name)
                .cloned()
                .ok_or_else(|| missing_reuse_evidence(name))
        })
        .collect::<Result<Vec<DatasetMemberReference>, StorageError>>()?;
    let removed_members = changes
        .removed_members()
        .iter()
        .map(|name| SafePathSegment::from_str(name))
        .collect::<Result<Vec<_>, StorageError>>()?;

    DatasetChangeSet::new(manifest.revision().clone(), changed_members, reused_members)?
        .with_removed_members(removed_members)
}

fn missing_reuse_evidence(name: &str) -> StorageError {
    StorageError::new(
        StorageErrorKind::ConcurrentWrite,
        format!("Session 待复用成员不属于当前数据集修订号：{name}"),
    )
}
