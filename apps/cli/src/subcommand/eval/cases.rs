//! 从真实 session 导出压缩评测 case。
//!
//! 每个 case 是一次「压缩前输入」快照：`messages.jsonl` + `case.json`。
//! `must_keep` 由 typed id / 文件路径按出现频次抽取并限流；来源显式标注为
//! `auto_identifiers`，人工/模型补充的条目后续追加时不改写本字段。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::constants::{ID_PATTERN, NOISE_PREFIXES, PATH_PATTERN};

use super::dataset::{estimate_tokens, iter_datasets, load_session, message_text};
use super::telemetry::{percentile, write_or_print};

#[derive(Debug, Serialize)]
pub(crate) struct Case {
    case_id: String,
    session_id: String,
    model: Option<String>,
    source_dataset: String,
    committed_step_count: usize,
    message_count: usize,
    estimated_tokens: u64,
    previous_summary: String,
    must_keep: Vec<String>,
    must_keep_source: String,
    messages_path: String,
}

pub(crate) fn extract_identifiers(texts: &[String]) -> Vec<String> {
    let mut seen: BTreeMap<String, ()> = BTreeMap::new();
    for text in texts {
        for found in ID_PATTERN.find_iter(text) {
            seen.insert(found.as_str().to_string(), ());
        }
        for found in PATH_PATTERN.find_iter(text) {
            let path = found.as_str();
            if path.len() < 5 || NOISE_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
                continue;
            }
            seen.insert(path.to_string(), ());
        }
    }
    seen.into_keys().collect()
}

/// 按出现频次挑选 must_keep：高频条目更可能承载任务事实。
pub(crate) fn rank_identifiers(texts: &[String], limit: usize) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = extract_identifiers(texts)
        .into_iter()
        .map(|item| (item, 0))
        .collect();
    for text in texts {
        for found in ID_PATTERN.find_iter(text) {
            if let Some(count) = counts.get_mut(found.as_str()) {
                *count += 1;
            }
        }
        for found in PATH_PATTERN.find_iter(text) {
            if let Some(count) = counts.get_mut(found.as_str()) {
                *count += 1;
            }
        }
    }
    let mut ordered: Vec<(String, usize)> = counts.into_iter().collect();
    ordered.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ordered
        .into_iter()
        .take(limit)
        .map(|(item, _)| item)
        .collect()
}

fn build_case(
    dataset: &Path,
    output_dir: &Path,
    limit: usize,
    max_tokens: u64,
) -> Result<Option<Case>, String> {
    let session = load_session(dataset)?;
    if session.messages.len() < 20 {
        return Ok(None);
    }
    let texts: Vec<String> = session.messages.iter().map(message_text).collect();
    let joined = texts.join("\n");
    let estimated_tokens = estimate_tokens(&joined);
    if estimated_tokens > max_tokens {
        return Ok(None);
    }
    let case_dir = output_dir.join(&session.session_id);
    std::fs::create_dir_all(&case_dir).map_err(|error| error.to_string())?;

    let mut lines = String::new();
    for message in &session.messages {
        lines.push_str(&serde_json::to_string(message).map_err(|error| error.to_string())?);
        lines.push('\n');
    }
    let messages_path = case_dir.join("messages.jsonl");
    std::fs::write(&messages_path, lines).map_err(|error| error.to_string())?;

    let case = Case {
        case_id: session.session_id.clone(),
        session_id: session.session_id.clone(),
        model: session.model.clone(),
        source_dataset: dataset.display().to_string(),
        committed_step_count: session.committed_step_count,
        message_count: session.messages.len(),
        estimated_tokens,
        previous_summary: session.previous_summary.clone().unwrap_or_default(),
        must_keep: rank_identifiers(&texts, limit),
        must_keep_source: "auto_identifiers".to_string(),
        messages_path: format!("{}/messages.jsonl", session.session_id),
    };
    let case_json = serde_json::to_string_pretty(&case).map_err(|error| error.to_string())?;
    std::fs::write(case_dir.join("case.json"), format!("{case_json}\n"))
        .map_err(|error| error.to_string())?;
    Ok(Some(case))
}

pub(crate) fn run(
    root: PathBuf,
    output_dir: PathBuf,
    limit: usize,
    min_steps: usize,
    max_must_keep: usize,
    max_tokens: u64,
) -> Result<(), String> {
    std::fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?;
    let mut index = String::new();
    let mut exported = 0usize;
    let mut tokens: Vec<u64> = Vec::new();
    let mut messages: Vec<u64> = Vec::new();
    let mut must_keep: Vec<u64> = Vec::new();
    for dataset in iter_datasets(&root) {
        if exported >= limit {
            break;
        }
        let Ok(session) = load_session(&dataset) else {
            continue;
        };
        if session.committed_step_count < min_steps {
            continue;
        }
        let Some(case) = build_case(&dataset, &output_dir, max_must_keep, max_tokens)? else {
            continue;
        };
        index.push_str(&serde_json::to_string(&case).map_err(|error| error.to_string())?);
        index.push('\n');
        tokens.push(case.estimated_tokens);
        messages.push(case.message_count as u64);
        must_keep.push(case.must_keep.len() as u64);
        exported += 1;
        println!(
            "[export] {} messages={} tokens≈{} must_keep={}",
            case.case_id,
            case.message_count,
            case.estimated_tokens,
            case.must_keep.len()
        );
    }
    write_or_print(Some(&output_dir.join("cases.jsonl")), index.trim_end())?;
    let summary = serde_json::json!({
        "cases": exported,
        "message_count": {
            "p50": percentile(&messages, 0.50),
            "p95": percentile(&messages, 0.95),
            "max": messages.iter().copied().max(),
        },
        "estimated_tokens": {
            "p50": percentile(&tokens, 0.50),
            "p95": percentile(&tokens, 0.95),
            "max": tokens.iter().copied().max(),
        },
        "must_keep": {
            "p50": percentile(&must_keep, 0.50),
            "max": must_keep.iter().copied().max(),
        },
    });
    let summary_json = serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?;
    write_or_print(Some(&output_dir.join("inputs.json")), &summary_json)?;
    println!("exported {exported} case(s) → {}", output_dir.display());
    Ok(())
}

#[cfg(test)]
#[path = "cases_tests.rs"]
mod cases_tests;
