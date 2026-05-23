//! 自动压缩断路器
//!
//! 跟踪连续压缩失败次数，超过阈值后停止尝试。

/// 连续自动压缩失败次数上限，超过后断路器跳闸。
pub const MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES: u8 = 3;

/// 跟踪会话内跨轮次的自动压缩状态。
#[derive(Debug, Clone, Default)]
pub struct AutoCompactState {
    /// 本次会话中已执行的压缩次数。
    pub compaction_count: u32,
    /// 连续失败次数。成功后重置。
    pub consecutive_failures: u8,
    /// 断路器是否已跳闸（跳闸后不再重试）。
    pub circuit_broken: bool,
}

impl AutoCompactState {
    /// 记录一次成功压缩 — 重置失败计数器。
    pub fn record_success(&mut self) {
        self.compaction_count += 1;
        self.consecutive_failures = 0;
        self.circuit_broken = false;
    }

    /// 记录一次失败压缩 — 递增失败计数器，
    /// 达到 `MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES` 后跳闸。
    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES {
            self.circuit_broken = true;
            log::warn!(
                "[autocompact] circuit breaker tripped after {} consecutive failures — skipping future attempts",
                self.consecutive_failures
            );
        }
    }

    /// 是否应尝试自动压缩。
    pub fn should_attempt(&self) -> bool {
        !self.circuit_broken
    }
}
