#![deny(clippy::print_stdout, clippy::print_stderr)]

//! Workflow 支撑域。
//!
//! v0.1.0 仅提供 Reasoning Graph / effort 调节能力；完整 Workflow 能力属于 v0.2.0。

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:workflow";

mod domain;

/// 跨 BC 公开 API。
pub mod api {
    pub use crate::domain::reasoning_graph::{ReasoningNode, ReasoningSignal};
    pub use crate::domain::reasoning_port::{ReasoningObservation, ReasoningPort};
}

/// Composition-only wiring。
pub fn adaptive_reasoning(
    config: share::config::ReasoningGraphConfig,
    initial: share::reasoning::ReasoningLevel,
) -> std::sync::Arc<dyn api::ReasoningPort> {
    std::sync::Arc::new(domain::reasoning_port::AdaptiveReasoningPort::new(
        domain::reasoning_graph::GraphRuntimeConfig::from_shared(&config),
        initial,
    ))
}
