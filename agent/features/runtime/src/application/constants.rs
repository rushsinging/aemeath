//! application 层行为常量（#1146 组3a 常量归位）。
//!
//! 各行为文件中语义稳定的 `const` 统一归位于此；值与语义不变。

use std::time::Duration;

// ─── activity/coordinator.rs ───

pub(crate) const MAX_RETAINED_TERMINAL_ACTIVITIES: usize = 64;

// ─── compact_generator.rs ───

/// Compact 摘要请求的最大输出 token（摘要可长，给足预算）。
///
/// 实际取值还受所选模型自身 `max_tokens` 上限约束（取两者较小值）。
pub(crate) const COMPACT_MAX_OUTPUT_TOKENS: u32 = 16_384;

/// 已配置 compact 模型但模型未声明输入窗口时的保守窗口。
///
/// 未知窗口 **MUST** fail closed：使用保守下限而不是注入窗口，避免向小窗口
/// 模型发出超窗口请求。
pub(crate) const COMPACT_UNKNOWN_MODEL_WINDOW: usize = 32_000;

// ─── hook/stop_coordination.rs ───

pub(crate) const INLINE_HOOK_OUTPUT_LIMIT: usize = 4_000;
pub(crate) const TUI_STDOUT_PREVIEW_LINES: usize = 3;
pub(crate) const TUI_STDERR_PREVIEW_LINES: usize = 5;

// ─── loop_engine/chat/config_reload.rs ───

pub(crate) const WATCH_DEPTH: u32 = 5;

// ─── model/invocation.rs ───

/// One initial invocation plus at most ten retries.
pub(crate) const DEFAULT_MAX_ATTEMPTS: u32 = 11;
pub(crate) const INITIAL_BACKOFF: Duration = Duration::from_secs(10);
pub(crate) const MAX_BACKOFF: Duration = Duration::from_secs(120);

// ─── prompt/build/prompt_build.rs ───

pub(crate) const INSTRUCTION_SEARCH_DEPTH: u32 = 5;

// ─── run/context.rs ───

/// EMA 平滑系数（#1626）：新观测占 0.3，历史占 0.7。
pub(crate) const CALIBRATION_EMA_ALPHA: f64 = 0.3;
/// 单次观测与滑动系数的 clamp 区间（#1626）：区间外视为异常，截断或丢弃。
pub(crate) const CALIBRATION_CLAMP: std::ops::RangeInclusive<f64> = 0.5..=2.0;

// ─── tool/execution_supervisor.rs ───

pub(crate) const DEFAULT_GRACE: Duration = Duration::from_millis(250);

// ─── tool/tool_result_materializer.rs ───

pub(crate) const COMPLETED_MATERIALIZATION_CAPACITY: usize = 256;
