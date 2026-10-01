//! Context domain 层共享生产常量（#1146 双轨归位）。

pub(crate) const HEURISTIC_CALIBRATION_RANGE: std::ops::RangeInclusive<f64> = 0.5..=2.0;
/// fallback/护栏中 previous_summary 允许嵌入的最大字符数（#1486）。
///
/// 多次 compact 时 previous_summary 若被全文 verbatim 嵌入会线性累加，
/// 最终撑爆 system prompt（真实事故：92 万字符 summary）。超过此上限时
/// 只保留 previous_summary 的关键尾部（最新状态），头部信息允许丢弃。
/// 定义在 domain 层，供 adapter（compact_summary）与 application
/// （active_summary 注入护栏）共同引用，避免 COLA 分层越界。
/// fallback/护栏中 previous_summary 允许嵌入的最大字符数（#1486）。
///
/// 多次 compact 时 previous_summary 若被全文 verbatim 嵌入会线性累加，
/// 最终撑爆 system prompt（真实事故：92 万字符 summary）。超过此上限时
/// 只保留 previous_summary 的关键尾部（最新状态），头部信息允许丢弃。
/// 定义在 domain 层，供 adapter（compact_summary）与 application
/// （active_summary 注入护栏）共同引用，避免 COLA 分层越界。
pub const FALLBACK_PREVIOUS_SUMMARY_CAP: usize = 20_000;

/// Compact 保留 tail（recent messages）的 token 预算封顶占窗口的百分数（#1773）。
///
/// 3% 刻意不用整数除法表达：`context_size / 33` 这类魔法除数会在 33 与 34
/// 之间反复横跳（33.3% 与 2.94% 混用），且调整比例时无法从代码看出意图。
/// Compact 保留 tail（recent messages）的 token 预算封顶占窗口的百分数（#1773）。
///
/// 3% 刻意不用整数除法表达：`context_size / 33` 这类魔法除数会在 33 与 34
/// 之间反复横跳（33.3% 与 2.94% 混用），且调整比例时无法从代码看出意图。
pub const COMPACT_TAIL_WINDOW_PERCENT: usize = 3;

/// clamp 后 effective window 仍低于此值时判定窗口配置错误（#1626）。
///
/// 低于该值的可用窗口连 system prompt 都无法稳定容纳，auto-compact
/// 只会风暴；此时应禁用 auto-compact 并告警（见 `MisconfiguredWindow`）。
/// clamp 后 effective window 仍低于此值时判定窗口配置错误（#1626）。
///
/// 低于该值的可用窗口连 system prompt 都无法稳定容纳，auto-compact
/// 只会风暴；此时应禁用 auto-compact 并告警（见 `MisconfiguredWindow`）。
pub const MIN_EFFECTIVE_WINDOW: usize = 1_024;

/// max_output 预留占窗口的比例上限（#1626）。
///
/// 未设护栏时 `max_output >= 窗口×98%`（如 8k 窗口 + 默认 8192 output）
/// max_output 预留占窗口的比例上限（#1626）。
///
/// 未设护栏时 `max_output >= 窗口×98%`（如 8k 窗口 + 默认 8192 output）
/// 会让 effective 归零、threshold 归零，任意一轮对话即恒触发
/// auto-compact，形成 compact 风暴直至熔断。预留 clamp 到窗口 25% 后
/// threshold 永不为 0；大窗口常规配置（如 200k 窗口 + 16k output）不受影响。
pub const MAX_OUTPUT_WINDOW_RATIO_CAP: usize = 4;
