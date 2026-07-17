//! ReasoningGraph v0.1.0 固定运行时策略。
//!
//! Config reasoning 退役后，Graph 始终运行，effort 直接取各节点
//! `default_effort()`，不再有 enabled / max / override 等可配置概念。
//! 本模块保留作为历史说明，不再导出任何运行时配置类型。
