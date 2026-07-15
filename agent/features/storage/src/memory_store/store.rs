use query::{current_timestamp_secs, entry_matches};
use share::memory::dedup::jaccard_similarity;
use share::memory::entry::{MemoryEntry, MemoryLayer};
use share::memory::error::{MemoryError, MemoryResult};
use share::memory::result::{AddResult, CompactResult, MemoryStats};
use share::memory::scoring::{eviction_score, injection_score};
use std::path::PathBuf;

mod persistence;
mod query;
mod validation;

#[cfg(test)]
mod tests;

pub struct MemoryStore {
    base_dir: PathBuf,
    project_file_name: String,
    max_entries: usize,
    similarity_threshold: f64,
}

impl MemoryStore {
    pub fn new(
        base_dir: impl Into<PathBuf>,
        project_file_name: impl Into<String>,
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
            project_file_name: project_file_name.into(),
            max_entries,
            similarity_threshold,
        })
    }

    pub fn add(&mut self, mut entry: MemoryEntry) -> MemoryResult<AddResult> {
        validation::validate_entry(&entry)?;
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
            return Ok(AddResult::NeedsEviction { candidates });
        }

        if entry.id.trim().is_empty() {
            entry.id = uuid::Uuid::now_v7().to_string();
        }
        let entry_id = entry.id.clone();
        entries.push(entry);
        self.write_active(
            entries
                .last()
                .map(|e| e.layer)
                .unwrap_or(MemoryLayer::Project),
            &entries,
        )?;
        Ok(AddResult::Added { id: entry_id })
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

    /// 只读版 `top_for_inject`：返回 top N 条目但**不 touch**（不更新 accessed_at）。
    /// 用于每轮 LLM 调用前的 memory 注入，避免排序漂移。
    pub fn top_for_inject_readonly(&self, limit: usize) -> MemoryResult<Vec<MemoryEntry>> {
        let mut all = self.all_active()?;
        self.sort_for_inject(&mut all);
        all.truncate(limit);
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

    pub fn evict(&mut self, ids: &[String]) -> MemoryResult<()> {
        self.archive_entries(ids)
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
}
