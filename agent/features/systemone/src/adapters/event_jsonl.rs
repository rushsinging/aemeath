//! 日切 jsonl 评分事件 segment store：append + legacy 审计迁移 + 保留 GC。
//!
//! 落盘形态（设计 `docs/design/02-modules/systemone/03-event-stream.md` §4）：
//! - 事件按 **UTC 日切** 追加到 `{scoring_dir}/events/{yyyy-mm-dd}.jsonl`，一行一条
//!   完整 `ScoringEvent` JSON；NEVER 改写已写入行；
//! - legacy `{scoring_dir}/audit.jsonl` 一次性旁路归档到 `events/legacy-audit.jsonl`
//!   后改名 `audit.jsonl.migrated`（NEVER 再向原路径追加，NEVER 丢弃旧行）；
//! - 保留 GC 按文件名日期删除过期 segment，0 = 禁用；失败只记运行日志，不阻断。
//!
//! IO 只存在于本 adapter（domain-no-direct-io）；调用方（Task3 audited）负责
//! append 失败的 fail-open 语义。

use std::path::PathBuf;

use chrono::{Duration, NaiveDate, Utc};

use crate::constants::{EVENTS_DIR_NAME, LEGACY_AUDIT_ARCHIVE_FILE, LEGACY_AUDIT_FILE};
use crate::domain::ScoringEvent;

use super::calibration_store::append_jsonl_line_sync;

/// 日切评分事件 segment store：`{scoring_dir}/events/{yyyy-mm-dd}.jsonl` 追加写。
#[derive(Debug)]
pub struct JsonlSegmentScoringEventStore {
    /// events/ 的父目录（scoring_dir）。
    scoring_dir: PathBuf,
    /// segment 保留天数；0 = 禁用 GC（绝不等于关闭事件写入）。
    retention_days: u32,
}

impl JsonlSegmentScoringEventStore {
    /// `scoring_dir` 为 events/ 的父目录；`retention_days` 0 = 禁用 GC。
    pub fn new(scoring_dir: PathBuf, retention_days: u32) -> Self {
        Self {
            scoring_dir,
            retention_days,
        }
    }

    /// append 一行事件到 `{scoring_dir}/events/{yyyy-mm-dd}.jsonl`。
    ///
    /// 文件名日期取 `event.timestamp` 的 **UTC** 日切（设计 §4.1）；timestamp
    /// 解析失败返回 `Err`，NEVER 静默丢弃。同步落盘：调用方（Task3 audited）
    /// 负责 fail-open。
    pub fn append(&self, event: &ScoringEvent) -> std::io::Result<()> {
        let segment_date = segment_date_of(&event.timestamp)?;
        let mut line = serde_json::to_string(event)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        line.push('\n');

        let events_dir = self.scoring_dir.join(EVENTS_DIR_NAME);
        std::fs::create_dir_all(&events_dir)?;
        let segment_path = events_dir.join(format!("{segment_date}.jsonl"));
        append_jsonl_line_sync(&segment_path, &line)
    }

    /// 一次性迁移：legacy `{scoring_dir}/audit.jsonl` 存在时逐行原样并入
    /// `{scoring_dir}/events/legacy-audit.jsonl`（append，不解析改写，无法解析的
    /// 行也原样抄入），成功后把原文件改名为 `audit.jsonl.migrated`。
    ///
    /// 文件不存在 → `Ok(())` 空操作；迁移 IO 失败 → 返回 `Err` 且原文件保持不动。
    pub fn migrate_legacy_audit(&self) -> std::io::Result<()> {
        let legacy_path = self.scoring_dir.join(LEGACY_AUDIT_FILE);
        let legacy_source = match std::fs::read_to_string(&legacy_path) {
            Ok(source) => source,
            // 仅「不存在」是合法空操作；其余读失败 MUST 冒泡，NEVER 吞错。
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };

        let events_dir = self.scoring_dir.join(EVENTS_DIR_NAME);
        std::fs::create_dir_all(&events_dir)?;
        let archive_path = events_dir.join(LEGACY_AUDIT_ARCHIVE_FILE);
        for legacy_line in legacy_source.lines() {
            append_jsonl_line_sync(&archive_path, &format!("{legacy_line}\n"))?;
        }

        // 归档全部成功后才改名原文件：失败时 legacy 原样保留，可安全重试。
        let migrated_path = self
            .scoring_dir
            .join(format!("{}.migrated", LEGACY_AUDIT_FILE));
        std::fs::rename(&legacy_path, &migrated_path)
    }

    /// 保留 GC：删除 `events/` 下文件名日期早于 `today - retention_days` 的
    /// `*.jsonl`；`retention_days = 0` 直接返回（禁用）。文件名日期无法解析的
    /// 跳过不删；GC 失败仅记 `log::warn!`（target 显式，无正文），不阻断。
    ///
    /// `today` 为注入的 "yyyy-mm-dd"（便于测试；生产路径在 Task3 接入时取当前日期）。
    pub fn retain_segments(&self, today: &str) {
        if self.retention_days == 0 {
            return;
        }
        let Some(today_date) = NaiveDate::parse_from_str(today, "%Y-%m-%d").ok() else {
            log::warn!(
                target: crate::LOG_TARGET,
                "scoring_event_gc_skipped reason=invalid_today"
            );
            return;
        };
        let cutoff_date = today_date - Duration::days(i64::from(self.retention_days));

        let events_dir = self.scoring_dir.join(EVENTS_DIR_NAME);
        let Ok(entries) = std::fs::read_dir(&events_dir) else {
            return;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "scoring_event_gc_failed reason=read_dir"
                );
                return;
            };
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            if !file_name.ends_with(".jsonl") {
                continue;
            }
            let Some(stem) = file_name.strip_suffix(".jsonl") else {
                continue;
            };
            let Ok(segment_date) = NaiveDate::parse_from_str(stem, "%Y-%m-%d") else {
                // 无法解析日期的文件名（legacy 归档等）：跳过不删。
                continue;
            };
            if segment_date >= cutoff_date {
                continue;
            }
            if let Err(error) = std::fs::remove_file(entry.path()) {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "scoring_event_gc_failed error={error}"
                );
            }
        }
    }
}

/// RFC3339 时间串 → UTC 日切 "yyyy-mm-dd"（append 的目标文件名日期）。
/// 解析失败返回 `InvalidData`，NEVER 静默兜底到别的日期。
fn segment_date_of(timestamp: &str) -> std::io::Result<String> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|instant| instant.with_timezone(&Utc).format("%Y-%m-%d").to_string())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
#[path = "event_jsonl_tests.rs"]
mod tests;
