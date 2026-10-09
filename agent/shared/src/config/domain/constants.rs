//! Config domain 层共享常量（#1146 归位：自 audit / context / snapshot /
//! models/runtime / ui 行为文件抽出，各文件经 re-export/use 保持原引用路径）。

// ── audit.rs：usage 队列默认值 ──
pub const DEFAULT_USAGE_QUEUE_CAPACITY: usize = 1024;
pub const DEFAULT_USAGE_SHUTDOWN_TIMEOUT_MS: u64 = 5_000;

// ── context.rs：auto-compact 阈值安全区间 ──
/// `auto_compact_threshold_ratio` 的安全区间下限。
///
/// 低于该值时阈值过小，任意一轮对话即可能恒触发 auto-compact，
/// 形成 compact 风暴直至熔断（#1626 教训）。
pub const AUTO_COMPACT_THRESHOLD_RATIO_MIN: f64 = 0.5;

/// `auto_compact_threshold_ratio` 的安全区间上限。
///
/// 高于该值时估算误差缓冲过薄——compact 请求自身占用的上下文可能
/// 超出窗口，触发后已无空间完成压缩。
pub const AUTO_COMPACT_THRESHOLD_RATIO_MAX: f64 = 0.95;

// ── models/runtime.rs：max_tokens 默认值 ──
pub const DEFAULT_MAX_TOKENS: u32 = 8192;

// ── snapshot.rs：hook 执行 / stop block 默认值 ──
pub(crate) const DEFAULT_HOOK_EXECUTION_MAX_ATTEMPTS: u8 = 3;
pub(crate) const DEFAULT_STOP_HOOK_MAX_BLOCKS: usize = 15;

// ── ui.rs：markdown 间距行数上限 ──
pub(crate) const MAX_MARKDOWN_SPACING_LINES: u8 = 8;

// ─── snapshot.rs ───
/// 截断阈值占 context window 的比例上限（1/200 = 0.5%）。
///
/// 大 tool_result 是 context 膨胀的主要来源（实测占内容池 71.5%），
/// 单条上限过大会把启发式估算顶到 auto-compact 阈值：1M 窗口下
/// 5% 即 50k chars 单条占用，300k 窗口下 15k chars 同理。
pub(crate) const WINDOW_SCALED_THRESHOLD_RATIO_DIVISOR: usize = 200;

/// 窗口收紧后的阈值下限：不得低于落盘占位符开销（head + tail + 元数据），
/// 否则落盘比保留原文更占 context（负收益）。取值同时决定「结果完整进
/// context」的比例：3k 时实测约 78% 的单条结果不触发落盘。
pub(crate) const MIN_WINDOW_SCALED_THRESHOLD_CHARS: usize = 3_000;
