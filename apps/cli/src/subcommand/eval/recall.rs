//! 真实压缩摘要的质量代理指标：用生产已发生的压缩结果算 must_keep 召回。
//!
//! 评测边界（MUST 如实呈现）：
//! - 输入是 session dataset 里**已经落盘的真实摘要**（`session-state.compact.summary`）
//!   与其覆盖的早期历史，不重跑压缩、不依赖 LLM 装配。
//! - recall 是「typed id / 文件路径是否仍出现在摘要里」的覆盖率代理，
//!   不是语义等价判断，也不代表模型能续接任务。
//! - 摘要里没出现 ≠ 信息丢失（可能被抽象成自然语言），因此只做趋势与对比，
//!   不做单条定论。

use std::path::PathBuf;

use serde::Serialize;

use super::cases::rank_identifiers;
use super::dataset::{estimate_tokens, iter_datasets, load_session, message_text, SessionSnapshot};
use super::score::must_keep_recall;
use super::telemetry::write_or_print;

#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct SessionRecall {
    session_id: String,
    early_messages: usize,
    tail_messages: usize,
    summary_chars: usize,
    early_tokens: u64,
    summary_tokens: u64,
    must_keep_total: usize,
    recall: Option<f64>,
    missed: Vec<String>,
    compression_ratio: Option<f64>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct RecallReport {
    sessions_considered: usize,
    sessions_with_compaction: usize,
    must_keep_recall_mean: Option<f64>,
    must_keep_recall_p50: Option<f64>,
    compression_ratio_mean: Option<f64>,
    compression_ratio_p50: Option<f64>,
    early_tokens_p50: Option<u64>,
    sessions: Vec<SessionRecall>,
}

fn percentile(values: &[f64], quantile: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((sorted.len() as f64 * quantile) as usize).min(sorted.len() - 1);
    Some(sorted[index])
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

/// 单个已压缩会话的摘要质量：must_keep 从早期历史抽取，验证是否还在摘要里。
pub(crate) fn evaluate(
    session: &SessionSnapshot,
    summary: &str,
    max_must_keep: usize,
) -> SessionRecall {
    let split = session.compacted_message_count.min(session.messages.len());
    let early = &session.messages[..split]; // allow unsafe_text_op: Vec slice
    let tail = &session.messages[split..]; // allow unsafe_text_op: Vec slice
    let early_texts: Vec<String> = early.iter().map(message_text).collect();
    let early_chars = early_texts.join("\n");
    let early_tokens = estimate_tokens(&early_chars);
    let summary_tokens = estimate_tokens(summary);
    let must_keep = rank_identifiers(&early_texts, max_must_keep);
    let (recall, missed) = must_keep_recall(&must_keep, summary);

    SessionRecall {
        session_id: session.session_id.clone(),
        early_messages: early.len(),
        tail_messages: tail.len(),
        summary_chars: summary.chars().count(),
        early_tokens,
        summary_tokens,
        must_keep_total: must_keep.len(),
        recall,
        missed: missed.into_iter().take(20).collect(),
        compression_ratio: if early_tokens == 0 {
            None
        } else {
            Some(summary_tokens as f64 / early_tokens as f64)
        },
    }
}

pub(crate) fn summarize(sessions: &[SessionRecall], considered: usize) -> RecallReport {
    let recalls: Vec<f64> = sessions.iter().filter_map(|item| item.recall).collect();
    let ratios: Vec<f64> = sessions
        .iter()
        .filter_map(|item| item.compression_ratio)
        .collect();
    let early: Vec<f64> = sessions
        .iter()
        .map(|item| item.early_tokens as f64)
        .collect();
    RecallReport {
        sessions_considered: considered,
        sessions_with_compaction: sessions.len(),
        must_keep_recall_mean: mean(&recalls),
        must_keep_recall_p50: percentile(&recalls, 0.50),
        compression_ratio_mean: mean(&ratios),
        compression_ratio_p50: percentile(&ratios, 0.50),
        early_tokens_p50: percentile(&early, 0.50).map(|value| value as u64),
        sessions: sessions.to_vec(),
    }
}

fn summary_of(session: &SessionSnapshot) -> Option<String> {
    session.previous_summary.clone()
}

pub(crate) fn run(
    root: PathBuf,
    limit: usize,
    max_must_keep: usize,
    output: Option<PathBuf>,
) -> Result<(), String> {
    let mut considered = 0usize;
    let mut sessions = Vec::new();
    for dataset in iter_datasets(&root) {
        if considered >= limit {
            break;
        }
        let Ok(session) = load_session(&dataset) else {
            continue;
        };
        considered += 1;
        if session.compact_start.is_none() {
            continue;
        }
        let Some(summary) = summary_of(&session) else {
            continue;
        };
        sessions.push(evaluate(&session, &summary, max_must_keep));
    }
    let report = summarize(&sessions, considered);
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    write_or_print(output.as_deref(), &json)
}

#[cfg(test)]
#[path = "recall_tests.rs"]
mod recall_tests;
