//! 生产实际反馈指标：只读真实 session，度量「压缩有没有让真实使用变差」。
//!
//! 评测边界（MUST 如实呈现）：
//! - 全部是**趋势/代理指标**，不做单点因果断言：压缩后重问也可能是任务本身需要。
//! - 只统计「压缩点之后（tail）」与「被摘要覆盖的早期历史（early）」的对比，
//!   不读取压缩实现内部状态，也不重新跑压缩。
//! - 未压缩的 session 直接跳过：没有 tail/early 对比就没有指标。

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

use super::constants::{
    DISSATISFACTION_MARKERS, INJECTION_PREFIXES, REQUERY_MIN_CHARS, REQUERY_SIMILARITY_THRESHOLD,
    REQUERY_WINDOW_TURNS, TOOL_READ_NAMES,
};
use super::dataset::{iter_datasets, load_session, SessionSnapshot, SessionStepMeta};
use super::telemetry::write_or_print;

#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct SessionFeedback {
    session_id: String,
    compacted_messages: usize,
    tail_messages: usize,
    tail_user_turns: usize,
    tail_short_turns: usize,
    requery_candidates: usize,
    requery_hits: usize,
    requery_rate: Option<f64>,
    repeated_read_paths: usize,
    tool_calls_early: usize,
    tool_errors_early: usize,
    error_rate_early: Option<f64>,
    tool_calls_tail: usize,
    tool_errors_tail: usize,
    error_rate_tail: Option<f64>,
    interrupted_steps_tail: usize,
    dissatisfaction_signals: usize,
    api_input_tokens_early: u64,
    api_input_tokens_tail: u64,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct FeedbackReport {
    sessions_considered: usize,
    sessions_with_compaction: usize,
    requery_rate_mean: Option<f64>,
    repeated_read_paths_total: usize,
    error_rate_early: Option<f64>,
    error_rate_tail: Option<f64>,
    interrupted_steps_total: usize,
    dissatisfaction_signals_total: usize,
    sessions: Vec<SessionFeedback>,
}

/// 字符 bigram 集合（过滤空白）：中英混合文本的稳定相似度基元。
pub(crate) fn bigrams(text: &str) -> BTreeSet<(char, char)> {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    chars.windows(2).map(|pair| (pair[0], pair[1])).collect()
}

/// Jaccard 相似度；任一侧为空返回 0.0（无基元可比，不算相似）。
pub(crate) fn jaccard(left: &str, right: &str) -> f64 {
    let left = bigrams(left);
    let right = bigrams(right);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(&right).count() as f64;
    let union = left.union(&right).count() as f64;
    intersection / union
}

/// 用户文本消息（排除 tool_result 载体与系统注入的提醒块）。
pub(crate) fn user_text(message: &Value) -> Option<String> {
    if message.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let mut parts = Vec::new();
    for block in message.get("content").and_then(Value::as_array)? {
        if block.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        let Some(text) = block.get("text").and_then(Value::as_str) else {
            continue;
        };
        // 系统 reminder / hook feedback 是注入内容，模板化且高频，
        // 计入相似度会让重问率虚高。
        if INJECTION_PREFIXES
            .iter()
            .any(|prefix| text.trim_start().starts_with(prefix))
        {
            continue;
        }
        parts.push(text.to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn is_read_tool(name: &str) -> bool {
    TOOL_READ_NAMES.contains(&name)
}

fn tool_read_path(block: &Value) -> Option<String> {
    if block.get("type").and_then(Value::as_str) != Some("tool_use") {
        return None;
    }
    let name = block
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !is_read_tool(name) {
        return None;
    }
    let input = block.get("input")?;
    for key in ["file_path", "path", "glob"] {
        if let Some(value) = input.get(key).and_then(Value::as_str) {
            return Some(value.to_string());
        }
    }
    None
}

fn count_tool_results(messages: &[Value]) -> (usize, usize) {
    let mut total = 0usize;
    let mut errors = 0usize;
    for message in messages {
        let Some(blocks) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        for block in blocks {
            if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                total += 1;
                if block
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    errors += 1;
                }
            }
        }
    }
    (total, errors)
}

fn rate(numerator: usize, denominator: usize) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}

/// 按压缩点切分 steps：返回 (early_steps, tail_steps)。
fn split_steps<'a>(
    steps: &'a [SessionStepMeta],
    compact_start: &(String, String),
) -> (&'a [SessionStepMeta], &'a [SessionStepMeta]) {
    let index = steps
        .iter()
        .position(|step| &(step.run_id.clone(), step.step_id.clone()) == compact_start)
        .unwrap_or(steps.len());
    // 按 step 边界切分（Vec 索引，非文本切片）。
    let early = &steps[..index]; // allow unsafe_text_op: Vec slice
    let tail = &steps[index..]; // allow unsafe_text_op: Vec slice
    (early, tail)
}

fn is_interrupted(step: &SessionStepMeta) -> bool {
    step.finalize_cause
        .as_deref()
        .is_some_and(|cause| cause != "Completed")
}

