//! 纯值常量（#1146 placement 归位）。

use std::time::Duration;

pub(crate) const MAX_RUNTIME_EVENTS_PER_FRAME: usize = 256;

/// 临时 status notice 存活时长。

/// 临时 status notice 存活时长。
pub(crate) const TRANSIENT_NOTICE_TTL: Duration = Duration::from_secs(5);
