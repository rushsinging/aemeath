//! 词法召回候选的 System One 重排：question 构造与概率序应用（纯函数）。
//!
//! 两级架构语义：词法召回 top-N → kev Choice 重排；重排只动 top-N 内部顺序，
//! 尾段保持词法序；评分失败由调用方整体回退词法序（本模块不感知错误）。

use systemone::{ScoringQuestion, ScoringState};

use crate::constants::{RERANK_CRITERION_MAX_CHARS, RERANK_INSTRUCTIONS};
use crate::domain::model::MemorySearchHit;

/// 用 `query_text` 与词法召回候选构造 Choice 评分请求。
///
/// criteria key 为序号（与 eval harness 一致，避免候选文案注入 key 空间）；
/// 候选内容截断到 `RERANK_CRITERION_MAX_CHARS`。
/// query 为空白或候选不足两个时返回 `None`（无重排必要）。
pub(crate) fn build_rerank_request(
    query_text: &str,
    candidates: &[MemorySearchHit],
) -> Option<(ScoringState, ScoringQuestion)> {
    let state = ScoringState::new(query_text.to_owned())?;
    if candidates.len() < 2 {
        return None;
    }
    let criteria: Vec<(String, String)> = candidates
        .iter()
        .enumerate()
        .take(ScoringQuestion::CHOICE_CRITERIA_MAX)
        .map(|(index, hit)| {
            let content: String = hit
                .entry
                .content
                .chars()
                .take(RERANK_CRITERION_MAX_CHARS)
                .collect();
            (index.to_string(), content)
        })
        .collect();
    let question = ScoringQuestion::choice(RERANK_INSTRUCTIONS, criteria)
        .expect("criteria 数量与内容已经本函数约束，构造不可失败");
    Some((state, question))
}

/// 按评分概率重排前 `reranked_count` 个候选；尾段保持词法序。
///
/// - key 为 `build_rerank_request` 的序号；缺失 key 的候选按原相对序沉到重排段末尾
/// - 概率相等时保持原词法相对序（稳定排序，确定性要求）
pub(crate) fn apply_rerank_order(
    hits: Vec<MemorySearchHit>,
    reranked_count: usize,
    probabilities: &[(String, f64)],
) -> Vec<MemorySearchHit> {
    let mut indexed: Vec<(usize, MemorySearchHit)> = hits.into_iter().enumerate().collect();
    let split = indexed.len().min(reranked_count);
    let mut head: Vec<(usize, MemorySearchHit)> = indexed.drain(..split).collect();
    let probability_of = |index: usize| {
        probabilities
            .iter()
            .find(|(key, _)| key == &index.to_string())
            .map(|(_, probability)| *probability)
    };
    head.sort_by(|(left_index, _), (right_index, _)| {
        match (probability_of(*left_index), probability_of(*right_index)) {
            (Some(left), Some(right)) => right.total_cmp(&left),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    head.into_iter()
        .chain(indexed)
        .map(|(_, hit)| hit)
        .collect()
}

#[cfg(test)]
#[path = "rerank_tests.rs"]
mod tests;
