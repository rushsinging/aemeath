//! 纯值常量（#1146 placement 归位）。

use std::time::Duration;

pub(crate) const MAX_RUNTIME_EVENTS_PER_FRAME: usize = 256;

/// 临时 status notice 存活时长。
/// 临时 status notice 存活时长。
pub(crate) const TRANSIENT_NOTICE_TTL: Duration = Duration::from_secs(5);

/// 本地二进制文件身份检测的最小间隔（挂 SpinnerTick 轮询，避免每帧 stat）。
pub(crate) const LOCAL_BIN_CHECK_INTERVAL: Duration = Duration::from_secs(3);
