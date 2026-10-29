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
    /// 压缩点之前的消息数（被摘要覆盖的 early 历史长度；0=未压缩）。
    compacted_message_count: usize,
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
///
/// 口径（#2133）：保留「出现次数 >= `min_frequency`」的标识（默认 3），
/// 按频次降序、同频按字典序，上限 `limit`（默认 1000）。大 session 里只
/// 出现一次的路径多为一次性噪声，纳入分母会系统性压低召回；若高频标识
/// 不足（小会话），回退按频次取 top-`limit`。
pub(crate) fn rank_identifiers(
    texts: &[String],
    limit: usize,
    min_frequency: usize,
) -> Vec<String> {
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
    let frequent: Vec<(String, usize)> = ordered
        .iter()
        .filter(|(_, count)| *count >= min_frequency.max(1))
        .cloned()
        .collect();
    let selected = if frequent.is_empty() {
        ordered
    } else {
        frequent
    };
    selected
        .into_iter()
        .take(limit)
        .map(|(item, _)| item)
        .collect()
}

fn build_case(
    dataset: &Path,
    output_dir: &Path,
    limit: usize,
    min_frequency: usize,
    max_tokens: u64,
    only_compacted: bool,
    min_compacted_tokens: u64,
) -> Result<Option<Case>, String> {
    let session = load_session(dataset)?;
    if session.messages.len() < 20 {
        return Ok(None);
    }
    // 真实生产压缩点：quiz/生产反馈只对发生过压缩的会话有意义。
    if only_compacted && session.compact_start.is_none() {
        return Ok(None);
    }
    let texts: Vec<String> = session.messages.iter().map(message_text).collect();
    let joined = texts.join("\n");
    let early_end = session.compacted_message_count.min(session.messages.len());
    let early_texts: Vec<String> = texts[..early_end].to_vec(); // allow unsafe_text_op: Vec slice
    let estimated_tokens = estimate_tokens(&joined);
    if estimated_tokens > max_tokens {
        return Ok(None);
    }
    if session.compact_start.is_some() {
        let compacted_texts: Vec<String> = session.messages
            [..session.compacted_message_count.min(session.messages.len())]
            .iter()
            .map(message_text)
            .collect(); // allow unsafe_text_op: Vec slice
        if estimate_tokens(&compacted_texts.join("\n")) < min_compacted_tokens {
            return Ok(None);
        }
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
        // must_keep 语义：被压缩历史里「应保留的事实」——只从 early 段抽取，
        // 与 recall 同口径（全量会把 tail 仍在窗口的标识算进分母）。
        must_keep: rank_identifiers(&early_texts, limit, min_frequency),
        must_keep_source: "auto_identifiers".to_string(),
        messages_path: format!("{}/messages.jsonl", session.session_id),
        compacted_message_count: session.compacted_message_count,
    };
    let case_json = serde_json::to_string_pretty(&case).map_err(|error| error.to_string())?;
    std::fs::write(case_dir.join("case.json"), format!("{case_json}\n"))
        .map_err(|error| error.to_string())?;
    Ok(Some(case))
}

/// `export` 的完整参数（避免超长签名；字段语义见 CLI `--help`）。
pub(crate) struct ExportOptions {
    pub(crate) root: PathBuf,
    pub(crate) output_dir: PathBuf,
    pub(crate) limit: usize,
    pub(crate) min_steps: usize,
    pub(crate) max_must_keep: usize,
    pub(crate) min_must_keep_frequency: usize,
    pub(crate) max_tokens: u64,
    pub(crate) only_compacted: bool,
    pub(crate) min_compacted_tokens: u64,
    pub(crate) session: Option<String>,
}

pub(crate) fn run(options: ExportOptions) -> Result<(), String> {
    let ExportOptions {
        root,
        output_dir,
        limit,
        min_steps,
        max_must_keep,
        min_must_keep_frequency,
        max_tokens,
        only_compacted,
        min_compacted_tokens,
        session: session_filter,
    } = options;
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
        if let Some(target) = session_filter.as_deref() {
            let stem = dataset
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_default();
            if stem != target {
                continue;
            }
        }
        let Ok(session) = load_session(&dataset) else {
            continue;
        };
        if session.committed_step_count < min_steps {
            continue;
        }
        let Some(case) = build_case(
            &dataset,
            &output_dir,
            max_must_keep,
            min_must_keep_frequency,
            max_tokens,
            only_compacted,
            min_compacted_tokens,
        )?
        else {
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
