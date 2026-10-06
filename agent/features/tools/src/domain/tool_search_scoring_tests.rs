//! `tool_search_scoring` 纯函数测试（#1835）。

use super::tool_search_scoring::*;

#[test]
fn lexical_confident_when_exact_or_name_contains_hit() {
    assert!(is_lexical_confident(&[100.0]), "exact 命中为高置信");
    assert!(is_lexical_confident(&[80.0]), "name contains 为高置信");
    assert!(is_lexical_confident(&[50.0, 80.0]), "任一命中 ≥80 即高置信");
}

#[test]
fn lexical_unconfident_when_only_description_or_no_hit() {
    assert!(!is_lexical_confident(&[50.0]), "仅 desc contains 为低置信");
    assert!(!is_lexical_confident(&[]), "零命中为低置信");
    assert!(!is_lexical_confident(&[50.0, 40.0]), "全部 <80 为低置信");
}

#[test]
fn scoring_order_sorts_by_probability_desc() {
    let items = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
    let probabilities = vec![
        ("0".to_string(), 0.2),
        ("1".to_string(), 0.5),
        ("2".to_string(), 0.3),
    ];
    let ordered = apply_scoring_order(items, &probabilities);
    assert_eq!(ordered, vec!["beta", "gamma", "alpha"]);
}

#[test]
fn scoring_order_sinks_missing_keys_preserving_relative_order() {
    let items = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
    // 仅 0/2 有概率：1 缺失沉底，0/2 按概率序。
    let probabilities = vec![("0".to_string(), 0.1), ("2".to_string(), 0.9)];
    let ordered = apply_scoring_order(items, &probabilities);
    assert_eq!(ordered, vec!["gamma", "alpha", "beta"]);
}

#[test]
fn scoring_order_is_stable_on_equal_probabilities() {
    let items = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
    let probabilities = vec![
        ("0".to_string(), 0.4),
        ("1".to_string(), 0.4),
        ("2".to_string(), 0.4),
    ];
    let ordered = apply_scoring_order(items, &probabilities);
    assert_eq!(ordered, vec!["alpha", "beta", "gamma"], "同概率保持原序");
}
