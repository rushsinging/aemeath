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
#[path = "scoring_tests.rs"]
mod tests;
