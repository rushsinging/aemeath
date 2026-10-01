//! 纯值常量（#1146 placement 归位）。

use std::time::Duration;

pub(crate) const RSS_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

pub(crate) const SLOW_FRAME_LOG_COOLDOWN: Duration = Duration::from_secs(5);

pub(crate) const SLOW_FRAME_THRESHOLD: Duration = Duration::from_millis(50);
