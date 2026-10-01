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
/// 截断阈值占 context window 的比例上限（1/20 = 5%）。
///
/// 定量阈值只对大窗口合理：128k 窗口下单条 50k chars（中文场景约
/// 50k tokens）即占 40%，会直接把启发式估算顶到 auto-compact 阈值。
pub(crate) const WINDOW_SCALED_THRESHOLD_RATIO_DIVISOR: usize = 20;

/// 窗口收紧后的阈值下限：过小的 preview 无法容纳有效的 head/tail 提示。
pub(crate) const MIN_WINDOW_SCALED_THRESHOLD_CHARS: usize = 4_000;
