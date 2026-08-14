use std::str::FromStr;
use std::sync::Arc;

use storage::api::{
    AtomicDatasetPort, DatasetChangeSet, DatasetKey, DatasetMember, DatasetMemberChange,
    DatasetMemberReference, Durability, SafePathSegment, StorageError, StorageErrorKind,
    StorageNamespace, WriteOptions,
};

use crate::domain::session::{
    CanonicalSession, SessionCommitPlan, SessionGenerationCodec, SessionGenerationManifest,
};

pub struct DatasetCanonicalSessionWriter {
    dataset: Arc<dyn AtomicDatasetPort>,
}

#[async_trait::async_trait]
impl crate::adapters::CanonicalSessionWriter for DatasetCanonicalSessionWriter {
    async fn commit(
        &self,
        session_id: &str,
        expected_revision: u64,
        plan: SessionCommitPlan,
    ) -> Result<(), String> {
        self.commit_plan(session_id, expected_revision, plan).await
    }

    /// 数据集为空集时以内存全量快照重建：磁盘是空集、内存是唯一真相源，
    /// 全量重建不丢任何数据。数据集非空时 fail-closed（防覆盖并发写者）。
    ///
    /// 重建后磁盘 manifest 修订号 = session.revision，后续增量提交自然对齐。
    async fn rebuild_empty_dataset(
        &self,
        session_id: &str,
        session: &CanonicalSession,
    ) -> Result<(), String> {
        let dataset_key = session_dataset_key(session_id).map_err(|error| error.to_string())?;
        let manifest = self
            .dataset
            .read_manifest(&dataset_key)
            .await
            .map_err(|error| error.to_string())?;
        if !manifest.members().is_empty() {
            return Err("Session 数据集非空，拒绝全量重建".to_string());
        }
        if session.revision == 0 {
            return Err("Session 修订号为 0，无需全量重建".to_string());
        }
        let plan =
            SessionCommitPlan::complete_snapshot(session).map_err(|error| error.to_string())?;
        plan.validate_rebuild_boundary(session_id, session.revision - 1)
            .map_err(|error| error.to_string())?;
        self.commit(session_id, plan).await
    }
}

impl DatasetCanonicalSessionWriter {
    pub fn new(dataset: Arc<dyn AtomicDatasetPort>) -> Self {
        Self { dataset }
    }

    pub async fn save_initial(&self, session: &CanonicalSession) -> Result<(), String> {
        let changes = SessionCommitPlan::initial(session).map_err(|error| error.to_string())?;
        self.commit(session.id.as_str(), changes).await
    }

    pub async fn save_incremental(
        &self,
        before: &CanonicalSession,
        after: &CanonicalSession,
        intent: crate::adapters::SessionSaveIntent,
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
        let mut changes = match intent {
            crate::adapters::SessionSaveIntent::CommitPartialHistory => {
                SessionCommitPlan::between_preserving_unloaded_steps(
                    before,
                    after,
                    &persisted_manifest,
                )
            }
            crate::adapters::SessionSaveIntent::ReplaceCompleteHistory => {
                SessionCommitPlan::between(before, after)
            }
        }
        .map_err(|error| error.to_string())?;
        changes
            .validate_commit_boundary(after.id.as_str(), before.revision, &persisted_manifest)
            .map_err(|error| error.to_string())?;
        promote_missing_reuse_evidence(&manifest, &mut changes)?;
        self.commit_with_manifest(&dataset_key, &manifest, changes)
            .await
    }

    pub async fn commit_plan(
        &self,
        session_id: &str,
        expected_revision: u64,
        mut plan: SessionCommitPlan,
    ) -> Result<(), String> {
        let dataset_key = session_dataset_key(session_id).map_err(|error| error.to_string())?;
        let manifest = self
            .dataset
            .read_manifest(&dataset_key)
            .await
            .map_err(|error| error.to_string())?;
        if manifest.members().is_empty() {
            plan.validate_initial_commit_boundary(session_id, expected_revision)
                .map_err(|error| error.to_string())?;
            promote_missing_reuse_evidence(&manifest, &mut plan)?;
            return self
                .commit_with_manifest(&dataset_key, &manifest, plan)
                .await;
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
        let persisted_revision = persisted_manifest.revision();
        if persisted_revision != expected_revision {
            return Err(format!(
                "Session 数据集修订号已变更: expected={expected_revision}, actual={persisted_revision}"
            ));
        }
        plan.validate_commit_boundary(session_id, expected_revision, &persisted_manifest)
            .map_err(|error| error.to_string())?;
        plan.reconcile_persisted_steps(&persisted_manifest)
            .map_err(|error| error.to_string())?;
        promote_missing_reuse_evidence(&manifest, &mut plan)?;
        self.commit_with_manifest(&dataset_key, &manifest, plan)
            .await
    }

    async fn commit(&self, session_id: &str, changes: SessionCommitPlan) -> Result<(), String> {
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
        changes: SessionCommitPlan,
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

fn promote_missing_reuse_evidence(
    manifest: &storage::api::DatasetManifest,
    plan: &mut SessionCommitPlan,
) -> Result<(), String> {
    plan.promote_reuse_fallbacks(|name| {
        SafePathSegment::from_str(name)
            .ok()
            .is_some_and(|safe_name| manifest.member_evidence(&safe_name).is_some())
    });
    let missing_name = plan.reused_members().iter().find(|name| {
        SafePathSegment::from_str(name)
            .ok()
            .is_none_or(|safe_name| manifest.member_evidence(&safe_name).is_none())
    });
    if let Some(name) = missing_name {
        return Err(missing_reuse_evidence(name).to_string());
    }
    Ok(())
}

fn map_session_changes(
    manifest: &storage::api::DatasetManifest,
    changes: SessionCommitPlan,
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
