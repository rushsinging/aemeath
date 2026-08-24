use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;

use storage::api::{
    AtomicDatasetPort, DatasetKey, DatasetMember, DatasetReadOutcome, Generation, SafePathSegment,
    StorageError,
};

use crate::adapters::{
    AtomicBlobSessionStore, DatasetCanonicalSessionWriter, LegacySessionDecoder,
};
use crate::application::SessionPersistenceService;
use crate::domain::session::{
    CanonicalSession, CommittedRunSlice, DisplayHistoryStepIndex, DisplayHistoryStepWindow,
    RunStepCursor, SessionGenerationCodec, SessionGenerationManifest, SessionGenerationWireError,
    SessionHistory, SessionStepMember, SessionStepReference,
};

pub struct PreparedDatasetResume {
    pub active_session: CanonicalSession,
    pub display_history: DisplayHistoryStepIndex,
}

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
        self.load_for_resume(session_id)
            .await
            .map(|prepared| prepared.active_session)
    }

    pub async fn load_for_resume(
        &self,
        session_id: &str,
    ) -> Result<PreparedDatasetResume, SessionGenerationWireError> {
        let started = Instant::now();
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
        log::debug!(
            target: crate::LOG_TARGET,
            "session_resume dataset_manifest_loaded session_id={} elapsed_ms={}",
            session_id,
            started.elapsed().as_secs_f64() * 1000.0
        );
        if matches!(primary_manifest, DatasetReadOutcome::NotFound) {
            return self.load_and_migrate_legacy(session_id).await;
        }

        match self
            .decode_generation(&dataset_key, Generation::Primary, primary_manifest)
            .await
        {
            Ok(session) => {
                log::debug!(
                    target: crate::LOG_TARGET,
                    "session_resume dataset_generation_decoded session_id={} elapsed_ms={}",
                    session_id,
                    started.elapsed().as_secs_f64() * 1000.0
                );
                Ok(session)
            }
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

    pub async fn load_display_history_steps(
        &self,
        session_id: &str,
        generation_revision: u64,
        member_names: &[String],
    ) -> Result<DisplayHistoryStepWindow, SessionGenerationWireError> {
        let dataset_key = session_dataset_key(session_id)?;
        let manifest_name = safe_member_name(SessionGenerationManifest::manifest_member_name())?;
        let manifest_outcome = self
            .dataset
            .read_consistent(&dataset_key, std::slice::from_ref(&manifest_name))
            .await
            .map_err(storage_error)?;
        let DatasetReadOutcome::Found(manifest_read) = manifest_outcome else {
            return Err(SessionGenerationWireError::InvalidManifest(
                "Session generation 不存在".to_string(),
            ));
        };
        let manifest =
            SessionGenerationCodec::decode_manifest(only_member_bytes(manifest_read.members())?)?;
        if manifest.session_id() != session_id || manifest.revision() != generation_revision {
            return Err(SessionGenerationWireError::StaleDisplayHistory {
                expected_revision: generation_revision,
                actual_revision: manifest.revision(),
            });
        }
        let references_by_name = manifest
            .steps()
            .iter()
            .map(|reference| (reference.member_name(), reference.cursor()))
            .collect::<HashMap<_, _>>();
        let safe_names = member_names
            .iter()
            .map(|name| {
                if !references_by_name.contains_key(name.as_str()) {
                    return Err(SessionGenerationWireError::InvalidManifest(format!(
                        "display history member 不属于当前 generation：{name}"
                    )));
                }
                safe_member_name(name)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let outcome = self
            .dataset
            .read_consistent(&dataset_key, &safe_names)
            .await
            .map_err(storage_error)?;
        let DatasetReadOutcome::Found(read) = outcome else {
            return Err(SessionGenerationWireError::InvalidManifest(
                "display history member 缺失".to_string(),
            ));
        };
        let members_by_name = read
            .members()
            .iter()
            .map(|member| (member.name().as_str(), member.bytes()))
            .collect::<HashMap<_, _>>();
        let steps = member_names
            .iter()
            .map(|name| {
                let bytes = members_by_name.get(name.as_str()).ok_or_else(|| {
                    SessionGenerationWireError::InvalidManifest(format!(
                        "display history member 缺失：{name}"
                    ))
                })?;
                let member = SessionGenerationCodec::decode_step(bytes)?;
                if references_by_name.get(name.as_str()).copied() != Some(member.cursor()) {
                    return Err(SessionGenerationWireError::InvalidManifest(
                        "display history member identity 不一致".to_string(),
                    ));
                }
                Ok(member)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(DisplayHistoryStepWindow::new(
            session_id,
            generation_revision,
            steps,
        ))
    }

    async fn load_and_migrate_legacy(
        &self,
        session_id: &str,
    ) -> Result<PreparedDatasetResume, SessionGenerationWireError> {
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
        let manifest = SessionGenerationManifest::new(
            session.id.clone(),
            session.revision,
            session
                .run_slices
                .iter()
                .flat_map(|slice| {
                    slice
                        .steps
                        .iter()
                        .map(|step| crate::domain::session::RunStepCursor {
                            run_id: slice.run_id.clone(),
                            step_id: step.step_id.clone(),
                        })
                })
                .collect(),
        )?
        .with_step_metadata(
            &session
                .run_slices
                .iter()
                .flat_map(|slice| {
                    slice.steps.iter().map(|step| {
                        crate::domain::session::SessionStepMember::new(
                            crate::domain::session::RunStepCursor {
                                run_id: slice.run_id.clone(),
                                step_id: step.step_id.clone(),
                            },
                            step.clone(),
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        )?;
        Ok(PreparedDatasetResume {
            display_history: DisplayHistoryStepIndex::from_session_and_manifest(
                &session, &manifest,
            ),
            active_session: session,
        })
    }

    async fn decode_generation(
        &self,
        dataset_key: &DatasetKey,
        generation: Generation,
        manifest_read: DatasetReadOutcome,
    ) -> Result<PreparedDatasetResume, SessionGenerationWireError> {
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

        let base_names = vec![
            safe_member_name(manifest.state_member_name())?,
            safe_member_name(manifest.metadata_member_name())?,
        ];
        let base_outcome = match generation {
            Generation::Primary => self.dataset.read_consistent(dataset_key, &base_names).await,
            Generation::Previous => self.dataset.read_previous(dataset_key, &base_names).await,
        }
        .map_err(storage_error)?;
        let DatasetReadOutcome::Found(base_read) = base_outcome else {
            return Err(SessionGenerationWireError::InvalidManifest(
                "Session state 或 metadata member 缺失".to_string(),
            ));
        };
        let base_members = base_read.members();
        let base_by_name = base_members
            .iter()
            .map(|member| (member.name().as_str(), member.bytes()))
            .collect::<HashMap<_, _>>();
        let state = SessionGenerationCodec::decode_state(
            base_by_name
                .get(manifest.state_member_name())
                .ok_or_else(|| {
                    SessionGenerationWireError::InvalidManifest(
                        "Session state member 缺失".to_string(),
                    )
                })?,
        )?;
        let metadata = SessionGenerationCodec::decode_metadata(
            base_by_name
                .get(manifest.metadata_member_name())
                .ok_or_else(|| {
                    SessionGenerationWireError::InvalidManifest(
                        "Session metadata member 缺失".to_string(),
                    )
                })?,
        )?;
        if state.session_id() != manifest.session_id()
            || metadata.session_id() != manifest.session_id()
            || metadata.revision() != manifest.revision()
        {
            return Err(SessionGenerationWireError::InvalidManifest(
                "Session state 与 generation manifest 不一致".to_string(),
            ));
        }
        let active_start = state.compact_start_at();
        let cleared_after = state.cleared_after();
        let visible_steps = visible_steps_after_boundaries(&manifest, active_start, cleared_after)?;
        let active_names = visible_steps
            .iter()
            .map(|step| safe_member_name(step.member_name()))
            .collect::<Result<Vec<_>, _>>()?;
        let active_outcome = match generation {
            Generation::Primary => {
                self.dataset
                    .read_consistent(dataset_key, &active_names)
                    .await
            }
            Generation::Previous => self.dataset.read_previous(dataset_key, &active_names).await,
        }
        .map_err(storage_error)?;
        let DatasetReadOutcome::Found(active_read) = active_outcome else {
            return Err(SessionGenerationWireError::InvalidManifest(
                "Session active step member 缺失".to_string(),
            ));
        };
        let mut members = base_members.to_vec();
        members.extend_from_slice(active_read.members());
        log::debug!(
            target: crate::LOG_TARGET,
            "session_resume members_requested session_id={} generation={:?} steps={} active_steps={} members={}",
            manifest.session_id(),
            generation,
            manifest.steps().len(),
            active_names.len(),
            members.len()
        );
        let mut active_session = assemble_session(&manifest, &members)?;
        if let Some(blob) = &self.legacy_blob {
            crate::adapters::accepted_input_ledger::AtomicBlobAcceptedInputLedger::new(
                Arc::clone(blob),
                manifest.session_id(),
            )
            .map_err(SessionGenerationWireError::InvalidManifest)?
            .overlay(&mut active_session)
            .await
            .map_err(SessionGenerationWireError::InvalidManifest)?;
        }
        Ok(PreparedDatasetResume {
            active_session,
            display_history: DisplayHistoryStepIndex::from_manifest_after_clear(
                &manifest,
                cleared_after,
            ),
        })
    }
}

/// 按可见边界（compact 起点 + `/clear` 逻辑断点）过滤 manifest steps：
/// 取两边界中更晚者之后的 step 引用，供 active 成员加载使用。
/// clear 边界必须命中当前 generation manifest，否则视为数据不一致。
fn visible_steps_after_boundaries<'a>(
    manifest: &'a SessionGenerationManifest,
    active_start: Option<&RunStepCursor>,
    cleared_after: Option<&RunStepCursor>,
) -> Result<Vec<&'a SessionStepReference>, SessionGenerationWireError> {
    let steps_after_clear: Vec<&SessionStepReference> = match cleared_after {
        Some(cleared) => {
            let position = manifest
                .steps()
                .iter()
                .position(|step| step.cursor() == cleared)
                .ok_or_else(|| {
                    SessionGenerationWireError::InvalidManifest(
                        "Session clear 边界不在此 generation manifest 中".to_string(),
                    )
                })?;
            manifest.steps().iter().skip(position + 1).collect()
        }
        None => manifest.steps().iter().collect(),
    };
    Ok(steps_after_clear
        .into_iter()
        .skip_while(|step| {
            active_start.is_some_and(|start| {
                step.cursor().run_id != start.run_id || step.cursor().step_id != start.step_id
            })
        })
        .collect())
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

    let active_start = state.compact_start_at();
    let visible_steps =
        visible_steps_after_boundaries(manifest, active_start, state.cleared_after())?;
    let mut active = active_start.is_none();
    let mut slices = Vec::<CommittedRunSlice>::new();
    for reference in visible_steps {
        let Some(bytes) = members_by_name.get(reference.member_name()) else {
            if !active {
                continue;
            }
            return Err(SessionGenerationWireError::InvalidManifest(format!(
                "Session step member 缺失：{}",
                reference.member_name()
            )));
        };
        let member: SessionStepMember = SessionGenerationCodec::decode_step(bytes)?;
        if member.cursor() != reference.cursor() {
            return Err(SessionGenerationWireError::InvalidManifest(
                "Session step reference 与 member identity 不一致".to_string(),
            ));
        }
        if !active && active_start.is_some_and(|cursor| cursor == reference.cursor()) {
            active = true;
        }
        if !active {
            continue;
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
