//! Workflow crate 常量（归位约定：模块级纯值常量集中于此）。
/// 本 crate 日志 target；路由登记见 `packages/global/logging` routing_guard。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:workflow";

/// 自动启动初始 Active Node（plan / Planner；#2091 KEV 自动启动骨架）。
pub const AUTOSTART_PLAN_NODE_ID: &str = "plan";
/// 自动启动初始 CompleteNode id。
pub const AUTOSTART_COMPLETE_NODE_ID: &str = "done";
