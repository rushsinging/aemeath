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

/// must_keep 默认最低出现次数（#2133）：只出现一次的路径多为一次性噪声。
pub(crate) const MIN_MUST_KEEP_FREQUENCY: usize = 3;

/// 评测脚本自身的高频路径，不代表上下文事实。
pub(crate) const NOISE_PREFIXES: [&str; 3] = ["eval/compact/", "results/", "datasets/"];

/// 压缩后重问判定：只观察最近 N 个 user turn（趋势口径，见 issue #2133）。
pub(crate) const REQUERY_WINDOW_TURNS: usize = 5;

/// 压缩后重问判定：只比较实质请求，短指令（继续/好）不参与。
pub(crate) const REQUERY_MIN_CHARS: usize = 8;

/// 压缩后重问判定：字符 bigram Jaccard 阈值。
pub(crate) const REQUERY_SIMILARITY_THRESHOLD: f64 = 0.8;

/// 显式不满信号关键词（弱信号，只做趋势）。
pub(crate) const DISSATISFACTION_MARKERS: [&str; 6] = [
    "前面说过",
    "不是说了",
    "刚才说",
    "已经说过",
    "重复一遍",
    "我不是说",
];

/// 系统注入的提醒块前缀：不是用户输入，不参与重问与不满判定。
pub(crate) const INJECTION_PREFIXES: [&str; 4] = [
    "<system-reminder",
    "<hook-feedback",
    "<reminder",
    "<task-progress",
];

/// 续接问答（P3）：出题者可见的 early 历史预算（估算 token）。
/// 对齐真实大 session（生产压缩发生在数十万 token 规模；1M 窗口模型
/// 输入 800K + 输出 16K 安全）。截断只防超窗，不做小样本简化。
pub(crate) const QUIZ_SOURCE_TOKEN_BUDGET: usize = 800_000;

/// 续接问答（P3）：答题者可见的 summary + tail 预算（估算 token）。
pub(crate) const ANSWER_SOURCE_TOKEN_BUDGET: usize = 200_000;

/// 续接问答（P3）：题数按 early 规模浮动——每个该 token 数出 1 题
/// （814K early → 20 题）。
pub(crate) const QUIZ_TOKENS_PER_QUESTION: usize = 40_000;

/// 续接问答（P3）：题数下限。
pub(crate) const QUIZ_MIN_QUESTIONS: usize = 10;

/// 续接问答（P3）：题数上限。
pub(crate) const QUIZ_MAX_QUESTIONS: usize = 30;

/// 读取类工具：路径集合差分用于衡量「压缩后重复读取」。
pub(crate) const TOOL_READ_NAMES: [&str; 3] = ["Read", "Grep", "Glob"];
