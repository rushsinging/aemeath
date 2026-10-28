//! `eval compact matrix`（P2）：同一批 case 跑多个 compact 模型，按模型出对照。
//!
//! 评测边界（MUST 如实呈现）：
//! - 每个模型对同一 case 各跑一次真实压缩（生产管线）；模型间变量唯一
//!   （同输入、同 previous_summary）。
//! - 指标含耗时、summary 规模、must_keep 召回与降级计数——**降级 case
//!   MUST 计入 degraded，不得当成功**；某模型失败率高时直接呈现，不剔除。
//! - 判官（语义质量）不在本命令范围（P3，`--judge` 评测时指定）。

use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;

use super::run::{load_case_json_lines, load_cases_index};
use super::score::must_keep_recall;
use super::telemetry::{percentile, write_or_print};

#[derive(Debug, Default, Serialize)]
pub(crate) struct ModelRow {
    model: String,
    cases: usize,
    succeeded: usize,
    failed: usize,
    degraded: usize,
    duration_ms_p50: Option<u64>,
    duration_ms_p95: Option<u64>,
    duration_ms_max: Option<u64>,
    summary_chars_mean: Option<f64>,
    must_keep_recall_mean: Option<f64>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct MatrixReport {
    models: Vec<ModelRow>,
    note: String,
}

async fn run_one(
    model: &str,
    case: &serde_json::Value,
    cases_dir: &std::path::Path,
) -> Result<composition::app::EvalCompactOutcome, String> {
    let messages_path = case
        .get("messages_path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "case 缺少 messages_path".to_string())?;
    let messages = load_case_json_lines(&cases_dir.join(messages_path))?;
    let previous = case
        .get("previous_summary")
        .and_then(|value| value.as_str())
        .filter(|text| !text.is_empty());
    composition::app::run_compact_for_eval(messages, previous, Some(model))
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn run(
    cases_dir: PathBuf,
    models: Vec<String>,
    limit: usize,
    output: Option<PathBuf>,
) -> Result<(), String> {
    if models.is_empty() {
        return Err("至少提供一个模型（--model 可重复传入）".to_string());
    }
    let index = load_cases_index(&cases_dir)?;
    let cases: Vec<&serde_json::Value> = index.iter().take(limit).collect();
    let mut report = MatrixReport {
        models: Vec::new(),
        note: "同一 case 对每个模型各跑一次真实压缩；degraded 计入该模型，不当成功。".to_string(),
    };

    for model in &models {
        let mut durations: Vec<u64> = Vec::new();
        let mut summary_chars: Vec<f64> = Vec::new();
        let mut recalls: Vec<f64> = Vec::new();
        let mut succeeded = 0usize;
        let mut failed = 0usize;
        let mut degraded = 0usize;
        for case in &cases {
            let started = Instant::now();
            match run_one(model, case, &cases_dir).await {
                Ok(outcome) => {
                    durations.push(started.elapsed().as_millis() as u64);
                    if outcome.quality != "Llm" {
                        degraded += 1;
                    }
                    let summary_len = outcome.summary.chars().count();
                    summary_chars.push(summary_len as f64);
                    let must_keep: Vec<String> = case
                        .get("must_keep")
                        .and_then(|value| value.as_array())
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    let (recall, _) = must_keep_recall(&must_keep, &outcome.summary);
                    if let Some(recall) = recall {
                        recalls.push(recall);
                    }
                    succeeded += 1;
                    println!(
                        "[matrix] {model} {} quality={} summary={summary_len} chars",
                        case.get("case_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default(),
                        outcome.quality
                    );
                }
                Err(error) => {
                    failed += 1;
                    println!(
                        "[matrix] {model} {} FAILED: {error}",
                        case.get("case_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                    );
                }
            }
        }
        let mean = |values: &[f64]| {
            if values.is_empty() {
                None
            } else {
                Some(values.iter().sum::<f64>() / values.len() as f64)
            }
        };
        report.models.push(ModelRow {
            model: model.clone(),
            cases: cases.len(),
            succeeded,
            failed,
            degraded,
            duration_ms_p50: percentile(&durations, 0.50),
            duration_ms_p95: percentile(&durations, 0.95),
            duration_ms_max: durations.iter().copied().max(),
            summary_chars_mean: mean(&summary_chars),
            must_keep_recall_mean: mean(&recalls),
        });
    }

    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    write_or_print(output.as_deref(), &json)
}

#[cfg(test)]
#[path = "matrix_tests.rs"]
mod matrix_tests;
