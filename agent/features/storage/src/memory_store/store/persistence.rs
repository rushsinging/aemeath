use share::memory::entry::{MemoryEntry, MemoryLayer};
use share::memory::error::{MemoryError, MemoryResult};
use std::path::{Path, PathBuf};

use super::MemoryStore;

impl MemoryStore {
    pub(super) fn read_active(&self, layer: MemoryLayer) -> MemoryResult<Vec<MemoryEntry>> {
        self.read_entries(&self.active_path(layer))
    }

    pub(super) fn write_active(
        &self,
        layer: MemoryLayer,
        entries: &[MemoryEntry],
    ) -> MemoryResult<()> {
        self.write_entries(&self.active_path(layer), entries)
    }

    pub(super) fn read_archive(&self, layer: MemoryLayer) -> MemoryResult<Vec<MemoryEntry>> {
        self.read_entries(&self.archive_path(layer))
    }

    pub(super) fn write_archive(
        &self,
        layer: MemoryLayer,
        entries: &[MemoryEntry],
    ) -> MemoryResult<()> {
        self.write_entries(&self.archive_path(layer), entries)
    }

    fn read_entries(&self, path: &Path) -> MemoryResult<Vec<MemoryEntry>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|error| MemoryError::file(path.display().to_string(), error))?;
        serde_json::from_str(&content).map_err(MemoryError::json)
    }

    fn write_entries(&self, path: &Path, entries: &[MemoryEntry]) -> MemoryResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| MemoryError::file(parent.display().to_string(), error))?;
        }
        let content = serde_json::to_string_pretty(entries).map_err(MemoryError::json)?;
        std::fs::write(path, content)
            .map_err(|error| MemoryError::file(path.display().to_string(), error))
    }

    pub(super) fn active_path(&self, layer: MemoryLayer) -> PathBuf {
        match layer {
            MemoryLayer::Global => self.base_dir.join("_global.json"),
            MemoryLayer::Project => self
                .base_dir
                .join(format!("{}.json", self.project_file_name)),
        }
    }

    pub(super) fn archive_path(&self, layer: MemoryLayer) -> PathBuf {
        match layer {
            MemoryLayer::Global => self.base_dir.join("_global_archive.json"),
            MemoryLayer::Project => self
                .base_dir
                .join(format!("{}_archive.json", self.project_file_name)),
        }
    }
}
