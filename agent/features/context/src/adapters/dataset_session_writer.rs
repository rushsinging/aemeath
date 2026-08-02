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
        scope: crate::adapters::SessionWriteScope,
    ) -> Result<(), String> {
        self.save_incremental(before, after, scope).await
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
        scope: crate::adapters::SessionWriteScope,
    ) -> Result<(), String> {
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
        let manifest_member_name =
            SafePathSegment::from_str(SessionGenerationManifest::manifest_member_name())
                .map_err(|error| error.to_string())?;
        let persisted_manifest = self
            .dataset
            .read_consistent(&dataset_key, std::slice::from_ref(&manifest_member_name))
            .await
            .map_err(|error| error.to_string())?;
        let storage::api::DatasetReadOutcome::Found(persisted_manifest) = persisted_manifest else {
            return Err("Session generation manifest 不存在".to_string());
        };
        let persisted_manifest = SessionGenerationCodec::decode_manifest(
            persisted_manifest
                .members()
                .first()
                .ok_or_else(|| "Session generation manifest 不存在".to_string())?
                .bytes(),
        )
        .map_err(|error| error.to_string())?;
        let changes = match scope {
            crate::adapters::SessionWriteScope::PreserveUnloadedHistory => {
                SessionChangeSet::between_preserving_unloaded_steps(
                    before,
                    after,
                    &persisted_manifest,
                )
            }
            crate::adapters::SessionWriteScope::ReplaceCompleteHistory => {
                SessionChangeSet::between(before, after)
            }
        }
        .map_err(|error| error.to_string())?;
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
