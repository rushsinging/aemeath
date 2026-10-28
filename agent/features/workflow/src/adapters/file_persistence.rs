//! 文件系统持久化（#2083）：`~/.agents/workflows/` 下的索引与实体。
//!
//! 布局：`index/{session_id}.json`（workflow_id 列表 + active 指针）与
//! `entities/{workflow_id}.json`（聚合快照）。全部写入走 temp+rename
//! 原子替换；损坏数据 fail-closed（Corrupt），不静默重置。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::domain::snapshot::WorkflowSnapshotData;
use crate::ports::{PersistenceError, WorkflowPersistence};

/// Session 索引文件形态。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct SessionIndexFile {
    workflow_ids: BTreeSet<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_workflow_id: Option<String>,
}

/// 文件系统持久化实现；同一 root 可多实例共享（进程内串行化索引写）。
pub struct FileWorkflowPersistence {
    root: PathBuf,
    index_lock: Mutex<()>,
}

impl FileWorkflowPersistence {
    /// root 通常为 `~/.agents/workflows`。
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            index_lock: Mutex::new(()),
        }
    }

    fn entity_path(&self, workflow_id: &str) -> PathBuf {
        self.root
            .join("entities")
            .join(format!("{workflow_id}.json"))
    }

    fn index_path(&self, session_id: &str) -> PathBuf {
        self.root.join("index").join(format!("{session_id}.json"))
    }

    fn load_index(&self, session_id: &str) -> Result<SessionIndexFile, PersistenceError> {
        let path = self.index_path(session_id);
        if !path.exists() {
            return Ok(SessionIndexFile::default());
        }
        let bytes = fs::read(&path).map_err(io(&path))?;
        serde_json::from_slice(&bytes).map_err(|_| corrupt(&path))
    }

    fn store_index(
        &self,
        session_id: &str,
        index: &SessionIndexFile,
    ) -> Result<(), PersistenceError> {
        let path = self.index_path(session_id);
        atomic_write(&path, serde_json::to_vec(index).expect("index serde")).map_err(io(&path))
    }
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> PersistenceError + '_ {
    move |error| PersistenceError::Io(format!("{}: {error}", path.display()))
}

fn corrupt(path: &Path) -> PersistenceError {
    PersistenceError::Corrupt(path.display().to_string())
}

/// temp + rename 原子替换；temp 文件与目标同目录保证同文件系统。
fn atomic_write(target: &Path, bytes: Vec<u8>) -> std::io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| std::io::Error::other("target has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.tmp",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workflow")
    ));
    fs::write(&temp, &bytes)?;
    fs::rename(&temp, target)?;
    Ok(())
}

impl WorkflowPersistence for FileWorkflowPersistence {
    fn save(
        &self,
        session_id: &str,
        snapshot: &WorkflowSnapshotData,
    ) -> Result<(), PersistenceError> {
        let entity_path = self.entity_path(&snapshot.workflow_id);
        let bytes = serde_json::to_vec(snapshot).map_err(|_| corrupt(&entity_path))?;
        atomic_write(&entity_path, bytes).map_err(io(&entity_path))?;

        let _guard = self.index_lock.lock().expect("index lock");
        let mut index = self.load_index(session_id)?;
        index.workflow_ids.insert(snapshot.workflow_id.clone());
        if snapshot.status == crate::domain::WorkflowStatus::Active {
            index.active_workflow_id = Some(snapshot.workflow_id.clone());
        } else if index.active_workflow_id.as_deref() == Some(snapshot.workflow_id.as_str()) {
            index.active_workflow_id = None;
        }
        self.store_index(session_id, &index)
    }

    fn load_all(&self, session_id: &str) -> Result<Vec<WorkflowSnapshotData>, PersistenceError> {
        let index = self.load_index(session_id)?;
        let mut snapshots = Vec::new();
        for workflow_id in &index.workflow_ids {
            let path = self.entity_path(workflow_id);
            if !path.exists() {
                // 实体缺失视为损坏（索引引用悬空），fail-closed。
                return Err(corrupt(&path));
            }
            let bytes = fs::read(&path).map_err(io(&path))?;
            let snapshot: WorkflowSnapshotData =
                serde_json::from_slice(&bytes).map_err(|_| corrupt(&path))?;
            if snapshot.workflow_id != *workflow_id {
                return Err(corrupt(&path));
            }
            snapshots.push(snapshot);
        }
        Ok(snapshots)
    }

    fn clear_index(&self, session_id: &str) -> Result<(), PersistenceError> {
        let _guard = self.index_lock.lock().expect("index lock");
        let path = self.index_path(session_id);
        if path.exists() {
            fs::remove_file(&path).map_err(io(&path))?;
        }
        Ok(())
    }
}
