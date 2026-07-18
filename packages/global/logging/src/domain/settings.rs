use log::LevelFilter;
use std::path::{Path, PathBuf};

/// 日志输出目的地，由 Composition 在进程启动时决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoggingOutputMode {
    File,
    Stderr,
}

/// Logging 初始化所需的完整不可变静态设置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoggingSettings {
    filter_directive: String,
    max_level: LevelFilter,
    output_mode: LoggingOutputMode,
    logs_dir: PathBuf,
    max_bytes: u64,
    max_backups: usize,
    retention_days: u64,
}

impl LoggingSettings {
    pub fn new(
        filter_directive: String,
        output_mode: LoggingOutputMode,
        logs_dir: PathBuf,
        max_bytes: u64,
        max_backups: usize,
        retention_days: u64,
    ) -> Self {
        let (filter_directive, max_level) = normalize_filter(filter_directive);
        Self {
            filter_directive,
            max_level,
            output_mode,
            logs_dir,
            max_bytes,
            max_backups,
            retention_days,
        }
    }

    pub fn filter_directive(&self) -> &str {
        &self.filter_directive
    }

    pub fn max_level(&self) -> LevelFilter {
        self.max_level
    }

    pub fn output_mode(&self) -> LoggingOutputMode {
        self.output_mode
    }

    pub fn logs_dir(&self) -> &Path {
        &self.logs_dir
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    pub fn max_backups(&self) -> usize {
        self.max_backups
    }

    pub fn retention_days(&self) -> u64 {
        self.retention_days
    }
}

fn normalize_filter(directive: String) -> (String, LevelFilter) {
    let directive = directive.trim();
    if directive.is_empty() {
        return ("warn".to_string(), LevelFilter::Warn);
    }

    let filter_part = directive
        .split_once('/')
        .map_or(directive, |(filter, _)| filter);
    let mut max = LevelFilter::Off;
    for raw_segment in filter_part.split(',') {
        let segment = raw_segment.trim();
        if segment.is_empty() {
            continue;
        }
        let parsed = match segment.split_once('=') {
            Some((_, level)) => parse_level(level.trim()),
            None => parse_level(segment).or(Some(LevelFilter::Trace)),
        };
        let Some(level) = parsed else {
            return ("warn".to_string(), LevelFilter::Warn);
        };
        max = max.max(level);
    }

    (directive.to_string(), max)
}

fn parse_level(value: &str) -> Option<LevelFilter> {
    match value.to_ascii_lowercase().as_str() {
        "trace" => Some(LevelFilter::Trace),
        "debug" => Some(LevelFilter::Debug),
        "info" => Some(LevelFilter::Info),
        "warn" => Some(LevelFilter::Warn),
        "error" => Some(LevelFilter::Error),
        "off" => Some(LevelFilter::Off),
        _ => None,
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
