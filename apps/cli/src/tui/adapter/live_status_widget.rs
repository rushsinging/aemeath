//! 实时状态行 adapter：把 `LiveStatusViewModel` 单向写回 `OutputArea` 的
//! `spinner` / `task_status_lines` 镜像字段。这是这两个镜像字段的唯一生产写入路径。
//!
//! Instant 处理：`SpinnerState.start: Instant` 无法由 ViewModel 提供（vm 用 frame
//! 推算 elapsed）。本 adapter 在 None→Some 时新建 `SpinnerState`（start=now），
//! Some→Some 时只更新 frame/verb/phase 并保留原 start，使 elapsed 自然增长。

use crate::tui::render::output_area::{OutputArea, SpinnerState};
use crate::tui::view_model::LiveStatusViewModel;

/// 据 ViewModel 写回 widget 的 spinner 与 task 状态镜像。
pub(crate) fn apply_live_status_to_widget(output_area: &mut OutputArea, vm: &LiveStatusViewModel) {
    match &vm.spinner {
        Some(view) => {
            if let Some(existing) = output_area.spinner.as_mut() {
                // Some→Some：保留 start（elapsed 持续增长），更新动画 + phase。
                existing.frame = view.frame;
                existing.verb = view.verb.clone();
                existing.phase = view.phase_text.clone();
            } else {
                // None→Some：新建，start 取当前时刻。
                output_area.spinner = Some(SpinnerState {
                    frame: view.frame,
                    verb: view.verb.clone(),
                    start: std::time::Instant::now(),
                    phase: view.phase_text.clone(),
                });
            }
        }
        None => {
            output_area.spinner = None;
        }
    }
    output_area.task_status_lines = vm.task_lines.clone();
}

/// 仅写回 task 状态行镜像，不触碰 spinner（Task 3.1 只接线 task 路径用）。
///
/// 背景：当前 30 处 spinner 触发点仍直改 widget，`RuntimeModel.spinner.active`
/// 恒为 false。若走 [`apply_live_status_to_widget`] 会用 Model 派生的 inactive
/// spinner 覆写并清掉触发点刚设的 widget spinner（破坏行为）。Task 4.1 统一
/// spinner 路径后此入口可退役。
pub(crate) fn apply_task_lines_to_widget(output_area: &mut OutputArea, lines: &[String]) {
    output_area.task_status_lines = lines.to_vec();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::SpinnerLineView;

    fn vm_with_spinner(frame: u64, verb: &str, phase: Option<&str>) -> LiveStatusViewModel {
        LiveStatusViewModel {
            spinner: Some(SpinnerLineView {
                frame,
                verb: verb.to_string(),
                elapsed_secs: 0,
                phase_text: phase.map(|s| s.to_string()),
            }),
            task_lines: Vec::new(),
        }
    }

    #[test]
    fn test_apply_none_to_some_creates_spinner() {
        let mut output = OutputArea::new();
        assert!(output.spinner.is_none());
        let vm = vm_with_spinner(3, "Brewing", Some("Thinking..."));

        apply_live_status_to_widget(&mut output, &vm);

        let s = output.spinner.as_ref().expect("spinner created");
        assert_eq!(s.frame, 3);
        assert_eq!(s.verb, "Brewing");
        assert_eq!(s.phase.as_deref(), Some("Thinking..."));
    }

    #[test]
    fn test_apply_some_to_some_preserves_start_updates_animation() {
        let mut output = OutputArea::new();
        apply_live_status_to_widget(&mut output, &vm_with_spinner(1, "Forging", None));
        let original_start = output.spinner.as_ref().unwrap().start;

        apply_live_status_to_widget(
            &mut output,
            &vm_with_spinner(7, "Weaving", Some("Generating...")),
        );

        let s = output.spinner.as_ref().expect("spinner present");
        assert_eq!(s.frame, 7);
        assert_eq!(s.verb, "Weaving");
        assert_eq!(s.phase.as_deref(), Some("Generating..."));
        // start 未重置
        assert_eq!(s.start, original_start);
    }

    #[test]
    fn test_apply_none_clears_spinner() {
        let mut output = OutputArea::new();
        apply_live_status_to_widget(&mut output, &vm_with_spinner(1, "Cooking", None));
        assert!(output.spinner.is_some());

        let cleared = LiveStatusViewModel::default();
        apply_live_status_to_widget(&mut output, &cleared);
        assert!(output.spinner.is_none());
    }

    #[test]
    fn test_apply_writes_task_lines() {
        let mut output = OutputArea::new();
        let vm = LiveStatusViewModel {
            spinner: None,
            task_lines: vec!["━━ Tasks: 1/2 ━━".to_string(), "✓ #1 done".to_string()],
        };

        apply_live_status_to_widget(&mut output, &vm);

        assert_eq!(
            output.task_status_lines,
            vec!["━━ Tasks: 1/2 ━━", "✓ #1 done"]
        );
    }

    #[test]
    fn test_apply_task_lines_writes_lines_only() {
        let mut output = OutputArea::new();
        apply_task_lines_to_widget(&mut output, &["a".to_string(), "b".to_string()]);
        assert_eq!(output.task_status_lines, vec!["a", "b"]);
    }

    #[test]
    fn test_apply_task_lines_preserves_spinner() {
        let mut output = OutputArea::new();
        // 先经完整路径设上 spinner。
        apply_live_status_to_widget(&mut output, &vm_with_spinner(2, "Brewing", None));
        assert!(output.spinner.is_some());

        // 仅写 task 行不应清掉 spinner（Task 3.1 关键不变式）。
        apply_task_lines_to_widget(&mut output, &["x".to_string()]);
        assert!(output.spinner.is_some());
        assert_eq!(output.task_status_lines, vec!["x"]);
    }

    #[test]
    fn test_apply_task_lines_empty_clears() {
        let mut output = OutputArea::new();
        apply_task_lines_to_widget(&mut output, &["a".to_string()]);
        apply_task_lines_to_widget(&mut output, &[]);
        assert!(output.task_status_lines.is_empty());
    }
}
