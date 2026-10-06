//! rerank 纯函数行为测试：question 构造、概率序应用、回退边界。

use super::*;
use crate::domain::model::{
    MemoryCategory, MemoryEntry, MemoryId, MemoryLocation, MemorySearchHit, MemorySource,
};
use crate::domain::MemoryLayer;

fn hit(content: &str) -> MemorySearchHit {
    MemorySearchHit {
        entry: MemoryEntry::new(
            MemoryId::now_v7(),
            10,
            MemoryLayer::Project,
            MemoryCategory::Fact,
            content,
            MemorySource::User,
        )
        .expect("entry 构造"),
        location: MemoryLocation::Active,
        outdated: false,
        ttl_expired: false,
        superseded_by: None,
        relevance: Some(0.5),
    }
}

fn contents(hits: &[MemorySearchHit]) -> Vec<String> {
    hits.iter().map(|hit| hit.entry.content.clone()).collect()
}

#[test]
fn build_rerank_request_constructs_choice_with_index_keys() {
    let candidates = vec![hit("候选甲内容"), hit("候选乙内容"), hit("候选丙内容")];
    let (state, question) = build_rerank_request("查询文本", &candidates).expect("应构造成功");
    assert_eq!(state.as_str(), "查询文本");
    match question {
        ScoringQuestion::Choice { criteria, .. } => {
            assert_eq!(criteria.len(), 3);
            assert_eq!(criteria[0], ("0".to_owned(), "候选甲内容".to_owned()));
            assert_eq!(criteria[2], ("2".to_owned(), "候选丙内容".to_owned()));
        }
        other => panic!("期望 Choice，实际 {other:?}"),
    }
}

#[test]
fn build_rerank_request_truncates_long_content() {
    let long_content = "长".repeat(1000);
    let candidates = vec![hit(&long_content), hit("短")];
    let (_, question) = build_rerank_request("查询", &candidates).expect("应构造成功");
    match question {
        ScoringQuestion::Choice { criteria, .. } => {
            assert_eq!(
                criteria[0].1.chars().count(),
                crate::constants::RERANK_CRITERION_MAX_CHARS,
                "超长内容应截断"
            );
        }
        other => panic!("期望 Choice，实际 {other:?}"),
    }
}

#[test]
fn build_rerank_request_returns_none_for_blank_query_or_few_candidates() {
    let candidates = vec![hit("甲"), hit("乙")];
    assert!(build_rerank_request("  ", &candidates).is_none());
    assert!(build_rerank_request("查询", &[hit("唯一")]).is_none());
    assert!(build_rerank_request("查询", &[]).is_none());
}

#[test]
fn apply_rerank_order_reorders_top_n_by_probability_desc() {
    let hits = vec![hit("甲"), hit("乙"), hit("丙"), hit("丁")];
    let probabilities = vec![
        ("0".to_owned(), 0.1),
        ("1".to_owned(), 0.7),
        ("2".to_owned(), 0.2),
    ];
    let reordered = apply_rerank_order(hits, 3, &probabilities);
    assert_eq!(contents(&reordered), vec!["乙", "丙", "甲", "丁"]);
}

#[test]
fn apply_rerank_order_keeps_tail_in_lexical_order() {
    let hits = vec![hit("甲"), hit("乙"), hit("尾一"), hit("尾二")];
    let probabilities = vec![("0".to_owned(), 0.9), ("1".to_owned(), 0.1)];
    let reordered = apply_rerank_order(hits, 2, &probabilities);
    assert_eq!(contents(&reordered), vec!["甲", "乙", "尾一", "尾二"]);
}

#[test]
fn apply_rerank_order_is_stable_for_equal_probabilities() {
    let hits = vec![hit("甲"), hit("乙"), hit("丙")];
    let probabilities = vec![
        ("0".to_owned(), 0.5),
        ("1".to_owned(), 0.5),
        ("2".to_owned(), 0.5),
    ];
    let reordered = apply_rerank_order(hits, 3, &probabilities);
    assert_eq!(
        contents(&reordered),
        vec!["甲", "乙", "丙"],
        "等概率保持词法序"
    );
}

#[test]
fn apply_rerank_order_sinks_candidates_with_missing_keys() {
    let hits = vec![hit("甲"), hit("乙"), hit("丙")];
    // 缺 key "1"（乙未评分）→ 乙沉到重排段末尾，其余按概率
    let probabilities = vec![("0".to_owned(), 0.3), ("2".to_owned(), 0.6)];
    let reordered = apply_rerank_order(hits, 3, &probabilities);
    assert_eq!(contents(&reordered), vec!["丙", "甲", "乙"]);
}
