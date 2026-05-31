use super::entry::MemoryEntry;

pub fn injection_score(entry: &MemoryEntry, now: u64) -> i64 {
    let pinned_bonus = if entry.pinned { 10_000 } else { 0 };
    let access_score = i64::from(entry.access_count.min(20)) * 100;
    let ttl_penalty = if entry.is_ttl_expired(now) { 5_000 } else { 0 };
    let outdated_penalty = if entry.outdated { 2_000 } else { 0 };

    pinned_bonus + access_score + recency_score(entry.accessed_at, now)
        - ttl_penalty
        - outdated_penalty
}

pub fn eviction_score(entry: &MemoryEntry, now: u64) -> i64 {
    if entry.pinned {
        return i64::MAX;
    }

    let age_days = now.saturating_sub(entry.accessed_at) / 86_400;
    let recency_weight = 100_i64.saturating_sub(age_days.min(100) as i64);
    i64::from(entry.access_count) * 10 + recency_weight
}

fn recency_score(accessed_at: u64, now: u64) -> i64 {
    let age_days = now.saturating_sub(accessed_at) / 86_400;
    match age_days {
        0 => 1_000,
        1..=7 => 800,
        8..=30 => 500,
        31..=90 => 200,
        _ => 50,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::entry::{MemoryCategory, MemoryLayer, MemorySource};

    fn entry() -> MemoryEntry {
        let mut entry = MemoryEntry::new(
            "memory-1",
            1_000_000,
            MemoryLayer::Project,
            MemoryCategory::Pattern,
            "测试",
            MemorySource::User,
        );
        entry.accessed_at = 1_000_000;
        entry
    }

    #[test]
    fn test_injection_score_pinned_wins() {
        let mut normal = entry();
        normal.access_count = 20;
        let mut pinned = entry();
        pinned.pinned = true;

        assert!(injection_score(&pinned, 1_000_000) > injection_score(&normal, 1_000_000));
    }

    #[test]
    fn test_injection_score_outdated_penalty() {
        let active = entry();
        let mut outdated = entry();
        outdated.outdated = true;

        assert!(injection_score(&active, 1_000_000) > injection_score(&outdated, 1_000_000));
    }

    #[test]
    fn test_injection_score_old_entry_lower() {
        let recent = entry();
        let mut old = entry();
        old.accessed_at = 1;

        assert!(injection_score(&recent, 1_000_000) > injection_score(&old, 1_000_000));
    }

    #[test]
    fn test_eviction_score_pinned_max() {
        let mut pinned = entry();
        pinned.pinned = true;

        assert_eq!(eviction_score(&pinned, 1_000_000), i64::MAX);
    }
}
