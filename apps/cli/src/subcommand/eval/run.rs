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
    cases: Vec<CaseRun>,
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
        let message_count = load_case_json_lines(
            &cases_dir.join(
                case.get("messages_path")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
            ),
        )
        .map(|messages| messages.len())
        .unwrap_or_default();
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
                report.cases.push(CaseRun {
                    case_id: case_id.clone(),
                    status: "succeeded".to_string(),
                    message_count,
                    summary_chars: Some(outcome.summary.chars().count()),
                    recent_messages: Some(outcome.recent_message_count),
                    quality: Some(outcome.quality.clone()),
                    elapsed_ms: Some(elapsed_ms),
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
                    error: Some(error),
                });
            }
        }
    }

    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    write_or_print(Some(&output_dir.join("run_summary.json")), &json)
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod run_tests;
