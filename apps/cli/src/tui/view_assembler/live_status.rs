//! 由 `RuntimeModel.spinner`（active + phase）+ view_state 动画（frame/verb）
//! 派生 `LiveStatusViewModel`。phase 语义→文案集中在此（DRY），文案与既有
//! `app/update/ui_event.rs` 字面量对齐。
//!
//! 本层可依赖 model（边界守卫只禁渲染库/副作用），但 ViewModel 输出仅含基本类型。

use crate::tui::model::runtime::model::RuntimeModel;
use crate::tui::model::runtime::spinner::{HookOutcome, SpinnerPhase};
use crate::tui::view_model::{LiveStatusViewModel, SpinnerLineView};
use crate::tui::view_state::SpinnerAnim;

/// 单个 SpinnerTick 周期（毫秒），与渲染层 90ms 节奏一致。
const TICK_MS: u64 = 90;

pub struct LiveStatusAssembler;

impl LiveStatusAssembler {
    /// 由 Model 业务态 + view_state 动画态派生实时状态行视图。
    pub fn assemble(runtime: &RuntimeModel, anim: &SpinnerAnim) -> LiveStatusViewModel {
        let spinner = if runtime.spinner.active {
            Some(SpinnerLineView {
                frame: anim.frame,
                verb: anim.verb.clone(),
                elapsed_secs: anim.frame * TICK_MS / 1000,
                phase_text: runtime.spinner.phase.as_ref().map(phase_text),
            })
        } else {
            None
        };
        LiveStatusViewModel {
            spinner,
            task_lines: runtime.task_status.lines.clone(),
        }
    }
}

/// 将 phase 语义转换为显示文案。文案与既有 `ui_event.rs` 字面量对齐。
fn phase_text(phase: &SpinnerPhase) -> String {
    match phase {
        SpinnerPhase::Thinking => "Thinking...".to_string(),
        SpinnerPhase::Generating => "Generating...".to_string(),
        SpinnerPhase::AgentWorking => "Agent working...".to_string(),
        SpinnerPhase::Reflecting => "Reflecting...".to_string(),
        SpinnerPhase::ThinkingQueued => "Thinking with queued input...".to_string(),
        SpinnerPhase::CallingTool(name) => format!("Calling {name}..."),
        SpinnerPhase::CallingTools { remaining } => {
            format!("Calling tools... ({remaining} running)")
        }
        SpinnerPhase::Hook {
            event,
            detail,
            outcome,
        } => match outcome {
            HookOutcome::Running => format!("Hook {event}: {detail}"),
            HookOutcome::Blocked => format!("Hook {event} blocked"),
            HookOutcome::Timeout => format!("Hook {event} timeout..."),
            HookOutcome::Done => format!("Hook {event} done"),
            HookOutcome::Failed => format!("Hook {event} failed: {detail}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::runtime::intent::RuntimeIntent;

    fn runtime_with_spinner(active: bool, phase: Option<SpinnerPhase>) -> RuntimeModel {
        let mut runtime = RuntimeModel::default();
        runtime.spinner.active = active;
        runtime.spinner.phase = phase;
        runtime
    }

    #[test]
    fn test_assemble_inactive_yields_no_spinner() {
        let runtime = runtime_with_spinner(false, Some(SpinnerPhase::Thinking));
        let anim = SpinnerAnim {
            frame: 5,
            verb: "Brewing".to_string(),
        };
        let vm = LiveStatusAssembler::assemble(&runtime, &anim);
        assert!(vm.spinner.is_none());
    }

    #[test]
    fn test_assemble_active_transfers_frame_verb_elapsed() {
        let runtime = runtime_with_spinner(true, None);
        let anim = SpinnerAnim {
            frame: 12,
            verb: "Forging".to_string(),
        };
        let vm = LiveStatusAssembler::assemble(&runtime, &anim);
        let view = vm.spinner.expect("spinner present");
        assert_eq!(view.frame, 12);
        assert_eq!(view.verb, "Forging");
        // 12 * 90 / 1000 = 1
        assert_eq!(view.elapsed_secs, 1);
        assert_eq!(view.phase_text, None);
    }

    #[test]
    fn test_assemble_elapsed_secs_zero_when_below_one_second() {
        let runtime = runtime_with_spinner(true, None);
        let anim = SpinnerAnim {
            frame: 10, // 10*90=900ms < 1s
            verb: "Cooking".to_string(),
        };
        let vm = LiveStatusAssembler::assemble(&runtime, &anim);
        assert_eq!(vm.spinner.unwrap().elapsed_secs, 0);
    }

    #[test]
    fn test_assemble_task_lines_pass_through() {
        let mut runtime = runtime_with_spinner(false, None);
        runtime.apply(RuntimeIntent::UpdateTaskLines(vec![
            "━━ Tasks: 0/1 ━━".to_string(),
            "□ #1 修复 bug".to_string(),
        ]));
        let vm = LiveStatusAssembler::assemble(&runtime, &SpinnerAnim::default());
        assert_eq!(vm.task_lines, vec!["━━ Tasks: 0/1 ━━", "□ #1 修复 bug"]);
    }

    #[test]
    fn test_phase_text_simple_variants() {
        assert_eq!(phase_text(&SpinnerPhase::Thinking), "Thinking...");
        assert_eq!(phase_text(&SpinnerPhase::Generating), "Generating...");
        assert_eq!(phase_text(&SpinnerPhase::AgentWorking), "Agent working...");
        assert_eq!(phase_text(&SpinnerPhase::Reflecting), "Reflecting...");
        assert_eq!(
            phase_text(&SpinnerPhase::ThinkingQueued),
            "Thinking with queued input..."
        );
    }

    #[test]
    fn test_phase_text_calling_tool_variants() {
        assert_eq!(
            phase_text(&SpinnerPhase::CallingTool("Read".to_string())),
            "Calling Read..."
        );
        assert_eq!(
            phase_text(&SpinnerPhase::CallingTools { remaining: 3 }),
            "Calling tools... (3 running)"
        );
    }

    #[test]
    fn test_phase_text_hook_outcomes() {
        let mk = |outcome| SpinnerPhase::Hook {
            event: "PreToolUse".to_string(),
            detail: "lint".to_string(),
            outcome,
        };
        assert_eq!(
            phase_text(&mk(HookOutcome::Running)),
            "Hook PreToolUse: lint"
        );
        assert_eq!(
            phase_text(&mk(HookOutcome::Blocked)),
            "Hook PreToolUse blocked"
        );
        assert_eq!(
            phase_text(&mk(HookOutcome::Timeout)),
            "Hook PreToolUse timeout..."
        );
        assert_eq!(phase_text(&mk(HookOutcome::Done)), "Hook PreToolUse done");
        assert_eq!(
            phase_text(&mk(HookOutcome::Failed)),
            "Hook PreToolUse failed: lint"
        );
    }

    #[test]
    fn test_assemble_active_with_phase_converts_text() {
        let runtime = runtime_with_spinner(true, Some(SpinnerPhase::Generating));
        let vm = LiveStatusAssembler::assemble(&runtime, &SpinnerAnim::default());
        assert_eq!(
            vm.spinner.unwrap().phase_text.as_deref(),
            Some("Generating...")
        );
    }
}
