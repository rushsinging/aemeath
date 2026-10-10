//! Connect 状态机策略常量（#1146 归位：自 service.rs / states.rs 抽出）。

use std::time::Duration;

/// Probe 调用注入的合法超时上限。该值是 Connect 服务的策略常量，不在
/// 客户端控制范围内，避免各入口漂移。
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
