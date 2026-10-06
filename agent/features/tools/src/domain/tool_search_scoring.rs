//! ToolSearch 的 System One 语义重排纯函数（#1835）。
//!
//! 策略：词法高置信短路（存在 exact / name contains 命中即不触发评分，省延迟）；
//! 低置信（仅 desc contains 或零命中）才把候选交 ScoringPort Choice 重排。
//! 本模块只做判定与排序，不感知 IO 与错误（评分失败由调用方整体回退词法序）。

/// 词法命中是否高置信（短路评分，直接用词法序）。
pub(crate) fn is_lexical_confident(lexical_scores: &[f64]) -> bool {
    lexical_scores
        .iter()
        .any(|score| *score >= crate::constants::LEXICAL_CONFIDENT_THRESHOLD)
}

/// 按评分概率重排候选；缺失 key 的候选按原相对序沉底，概率相等保持原序（稳定）。
///
/// `probabilities` 的 key 为候选序号（与 build 侧约定一致，避免候选文案注入 key 空间）。
pub(crate) fn apply_scoring_order<T>(items: Vec<T>, probabilities: &[(String, f64)]) -> Vec<T> {
    let probability_of = |index: usize| {
        probabilities
            .iter()
            .find(|(key, _)| key == &index.to_string())
            .map(|(_, probability)| *probability)
    };
    let mut indexed: Vec<(usize, T)> = items.into_iter().enumerate().collect();
    indexed.sort_by(|(left_index, _), (right_index, _)| {
        match (probability_of(*left_index), probability_of(*right_index)) {
            (Some(left), Some(right)) => right.total_cmp(&left),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    indexed.into_iter().map(|(_, item)| item).collect()
}
