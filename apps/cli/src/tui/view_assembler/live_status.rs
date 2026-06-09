//! 由 `RuntimeModel.spinner`（active + phase）+ view_state 动画（frame/verb）
//! 派生 `LiveStatusViewModel`。phase 语义→文案集中在此（DRY），文案与既有
//! `app/update/ui_event.rs` 字面量对齐。
//!
//! 本层可依赖 model（边界守卫只禁渲染库/副作用），但 ViewModel 输出仅含基本类型。

use crate::tui::model::runtime::model::RuntimeModel;
use crate::tui::model::runtime::spinner::{HookOutcome, SpinnerPhase};
use crate::tui::view_model::{LiveStatusViewModel, SpinnerLineView};
use crate::tui::view_state::SpinnerAnim;

pub struct LiveStatusAssembler;

impl LiveStatusAssembler {
    /// 由 Model 业务态 + view_state 动画态 + 排队输入派生实时状态行视图。
    ///
    /// 排队输入真相目前归 `ConversationModel::queued_submissions`；调用方只传入文本切片，
    /// 本层负责统一格式化为 live-status 预览行，避免 OutputArea 自持排队状态。
    pub fn assemble(
        runtime: &RuntimeModel,
        anim: &SpinnerAnim,
        queued_texts: &[String],
    ) -> LiveStatusViewModel {
        let spinner = if runtime.spinner.active {
            Some(SpinnerLineView {
                frame: anim.frame,
                verb: anim.verb.clone(),
                elapsed_secs: anim.elapsed_secs(),
                phase_elapsed_secs: anim.phase_elapsed_secs(),
                phase_text: runtime.spinner.phase.as_ref().map(phase_text),
            })
        } else {
            None
        };
        let queued_lines = queued_texts
            .iter()
            .map(|text| format!("> {text}"))
            .collect();
        LiveStatusViewModel {
            spinner,
            queued_lines,
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
            phase_frame: 5,
            phase: Some(SpinnerPhase::Thinking),
            verb: "Brewing".to_string(),
        };
        let vm = LiveStatusAssembler::assemble(&runtime, &anim, &[]);
        assert!(vm.spinner.is_none());
    }

    #[test]
    fn test_assemble_active_transfers_frame_verb() {
        let runtime = runtime_with_spinner(true, None);
        let anim = SpinnerAnim {
            frame: 12,
            phase_frame: 3,
            phase: None,
            verb: "Forging".to_string(),
        };
        let vm = LiveStatusAssembler::assemble(&runtime, &anim, &[]);
        let view = vm.spinner.expect("spinner present");
        assert_eq!(view.frame, 12);
        assert_eq!(view.verb, "Forging");
        assert_eq!(view.elapsed_secs, 1);
        assert_eq!(view.phase_elapsed_secs, 0);
        assert_eq!(view.phase_text, None);
    }

    #[test]
    fn test_assemble_uses_independent_phase_elapsed() {
        let runtime = runtime_with_spinner(true, Some(SpinnerPhase::Generating));
        let anim = SpinnerAnim {
            frame: 30,
            phase_frame: 5,
            phase: Some(SpinnerPhase::Generating),
            verb: "Forging".to_string(),
        };
        let vm = LiveStatusAssembler::assemble(&runtime, &anim, &[]);
        let view = vm.spinner.expect("spinner present");
        assert_eq!(view.elapsed_secs, 2);
        assert_eq!(view.phase_elapsed_secs, 0);
        assert_ne!(view.elapsed_secs, view.phase_elapsed_secs);
    }

    #[test]
    fn test_assemble_task_lines_pass_through() {
        let mut runtime = runtime_with_spinner(false, None);
        runtime.apply(RuntimeIntent::UpdateTaskLines(vec![
            "━━ Tasks: 0/1 ━━".to_string(),
            "□ #1 修复 bug".to_string(),
        ]));
        let vm = LiveStatusAssembler::assemble(&runtime, &SpinnerAnim::default(), &[]);
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
        assert_eq!(phase_text(&mk(HookOutcome::Done)), "Hook PreToolUse done");
        assert_eq!(
            phase_text(&mk(HookOutcome::Failed)),
            "Hook PreToolUse failed: lint"
        );
    }

    #[test]
    fn test_assemble_active_with_phase_converts_text() {
        let runtime = runtime_with_spinner(true, Some(SpinnerPhase::Generating));
        let vm = LiveStatusAssembler::assemble(&runtime, &SpinnerAnim::default(), &[]);
        assert_eq!(
            vm.spinner.unwrap().phase_text.as_deref(),
            Some("Generating...")
        );
    }

    #[test]
    fn test_assemble_queued_lines_format() {
        let runtime = runtime_with_spinner(true, None);
        let vm = LiveStatusAssembler::assemble(
            &runtime,
            &SpinnerAnim::default(),
            &["hello".to_string(), "world".to_string()],
        );
        assert_eq!(vm.queued_lines, vec!["> hello", "> world"]);
    }

    #[test]
    fn test_assemble_empty_queued_yields_no_lines() {
        let runtime = runtime_with_spinner(true, None);
        let vm = LiveStatusAssembler::assemble(&runtime, &SpinnerAnim::default(), &[]);
        assert!(vm.queued_lines.is_empty());
    }
}
