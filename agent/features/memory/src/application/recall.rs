//! 记忆主动召回（per-message recall）：为一条用户消息找出最相关的记忆。
//!
//! 与显式 `search` 的区别：search 面向工具调用（词法序或内嵌重排的命中列表），
//! 本服务面向 reminder 注入——必须把评分概率带出给调用方做阈值门/预算决策。

use crate::domain::rerank::{apply_rerank_order, build_rerank_request};
use crate::domain::{MemoryEntry, MemoryLocation, MemorySearchQuery};
use crate::ports::MemoryPort;

/// 一条召回的记忆（附评分概率）。
#[derive(Debug, Clone, PartialEq)]
pub struct RecalledMemory {
    pub entry: MemoryEntry,
    pub location: MemoryLocation,
    /// System One 评分概率（归一化到候选集内）。
    pub probability: f64,
}

/// 召回 `query_text` 的最相关记忆：词法召回 → kev 评分 → 带概率返回 top-K。
///
/// 候选 ≥2 走 Choice 重排；恰好 1 个候选走 Noul 相关性判定（Choice 题型下限 2）。
/// 词法零命中时返回空（调用方本 turn 不注入）；评分失败向上传播
/// `ScoringUnavailable`（调用方静默缺席，NEVER 阻断 turn）。
pub async fn recall_relevant(
    memory: &dyn MemoryPort,
    scorer: &dyn systemone::ScoringPort,
    query_text: &str,
    now: u64,
    recall_limit: usize,
    top_k: usize,
) -> Result<Vec<RecalledMemory>, systemone::ScoringUnavailable> {
    let result = memory
        .search(&MemorySearchQuery {
            text: query_text.to_owned(),
            limit: recall_limit,
            layer: None,
            category: None,
            include_archive: false,
            now,
        })
        .await;
    if result.hits.is_empty() {
        return Ok(Vec::new());
    }
    if result.hits.len() == 1 {
        return recall_single(
            memory,
            scorer,
            query_text,
            result.hits.into_iter().next().expect("已判非空"),
        )
        .await;
    }
    let top_n = result.hits.len().min(crate::constants::RERANK_TOP_N);
    let Some((state, question)) = build_rerank_request(query_text, &result.hits[..top_n]) else {
        return Ok(Vec::new());
    };
    let answers = scorer.answer(&state, &[question]).await?;
    let Some(systemone::ScoringAnswer::Choice { probabilities, .. }) = answers.first() else {
        return Ok(Vec::new());
    };
    let probability_of = |index: usize| {
        probabilities
            .iter()
            .find(|(key, _)| key == &index.to_string())
            .map(|(_, probability)| *probability)
    };
    // 重排 + 概率配对（同一次遍历内记下原序号，避免二次查找）。
    let indexed_hits: Vec<(usize, _)> = result
        .hits
        .iter()
        .take(top_n)
        .cloned()
        .enumerate()
        .collect();
    let reordered = apply_rerank_order(result.hits.clone(), top_n, probabilities);
    let original_index_of = |entry_id: &crate::domain::MemoryId| {
        indexed_hits
            .iter()
            .find(|(_, hit)| &hit.entry.id == entry_id)
            .map(|(index, _)| *index)
    };
    Ok(reordered
        .into_iter()
        .filter_map(|hit| {
            let probability = original_index_of(&hit.entry.id).and_then(probability_of)?;
            Some(RecalledMemory {
                entry: hit.entry,
                location: hit.location,
                probability,
            })
        })
        .take(top_k)
        .collect())
}

/// 单候选召回：Noul 判定该候选与消息的相关性（p_true 即概率）。
async fn recall_single(
    _memory: &dyn MemoryPort,
    scorer: &dyn systemone::ScoringPort,
    query_text: &str,
    hit: crate::domain::MemorySearchHit,
) -> Result<Vec<RecalledMemory>, systemone::ScoringUnavailable> {
    let state =
        systemone::ScoringState::new(format!("{}\n\n候选记忆：{}", query_text, hit.entry.content));
    let Some(state) = state else {
        return Ok(Vec::new());
    };
    let question =
        systemone::ScoringQuestion::noul(crate::constants::RECALL_SINGLE_NOUL_INSTRUCTIONS, None)
            .expect("固定文案构造不可失败");
    let answers = scorer.answer(&state, &[question]).await?;
    let Some(systemone::ScoringAnswer::Noul { p_true, .. }) = answers.first() else {
        return Ok(Vec::new());
    };
    Ok(vec![RecalledMemory {
        entry: hit.entry,
        location: hit.location,
        probability: *p_true,
    }])
}

#[cfg(test)]
#[path = "recall_tests.rs"]
mod tests;
