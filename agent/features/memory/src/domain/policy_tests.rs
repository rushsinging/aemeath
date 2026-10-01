use super::constants::MAX_SUPERSEDE_CHAIN_DEPTH;
use std::collections::HashMap;
use std::time::Duration;

use super::*;
use crate::domain::MemoryCategory;

/// 测试用的确定性 UUID：同一序号恒得同一 id，链式断言因此可读。
fn test_id(index: usize) -> MemoryId {
    MemoryId::new(format!("00000000-0000-0000-0000-{index:012x}")).expect("static UUID is valid")
}

fn entry(index: usize, now: u64) -> MemoryEntry {
    MemoryEntry::new(
        test_id(index),
        now,
        MemoryLayer::Project,
        MemoryCategory::Decision,
        format!("content of entry {index}"),
        MemorySource::Llm,
    )
    .expect("test entry must be valid")
}

/// 以「序号 → 序号」构造取代关系映射。
fn supersede_map(links: &[(usize, usize)]) -> HashMap<MemoryId, MemoryId> {
    links
        .iter()
        .map(|(from, to)| (test_id(*from), test_id(*to)))
        .collect()
}

fn lookup_from(links: &HashMap<MemoryId, MemoryId>) -> impl Fn(&MemoryId) -> Option<MemoryId> + '_ {
    move |id: &MemoryId| links.get(id).cloned()
}

/// M10：被取代条目不可注入，且 pinned 不能绕过。
#[test]
fn superseded_entries_are_never_injection_eligible_even_when_pinned() {
    let now = 1_000;
    let mut plain = entry(0, now);
    assert!(
        is_injection_eligible(&plain, now),
        "baseline entry stays eligible"
    );

    let mut pinned = entry(1, now);
    pinned.pinned = true;
    assert!(
        is_injection_eligible(&pinned, now),
        "pinned alone does not block"
    );

    plain.superseded_by = Some(test_id(2));
    pinned.superseded_by = Some(test_id(3));

    assert!(!is_injection_eligible(&plain, now));
    assert!(
        !is_injection_eligible(&pinned, now),
        "M10 sits on the same layer as M5/M8: pinned must not bypass it"
    );
}

/// M10 与既有标记并存：任一标记都足以排除注入。
#[test]
fn injection_eligibility_rejects_every_exclusion_reason() {
    let now = 1_000;

    let mut outdated = entry(0, now);
    outdated.outdated = true;
    assert!(!is_injection_eligible(&outdated, now), "M5 still holds");

    let mut expiring = entry(1, now);
    expiring.ttl = Some(Duration::from_secs(10));
    assert!(
        !is_injection_eligible(&expiring, now + 20),
        "M8 still holds"
    );

    let mut superseded = entry(2, now);
    superseded.superseded_by = Some(test_id(3));
    assert!(!is_injection_eligible(&superseded, now), "M10 holds");
}

/// 链 0←1←2（0 已被 1 取代，1 已被 2 取代）。取代链尾或链中都不成环。
#[test]
fn appending_a_new_version_at_the_head_of_a_chain_is_accepted() {
    let links = supersede_map(&[(0, 1), (1, 2)]);
    let fresh = test_id(99);

    for superseded in [0usize, 1, 2] {
        assert!(
            !would_create_supersede_cycle(&fresh, &test_id(superseded), lookup_from(&links)),
            "entry {superseded} can be superseded by a fresh entry"
        );
    }
}

/// 链下游取代上游时闭合回路：让 0 取代 2 会形成 0←1←2←0。
#[test]
fn a_supersede_link_is_rejected_when_the_new_entry_is_already_upstream() {
    let links = supersede_map(&[(0, 1), (1, 2)]);

    assert!(
        would_create_supersede_cycle(&test_id(0), &test_id(2), lookup_from(&links)),
        "0 already supersedes 1, so 0 superseding 2 closes 0 <- 1 <- 2 <- 0"
    );
    assert!(
        would_create_supersede_cycle(&test_id(0), &test_id(1), lookup_from(&links)),
        "0 already supersedes 1; re-superseding 1 is a two-node cycle"
    );
}

/// 自环：条目不能取代自己。
#[test]
fn an_entry_cannot_supersede_itself() {
    let links = HashMap::new();
    assert!(would_create_supersede_cycle(
        &test_id(0),
        &test_id(0),
        lookup_from(&links)
    ));
}

/// 纵深防御：新条目位于一条超长链的中段、目标在护栏之外时拒绝建立关系，
/// 而不是无限上溯。数据损坏导致的超长链在此收敛为拒绝。
#[test]
fn an_over_long_existing_chain_is_treated_as_a_cycle() {
    let links = (0..MAX_SUPERSEDE_CHAIN_DEPTH + 10)
        .map(|index| (test_id(index), test_id(index + 1)))
        .collect::<HashMap<_, _>>();

    // 新条目 5 的上游链一路指向远超护栏的 271，不可能走完。
    assert!(would_create_supersede_cycle(
        &test_id(5),
        &test_id(MAX_SUPERSEDE_CHAIN_DEPTH + 15),
        lookup_from(&links)
    ));
}

/// 护栏边界：恰好达到上限的链仍被完整判定，既不误杀合法长链，
/// 也能识别链尾的回路。
#[test]
fn a_chain_at_the_guard_boundary_is_still_evaluated() {
    let links = (0..MAX_SUPERSEDE_CHAIN_DEPTH)
        .map(|index| (test_id(index), test_id(index + 1)))
        .collect::<HashMap<_, _>>();

    assert!(
        !would_create_supersede_cycle(&test_id(9999), &test_id(0), lookup_from(&links)),
        "a legal chain at the boundary must still accept a new head"
    );
    assert!(
        would_create_supersede_cycle(
            &test_id(0),
            &test_id(MAX_SUPERSEDE_CHAIN_DEPTH),
            lookup_from(&links)
        ),
        "the chain tail is still recognised at exactly the guard depth"
    );
}

/// 兼容：旧持久化格式没有该字段，反序列化得到 None 且仍可注入。
#[test]
fn a_persisted_entry_without_the_supersede_field_loads_as_not_superseded() {
    let legacy = r#"{
        "id": "00000000-0000-0000-0000-000000000001",
        "layer": "project",
        "category": "decision",
        "content": "legacy entry",
        "source": "llm",
        "created_at": 10,
        "last_confirmed_at": 10
    }"#;

    let entry: MemoryEntry = serde_json::from_str(legacy).expect("legacy entry must decode");
    assert_eq!(entry.superseded_by, None);
    assert!(is_injection_eligible(&entry, 10));
}

/// 往返：设置后能读回；未设置时不占用载荷。
#[test]
fn the_supersede_field_round_trips_and_stays_absent_by_default() {
    let now = 1_000;
    let mut superseded = entry(0, now);
    superseded.superseded_by = Some(test_id(1));

    let encoded = serde_json::to_value(&superseded).expect("entry must encode");
    assert_eq!(
        encoded["superseded_by"],
        serde_json::json!("00000000-0000-0000-0000-000000000001")
    );
    let decoded: MemoryEntry = serde_json::from_value(encoded).expect("entry must decode");
    assert_eq!(decoded, superseded);

    let plain = serde_json::to_value(entry(2, now)).expect("entry must encode");
    assert!(
        plain.get("superseded_by").is_none(),
        "unset supersede must not occupy the payload: {plain}"
    );
}
