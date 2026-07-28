use super::published_language::*;

#[test]
fn tool_outcome_exposes_timeout_and_unconfirmed_terminals() {
    let timed_out =
        ToolOutcome::timed_out("达到 effective deadline", CleanupConfirmation::Confirmed);
    let unconfirmed = ToolOutcome::cancellation_unconfirmed(
        "底层工作未确认停止",
        vec!["外部进程可能仍在运行".to_string()],
        vec!["call-child-1".to_string()],
    );

    assert!(matches!(
        timed_out,
        ToolOutcome::TimedOut(ToolTerminalDetails {
            cleanup: CleanupConfirmation::Confirmed,
            ..
        })
    ));
    assert!(matches!(
        unconfirmed,
        ToolOutcome::CancellationUnconfirmed(ToolTerminalDetails {
            cleanup: CleanupConfirmation::Unconfirmed,
            ..
        })
    ));
}

#[test]
fn timeout_terminal_preserves_safe_diagnostics() {
    let outcome = ToolOutcome::CancellationUnconfirmed(ToolTerminalDetails {
        safe_reason: "远端未确认取消".to_string(),
        possible_side_effects: vec!["请求可能已提交".to_string()],
        unfinished_call_ids: vec!["remote-42".to_string()],
        cleanup: CleanupConfirmation::Unconfirmed,
    });

    let ToolOutcome::CancellationUnconfirmed(details) = outcome else {
        panic!("应为 CancellationUnconfirmed");
    };
    assert_eq!(details.safe_reason, "远端未确认取消");
    assert_eq!(details.possible_side_effects, ["请求可能已提交"]);
    assert_eq!(details.unfinished_call_ids, ["remote-42"]);
}
