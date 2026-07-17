use crate::*;
use async_trait::async_trait;
use std::sync::RwLock;

#[derive(Debug, Clone, Copy)]
pub struct MemoryPolicy {
    pub max_entries: usize,
    pub similarity_threshold: f64,
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        Self {
            max_entries: 100,
            similarity_threshold: 0.8,
        }
    }
}

#[derive(Default)]
struct MemoryState {
    active: Vec<MemoryEntry>,
    archive: Vec<MemoryEntry>,
    revision: u64,
}

pub struct InMemoryMemory {
    policy: MemoryPolicy,
    state: RwLock<MemoryState>,
}

impl InMemoryMemory {
    pub fn new(policy: MemoryPolicy) -> Result<Self, MemoryError> {
        if policy.max_entries == 0 {
            return Err(MemoryError::InvalidEntry {
                message: "max_entries 必须大于 0".to_string(),
            });
        }
        if !(0.0..=1.0).contains(&policy.similarity_threshold) {
            return Err(MemoryError::InvalidEntry {
                message: "similarity_threshold 必须在 0 到 1 之间".to_string(),
            });
        }
        Ok(Self {
            policy,
            state: RwLock::new(MemoryState::default()),
        })
    }

    pub fn revision(&self) -> u64 {
        self.state
            .read()
            .expect("memory state lock poisoned")
            .revision
    }
}

#[async_trait]
impl MemoryPort for InMemoryMemory {
    fn retrieve_for_inject(&self, query: &MemoryQuery) -> MemorySearchResult {
        let state = self.state.read().expect("memory state lock poisoned");
        let mut entries = state
            .active
            .iter()
            .filter(|entry| matches_filters(entry, query.layer, query.category))
            .filter(|entry| is_injection_eligible(entry, query.now))
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| std::cmp::Reverse(injection_score(entry, query.now)));
        entries.truncate(query.limit);
        MemorySearchResult {
            mode: MemoryRetrievalMode::InjectionPriority,
            hits: entries
                .into_iter()
                .map(|entry| MemorySearchHit {
                    entry,
                    location: MemoryLocation::Active,
                    outdated: false,
                    ttl_expired: false,
                    relevance: None,
                })
                .collect(),
        }
    }

    fn search(&self, query: &MemorySearchQuery) -> MemorySearchResult {
        let state = self.state.read().expect("memory state lock poisoned");
        let active = state
            .active
            .iter()
            .map(|entry| (entry, MemoryLocation::Active));
        let archive = state
            .archive
            .iter()
            .map(|entry| (entry, MemoryLocation::Archive));
        let entries: Box<dyn Iterator<Item = (&MemoryEntry, MemoryLocation)>> =
            if query.include_archive {
                Box::new(active.chain(archive))
            } else {
                Box::new(active)
            };
        let mut hits = entries
            .filter(|(entry, _)| matches_filters(entry, query.layer, query.category))
            .filter_map(|(entry, location)| {
                relevance(entry, &query.text).map(|relevance| MemorySearchHit {
                    entry: entry.clone(),
                    location,
                    outdated: entry.outdated,
                    ttl_expired: entry.is_ttl_expired(query.now),
                    relevance: Some(relevance),
                })
            })
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .relevance
                .partial_cmp(&left.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    search_tie_break_score(&right.entry, query.now)
                        .cmp(&search_tie_break_score(&left.entry, query.now))
                })
                .then_with(|| left.entry.id.cmp(&right.entry.id))
        });
        hits.truncate(query.limit);
        MemorySearchResult {
            mode: MemoryRetrievalMode::ExplicitSearch,
            hits,
        }
    }

    async fn write(&self, mut entry: MemoryEntry) -> Result<WriteResult, MemoryError> {
        validate_content(&entry.content)?;
        let mut state = self.state.write().expect("memory state lock poisoned");
        if state.active.iter().any(|stored| stored.id == entry.id)
            || state.archive.iter().any(|stored| stored.id == entry.id)
        {
            return Err(MemoryError::InvalidEntry {
                message: "记忆 ID 必须唯一".to_string(),
            });
        }
        if let Some(existing) = state.active.iter_mut().find(|stored| {
            stored.layer == entry.layer
                && jaccard_similarity(&stored.content, &entry.content)
                    >= self.policy.similarity_threshold
        }) {
            existing.tags.append(&mut entry.tags);
            existing.tags.sort();
            existing.tags.dedup();
            existing.accessed_at = entry.created_at;
            existing.access_count = existing.access_count.saturating_add(1);
            let existing_id = existing.id;
            state.revision = state.revision.saturating_add(1);
            return Ok(WriteResult::Merged { existing_id });
        }
        let layer_entries = state
            .active
            .iter()
            .filter(|stored| stored.layer == entry.layer)
            .cloned()
            .collect::<Vec<_>>();
        if layer_entries.len() >= self.policy.max_entries {
            return Ok(WriteResult::NeedsEviction {
                candidates: eviction_candidates(&layer_entries, 3, entry.created_at),
            });
        }
        let id = entry.id;
        state.active.push(entry);
        state.revision = state.revision.saturating_add(1);
        Ok(WriteResult::Added { id })
    }

    async fn update(&self, id: &MemoryId, content: &str) -> Result<bool, MemoryError> {
        validate_content(content)?;
        self.mutate_entry(id, |entry| entry.content = content.to_string())
    }

    async fn delete(&self, id: &MemoryId) -> Result<bool, MemoryError> {
        let mut state = self.state.write().expect("memory state lock poisoned");
        let before = state.active.len();
        state.active.retain(|entry| &entry.id != id);
        if state.active.len() == before {
            return Ok(false);
        }
        state.revision = state.revision.saturating_add(1);
        Ok(true)
    }

    async fn pin(&self, id: &MemoryId, pinned: bool) -> Result<bool, MemoryError> {
        self.mutate_entry(id, |entry| entry.pinned = pinned)
    }

    async fn mark_outdated(&self, id: &MemoryId) -> Result<bool, MemoryError> {
        self.mutate_entry(id, |entry| entry.outdated = true)
    }

    async fn apply_reflection(
        &self,
        _output: &ReflectionOutput,
    ) -> Result<ReflectionApplyResult, MemoryError> {
        Ok(ReflectionApplyResult::default())
    }

    async fn archive(&self, ids: &[MemoryId]) -> Result<(), MemoryError> {
        let mut state = self.state.write().expect("memory state lock poisoned");
        let mut moved = Vec::new();
        state.active.retain(|entry| {
            if ids.contains(&entry.id) && !entry.pinned {
                moved.push(entry.clone());
                false
            } else {
                true
            }
        });
        if !moved.is_empty() {
            state.archive.extend(moved);
            state.revision = state.revision.saturating_add(1);
        }
        Ok(())
    }

    async fn compact(&self) -> Result<CompactResult, MemoryError> {
        let mut state = self.state.write().expect("memory state lock poisoned");
        let mut candidates = Vec::new();
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let layer_entries = state
                .active
                .iter()
                .filter(|entry| entry.layer == layer)
                .cloned()
                .collect::<Vec<_>>();
            let excess = layer_entries.len().saturating_sub(self.policy.max_entries);
            candidates.extend(eviction_candidates(
                &layer_entries,
                excess,
                layer_entries
                    .iter()
                    .map(|entry| entry.accessed_at)
                    .max()
                    .unwrap_or(0),
            ));
        }
        let ids = candidates.iter().map(|entry| entry.id).collect::<Vec<_>>();
        let mut moved = Vec::new();
        state.active.retain(|entry| {
            if ids.contains(&entry.id) {
                moved.push(entry.clone());
                false
            } else {
                true
            }
        });
        let archived = moved.len();
        if archived > 0 {
            state.archive.extend(moved);
            state.revision = state.revision.saturating_add(1);
        }
        Ok(CompactResult {
            archived,
            remaining: state.active.len(),
        })
    }

    fn list(&self, layer: Option<MemoryLayer>) -> Vec<MemoryEntry> {
        self.state
            .read()
            .expect("memory state lock poisoned")
            .active
            .iter()
            .filter(|entry| layer.is_none_or(|layer| entry.layer == layer))
            .cloned()
            .collect()
    }

    fn stats(&self) -> MemoryStats {
        let state = self.state.read().expect("memory state lock poisoned");
        MemoryStats {
            global_count: count_layer(&state.active, MemoryLayer::Global),
            global_archive_count: count_layer(&state.archive, MemoryLayer::Global),
            project_count: count_layer(&state.active, MemoryLayer::Project),
            project_archive_count: count_layer(&state.archive, MemoryLayer::Project),
        }
    }
}