pub(crate) fn evaluate(session: &SessionSnapshot) -> Option<SessionFeedback> {
    let compact_start = session.compact_start.as_ref()?;
    let split = session.compacted_message_count.min(session.messages.len());
    let early = &session.messages[..split]; // allow unsafe_text_op: Vec slice
    let tail = &session.messages[split..]; // allow unsafe_text_op: Vec slice

    // 只比较实质请求：短指令（继续/好/ok）之间相似度不可靠，会虚高重问率。
    let early_users: Vec<String> = early
        .iter()
        .filter_map(user_text)
        .filter(|text| text.chars().count() >= REQUERY_MIN_CHARS)
        .collect();
    let tail_users: Vec<String> = tail
        .iter()
        .filter_map(user_text)
        .filter(|text| text.chars().count() >= REQUERY_MIN_CHARS)
        .collect();

    // 重问：只看压缩后最近 N 个 user turn，与早期任一 user 消息高度相似即命中。
    let tail_all_users: Vec<String> = tail.iter().filter_map(user_text).collect();
    let candidates: Vec<&String> = tail_users.iter().rev().take(REQUERY_WINDOW_TURNS).collect();
    let mut requery_hits = 0usize;
    for candidate in &candidates {
        let similar = early_users
            .iter()
            .any(|earlier| jaccard(candidate, earlier) >= REQUERY_SIMILARITY_THRESHOLD);
        if similar {
            requery_hits += 1;
        }
    }

    // 重复读取：tail 再次访问 early 已读过的路径。
    let early_paths: BTreeSet<String> = early
        .iter()
        .filter_map(|message| message.get("content").and_then(Value::as_array))
        .flatten()
        .filter_map(tool_read_path)
        .collect();
    let repeated_read_paths = tail
        .iter()
        .filter_map(|message| message.get("content").and_then(Value::as_array))
        .flatten()
        .filter_map(tool_read_path)
        .filter(|path| early_paths.contains(path))
        .count();

    let (tool_calls_early, tool_errors_early) = count_tool_results(early);
    let (tool_calls_tail, tool_errors_tail) = count_tool_results(tail);
    let (early_steps, tail_steps) = split_steps(&session.steps, compact_start);

    let dissatisfaction_signals = tail_users
        .iter()
        .filter(|text| {
            DISSATISFACTION_MARKERS
                .iter()
                .any(|marker| text.contains(marker))
        })
        .count();

    Some(SessionFeedback {
        session_id: session.session_id.clone(),
        compacted_messages: split,
        tail_messages: tail.len(),
        tail_user_turns: tail_all_users.len(),
        tail_short_turns: tail_all_users
            .iter()
            .filter(|text| text.chars().count() < REQUERY_MIN_CHARS)
            .count(),
        requery_candidates: candidates.len(),
        requery_hits,
        requery_rate: rate(requery_hits, candidates.len()),
        repeated_read_paths,
        tool_calls_early,
        tool_errors_early,
        error_rate_early: rate(tool_errors_early, tool_calls_early),
        tool_calls_tail,
        tool_errors_tail,
        error_rate_tail: rate(tool_errors_tail, tool_calls_tail),
        interrupted_steps_tail: tail_steps
            .iter()
            .filter(|step| is_interrupted(step))
            .count(),
        dissatisfaction_signals,
        api_input_tokens_early: early_steps.iter().map(|step| step.api_input_tokens).sum(),
        api_input_tokens_tail: tail_steps.iter().map(|step| step.api_input_tokens).sum(),
    })
}

pub(crate) fn summarize(sessions: &[SessionFeedback], considered: usize) -> FeedbackReport {
    let requery: Vec<f64> = sessions
        .iter()
        .filter_map(|item| item.requery_rate)
        .collect();
    let mean = |values: &[f64]| {
        if values.is_empty() {
            None
        } else {
            Some(values.iter().sum::<f64>() / values.len() as f64)
        }
    };
    let total_errors =
        |selector: fn(&SessionFeedback) -> usize| sessions.iter().map(selector).sum::<usize>();
    let early_errors = total_errors(|item| item.tool_errors_early);
    let early_calls = total_errors(|item| item.tool_calls_early);
    let tail_errors = total_errors(|item| item.tool_errors_tail);
    let tail_calls = total_errors(|item| item.tool_calls_tail);

    FeedbackReport {
        sessions_considered: considered,
        sessions_with_compaction: sessions.len(),
        requery_rate_mean: mean(&requery),
        repeated_read_paths_total: sessions.iter().map(|item| item.repeated_read_paths).sum(),
        error_rate_early: rate(early_errors, early_calls),
        error_rate_tail: rate(tail_errors, tail_calls),
        interrupted_steps_total: sessions
            .iter()
            .map(|item| item.interrupted_steps_tail)
            .sum(),
        dissatisfaction_signals_total: sessions
            .iter()
            .map(|item| item.dissatisfaction_signals)
            .sum(),
        sessions: sessions.to_vec(),
    }
}

pub(crate) fn run(root: PathBuf, limit: usize, output: Option<PathBuf>) -> Result<(), String> {
    let datasets = iter_datasets(&root);
    let mut considered = 0usize;
    let mut sessions = Vec::new();
    for dataset in datasets {
        if considered >= limit {
            break;
        }
        let Ok(session) = load_session(&dataset) else {
            continue;
        };
        considered += 1;
        if let Some(feedback) = evaluate(&session) {
            sessions.push(feedback);
        }
    }
    let report = summarize(&sessions, considered);
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    write_or_print(output.as_deref(), &json)
}

#[cfg(test)]
#[path = "feedback_tests.rs"]
mod feedback_tests;
