//! Spinner 动画态（frame + verb）。属 view_state：易变渲染态，非业务真相。
//!
//! 业务真相（active + phase）在 `model/runtime/spinner.rs`。这里只承载每 90ms
//! SpinnerTick 推进的 `frame`，以及 spinner 由 inactive→active 时一次性随机选定
//! 的 `verb`。`elapsed` 由 `frame * 90ms` 推算，无需 `Instant`（见 spec 真相边界）。

use crate::tui::model::runtime::spinner::SpinnerPhase;
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
    /// 当前 phase 的动画帧计数器；phase 切换时重置，总 frame 不重置。
    pub phase_frame: u64,
    /// 已同步到 view_state 的 phase，用于检测 phase 切换。
    pub phase: Option<SpinnerPhase>,
    /// 当前动词文本（active 期间稳定）。
    pub verb: String,
}

impl SpinnerAnim {
    /// 推进一帧（饱和递增，wrapping 行为与渲染层 `tick_spinner` 对齐）。
    pub fn advance(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.phase_frame = self.phase_frame.wrapping_add(1);
    }

    /// 根据固定 90ms ticker 估算已运行秒数。
    pub fn elapsed_secs(&self) -> u64 {
        self.frame.saturating_mul(90) / 1000
    }

    /// 根据固定 90ms ticker 估算当前 phase 已运行秒数。
    pub fn phase_elapsed_secs(&self) -> u64 {
        self.phase_frame.saturating_mul(90) / 1000
    }

    /// 同步业务 phase；显示语义变化时只重置 phase 计时，不重置总计时和 verb。
    pub fn sync_phase(&mut self, phase: Option<SpinnerPhase>) {
        if !same_phase(self.phase.as_ref(), phase.as_ref()) {
            self.phase_frame = 0;
        }
        self.phase = phase;
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

fn same_phase(current: Option<&SpinnerPhase>, next: Option<&SpinnerPhase>) -> bool {
    match (current, next) {
        (None, None) => true,
        (Some(SpinnerPhase::Thinking), Some(SpinnerPhase::Thinking)) => true,
        (Some(SpinnerPhase::Generating), Some(SpinnerPhase::Generating)) => true,
        (Some(SpinnerPhase::AgentWorking), Some(SpinnerPhase::AgentWorking)) => true,
        (Some(SpinnerPhase::Reflecting), Some(SpinnerPhase::Reflecting)) => true,
        (Some(SpinnerPhase::CallingTool(current)), Some(SpinnerPhase::CallingTool(next))) => {
            current == next
        }
        (Some(SpinnerPhase::CallingTools { .. }), Some(SpinnerPhase::CallingTools { .. })) => true,
        (Some(SpinnerPhase::Hook { .. }), Some(SpinnerPhase::Hook { .. })) => current == next,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_anim_default_is_empty() {
        let anim = SpinnerAnim::default();
        assert_eq!(anim.frame, 0);
        assert_eq!(anim.phase_frame, 0);
        assert_eq!(anim.phase, None);
        assert_eq!(anim.verb, "");
    }

    #[test]
    fn test_spinner_anim_advance_increments_frame() {
        let mut anim = SpinnerAnim::default();
        anim.advance();
        anim.advance();
        assert_eq!(anim.frame, 2);
        assert_eq!(anim.phase_frame, 2);
    }

    #[test]
    fn test_spinner_anim_advance_wraps_at_max() {
        let mut anim = SpinnerAnim {
            frame: u64::MAX,
            phase_frame: u64::MAX,
            phase: None,
            verb: String::new(),
        };
        anim.advance();
        assert_eq!(anim.frame, 0);
        assert_eq!(anim.phase_frame, 0);
    }

    #[test]
    fn test_spinner_anim_elapsed_secs_uses_fixed_tick_rate() {
        let anim = SpinnerAnim {
            frame: 12,
            phase_frame: 4,
            phase: None,
            verb: String::new(),
        };
        assert_eq!(anim.elapsed_secs(), 1);
        assert_eq!(anim.phase_elapsed_secs(), 0);
    }

    #[test]
    fn test_spinner_anim_pick_verb_selects_from_pool_and_is_stable() {
        let mut anim = SpinnerAnim {
            frame: 42,
            phase_frame: 7,
            phase: None,
            verb: String::new(),
        };
        anim.pick_verb();
        let chosen = anim.verb.clone();
        assert!(SPINNER_VERBS.contains(&chosen.as_str()));
        // pick_verb 复位 frame
        assert_eq!(anim.frame, 0);
        assert_eq!(anim.phase_frame, 0);
        // 不再调用 pick_verb，verb 保持稳定
        anim.advance();
        assert_eq!(anim.verb, chosen);
    }

    #[test]
    fn test_spinner_anim_sync_phase_resets_only_phase_frame() {
        let mut anim = SpinnerAnim {
            frame: 30,
            phase_frame: 20,
            phase: None,
            verb: "Forging".to_string(),
        };
        anim.sync_phase(Some(
            crate::tui::model::runtime::spinner::SpinnerPhase::Thinking,
        ));
        assert_eq!(anim.frame, 30);
        assert_eq!(anim.phase_frame, 0);
        assert_eq!(anim.verb, "Forging");
    }

    #[test]
    fn test_spinner_anim_sync_phase_does_not_reset_for_calling_tools_remaining_change() {
        let mut anim = SpinnerAnim {
            frame: 30,
            phase_frame: 20,
            phase: Some(
                crate::tui::model::runtime::spinner::SpinnerPhase::CallingTools { remaining: 3 },
            ),
            verb: "Forging".to_string(),
        };
        anim.sync_phase(Some(
            crate::tui::model::runtime::spinner::SpinnerPhase::CallingTools { remaining: 2 },
        ));
        assert_eq!(anim.frame, 30);
        assert_eq!(anim.phase_frame, 20);
        assert_eq!(anim.verb, "Forging");
    }

    #[test]
    fn test_spinner_anim_sync_phase_resets_for_calling_tool_name_change() {
        let mut anim = SpinnerAnim {
            frame: 30,
            phase_frame: 20,
            phase: Some(
                crate::tui::model::runtime::spinner::SpinnerPhase::CallingTool("Read".to_string()),
            ),
            verb: "Forging".to_string(),
        };
        anim.sync_phase(Some(
            crate::tui::model::runtime::spinner::SpinnerPhase::CallingTool("Edit".to_string()),
        ));
        assert_eq!(anim.frame, 30);
        assert_eq!(anim.phase_frame, 0);
        assert_eq!(anim.verb, "Forging");
    }

    #[test]
    fn test_spinner_anim_sync_phase_does_not_reset_for_same_calling_tool_name() {
        let mut anim = SpinnerAnim {
            frame: 30,
            phase_frame: 20,
            phase: Some(
                crate::tui::model::runtime::spinner::SpinnerPhase::CallingTool("Read".to_string()),
            ),
            verb: "Forging".to_string(),
        };
        anim.sync_phase(Some(
            crate::tui::model::runtime::spinner::SpinnerPhase::CallingTool("Read".to_string()),
        ));
        assert_eq!(anim.frame, 30);
        assert_eq!(anim.phase_frame, 20);
        assert_eq!(anim.verb, "Forging");
    }
}
