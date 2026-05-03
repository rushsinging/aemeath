use super::dedup::jaccard_similarity;
use super::entry::{current_timestamp_secs, MemoryEntry, MemoryLayer};
use super::error::{MemoryError, MemoryResult};
use super::scoring::{eviction_score, injection_score};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum AddResult {
    Added,
    Merged { existing_id: String },
    NeedsEviction { candidates: Vec<MemoryEntry> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactResult {
    pub archived: usize,
    pub remaining: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStats {
    pub global_count: usize,
    pub global_archive_count: usize,
    pub project_count: usize,
    pub project_archive_count: usize,
    pub reminders_count: usize,
}

pub struct MemoryStore {
    base_dir: PathBuf,
    project_hash: String,
    max_entries: usize,
    similarity_threshold: f64,
}

impl MemoryStore {
    pub fn new(
        base_dir: impl Into<PathBuf>,
        project_hash: impl Into<String>,
        max_entries: usize,
        similarity_threshold: f64,
    ) -> MemoryResult<Self> {
        if max_entries == 0 {
            return Err(MemoryError::config("max_entries 必须大于 0"));
        }
        if !(0.0..=1.0).contains(&similarity_threshold) {
            return Err(MemoryError::config(
                "similarity_threshold 必须在 0 到 1 之间",
            ));
        }

        Ok(Self {
            base_dir: base_dir.into(),
            project_hash: project_hash.into(),
            max_entries,
            similarity_threshold,
        })
    }

    pub fn add(&mut self, mut entry: MemoryEntry) -> MemoryResult<AddResult> {
        self.validate_entry(&entry)?;
        let mut entries = self.read_active(entry.layer)?;

        if let Some(existing) = entries.iter_mut().find(|existing| {
            jaccard_similarity(&existing.content, &entry.content) >= self.similarity_threshold
        }) {
            existing.tags.extend(entry.tags.clone());
            existing.tags.sort();
            existing.tags.dedup();
            existing.touch(current_timestamp_secs());
            let existing_id = existing.id.clone();
            self.write_active(entry.layer, &entries)?;
            return Ok(AddResult::Merged { existing_id });
        }

        if entries.len() >= self.max_entries {
            let candidates = self.eviction_candidates_from_entries(&entries, 3);
            if !candidates.is_empty() {
                return Ok(AddResult::NeedsEviction { candidates });
            }
        }

        if entry.id.trim().is_empty() {
            entry.id = uuid::Uuid::now_v7().to_string();
        }
        entries.push(entry);
        self.write_active(
            entries
                .last()
                .map(|e| e.layer)
                .unwrap_or(MemoryLayer::Project),
            &entries,
        )?;
        Ok(AddResult::Added)
    }

    pub fn delete(&mut self, id: &str) -> MemoryResult<()> {
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let mut entries = self.read_active(layer)?;
            let before = entries.len();
            entries.retain(|entry| entry.id != id);
            if entries.len() != before {
                self.write_active(layer, &entries)?;
                return Ok(());
            }
        }
        Err(MemoryError::not_found(id))
    }

    pub fn update(&mut self, id: &str, content: &str) -> MemoryResult<()> {
        if content.trim().is_empty() {
            return Err(MemoryError::invalid_input("记忆内容不能为空"));
        }
        self.update_entry(id, |entry| {
            entry.content = content.to_string();
        })
    }

    pub fn pin(&mut self, id: &str, pinned: bool) -> MemoryResult<()> {
        self.update_entry(id, |entry| {
            entry.pinned = pinned;
        })
    }

    pub fn mark_outdated(&mut self, id: &str) -> MemoryResult<()> {
        self.update_entry(id, |entry| {
            entry.outdated = true;
        })
    }

    pub fn search(&self, query: &str, limit: usize) -> MemoryResult<Vec<MemoryEntry>> {
        let query = query.to_lowercase();
        let mut results = self.all_active()?;
        results.extend(self.all_archive()?);
        results.retain(|entry| entry_matches(entry, &query));
        self.sort_for_inject(&mut results);
        results.truncate(limit);
        Ok(results)
    }

    pub fn top_for_inject(&mut self, limit: usize) -> MemoryResult<Vec<MemoryEntry>> {
        let now = current_timestamp_secs();
        let mut all = self.all_active()?;
        self.sort_for_inject(&mut all);
        all.truncate(limit);

        for entry in &all {
            let _ = self.update_entry(&entry.id, |stored| stored.touch(now));
        }
        Ok(all)
    }

    pub fn needs_eviction(&self, layer: MemoryLayer) -> MemoryResult<bool> {
        Ok(self.read_active(layer)?.len() >= self.max_entries)
    }

    pub fn eviction_candidates(
        &self,
        layer: MemoryLayer,
        count: usize,
    ) -> MemoryResult<Vec<MemoryEntry>> {
        let entries = self.read_active(layer)?;
        Ok(self.eviction_candidates_from_entries(&entries, count))
    }

    pub fn archive_entries(&mut self, ids: &[String]) -> MemoryResult<()> {
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let mut active = self.read_active(layer)?;
            let mut archived = self.read_archive(layer)?;
            let mut moved = Vec::new();
            active.retain(|entry| {
                if ids.contains(&entry.id) {
                    moved.push(entry.clone());
                    false
                } else {
                    true
                }
            });
            if !moved.is_empty() {
                archived.extend(moved);
                self.write_active(layer, &active)?;
                self.write_archive(layer, &archived)?;
            }
        }
        Ok(())
    }

    pub fn compact(&mut self) -> MemoryResult<CompactResult> {
        let mut archived = 0;
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            if self.needs_eviction(layer)? {
                let candidates = self.eviction_candidates(layer, 10)?;
                let ids = candidates
                    .into_iter()
                    .map(|entry| entry.id)
                    .collect::<Vec<_>>();
                archived += ids.len();
                self.archive_entries(&ids)?;
            }
        }
        Ok(CompactResult {
            archived,
            remaining: self.all_active()?.len(),
        })
    }

    pub fn stats(&self, reminders_count: usize) -> MemoryResult<MemoryStats> {
        Ok(MemoryStats {
            global_count: self.read_active(MemoryLayer::Global)?.len(),
            global_archive_count: self.read_archive(MemoryLayer::Global)?.len(),
            project_count: self.read_active(MemoryLayer::Project)?.len(),
            project_archive_count: self.read_archive(MemoryLayer::Project)?.len(),
            reminders_count,
        })
    }

    pub fn list(&self, layer: Option<MemoryLayer>) -> MemoryResult<Vec<MemoryEntry>> {
        match layer {
            Some(layer) => self.read_active(layer),
            None => self.all_active(),
        }
    }

    fn validate_entry(&self, entry: &MemoryEntry) -> MemoryResult<()> {
        if entry.content.trim().is_empty() {
            return Err(MemoryError::invalid_input("记忆内容不能为空"));
        }
        if entry.content.chars().count() > 500 {
            return Err(MemoryError::invalid_input("记忆内容不能超过 500 字符"));
        }
        Ok(())
    }

    fn update_entry<F>(&mut self, id: &str, mut update: F) -> MemoryResult<()>
    where
        F: FnMut(&mut MemoryEntry),
    {
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let mut entries = self.read_active(layer)?;
            if let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) {
                update(entry);
                self.write_active(layer, &entries)?;
                return Ok(());
            }
        }
        Err(MemoryError::not_found(id))
    }

    fn all_active(&self) -> MemoryResult<Vec<MemoryEntry>> {
        let mut entries = self.read_active(MemoryLayer::Global)?;
        entries.extend(self.read_active(MemoryLayer::Project)?);
        Ok(entries)
    }

    fn all_archive(&self) -> MemoryResult<Vec<MemoryEntry>> {
        let mut entries = self.read_archive(MemoryLayer::Global)?;
        entries.extend(self.read_archive(MemoryLayer::Project)?);
        Ok(entries)
    }

    fn sort_for_inject(&self, entries: &mut [MemoryEntry]) {
        let now = current_timestamp_secs();
        entries.sort_by_key(|entry| std::cmp::Reverse(injection_score(entry, now)));
    }

    fn eviction_candidates_from_entries(
        &self,
        entries: &[MemoryEntry],
        count: usize,
    ) -> Vec<MemoryEntry> {
        let now = current_timestamp_secs();
        let mut candidates = entries
            .iter()
            .filter(|entry| !entry.pinned)
            .cloned()
            .collect::<Vec<_>>();
        candidates.sort_by_key(|entry| eviction_score(entry, now));
        candidates.truncate(count);
        candidates
    }

    fn read_active(&self, layer: MemoryLayer) -> MemoryResult<Vec<MemoryEntry>> {
        self.read_entries(&self.active_path(layer))
    }

    fn write_active(&self, layer: MemoryLayer, entries: &[MemoryEntry]) -> MemoryResult<()> {
        self.write_entries(&self.active_path(layer), entries)
    }

    fn read_archive(&self, layer: MemoryLayer) -> MemoryResult<Vec<MemoryEntry>> {
        self.read_entries(&self.archive_path(layer))
    }

    fn write_archive(&self, layer: MemoryLayer, entries: &[MemoryEntry]) -> MemoryResult<()> {
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

    fn active_path(&self, layer: MemoryLayer) -> PathBuf {
        match layer {
            MemoryLayer::Global => self.base_dir.join("_global.json"),
            MemoryLayer::Project => self.base_dir.join(format!("{}.json", self.project_hash)),
        }
    }

    fn archive_path(&self, layer: MemoryLayer) -> PathBuf {
        match layer {
            MemoryLayer::Global => self.base_dir.join("_global_archive.json"),
            MemoryLayer::Project => self
                .base_dir
                .join(format!("{}_archive.json", self.project_hash)),
        }
    }
}

fn entry_matches(entry: &MemoryEntry, query: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    entry.content.to_lowercase().contains(query)
        || entry
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(query))
        || format!("{:?}", entry.category)
            .to_lowercase()
            .contains(query)
        || format!("{:?}", entry.layer).to_lowercase().contains(query)
}
