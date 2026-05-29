//! Spinner 动画态（frame + verb）。属 view_state：易变渲染态，非业务真相。
//!
//! 业务真相（active + phase）在 `model/runtime/spinner.rs`。这里只承载每 90ms
//! SpinnerTick 推进的 `frame`，以及 spinner 由 inactive→active 时一次性随机选定
//! 的 `verb`。`elapsed` 由 `frame * 90ms` 推算，无需 `Instant`（见 spec 真相边界）。

use rand::prelude::IndexedRandom;

/// 装饰性动词池。verb 选定移入 view_state 后，此处为该池的唯一真相来源
/// （Task 4.1 已删除原 `render/output_area/spinner.rs::SPINNER_VERBS`）。
const SPINNER_VERBS: &[&str] = &[
    "Thinking",
    "Pondering",
    "Crafting",
    "Computing",
    "Brewing",
    "Weaving",
    "Conjuring",
    "Forging",
    "Hatching",
    "Cooking",
    "Channeling",
    "Ruminating",
    "Composing",
    "Imagining",
    "Processing",
    "Puzzling",
    "Mulling",
    "Noodling",
    "Tinkering",
    "Crystallizing",
    "Synthesizing",
    "Architecting",
    "Orchestrating",
    "Incubating",
    "Fermenting",
    "Simmering",
    "Percolating",
    "Cogitating",
    "Meandering",
    "Harmonizing",
];

const DEFAULT_VERB: &str = "Thinking";

/// Spinner 动画易变态。`verb` 在 active 期间稳定，仅在 `pick_verb` 调用时重选。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpinnerAnim {
    /// 动画帧计数器，只能由固定 ticker 推进。
    pub frame: u64,
    /// 当前动词文本（active 期间稳定）。
    pub verb: String,
}

impl SpinnerAnim {
    /// 推进一帧（饱和递增，wrapping 行为与渲染层 `tick_spinner` 对齐）。
    pub fn advance(&mut self) {
        self.frame = self.frame.wrapping_add(1);
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_anim_default_is_empty() {
        let anim = SpinnerAnim::default();
        assert_eq!(anim.frame, 0);
        assert_eq!(anim.verb, "");
    }

    #[test]
    fn test_spinner_anim_advance_increments_frame() {
        let mut anim = SpinnerAnim::default();
        anim.advance();
        anim.advance();
        assert_eq!(anim.frame, 2);
    }

    #[test]
    fn test_spinner_anim_advance_wraps_at_max() {
        let mut anim = SpinnerAnim {
            frame: u64::MAX,
            verb: String::new(),
        };
        anim.advance();
        assert_eq!(anim.frame, 0);
    }

    #[test]
    fn test_spinner_anim_pick_verb_selects_from_pool_and_is_stable() {
        let mut anim = SpinnerAnim {
            frame: 42,
            verb: String::new(),
        };
        anim.pick_verb();
        let chosen = anim.verb.clone();
        assert!(SPINNER_VERBS.contains(&chosen.as_str()));
        // pick_verb 复位 frame
        assert_eq!(anim.frame, 0);
        // 不再调用 pick_verb，verb 保持稳定
        anim.advance();
        assert_eq!(anim.verb, chosen);
    }
}
