use super::MemoryEntry;
use std::collections::HashSet;

pub fn is_injection_eligible(entry: &MemoryEntry, now: u64) -> bool {
    !entry.outdated && !entry.is_ttl_expired(now)
}

pub fn injection_score(entry: &MemoryEntry, now: u64) -> i64 {
    debug_assert!(is_injection_eligible(entry, now));
    search_tie_break_score(entry, now)
}

pub fn search_tie_break_score(entry: &MemoryEntry, now: u64) -> i64 {
    let pinned_bonus = if entry.pinned { 10_000 } else { 0 };
    let access_score = i64::from(entry.access_count.min(20)) * 100;
    pinned_bonus + access_score + recency_score(entry.accessed_at, now)
}

pub fn eviction_score(entry: &MemoryEntry, now: u64) -> i64 {
    if entry.pinned {
        return i64::MAX;
    }
    let age_days = now.saturating_sub(entry.accessed_at) / 86_400;
    let recency_weight = 100_i64.saturating_sub(age_days.min(100) as i64);
    i64::from(entry.access_count) * 10 + recency_weight
}

pub fn eviction_candidates(entries: &[MemoryEntry], count: usize, now: u64) -> Vec<MemoryEntry> {
    let mut candidates = entries
        .iter()
        .filter(|entry| !entry.pinned)
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by_key(|entry| eviction_score(entry, now));
    candidates.truncate(count);
    candidates
}

pub fn jaccard_similarity(left: &str, right: &str) -> f64 {
    let left = tokenize(left);
    let right = tokenize(right);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(&right).count();
    let union = left.union(&right).count();
    intersection as f64 / union as f64
}

fn tokenize(value: &str) -> HashSet<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn recency_score(accessed_at: u64, now: u64) -> i64 {
    match now.saturating_sub(accessed_at) / 86_400 {
        0 => 1_000,
        1..=7 => 800,
        8..=30 => 500,
        31..=90 => 200,
        _ => 50,
    }
}
