use super::*;
use crate::application::hook_adapter::{
    RuntimeHookDirective, RuntimeHookDispatch, RuntimeHookDisplayMessage,
    RuntimeHookDisplayMessageKind, RuntimeHookExecution, RuntimeHookExecutionStatus,
    RuntimeHookReason,
};
use hook::HookPoint;
use std::time::Duration;

fn stop_hook_feedback_for_test(dispatch: &RuntimeHookDispatch) -> Option<String> {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    if matches!(dispatch.directive, RuntimeHookDirective::Block { .. }) {
        Some(runtime.block_on(stop_hook_feedback(dispatch, "test-session", "zh")))
    } else {
        None
    }
}

fn block_dispatch(
    source: &str,
    stdout: &str,
    stderr: Option<&str>,
    system_message: Option<&str>,
) -> RuntimeHookDispatch {
    let mut messages = Vec::new();
    if let Some(msg) = system_message {
        messages.push(RuntimeHookDisplayMessage {
            point: HookPoint::Stop,
            source: source.to_string(),
            execution_ordinal: 1,
            attempt: 1,
            kind: RuntimeHookDisplayMessageKind::SystemMessage,
            text: msg.to_string(),
        });
    }
    RuntimeHookDispatch {
        directive: RuntimeHookDirective::Block {
            reason: RuntimeHookReason::ExitCode {
                code: 2,
                stderr: stderr.unwrap_or("").to_string(),
            },
        },
        executions: vec![RuntimeHookExecution {
            status: RuntimeHookExecutionStatus::Blocked,
            attempts: 1,
            exit_code: Some(2),
            stdout: stdout.to_string(),
            stderr: stderr.unwrap_or("").to_string(),
            duration: Duration::from_millis(10),
        }],
        messages,
    }
}

fn continue_dispatch() -> RuntimeHookDispatch {
    RuntimeHookDispatch {
        directive: RuntimeHookDirective::Continue,
        executions: vec![RuntimeHookExecution {
            status: RuntimeHookExecutionStatus::Success,
            attempts: 1,
            exit_code: Some(0),
            stdout: "done".to_string(),
            stderr: "".to_string(),
            duration: Duration::from_millis(5),
        }],
        messages: Vec::new(),
    }
}

#[test]
fn test_stop_hook_feedback_returns_none_without_block() {
    let dispatch = continue_dispatch();
    assert!(stop_hook_feedback_for_test(&dispatch).is_none());
}

#[test]
fn test_stop_hook_feedback_uses_error_when_blocked() {
    let dispatch = block_dispatch("check.sh", "", Some("failed"), None);

    let feedback = stop_hook_feedback_for_test(&dispatch).unwrap();

    assert!(feedback.contains("Stop hook"));
    assert!(feedback.contains("failed"));
}

#[test]
fn test_stop_hook_feedback_uses_stdout_when_blocked() {
    let dispatch = block_dispatch("check.sh", "unsafe op found\n", None, None);

    let feedback = stop_hook_feedback_for_test(&dispatch).unwrap();

    assert!(feedback.contains("Stop hook"));
    assert!(feedback.contains("unsafe op found"));
}

#[test]
fn test_stop_hook_feedback_uses_system_message_when_blocked() {
    let dispatch = block_dispatch("line-check.sh", "", None, Some("line limit exceeded"));

    let feedback = stop_hook_feedback_for_test(&dispatch).unwrap();

    assert!(feedback.contains("line-check.sh"));
    assert!(feedback.contains("line limit exceeded"));
}
