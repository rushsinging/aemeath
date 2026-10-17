use super::constants::{MAX_SUPERSEDE_CHAIN_DEPTH, MIN_SYNTHESIS_EVIDENCE};
use super::EvictionCandidate;
use super::MemoryEntry;
use super::MemoryId;
use super::MemoryKind;
use std::collections::HashSet;

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

/// M10：被取代条目不可注入。与 M5（outdated）、M8（TTL）同层，
/// `pinned` 不绕过——pinned 保护「不要淘汰」，不保护「不要被取代」。
pub fn is_injection_eligible(entry: &MemoryEntry, now: u64) -> bool {
    entry.superseded_by.is_none() && !entry.outdated && !entry.is_ttl_expired(now)
}

/// M12：反思输入排除失效条目。
///
/// 判定与 [`is_injection_eligible`] 同源（失效就是失效），但语义独立——注入
/// 关心「当前对话要不要带这条」，反思关心「LLM 能不能基于这条产出新建议」。
/// 051 §8.2 论证：读入失效条目会形成「建议 → 写入 → 下次反思基于它再产出
/// 建议」的污染循环。
///
/// 不排斥 pinned 与归纳产物：反思需要看到来源事实与已成立的结论，才能判断
/// 它们是否仍然有效。
pub fn is_reflection_input_eligible(entry: &MemoryEntry, now: u64) -> bool {
    is_injection_eligible(entry, now)
}

/// 容量口径（#1987 用户拍板）：active 中的 outdated / 被取代残留已不可注入，
/// MUST NOT 计入 max_entries 与 reflect 收敛目标——max_entries / target 约束的
/// 都是「有效 active」。
pub fn effective_active_count(entries: &[MemoryEntry]) -> usize {
    entries
        .iter()
        .filter(|entry| !entry.outdated && entry.superseded_by.is_none())
        .count()
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

/// 覆盖式让位的注入顺序（#1777）：**不改分数，只改顺序**——把
/// `kind = Synthesized` 的归纳结论排在全部普通条���之前，组内仍按
/// `injection_score` 降序。
///
/// 调用方（Context 的注入填充）据此做两段填充：结论段先填，其 `evidence`
/// 指向的来源在第二段让位；结论未入选时来源照常参与。固定降权系数会在
/// 「结论未被选中」时误伤来源，因此不存在。
pub fn order_for_injection(entries: &mut [MemoryEntry], now: u64) {
    entries.sort_by(|left, right| {
        let group = |entry: &MemoryEntry| match entry.kind {
            MemoryKind::Synthesized => 0,
            MemoryKind::Raw => 1,
        };
        group(left)
            .cmp(&group(right))
            .then_with(|| injection_score(right, now).cmp(&injection_score(left, now)))
            .then_with(|| left.id.cmp(&right.id))
    });
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
    // MemoryId 是 Copy：按位解引用拷贝即离开借用域，无需 clone。
    let mut cursor = Some(*new_entry_id);
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
    jaccard_token_sets(&tokenize(left), &tokenize(right))
}

/// 预分词集合上的 Jaccard（#1987 现状段两两比对的批量入口，避免重复分词）。
pub(crate) fn jaccard_token_sets(
    left: &std::collections::HashSet<String>,
    right: &std::collections::HashSet<String>,
) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(right).count();
    let union = left.union(right).count();
    intersection as f64 / union as f64
}

pub(crate) fn token_set(value: &str) -> std::collections::HashSet<String> {
    tokenize(value)
}

/// #1987：分词口径——拉丁/数字段照旧按词切；CJK 连续段拆字符 bigram。
/// 中文整句是天然连续段（无空格），单整段 token 让 Jaccard 对中文重复对
/// 全量漏检（真 corpus 实测 eligible 全中文时 0 检出）；bigram 修复该盲区。
fn tokenize(value: &str) -> HashSet<String> {
    let mut tokens = HashSet::new();
    let mut latin_run = String::new();
    let mut cjk_run = String::new();
    let flush_latin = |run: &mut String, tokens: &mut HashSet<String>| {
        if !run.is_empty() {
            tokens.insert(run.to_lowercase());
            run.clear();
        }
    };
    for character in value.chars() {
        if is_cjk(character) {
            flush_latin(&mut latin_run, &mut tokens);
            cjk_run.push(character);
        } else if character.is_alphanumeric() {
            if !cjk_run.is_empty() {
                emit_cjk_bigrams(&cjk_run, &mut tokens);
                cjk_run.clear();
            }
            latin_run.push(character);
        } else {
            flush_latin(&mut latin_run, &mut tokens);
            if !cjk_run.is_empty() {
                emit_cjk_bigrams(&cjk_run, &mut tokens);
                cjk_run.clear();
            }
        }
    }
    flush_latin(&mut latin_run, &mut tokens);
    if !cjk_run.is_empty() {
        emit_cjk_bigrams(&cjk_run, &mut tokens);
    }
    tokens
}

fn is_cjk(character: char) -> bool {
    matches!(character as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

fn emit_cjk_bigrams(run: &str, tokens: &mut HashSet<String>) {
    let characters: Vec<char> = run.chars().collect();
    if characters.len() == 1 {
        tokens.insert(characters[0].to_string());
        return;
    }
    for window in characters.windows(2) {
        tokens.insert(window.iter().collect::<String>());
    }
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
