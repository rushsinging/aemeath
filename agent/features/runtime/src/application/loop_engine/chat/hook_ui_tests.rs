//! Hook UI 权威终态投影测试。

use crate::application::hook::outcome_mapper::{
    RuntimeHookDirective, RuntimeHookDispatch, RuntimeHookExecution, RuntimeHookExecutionStatus,
};
use crate::application::loop_engine::chat::hook_ui::project_hook_dispatch;
use crate::application::loop_engine::chat::{
    ChatEventSink, EventFuture, RuntimeHookEventStatus, RuntimeStreamEvent,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Default)]
struct RecordingSink(Arc<Mutex<Vec<RuntimeStreamEvent>>>);

impl ChatEventSink for RecordingSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        let events = Arc::clone(&self.0);
        Box::pin(async move {
            events.lock().expect("event lock").push(event);
        })
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.0.lock().expect("event lock").push(event);
    }
}

fn execution(
    status: RuntimeHookExecutionStatus,
    attempts: u8,
    exit_code: Option<i32>,
    stderr: &str,
) -> RuntimeHookExecution {
    RuntimeHookExecution {
        status,
        attempts,
        exit_code,
        stdout: String::new(),
        stderr: stderr.to_string(),
        duration: Duration::from_millis(1),
    }
}

#[tokio::test]
async fn retry_then_success_projects_succeeded_from_final_directive() {
    let sink = RecordingSink::default();
    let dispatch = RuntimeHookDispatch {
        directive: RuntimeHookDirective::Continue,
        executions: vec![
            execution(
                RuntimeHookExecutionStatus::ExecutionFailed {
                    error: "首次失败".to_string(),
                },
                1,
                Some(1),
                "first failure",
            ),
            execution(RuntimeHookExecutionStatus::Success, 2, Some(0), ""),
        ],
        messages: Vec::new(),
        block_detail: None,
    };

    project_hook_dispatch(&sink, hook::HookPoint::Stop, &dispatch).await;

    let events = sink.0.lock().expect("event lock");
    let RuntimeStreamEvent::HookEvent(event) = events.last().expect("hook event") else {
        panic!("expected HookEvent");
    };
    assert_eq!(event.status, RuntimeHookEventStatus::Succeeded);
    let result = event.result.as_ref().expect("final execution result");
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.decision.as_deref(), Some("continue"));
    assert!(result.reason.is_none());
    assert!(result.stderr.is_empty());
}

#[tokio::test]
async fn exhausted_execution_failure_projects_blocked_with_final_diagnostics() {
    let sink = RecordingSink::default();
    let final_execution = execution(
        RuntimeHookExecutionStatus::ExecutionFailed {
            error: "最终执行失败".to_string(),
        },
        3,
        Some(1),
        "guard failed",
    );
    let dispatch = RuntimeHookDispatch {
        directive: RuntimeHookDirective::Block {
            reason: crate::application::hook::outcome_mapper::RuntimeHookReason::StopHookExecutionFailed {
                error: "最终执行失败".to_string(),
            },
        },
        executions: vec![final_execution.clone()],
        messages: Vec::new(),
        block_detail: Some(
            crate::application::hook::outcome_mapper::RuntimeHookBlockDetail {
                command: "check-agent-stop.sh".to_string(),
                execution_ordinal: 1,
                execution: final_execution,
            },
        ),
    };

    project_hook_dispatch(&sink, hook::HookPoint::Stop, &dispatch).await;

    let events = sink.0.lock().expect("event lock");
    let RuntimeStreamEvent::HookEvent(event) = events.last().expect("hook event") else {
        panic!("expected HookEvent");
    };
    assert_eq!(event.status, RuntimeHookEventStatus::Blocked);
    assert_eq!(event.command.as_deref(), Some("check-agent-stop.sh"));
    let result = event.result.as_ref().expect("final execution result");
    assert_eq!(result.exit_code, Some(1));
    assert_eq!(result.decision.as_deref(), Some("block"));
    assert_eq!(result.reason.as_deref(), Some("最终执行失败"));
    assert_eq!(result.stderr, "guard failed");
}
