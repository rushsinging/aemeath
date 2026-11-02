//! 压缩遥测基线：读 `context.log` 出压缩画像。
//!
//! 评测边界（MUST 如实呈现）：
//! - 只描述「发生过什么」——频率、耗时、分块形状、降级与模型标识，
//!   不评价摘要内容质量。
//! - 日志不记录触发来源（自动/手动），因此不输出「自动压缩占比」。
//! - 2026-10-28 之前的日志没有 `model=` / `quality=` / `elapsed_ms=`，
//!   分别归入 `unknown` 或用「开始→完成」时间戳推算，不与新日志混算。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset};
use serde::Serialize;

use super::constants::{DONE_RE, MAP_DONE_RE, MAP_REDUCE_RE, REDUCE_DONE_RE, REFRESH_RE, START_RE};

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub(crate) struct Percentiles {
    pub(crate) p50: Option<u64>,
    pub(crate) p95: Option<u64>,
    pub(crate) max: Option<u64>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct ModelStats {
    count: usize,
    duration_ms_p50: Option<u64>,
    duration_ms_p95: Option<u64>,
    duration_ms_max: Option<u64>,
    early_tokens_p50: Option<u64>,
    early_tokens_p95: Option<u64>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct ChunkShape {
    compactions_with_map: usize,
    chunks_p50: Option<u64>,
    chunks_p95: Option<u64>,
    chunks_max: Option<u64>,
    chunk_ms_p50_median: Option<u64>,
    map_ms_p95: Option<u64>,
    reduce_ms_p95: Option<u64>,
    refresh_compactions: usize,
    refresh_ms_max: Option<u64>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct Notes {
    unmatched_sessions: usize,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct Source {
    log: String,
    since: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct TelemetryReport {
    total_compactions: usize,
    per_day: BTreeMap<String, usize>,
    duration_ms: Percentiles,
    early_tokens: Percentiles,
    by_model: BTreeMap<String, ModelStats>,
    chunk_shape: ChunkShape,
    quality: BTreeMap<String, usize>,
    degraded_chunks_total: u64,
    notes: Notes,
    source: Source,
}

#[derive(Debug, Default)]
pub(crate) struct CompactionRecord {
    started_at: Option<DateTime<FixedOffset>>,
    finished_at: Option<DateTime<FixedOffset>>,
    early_tokens: u64,
    model: Option<String>,
    chunks: Option<u64>,
    waves: Option<u64>,
    chunk_ms_p50: Option<u64>,
    map_ms: Option<u64>,
    reduce_ms: Option<u64>,
    refresh_rounds: usize,
    refresh_ms: u64,
    degraded_chunks: Option<u64>,
    quality: Option<String>,
    elapsed_ms: Option<u64>,
}

impl CompactionRecord {
    /// 优先用日志里的 elapsed_ms；旧日志缺失时按开始/完成时间戳推算。
    fn duration_ms(&self) -> Option<u64> {
        if let Some(elapsed) = self.elapsed_ms {
            return Some(elapsed);
        }
        let started = self.started_at?;
        let finished = self.finished_at?;
        (finished - started).num_milliseconds().try_into().ok()
    }
}

pub(crate) fn percentile(values: &[u64], quantile: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let index = ((sorted.len() as f64 * quantile) as usize).min(sorted.len() - 1);
    Some(sorted[index])
}

fn percentiles(values: &[u64]) -> Percentiles {
    Percentiles {
        p50: percentile(values, 0.50),
        p95: percentile(values, 0.95),
        max: values.iter().copied().max(),
    }
}

fn parse_time(value: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

fn quality_label(raw: &str) -> String {
    raw.split('{').next().unwrap_or(raw).trim().to_string()
}

/// 解析日志：按 session 把「开始」与「完成」配对成一次压缩。
pub(crate) fn parse_log(
    lines: impl Iterator<Item = String>,
    since: Option<&str>,
) -> (Vec<CompactionRecord>, usize) {
    let mut pending: BTreeMap<String, CompactionRecord> = BTreeMap::new();
    let mut records: Vec<CompactionRecord> = Vec::new();

    for line in lines {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let ts = entry.get("ts").and_then(|v| v.as_str()).unwrap_or_default();
        if let Some(since) = since {
            if ts < since {
                continue;
            }
        }
        let session = entry
            .get("session")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let message = entry
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if let Some(captures) = START_RE.captures(message) {
            // 同 session 连续压缩：旧未配对记录直接丢弃（日志截断/崩溃）。
            let mut record = CompactionRecord {
                started_at: parse_time(ts),
                ..Default::default()
            };
            record.early_tokens = captures[3].parse().unwrap_or_default();
            record.model = captures.get(6).map(|m| m.as_str().to_string());
            pending.insert(session, record);
            continue;
        }

        let Some(record) = pending.get_mut(&session) else {
            continue;
        };

        if let Some(captures) = MAP_REDUCE_RE.captures(message) {
            record.chunks = captures[1].parse().ok();
            if record.model.is_none() {
                record.model = captures.get(4).map(|m| m.as_str().to_string());
            }
            if let Some(waves) = captures.get(3) {
                record.waves = waves.as_str().parse().ok();
            }
        } else if let Some(captures) = MAP_DONE_RE.captures(message) {
            record.chunks = captures[1].parse().ok();
            record.degraded_chunks = captures[3].parse().ok();
            if let Some(waves) = captures.get(4) {
                record.waves = waves.as_str().parse().ok();
            }
            if let Some(p50) = captures.get(5) {
                record.chunk_ms_p50 = p50.as_str().parse().ok();
            }
            if let Some(map_ms) = captures.get(8) {
                record.map_ms = map_ms.as_str().parse().ok();
            }
        } else if let Some(captures) = REDUCE_DONE_RE.captures(message) {
            if let Some(reduce_ms) = captures.get(2) {
                record.reduce_ms = reduce_ms.as_str().parse().ok();
            }
        } else if let Some(captures) = REFRESH_RE.captures(message) {
            record.refresh_rounds += 1;
            if let Some(refresh_ms) = captures.get(4) {
                record.refresh_ms += refresh_ms.as_str().parse::<u64>().unwrap_or_default();
            }
        } else if let Some(captures) = DONE_RE.captures(message) {
            record.finished_at = parse_time(ts);
            if let Some(quality) = captures.get(3) {
                record.quality = Some(quality.as_str().to_string());
            }
            if record.model.is_none() {
                record.model = captures.get(4).map(|m| m.as_str().to_string());
            }
            if let Some(elapsed) = captures.get(5) {
                record.elapsed_ms = elapsed.as_str().parse().ok();
            }
            let finished = pending.remove(&session).unwrap_or_default();
            records.push(finished);
        }
    }

    (records, pending.len())
}

pub(crate) fn summarize(records: &[CompactionRecord]) -> TelemetryReport {
    let durations: Vec<u64> = records
        .iter()
        .filter_map(CompactionRecord::duration_ms)
        .collect();
    let early: Vec<u64> = records.iter().map(|r| r.early_tokens).collect();
    let chunks: Vec<u64> = records.iter().filter_map(|r| r.chunks).collect();
    let chunk_p50: Vec<u64> = records.iter().filter_map(|r| r.chunk_ms_p50).collect();
    let map_ms: Vec<u64> = records.iter().filter_map(|r| r.map_ms).collect();
    let reduce_ms: Vec<u64> = records.iter().filter_map(|r| r.reduce_ms).collect();

    let mut per_day: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_model: BTreeMap<String, (Vec<u64>, Vec<u64>)> = BTreeMap::new();
    let mut quality: BTreeMap<String, usize> = BTreeMap::new();
    let mut degraded_chunks_total = 0u64;

    for record in records {
        if let Some(started) = record.started_at {
            *per_day
                .entry(started.format("%Y-%m-%d").to_string())
                .or_default() += 1;
        }
        let model = record
            .model
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let bucket = by_model.entry(model).or_default();
        if let Some(duration) = record.duration_ms() {
            bucket.0.push(duration);
        }
        bucket.1.push(record.early_tokens);
        let label = record
            .quality
            .as_deref()
            .map(quality_label)
            .unwrap_or_else(|| "unknown".to_string());
        *quality.entry(label).or_default() += 1;
        degraded_chunks_total += record.degraded_chunks.unwrap_or_default();
    }

    let by_model = by_model
        .into_iter()
        .map(|(model, (durations, early))| {
            let stats = ModelStats {
                count: early.len(),
                duration_ms_p50: percentile(&durations, 0.50),
                duration_ms_p95: percentile(&durations, 0.95),
                duration_ms_max: durations.iter().copied().max(),
                early_tokens_p50: percentile(&early, 0.50),
                early_tokens_p95: percentile(&early, 0.95),
            };
            (model, stats)
        })
        .collect();

    TelemetryReport {
        total_compactions: records.len(),
        per_day,
        duration_ms: percentiles(&durations),
        early_tokens: percentiles(&early),
        by_model,
        chunk_shape: ChunkShape {
            compactions_with_map: chunks.len(),
            chunks_p50: percentile(&chunks, 0.50),
            chunks_p95: percentile(&chunks, 0.95),
            chunks_max: chunks.iter().copied().max(),
            chunk_ms_p50_median: percentile(&chunk_p50, 0.50),
            map_ms_p95: percentile(&map_ms, 0.95),
            reduce_ms_p95: percentile(&reduce_ms, 0.95),
            refresh_compactions: records.iter().filter(|r| r.refresh_rounds > 0).count(),
            refresh_ms_max: records
                .iter()
                .map(|r| r.refresh_ms)
                .max()
                .filter(|v| *v > 0),
        },
        quality,
        degraded_chunks_total,
        notes: Notes::default(),
        source: Source::default(),
    }
}

pub(crate) fn run(
    log: PathBuf,
    since: Option<String>,
    output: Option<PathBuf>,
) -> Result<(), String> {
    let file = std::fs::File::open(&log)
        .map_err(|error| format!("打开日志失败 {}: {error}", log.display()))?;
    let lines =
        std::io::BufRead::lines(std::io::BufReader::new(file)).map(|line| line.unwrap_or_default());
    let (records, unmatched) = parse_log(lines, since.as_deref());
    let mut report = summarize(&records);
    report.notes.unmatched_sessions = unmatched;
    report.source = Source {
        log: log.display().to_string(),
        since,
    };
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    write_or_print(output.as_deref(), &json)
}

pub(crate) fn write_or_print(output: Option<&Path>, content: &str) -> Result<(), String> {
    match output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            std::fs::write(path, format!("{content}\n")).map_err(|error| error.to_string())
        }
        None => {
            println!("{content}");
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod telemetry_tests;