impl InMemoryMemory {
    fn mutate_entry(
        &self,
        id: &MemoryId,
        mutation: impl FnOnce(&mut MemoryEntry),
    ) -> Result<bool, MemoryError> {
        let mut state = self.state.write().expect("memory state lock poisoned");
        let Some(entry) = state.active.iter_mut().find(|entry| &entry.id == id) else {
            return Ok(false);
        };
        mutation(entry);
        state.revision = state.revision.saturating_add(1);
        Ok(true)
    }
}

fn validate_content(content: &str) -> Result<(), MemoryError> {
    if content.trim().is_empty() {
        return Err(MemoryError::InvalidEntry {
            message: "记忆内容不能为空".to_string(),
        });
    }
    Ok(())
}

fn matches_filters(
    entry: &MemoryEntry,
    layer: Option<MemoryLayer>,
    category: Option<MemoryCategory>,
) -> bool {
    layer.is_none_or(|layer| entry.layer == layer)
        && category.is_none_or(|category| entry.category == category)
}

fn relevance(entry: &MemoryEntry, text: &str) -> Option<f64> {
    let text = text.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }
    let content = entry.content.to_lowercase();
    let category = format!("{:?}", entry.category).to_lowercase();
    let layer = format!("{:?}", entry.layer).to_lowercase();
    if content == text {
        Some(1.0)
    } else if content.contains(&text)
        || entry
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(&text))
        || category.contains(&text)
        || layer.contains(&text)
    {
        Some(0.5)
    } else {
        None
    }
}

fn count_layer(entries: &[MemoryEntry], layer: MemoryLayer) -> usize {
    entries.iter().filter(|entry| entry.layer == layer).count()
}
