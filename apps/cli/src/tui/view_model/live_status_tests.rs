use super::*;
#[test]
fn test_live_status_view_model_default_is_empty() {
    let vm = LiveStatusViewModel::default();
    assert!(vm.spinner.is_none());
    assert!(vm.task_lines.is_empty());
}
#[test]
fn test_spinner_line_view_holds_fields() {
    let view = SpinnerLineView {
        frame: 9,
        verb: "Thinking".to_string(),
        elapsed_secs: 0,
        phase_elapsed_secs: Some(0),
        phase_text: Some("Thinking...".to_string()),
        detail_text: None,
    };
    assert_eq!(view.frame, 9);
    assert_eq!(view.elapsed_secs, 0);
    assert_eq!(view.phase_elapsed_secs, Some(0));
    assert_eq!(view.phase_text.as_deref(), Some("Thinking..."));
}
#[test]
fn test_live_status_view_model_equality() {
    let a = LiveStatusViewModel {
        spinner: Some(SpinnerLineView {
            frame: 1,
            verb: "Brewing".to_string(),
            elapsed_secs: 0,
            phase_elapsed_secs: Some(0),
            phase_text: None,
            detail_text: None,
        }),
        queued_lines: vec!["> hello".to_string()],
        task_lines: vec!["□ #1".to_string()],
        compact_progress: None,
    };
    let b = a.clone();
    assert_eq!(a, b);
    let c = LiveStatusViewModel::default();
    assert_ne!(a, c);
}
