use std::{io, path::Path, sync::Arc};

use async_trait::async_trait;

use crate::contract::{
    AtomicBlobPort, ReadOutcome, StorageError, StorageKey, WriteOptions, WriteReceipt,
};

mod commit;
mod path;

use commit::{CommitHooks, StageNameSource, UuidStageNameSource};
use path::CapabilityRoot;

#[derive(Clone)]
pub struct FileAtomicBlobAdapter {
    root: Arc<CapabilityRoot>,
    stage_names: Arc<dyn StageNameSource>,
    hooks: Arc<dyn CommitHooks>,
}

impl FileAtomicBlobAdapter {
    pub fn open(root: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            root: Arc::new(CapabilityRoot::open(root.as_ref())?),
            stage_names: Arc::new(UuidStageNameSource),
            hooks: Arc::new(commit::NoopCommitHooks),
        })
    }

    #[cfg(test)]
    fn with_test_seams(
        root: impl AsRef<Path>,
        stage_names: Arc<dyn StageNameSource>,
        hooks: Arc<dyn CommitHooks>,
    ) -> io::Result<Self> {
        Ok(Self {
            root: Arc::new(CapabilityRoot::open(root.as_ref())?),
            stage_names,
            hooks,
        })
    }
}

#[async_trait]
impl AtomicBlobPort for FileAtomicBlobAdapter {
    async fn read(&self, key: &StorageKey) -> Result<ReadOutcome, StorageError> {
        let root = Arc::clone(&self.root);
        let key = key.clone();
        let task_key = key.clone();
        tokio::task::spawn_blocking(move || root.read(&task_key))
            .await
            .map_err(|error| {
                path::join_error(&key, crate::contract::StorageOperation::Read, error)
            })?
    }

    async fn write_atomic(
        &self,
        key: &StorageKey,
        bytes: &[u8],
        _options: &WriteOptions,
    ) -> Result<WriteReceipt, StorageError> {
        let root = Arc::clone(&self.root);
        let stage_names = Arc::clone(&self.stage_names);
        let hooks = Arc::clone(&self.hooks);
        let key = key.clone();
        let task_key = key.clone();
        let bytes = bytes.to_vec();
        tokio::task::spawn_blocking(move || {
            commit::write_atomic(
                &root,
                &task_key,
                &bytes,
                stage_names.as_ref(),
                hooks.as_ref(),
            )
        })
        .await
        .map_err(|error| {
            path::join_error(&key, crate::contract::StorageOperation::WriteAtomic, error)
        })?
    }
}

#[cfg(test)]
mod tests;
