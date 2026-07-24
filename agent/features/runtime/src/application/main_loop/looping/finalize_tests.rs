use super::*;
use crate::application::hook_types::{
    RuntimeHookDirective, RuntimeHookDispatch, RuntimeHookDisplayMessage,
    RuntimeHookDisplayMessageKind, RuntimeHookExecution, RuntimeHookExecutionStatus,
    RuntimeHookReason,
};
use hook::HookPoint;
use std::time::Duration;

fn stop_hook_feedback_for_test(dispatch: &RuntimeHookDispatch) -> Option<StopHookFeedbackMessage> {
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
        block_detail: Some(crate::application::hook_types::RuntimeHookBlockDetail {
            command: source.to_string(),
            execution_ordinal: 1,
            execution: RuntimeHookExecution {
                status: RuntimeHookExecutionStatus::Blocked,
                attempts: 1,
                exit_code: Some(2),
                stdout: stdout.to_string(),
                stderr: stderr.unwrap_or("").to_string(),
                duration: Duration::from_millis(10),
            },
        }),
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
        block_detail: None,
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

    assert!(feedback.llm_text.contains("Stop hook"));
    assert!(feedback.llm_text.contains("failed"));
}

#[test]
fn test_stop_hook_feedback_uses_stdout_when_blocked() {
    let dispatch = block_dispatch("check.sh", "unsafe op found\n", None, None);

    let feedback = stop_hook_feedback_for_test(&dispatch).unwrap();

    assert!(feedback.llm_text.contains("Stop hook"));
    assert!(feedback.llm_text.contains("unsafe op found"));
}

#[test]
fn long_stop_hook_output_uses_file_pointer_for_llm_text() {
    let long_output = "x".repeat(INLINE_HOOK_OUTPUT_LIMIT + 1);
    let dispatch = block_dispatch("check-agent-stop.sh", &long_output, None, None);

    let feedback = stop_hook_feedback_for_test(&dispatch).unwrap();

    let path = feedback
        .payload
        .output_file
        .as_deref()
        .expect("long output must be persisted");
    assert!(std::path::Path::new(path).is_file());
    assert!(feedback.llm_text.contains(path));
    assert!(!feedback.llm_text.contains(&long_output));
    let _ = std::fs::remove_file(path);
}

#[test]
fn stop_hook_preview_limits_stdout_to_three_lines_and_stderr_to_five_lines() {
    let stdout = "one\ntwo\nthree\nfour";
    let stderr = "a\nb\nc\nd\ne\nf";
    let dispatch = block_dispatch("check-agent-stop.sh", stdout, Some(stderr), None);

    let feedback = stop_hook_feedback_for_test(&dispatch).unwrap();

    assert_eq!(feedback.payload.stdout_preview, "one\ntwo\nthree");
    assert!(feedback.payload.stdout_truncated);
    assert_eq!(feedback.payload.stderr_preview, "a\nb\nc\nd\ne");
    assert!(feedback.payload.stderr_truncated);
}

#[test]
fn stop_hook_preview_keeps_exact_stdout_and_stderr_line_limits() {
    let stdout = "one\ntwo\nthree";
    let stderr = "a\nb\nc\nd\ne";
    let dispatch = block_dispatch("check-agent-stop.sh", stdout, Some(stderr), None);

    let feedback = stop_hook_feedback_for_test(&dispatch).unwrap();

    assert!(!feedback.payload.stdout_truncated);
    assert!(!feedback.payload.stderr_truncated);
}

#[test]
fn test_stop_hook_feedback_uses_system_message_when_blocked() {
    let dispatch = block_dispatch("line-check.sh", "", None, Some("line limit exceeded"));

    let feedback = stop_hook_feedback_for_test(&dispatch).unwrap();

    assert!(feedback.payload.command.contains("line-check.sh"));
    assert_eq!(feedback.payload.reason, "exit code 2");
}

// ── run_stop_hook_before_finish integration ──────────────────────

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Mock HookPort that counts dispatch_at calls.
struct CountingHookPort {
    dispatch_count: AtomicUsize,
    last_point: std::sync::Mutex<Option<String>>,
}

impl CountingHookPort {
    fn new() -> Self {
        Self {
            dispatch_count: AtomicUsize::new(0),
            last_point: std::sync::Mutex::new(None),
        }
    }
    fn count(&self) -> usize {
        self.dispatch_count.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl HookPort for CountingHookPort {
    async fn dispatch(
        &self,
        invocation: hook::HookInvocation,
        _cancellation: &CancellationToken,
    ) -> hook::HookOutcome {
        self.dispatch_count.fetch_add(1, Ordering::SeqCst);
        *self.last_point.lock().unwrap() = Some(format!("{:?}", invocation.point()));
        hook::HookOutcome::proceed()
    }
}

#[derive(Clone)]
struct NoopSink;
impl crate::application::main_loop::ChatEventSink for NoopSink {
    fn send_event<'a>(
        &'a self,
        _event: crate::application::main_loop::RuntimeStreamEvent,
    ) -> crate::application::main_loop::EventFuture<'a> {
        Box::pin(async {})
    }
    fn try_send_event(&self, _event: crate::application::main_loop::RuntimeStreamEvent) {}
    fn send_domain_event<'a>(
        &'a self,
        _event: crate::domain::agent_run::RunDomainEvent,
    ) -> crate::application::main_loop::EventFuture<'a> {
        Box::pin(async {})
    }
}

#[tokio::test]
async fn run_stop_hook_invokes_hook_port_with_stop_invocation() {
    let counting = Arc::new(CountingHookPort::new());
    let hook_port: Arc<dyn HookPort> = counting.clone();
    let outcome = crate::application::subagent::runner::AgentRunOutcome {
        status: crate::application::subagent::runner::AgentRunStatus::Completed,
        turns: 1,
        duration: std::time::Duration::from_secs(1),
        role: None,
        model: "test-model".to_string(),
    };
    let cancel = CancellationToken::new();

    let result = super::run_stop_hook_before_finish(
        &outcome,
        &NoopSink,
        &hook_port,
        "test-session",
        "zh",
        std::path::Path::new("/tmp"),
        &cancel,
    )
    .await;

    assert_eq!(counting.count(), 1, "dispatch must be called once");
    assert!(result.is_none(), "Continue → None");
}
