//! `eval compact run`：对导出的 case 跑一次真实压缩（P1 执行面）。
//!
//! 评测边界（MUST 如实呈现）：
//! - 压缩执行由组合根（composition）完成：模型解析与生产同路径
//!   （`CompactModelResolver` + `context.compact_model`），压缩走生产管线
//!   `compact_messages_with_llm`，不用简化替身。
//! - 未配置 compact 模型时显式失败退出——评测不静默降级为本地压缩。
//! - 输出 summary 文本；`CompactMetricsData` 结构化指标待 #2136 合入后接入。

use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;

use super::telemetry::write_or_print;

#[derive(Debug, Serialize)]
pub(crate) struct CaseRun {
    case_id: String,
    status: String,
    message_count: usize,
    summary_chars: Option<usize>,
    recent_messages: Option<usize>,
    quality: Option<String>,
    elapsed_ms: Option<u64>,
    /// 输入规模估算（与压缩触发口径一致），用于速度归一化。
    estimated_input_tokens: Option<u64>,
    /// 速度指标：毫秒 / 1K 输入 token（消除输入规模差异，跨 session 可比）。
    ms_per_1k_input_tokens: Option<f64>,
    error: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct RunReport {
    cases_total: usize,
    succeeded: usize,
    failed: usize,
    skipped: usize,
    binding_model: Option<String>,
    context_size: Option<usize>,
    /// 单次真实压缩耗时分布（ms）。
    elapsed_ms_p50: Option<u64>,
    elapsed_ms_p95: Option<u64>,
    elapsed_ms_max: Option<u64>,
    /// 速度归一化均值：ms / 1K 输入 token。
    ms_per_1k_input_tokens_mean: Option<f64>,
    cases: Vec<CaseRun>,
}

/// 速度派生：毫秒 / 1K 输入 token；输入为 0 时返回 None（不可比）。
pub(crate) fn ms_per_1k_input_tokens(elapsed_ms: u64, estimated_input_tokens: u64) -> Option<f64> {
    if estimated_input_tokens == 0 {
        None
    } else {
        Some(elapsed_ms as f64 * 1000.0 / estimated_input_tokens as f64)
    }
}

fn percentile_u64(values: &[u64], quantile: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
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

pub(crate) fn load_case_json_lines(
    path: &std::path::Path,
) -> Result<Vec<serde_json::Value>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("读取 {}: {error}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|error| format!("解析失败: {error}")))
        .collect()
}

pub(crate) fn load_cases_index(
    cases_dir: &std::path::Path,
) -> Result<Vec<serde_json::Value>, String> {
    let text = std::fs::read_to_string(cases_dir.join("cases.jsonl"))
        .map_err(|error| format!("读取 cases.jsonl 失败: {error}"))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|error| format!("解析 case 失败: {error}")))
        .collect()
}

/// 单个 case 的执行（消息 JSON → 组合根真实压缩）。
async fn run_case(
    case: &serde_json::Value,
    cases_dir: &std::path::Path,
    selection: Option<&str>,
    reasoning: Option<&str>,
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
    composition::app::run_compact_for_eval(messages, previous, selection, reasoning)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn run(
    cases_dir: PathBuf,
    output_dir: PathBuf,
    limit: usize,
    model: Option<String>,
    reasoning: Option<String>,
) -> Result<(), String> {
    std::fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?;
    let index = load_cases_index(&cases_dir)?;
    let mut report = RunReport::default();

    for case in index.iter().take(limit) {
        let case_id = case
            .get("case_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        report.cases_total += 1;
        let messages = load_case_json_lines(
            &cases_dir.join(
                case.get("messages_path")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
            ),
        )
        .unwrap_or_default();
        let message_count = messages.len();
        let estimated_input_tokens = crate::subcommand::eval::dataset::estimate_tokens(
            &messages
                .iter()
                .map(crate::subcommand::eval::dataset::message_text)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let started = Instant::now();
        match run_case(case, &cases_dir, model.as_deref(), reasoning.as_deref()).await {
            Ok(outcome) => {
                let elapsed_ms = started.elapsed().as_millis() as u64;
                report.binding_model = report.binding_model.or(Some(outcome.model.clone()));
                report.context_size = report.context_size.or(Some(outcome.context_size));
                let summary_path = output_dir.join(format!("{case_id}.summary.txt"));
                std::fs::write(&summary_path, &outcome.summary)
                    .map_err(|error| format!("写 {}: {error}", summary_path.display()))?;
                report.succeeded += 1;
                let speed = ms_per_1k_input_tokens(elapsed_ms, estimated_input_tokens);
                report.cases.push(CaseRun {
                    case_id: case_id.clone(),
                    status: "succeeded".to_string(),
                    message_count,
                    summary_chars: Some(outcome.summary.chars().count()),
                    recent_messages: Some(outcome.recent_message_count),
                    quality: Some(outcome.quality.clone()),
                    elapsed_ms: Some(elapsed_ms),
                    estimated_input_tokens: Some(estimated_input_tokens),
                    ms_per_1k_input_tokens: speed,
                    error: None,
                });
                println!(
                    "[run] {case_id} summary={} chars recent={} quality={} elapsed_ms={elapsed_ms}",
                    outcome.summary.chars().count(),
                    outcome.recent_message_count,
                    outcome.quality
                );
            }
            Err(error) => {
                report.failed += 1;
                report.cases.push(CaseRun {
                    case_id,
                    status: "failed".to_string(),
                    message_count,
                    summary_chars: None,
                    recent_messages: None,
                    quality: None,
                    elapsed_ms: Some(started.elapsed().as_millis() as u64),
                    estimated_input_tokens: Some(estimated_input_tokens),
                    ms_per_1k_input_tokens: None,
                    error: Some(error),
                });
            }
        }
    }

    let durations: Vec<u64> = report
        .cases
        .iter()
        .filter_map(|case| case.elapsed_ms)
        .collect();
    report.elapsed_ms_p50 = percentile_u64(&durations, 0.50);
    report.elapsed_ms_p95 = percentile_u64(&durations, 0.95);
    report.elapsed_ms_max = durations.iter().copied().max();
    report.ms_per_1k_input_tokens_mean = mean(
        &report
            .cases
            .iter()
            .filter_map(|case| case.ms_per_1k_input_tokens)
            .collect::<Vec<_>>(),
    );

    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    write_or_print(Some(&output_dir.join("run_summary.json")), &json)
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod run_tests;
