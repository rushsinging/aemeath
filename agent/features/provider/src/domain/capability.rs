//! Provider driver capability.

use serde::{Deserialize, Serialize};
pub use share::reasoning::ReasoningLevel;

use crate::published_language::{ReasoningCapabilityData, ReasoningMappingKindData};

/// 由客户端上报的最大推理档位构造推理能力：`Off..=max` 全部支持。
///
/// 组合根装配 capability 时的唯一阶梯推导（自 composition 收编）；
/// mapping 暂固定 Effort，随 ReasoningMappingKindData 消费化处置调整。
pub fn reasoning_capability_from_max(max: ReasoningLevel) -> ReasoningCapabilityData {
    let all_levels = [
        ReasoningLevel::Off,
        ReasoningLevel::Minimal,
        ReasoningLevel::Low,
        ReasoningLevel::Medium,
        ReasoningLevel::High,
        ReasoningLevel::Xhigh,
        ReasoningLevel::Max,
    ];
    let supported: Vec<_> = all_levels
        .into_iter()
        .filter(|level| *level <= max)
        .collect();
    ReasoningCapabilityData::new(supported, ReasoningMappingKindData::Effort)
        .unwrap_or_else(|_| ReasoningCapabilityData::none())
}

/// Provider driver 身份词表——唯一真相源在
/// `share::config::domain::driver_kind::DriverKind`（#1861 C8 收敛）。
pub use share::config::domain::driver_kind::DriverKind as ProviderDriverKind;

#[cfg(test)]
#[path = "capability_tests.rs"]
mod tests;
