//! 按 case 计算确定性指标。
//!
//! 评测边界（MUST 如实呈现）：
//! - recall 度量的是「typed id / 文件路径是否仍在 summary 中」的覆盖率代理，
//!   不是内容正确性，也不是续接可用性。
//! - 空 gold 不算 0 分；skipped case 不污染均值分母。
//! - 压缩输出由 `--summaries` 目录提供（`<case_id>.summary.txt`），
//!   生产路径应改为真实压缩执行面（见 issue #2133）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::telemetry::write_or_print;

#[derive(Debug, Deserialize)]
pub(crate) struct CaseInput {
    case_id: String,
    #[serde(default)]
    message_count: usize,
    #[serde(default)]
    estimated_tokens: u64,
    #[serde(default)]
    must_keep: Vec<String>,
    #[serde(default)]
    structure_health: Option<StructureInput>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct StructureInput {
    #[serde(default)]
    quality: Option<String>,
    #[serde(default)]
    degraded_chunks: usize,
    #[serde(default)]
    repair_attempts: usize,
}

#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct CaseResult {
    case_id: String,
    message_count: usize,
    estimated_tokens: u64,
    must_keep_total: usize,
    recall: Option<f64>,
    missed: Vec<String>,
    compression_ratio: Option<f64>,
    summary_chars: Option<usize>,
    skipped_reason: Option<String>,
    degraded: Option<bool>,
    repair_attempts: usize,
    quality: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct ScoreReport {
    total: usize,
    evaluated: usize,
    skipped: usize,
    must_keep_recall_mean: Option<f64>,
    compression_ratio_mean: Option<f64>,
    degraded_cases: usize,
    repair_attempts_total: usize,
    quality: BTreeMap<String, usize>,
    summary_source: Option<String>,
    cases: Vec<CaseResult>,
}

/// 覆盖率：must_keep 中仍在文本里出现的比例；空集合返回 None。
pub(crate) fn must_keep_recall(must_keep: &[String], text: &str) -> (Option<f64>, Vec<String>) {
    if must_keep.is_empty() {
        return (None, Vec::new());
    }
    let missed: Vec<String> = must_keep
        .iter()
        .filter(|item| !text.contains(item.as_str()))
        .cloned()
        .collect();
    let hit = must_keep.len() - missed.len();
    (Some(hit as f64 / must_keep.len() as f64), missed)
}

/// 压缩率：摘要 token / 早期 token；输入为 0 时返回 None。
pub(crate) fn compression_ratio(early_tokens: u64, summary_chars: usize) -> Option<f64> {
    if early_tokens == 0 {
        return None;
    }
    // summary 以中英混合计：字符 ×0.6 与 runtime 的中文口径同量级。
    let summary_tokens = summary_chars as f64 * 0.6;
    Some(summary_tokens / early_tokens as f64)
}

pub(crate) fn evaluate_case(case: &CaseInput, summary: Option<&str>) -> CaseResult {
    let mut result = CaseResult {
        case_id: case.case_id.clone(),
        message_count: case.message_count,
        estimated_tokens: case.estimated_tokens,
        must_keep_total: case.must_keep.len(),
        ..Default::default()
    };
    if let Some(health) = case.structure_health.as_ref() {
        result.quality = health.quality.clone();
        result.repair_attempts = health.repair_attempts;
        result.degraded = Some(health.degraded_chunks > 0);
    }
    let Some(summary) = summary else {
        result.skipped_reason = Some("no_summary".to_string());
        return result;
    };
    let (recall, missed) = must_keep_recall(&case.must_keep, summary);
    result.recall = recall;
    result.missed = missed.into_iter().take(20).collect();
    result.compression_ratio = compression_ratio(case.estimated_tokens, summary.chars().count());
    result.summary_chars = Some(summary.chars().count());
    result
}

pub(crate) fn summarize(cases: &[CaseResult]) -> ScoreReport {
    let mut report = ScoreReport {
        total: cases.len(),
        ..Default::default()
    };
    let mut recalls = Vec::new();
    let mut ratios = Vec::new();
    for case in cases {
        if case.recall.is_none() && case.compression_ratio.is_none() {
            report.skipped += 1;
            continue;
        }
        report.evaluated += 1;
        if let Some(recall) = case.recall {
            recalls.push(recall);
        }
        if let Some(ratio) = case.compression_ratio {
            ratios.push(ratio);
        }
        if case.degraded.unwrap_or(false) {
            report.degraded_cases += 1;
        }
        report.repair_attempts_total += case.repair_attempts;
        let quality = case
            .quality
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        *report.quality.entry(quality).or_default() += 1;
    }
    report.must_keep_recall_mean = mean(&recalls);
    report.compression_ratio_mean = mean(&ratios);
    report.cases = cases.to_vec();
    report
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn load_cases(path: &Path) -> Result<Vec<CaseInput>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("读取 cases 失败 {}: {error}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<CaseInput>(line)
                .map_err(|error| format!("解析 case 失败: {error}"))
        })
        .collect()
}

fn read_summary(summaries: Option<&Path>, case_id: &str) -> Option<String> {
    let path = summaries?.join(format!("{case_id}.summary.txt"));
    std::fs::read_to_string(path).ok()
}

pub(crate) fn run(
    cases: PathBuf,
    summaries: Option<PathBuf>,
    output: Option<PathBuf>,
) -> Result<(), String> {
    let inputs = load_cases(&cases.join("cases.jsonl"))?;
    let results: Vec<CaseResult> = inputs
        .iter()
        .map(|case| {
            evaluate_case(
                case,
                read_summary(summaries.as_deref(), &case.case_id).as_deref(),
            )
        })
        .collect();
    let mut report = summarize(&results);
    report.summary_source = summaries.map(|path| path.display().to_string());
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    write_or_print(output.as_deref(), &json)
}

#[cfg(test)]
#[path = "score_tests.rs"]
mod score_tests;
