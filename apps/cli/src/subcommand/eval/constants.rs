//! `aemeath eval compact` 的纯值常量（#1146 placement 归位）。
//!
//! 日志格式正则与噪声前缀集中在此：格式变更时只改这一处。

use once_cell::sync::Lazy;
use regex::Regex;

pub(crate) static START_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\[compact\] 开始：messages=(\d+) early=(\d+) early_tokens=(\d+) previous_summary=(\d+) chars mode=(\w+)(?: model=(\S+))?",
    )
    .expect("start regex")
});

pub(crate) static MAP_REDUCE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\[compact\] map-reduce：(\d+) chunks.*并发上限 (\d+)(?:，波次 (\d+))?(?: model=(\S+))?",
    )
    .expect("map-reduce regex")
});

pub(crate) static MAP_DONE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\[compact\] map 完成：total_chunks=(\d+) successful_chunks=(\d+) degraded_chunks=(\d+)(?: waves=(\d+))?(?: chunk_ms_p50=(\d+) chunk_ms_p95=(\d+) chunk_ms_max=(\d+))?(?: elapsed_ms=(\d+))?",
    )
    .expect("map-done regex")
});

pub(crate) static REDUCE_DONE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\[compact\] reduce 合并完成：\d+ chars（预算 \d+ tokens）(?:facts=(\d+))?(?: elapsed_ms=(\d+))?",
    )
    .expect("reduce regex")
});

pub(crate) static REFRESH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\[compact\] 汇总超预算，再压 round (\d+)：(\d+) -> (\d+) tokens.*elapsed_ms=(\d+)")
        .expect("refresh regex")
});

pub(crate) static DONE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\[compact\] 完成：summary=(\d+) chars recent_messages=(\d+)(?: quality=(\S+))?(?: model=(\S+))?(?: elapsed_ms=(\d+))?",
    )
    .expect("done regex")
});

pub(crate) static ID_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(?:run|stp|mem|cht|bgp|irq|ws|sess)_[A-Za-z0-9]{6,}\b").expect("id regex")
});

pub(crate) static PATH_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?:[\w.@-]+/)*[\w.@-]+\.(?:rs|py|md|json|jsonl|toml|yaml|yml|sh|ts|tsx|js|html|css)(?::\d+)?",
    )
    .expect("path regex")
});

/// 评测脚本自身的高频路径，不代表上下文事实。
pub(crate) const NOISE_PREFIXES: [&str; 3] = ["eval/compact/", "results/", "datasets/"];
