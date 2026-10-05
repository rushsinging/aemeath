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

// ─── loop_engine/chat/stall.rs ───

pub(crate) const FINGERPRINT_WINDOW: usize = 4;
pub(crate) const FINGERPRINT_MAX_REPEAT: usize = 3;

// ─── loop_engine/chat/input_gate.rs ───

/// `/reflect-now` 在 busy 期间被丢弃时的统一提示文案（裁决 3：busy NEVER 排队）。
/// gate busy 分支与 run_launch pending 消费丢弃点（`drop_queued_reflect_now`）
/// 共用本常量，NEVER 复制第二份。
pub(crate) const REFLECT_NOW_BUSY_DROP_NOTICE: &str =
    "Reflection 已在运行或等待运行结束，已跳过本次手动触发；稍后再试。";

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

// ─── loop_engine/chat/reminder_sources.rs ───

/// TaskProgress 周期重注入间隔（step 数）。`run_started` 以 step=0 推进，
/// 提供首次注入；此后每达间隔现场重建（对抗注意力衰减）。
/// specs/3.4 修订前先以常量落地，可配置化随 config 演进。
pub(crate) const TASK_PROGRESS_REFRESH_INTERVAL_STEPS: u32 = 8;

/// 记忆召回：词法召回候选数（重排在其 top-N 内进行）。
pub(crate) const MEMORY_RECALL_RECALL_LIMIT: usize = 20;

/// 记忆召回：注入条数上限。
pub(crate) const MEMORY_RECALL_TOP_K: usize = 3;

/// 记忆召回：注入内容的字符预算（超预算依次减条，宁可不注入不超预算）。
pub(crate) const MEMORY_RECALL_BUDGET_CHARS: usize = 1_200;

/// 记忆召回：单条内容预览字符上限。
pub(crate) const MEMORY_RECALL_PREVIEW_CHARS: usize = 200;

/// 记忆召回：相关性阈值门——top1 评分概率低于此值时本 turn 不注入
///（防噪音稀释上下文）。
pub(crate) const MEMORY_RECALL_THRESHOLD: f64 = 0.5;
