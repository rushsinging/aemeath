//! Provider driver capability.

pub use share::reasoning::ReasoningLevel;

/// 档位阶梯归一化：升序 + 去重（resolve/maximum 依赖升序不变量）。
pub(crate) fn normalize_levels(
    levels: impl IntoIterator<Item = ReasoningLevel>,
) -> Vec<ReasoningLevel> {
    let mut supported: Vec<_> = levels.into_iter().collect();
    supported.sort_unstable();
    supported.dedup();
    supported
}

/// resolve 内核：支持阶梯中 ≤requested 的最大档，Off 兜底。
///
/// `ModelInfo::resolve_reasoning` 与 openai driver 的共享实现。
pub(crate) fn resolve_supported(
    supported: &[ReasoningLevel],
    requested: ReasoningLevel,
) -> ReasoningLevel {
    supported
        .iter()
        .rev()
        .copied()
        .find(|level| *level <= requested)
        .unwrap_or(ReasoningLevel::Off)
}

/// 阶梯最高档；空阶梯以 Off 兜底。
pub(crate) fn maximum_supported(supported: &[ReasoningLevel]) -> ReasoningLevel {
    supported.last().copied().unwrap_or(ReasoningLevel::Off)
}

/// 由客户端上报的最大推理档位构造支持阶梯：`Off..=max` 全部支持。
///
/// 组合根装配 capability 时的唯一阶梯推导（自 composition 收编）；
pub(crate) fn supported_reasoning_from_max(max: ReasoningLevel) -> Vec<ReasoningLevel> {
    let all_levels = [
        ReasoningLevel::Off,
        ReasoningLevel::Minimal,
        ReasoningLevel::Low,
        ReasoningLevel::Medium,
        ReasoningLevel::High,
        ReasoningLevel::Xhigh,
        ReasoningLevel::Max,
    ];
    all_levels
        .into_iter()
        .filter(|level| *level <= max)
        .collect()
}

/// Provider driver 身份词表——唯一真相源在
/// `share::config::domain::driver_kind::DriverKind`（#1861 C8 收敛）。
pub use share::config::domain::driver_kind::DriverKind as ProviderDriverKind;

#[cfg(test)]
#[path = "capability_tests.rs"]
mod tests;
