use super::EvictionCandidate;
use super::MemoryEntry;
use super::MemoryId;
use super::MemoryKind;
use std::collections::HashSet;

/// M13 的证据下限（#1776）：少于两条来源的「归纳」等价于复制既有条目，
/// MUST NOT 产出。
pub const MIN_SYNTHESIS_EVIDENCE: usize = 2;

/// 把建议声明的 `synthesizes` 映射为归纳产物的类型与证据，返回是否构成归纳。
///
/// 不足下限时**不修改** entry：内容照常写入，但保持 `Raw` 且无 evidence——
/// 产出一条普通记忆远好于丢弃模型给出的内容。
pub fn apply_synthesis(entry: &mut MemoryEntry, synthesizes: &[MemoryId]) -> bool {
    if synthesizes.len() < MIN_SYNTHESIS_EVIDENCE {
        return false;
    }
    entry.kind = MemoryKind::Synthesized;
    entry.evidence = synthesizes.to_vec();
    true
}

/// 取代链上溯的最大步数（#1774）。合法链长不会超过 `max_entries`；
/// 超过该上限说明数据已损坏，按成环处理以收敛写入路径。
pub const MAX_SUPERSEDE_CHAIN_DEPTH: usize = 256;

/// M10：被取代条目不可注入。与 M5（outdated）、M8（TTL）同层，
/// `pinned` 不绕过——pinned 保护「不要淘汰」，不保护「不要被取代」。
pub fn is_injection_eligible(entry: &MemoryEntry, now: u64) -> bool {
    entry.superseded_by.is_none() && !entry.outdated && !entry.is_ttl_expired(now)
}

/// 把 `target` 的取代关系指向 `superseding`，返回是否写入。
///
/// 找不到 `target` 时返回 `false`，调用方据此判定为「跳过」而非写坏
/// （不存在半条关系：单字段赋值要么生效要么不生效）。active 与 archive
/// 都要试——被取代的条目本就可能已经归档。
pub(crate) fn assign_supersede(
    entries: &mut [MemoryEntry],
    target: &MemoryId,
    superseding: MemoryId,
) -> bool {
    match entries.iter_mut().find(|entry| &entry.id == target) {
        Some(entry) => {
            entry.superseded_by = Some(superseding);
            true
        }
        None => false,
    }
}

/// 从给定条目集合解析「该条目被谁取代」，用于 `would_create_supersede_cycle`。
pub(crate) fn supersede_chain_of<'a>(
    groups: &'a [&'a [MemoryEntry]],
) -> impl Fn(&MemoryId) -> Option<MemoryId> + 'a {
    move |id: &MemoryId| {
        groups
            .iter()
            .flat_map(|entries| entries.iter())
            .find(|entry| &entry.id == id)
            .and_then(|entry| entry.superseded_by)
    }
}

/// M9：建立 `superseded_by` 关系前校验不成环。
///
/// `superseded.superseded_by = new` 这条边若让新条目重新出现在被取代条目的
/// 既有上游链上，就闭合了回路。`lookup` 解析「该条目被谁取代」；返回 `None`
/// 表示链尾。
pub fn would_create_supersede_cycle(
    new_entry_id: &MemoryId,
    superseded_id: &MemoryId,
    lookup: impl Fn(&MemoryId) -> Option<MemoryId>,
) -> bool {
    if new_entry_id == superseded_id {
        return true;
    }
    let mut cursor = Some(new_entry_id.clone());
    let mut steps = 0usize;
    while let Some(current) = cursor {
        if &current == superseded_id {
            return true;
        }
        if steps == MAX_SUPERSEDE_CHAIN_DEPTH {
            return true;
        }
        cursor = lookup(&current);
        steps += 1;
    }
    false
}

pub fn injection_score(entry: &MemoryEntry, now: u64) -> i64 {
    debug_assert!(is_injection_eligible(entry, now));
    search_tie_break_score(entry, now)
}

pub fn search_tie_break_score(entry: &MemoryEntry, now: u64) -> i64 {
    let pinned_bonus = if entry.pinned { 10_000 } else { 0 };
    let confirmation_score = i64::from(entry.confirmation_count.min(20)) * 100;
    pinned_bonus + confirmation_score + recency_score(entry.last_confirmed_at, now)
}

pub fn eviction_score(entry: &MemoryEntry, now: u64) -> i64 {
    if entry.pinned {
        return i64::MAX;
    }
    let age_days = now.saturating_sub(entry.last_confirmed_at) / 86_400;
    let recency_weight = 100_i64.saturating_sub(age_days.min(100) as i64);
    i64::from(entry.confirmation_count) * 10 + recency_weight
}

pub fn eviction_candidate(entry: MemoryEntry, now: u64) -> EvictionCandidate {
    let age_days = now.saturating_sub(entry.last_confirmed_at) / 86_400;
    EvictionCandidate {
        ttl_expired: entry.is_ttl_expired(now),
        eviction_score: eviction_score(&entry, now),
        eviction_reason: format!(
            "未固定；确认次数={}；距最后确认={}天",
            entry.confirmation_count, age_days
        ),
        entry,
    }
}

pub fn eviction_candidates(
    entries: &[MemoryEntry],
    count: usize,
    now: u64,
) -> Vec<EvictionCandidate> {
    let mut candidates = entries
        .iter()
        .filter(|entry| !entry.pinned)
        .cloned()
        .map(|entry| eviction_candidate(entry, now))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.eviction_score
            .cmp(&right.eviction_score)
            .then_with(|| left.entry.id.cmp(&right.entry.id))
    });
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

fn recency_score(last_confirmed_at: u64, now: u64) -> i64 {
    match now.saturating_sub(last_confirmed_at) / 86_400 {
        0 => 1_000,
        1..=7 => 800,
        8..=30 => 500,
        31..=90 => 200,
        _ => 50,
    }
}
