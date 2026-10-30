//! RSI 过程事件流（#2070）：周期全链路事实落盘，评测建立在可观测
//! 过程之上（对齐 memory reflect / systemone 事件流口径）。
//!
//! 落盘 `~/.agents/harness/events/YYYY-MM-DD.jsonl`：真 append-only、
//! 按日切分、30 天 GC、fail-open（写失败 warn NEVER 阻断周期）。
//! dry-run 同样落盘（mode 标记）——预演产物是评测输入。

use crate::domain::errors::HarnessError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 单条过程事件（type 标记 + 载荷）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RsiCycleEvent {
    CycleStart {
        cycle_id: String,
        trigger: String,
        model: String,
        dry_run: bool,
        blocks: usize,
    },
    BlockEvidence {
        block_index: usize,
        sessions_runs: Vec<String>,
        signals: Vec<String>,
    },
    RouteCandidates {
        block_index: usize,
        signal_id: String,
        candidates: Vec<String>,
        reason: String,
    },
    RouteSelected {
        block_index: usize,
        signal_id: String,
        component: String,
        stage: String,
        reason: String,
    },
    RouteUnresolved {
        block_index: usize,
        signal_id: String,
        reason: String,
    },
    ComponentRouteRejected {
        block_index: usize,
        component: String,
        signal_ids: Vec<String>,
        message: String,
    },
    ExecutionTargetFixed {
        block_index: usize,
        component: String,
        signal_ids: Vec<String>,
    },
    LlmCall {
        stage: String,
        block_index: usize,
        duration_ms: u64,
        prompt_chars: usize,
        response_chars: usize,
        usage: crate::domain::improvement::RsiLlmUsage,
        error: Option<String>,
    },
    DiagnosisSummarized {
        issues: usize,
        occurrences: usize,
    },
    DiagnosisParsed {
        block_index: usize,
        issues: usize,
    },
    ProposalParsed {
        block_index: usize,
        reason: String,
        edits: usize,
    },
    ProposalRejected {
        block_index: usize,
        stage: String,
        message: String,
    },
    EditApplied {
        component: String,
        action: String,
        old_chars: usize,
        new_chars: usize,
    },
    ComposeResult {
        label: String,
    },
    LessonRecorded {
        lesson_id: String,
        signal: String,
    },
    StrengthGate {
        signal_id: String,
        tier: String,
        rationale: String,
    },
    LessonReviewed {
        lesson_id: String,
        action: String,
        rationale: String,
    },
    LessonVerified {
        lesson_id: String,
        component: String,
        target_signal: String,
        status: String,
    },
    LessonRetired {
        component: String,
        count: usize,
    },
    CycleEnd {
        summary: String,
        duration_ms: u64,
    },
}

/// 事件记录器：append JSONL + 按日切分 + GC（fail-open）。
pub struct RsiEventRecorder {
    events_dir: PathBuf,
}

impl RsiEventRecorder {
    pub fn new(events_dir: impl Into<PathBuf>) -> Self {
        Self {
            events_dir: events_dir.into(),
        }
    }

    /// 追加一条事件（fail-open：写失败只 warn）。
    pub fn record(&self, event: &RsiCycleEvent) {
        if let Err(error) = self.try_record(event) {
            log::warn!(
                target: crate::LOG_TARGET,
                "RSI 事件落盘失败（周期继续）：{error}"
            );
        }
    }

    fn try_record(&self, event: &RsiCycleEvent) -> Result<(), HarnessError> {
        std::fs::create_dir_all(&self.events_dir).map_err(|error| {
            HarnessError::storage(
                crate::constants::ERROR_DOMAIN,
                format!("创建事件目录失败：{error}"),
            )
        })?;
        let path = self.today_path();
        let line = serde_json::to_string(event).map_err(|error| {
            HarnessError::invalid(
                crate::constants::ERROR_DOMAIN,
                format!("事件序列化失败：{error}"),
            )
        })?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| {
                HarnessError::storage(
                    crate::constants::ERROR_DOMAIN,
                    format!("打开事件文件失败：{error}"),
                )
            })?;
        writeln!(file, "{line}").map_err(|error| {
            HarnessError::storage(
                crate::constants::ERROR_DOMAIN,
                format!("写事件失败：{error}"),
            )
        })
    }

    fn today_path(&self) -> PathBuf {
        self.events_dir
            .join(format!("{}.jsonl", current_date_string()))
    }

    /// GC：删除超过保留天数的日切文件（fail-open，缺失目录跳过）。
    pub fn collect_garbage(&self, retention_days: u64) -> usize {
        let Ok(entries) = std::fs::read_dir(&self.events_dir) else {
            return 0;
        };
        let cutoff = current_date_days() - retention_days as i64;
        let mut removed = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path
                .file_name()
                .and_then(|n| name_with_day(&n.to_string_lossy()))
            else {
                continue;
            };
            if name < cutoff && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    /// 读取某日事件（评测用）。
    pub fn read_day(&self, day: &str) -> Result<Vec<RsiCycleEvent>, HarnessError> {
        let path = self.events_dir.join(format!("{day}.jsonl"));
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(HarnessError::storage(
                    crate::constants::ERROR_DOMAIN,
                    format!("读取事件失败：{error}"),
                ))
            }
        };
        let mut events = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str(line) {
                events.push(event);
            }
        }
        Ok(events)
    }
}

/// 从 `YYYY-MM-DD.jsonl` 解析天序（非日切文件返回 None）。
fn name_with_day(file_name: &str) -> Option<i64> {
    let stem = file_name.strip_suffix(".jsonl")?;
    let (year, rest) = stem.split_once('-')?;
    let (month, day) = rest.split_once('-')?;
    Some(day_number(
        year.parse().ok()?,
        month.parse().ok()?,
        day.parse().ok()?,
    ))
}

/// 当前日期字符串（UTC 日）。
fn current_date_string() -> String {
    let days = current_date_days();
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn current_date_days() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64)
        .unwrap_or(0)
}

fn day_number(year: i64, month: i64, day: i64) -> i64 {
    // Howard Hinnant days_from_civil
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_appends_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = RsiEventRecorder::new(dir.path());
        recorder.record(&RsiCycleEvent::CycleStart {
            cycle_id: "c1".into(),
            trigger: "manual".into(),
            model: "m".into(),
            dry_run: true,
            blocks: 2,
        });
        recorder.record(&RsiCycleEvent::CycleEnd {
            summary: "采纳 1".into(),
            duration_ms: 42,
        });
        let today = current_date_string();
        let events = recorder.read_day(&today).unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            RsiCycleEvent::CycleStart { dry_run: true, .. }
        ));
    }

    #[test]
    fn gc_removes_old_tailed_files() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = RsiEventRecorder::new(dir.path());
        // 写一个「很旧」的日切文件（1970-01-02）
        std::fs::write(dir.path().join("1970-01-02.jsonl"), "{}\n").unwrap();
        let today = current_date_string();
        std::fs::write(dir.path().join(format!("{today}.jsonl")), "{}\n").unwrap();
        let removed = recorder.collect_garbage(crate::constants::EVENT_RETENTION_DAYS);
        assert_eq!(removed, 1, "只删过期文件");
        assert!(!dir.path().join("1970-01-02.jsonl").exists());
        assert!(dir.path().join(format!("{today}.jsonl")).exists());
    }

    #[test]
    fn date_roundtrip() {
        let days = day_number(2026, 10, 24);
        assert_eq!(civil_from_days(days), (2026, 10, 24));
        assert!(name_with_day("2026-10-24.jsonl").is_some());
        assert!(name_with_day("notes.txt").is_none());
    }
}
