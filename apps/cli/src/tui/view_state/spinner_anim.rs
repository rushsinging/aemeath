//! Spinner 纯动画态（frame + verb）。属 view_state：易变渲染态，非业务真相。
//!
//! Run 生命周期与活动文案均由 typed Run snapshot 派生；这里只承载每 90ms
//! SpinnerTick 推进的 frame 与活动区间内稳定的随机 verb。

use super::constants::{DEFAULT_VERB, SPINNER_VERBS};
use rand::prelude::IndexedRandom;

/// Spinner 动画易变态。`verb` 在 active 期间稳定，仅在 `pick_verb` 调用时重选。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpinnerAnim {
    /// 动画帧计数器，只能由固定 ticker 推进。
    pub frame: u64,
    /// 当前活动显示区间的动画帧计数器。
    pub phase_frame: u64,
    /// 当前动词文本（active 期间稳定）。
    pub verb: String,
}

impl SpinnerAnim {
    /// 推进一帧（饱和递增，wrapping 行为与渲染层 `tick_spinner` 对齐）。
    pub fn advance(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.phase_frame = self.phase_frame.wrapping_add(1);
    }

    /// 随机选定一个 verb（effectful：用 rng，故归 view_state 更新边界）。
    /// 选一次后稳定，直到下次显式调用。同时把 frame 复位到 0。
    pub fn pick_verb(&mut self) {
        let mut rng = rand::rng();
        self.verb = SPINNER_VERBS
            .choose(&mut rng)
            .unwrap_or(&DEFAULT_VERB)
            .to_string();
        self.frame = 0;
        self.phase_frame = 0;
    }
}

#[cfg(test)]
#[path = "spinner_anim_tests.rs"]
mod tests;
