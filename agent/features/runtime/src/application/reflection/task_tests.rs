use super::*;
use std::{future::pending, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

fn successful_payload() -> CompleteReflectionResult {
    CompleteReflectionResult {
        output: memory::api::reflection::ReflectionOutput::default(),
        input_tokens: 0,
        output_tokens: 0,
        apply_result: None,
        error_category: None,
        record_id: None,
    }
}

fn payload_with_applied_changes(added: usize, outdated_marked: usize) -> CompleteReflectionResult {
    CompleteReflectionResult {
        apply_result: Some(memory::api::reflection::ReflectionApplyResult {
            attempted: added + outdated_marked,
            completed: added + outdated_marked,
            suggestions_added: added,
            outdated_marked,
            superseded: 0,
        }),
        ..successful_payload()
    }
}

fn request() -> ReflectionTaskRequest {
    ReflectionTaskRequest::new(ReflectionTaskTrigger::PreCompact, vec![])
}

fn assert_completed(
    outcome: ReflectionRunOutcome,
    expected_status: ReflectionTaskCompletionStatus,
) -> ReflectionTaskCompletion {
    match outcome {
        ReflectionRunOutcome::Completed(completion) => {
            assert_eq!(completion.status, expected_status);
            completion
        }
        ReflectionRunOutcome::DisabledSkipped => {
            panic!("the run must not be skipped by configuration")
        }
    }
}

#[tokio::test]
async fn run_awaits_execution_and_reports_succeeded() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(successful_payload())
    });

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Succeeded,
    );
}

#[tokio::test]
async fn each_run_uses_the_executor_currently_installed() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));

    for marker in ["initial", "switched"] {
        let observed = Arc::clone(&observed);
        let marker = marker.to_string();
        assert_completed(
            adapter
                .run_future(
                    ReflectionTaskTrigger::PreCompact,
                    CancellationToken::new(),
                    move |_cancel| async move {
                        observed.lock().await.push(marker);
                        Ok(successful_payload())
                    },
                )
                .await,
            ReflectionTaskCompletionStatus::Succeeded,
        );
    }

    assert_eq!(*observed.lock().await, ["initial", "switched"]);
}

#[tokio::test]
async fn the_run_receives_the_messages_it_was_given() {
    let observed = Arc::new(Mutex::new(Vec::<String>::new()));
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), {
        let observed = Arc::clone(&observed);
        move |request: ReflectionTaskRequest, _cancel| {
            let observed = Arc::clone(&observed);
            async move {
                *observed.lock().await = request
                    .messages
                    .iter()
                    .map(share::message::Message::text_content)
                    .collect();
                Ok(successful_payload())
            }
        }
    });

    assert_completed(
        adapter
            .run(ReflectionTaskRequest::new(
                ReflectionTaskTrigger::PreCompact,
                vec![share::message::Message::user("before compact")],
            ))
            .await,
        ReflectionTaskCompletionStatus::Succeeded,
    );

    assert_eq!(*observed.lock().await, ["before compact"]);
}

#[tokio::test]
async fn the_run_cancellation_token_wins_over_the_executor() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        pending::<()>().await;
        #[allow(unreachable_code)]
        Ok(successful_payload())
    });
    let cancel = CancellationToken::new();
    cancel.cancel();

    assert_completed(
        adapter
            .run_future(
                ReflectionTaskTrigger::Interval { step_count: 3 },
                cancel,
                move |_cancel| async {
                    pending::<()>().await;
                    #[allow(unreachable_code)]
                    Ok(successful_payload())
                },
            )
            .await,
        ReflectionTaskCompletionStatus::Cancelled,
    );
}

#[tokio::test]
async fn timeout_reports_timed_out_without_waiting_for_the_executor() {
    let adapter =
        ReflectionTaskAdapter::new(Duration::from_millis(20), |_request, _cancel| async {
            pending::<()>().await;
            #[allow(unreachable_code)]
            Ok(successful_payload())
        });

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::TimedOut,
    );
}

#[tokio::test]
async fn applied_changes_accumulate_into_one_notice_taken_exactly_once() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(payload_with_applied_changes(2, 1))
    });

    let completion = assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Succeeded,
    );
    assert_eq!(
        completion
            .metadata
            .as_ref()
            .map(|item| item.applied_changes()),
        Some(3)
    );
    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Succeeded,
    );

    assert_eq!(
        adapter.take_memory_update_notice(),
        Some(MemoryUpdateNotice { changed: 6 })
    );
    assert_eq!(adapter.take_memory_update_notice(), None);
}

#[tokio::test]
async fn zero_applied_changes_never_produce_a_notice() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(payload_with_applied_changes(0, 0))
    });

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Succeeded,
    );

    assert_eq!(adapter.take_memory_update_notice(), None);
}

#[tokio::test]
async fn a_failed_run_never_produces_a_notice() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(CompleteReflectionResult {
            error_category: Some(memory::api::reflection::ReflectionErrorCategory::Apply),
            ..payload_with_applied_changes(3, 0)
        })
    });

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Failed,
    );

    assert_eq!(adapter.take_memory_update_notice(), None);
}

#[tokio::test]
async fn a_timed_out_run_never_produces_a_notice() {
    let adapter =
        ReflectionTaskAdapter::new(Duration::from_millis(20), |_request, _cancel| async {
            pending::<()>().await;
            #[allow(unreachable_code)]
            Ok(successful_payload())
        });

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::TimedOut,
    );

    assert_eq!(adapter.take_memory_update_notice(), None);
}

/// Logs use the reflection facts that only a terminal run can report, and the
/// configuration gate reports why it skipped. Both are `info` because each run
/// happens at most once per trigger window.
/// Drain once, then filter: the capture is a single queue, so a per-needle
/// drain would swallow the lines the next needle looks for.
fn captured_info_lines() -> Vec<String> {
    crate::test_log::drain()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Info)
        .map(|(_, message)| message)
        .collect()
}

fn lines_matching<'a>(lines: &'a [String], needle: &str) -> Vec<&'a String> {
    lines.iter().filter(|line| line.contains(needle)).collect()
}

#[tokio::test(flavor = "current_thread")]
async fn a_terminal_run_logs_its_token_usage_and_applied_counts() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(CompleteReflectionResult {
            input_tokens: 321,
            output_tokens: 123,
            ..payload_with_applied_changes(2, 1)
        })
    });
    let _guard = crate::test_log::begin();

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Succeeded,
    );

    let lines = captured_info_lines();
    let tokens = lines_matching(&lines, "[reflection_tokens]");
    assert_eq!(tokens.len(), 1, "{lines:?}");
    assert!(tokens[0].contains("input_tokens=321"), "{}", tokens[0]);
    assert!(tokens[0].contains("output_tokens=123"), "{}", tokens[0]);

    let applied = lines_matching(&lines, "[reflection_applied]");
    assert_eq!(applied.len(), 1, "{lines:?}");
    assert!(applied[0].contains("added=2"), "{}", applied[0]);
    assert!(applied[0].contains("outdated=1"), "{}", applied[0]);

    let terminal = lines_matching(&lines, "[reflection_terminal]");
    assert_eq!(terminal.len(), 1, "{lines:?}");
    assert!(terminal[0].contains("status=succeeded"), "{}", terminal[0]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_run_without_changes_logs_no_applied_line() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(CompleteReflectionResult {
            input_tokens: 7,
            output_tokens: 3,
            ..payload_with_applied_changes(0, 0)
        })
    });
    let _guard = crate::test_log::begin();

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Succeeded,
    );

    let lines = captured_info_lines();
    assert!(
        lines_matching(&lines, "[reflection_applied]").is_empty(),
        "zero changes must stay out of the log: {lines:?}"
    );
    assert_eq!(
        lines_matching(&lines, "[reflection_tokens]").len(),
        1,
        "{lines:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_run_whose_provider_failed_logs_no_applied_line() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Err(ReflectionError::LlmCall)
    });
    let _guard = crate::test_log::begin();

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Failed,
    );

    let lines = captured_info_lines();
    let terminal = lines_matching(&lines, "[reflection_terminal]");
    assert_eq!(terminal.len(), 1, "{lines:?}");
    assert!(
        terminal[0].contains("error_category=llm"),
        "{}",
        terminal[0]
    );
    assert!(
        lines_matching(&lines, "[reflection_applied]").is_empty(),
        "a run that never applied must not claim a change: {lines:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn each_configuration_gate_reports_its_own_reason() {
    for (config, expected_reason) in [
        (
            share::config::MemoryConfig {
                enabled: false,
                ..share::config::MemoryConfig::default()
            },
            "memory_off",
        ),
        (
            share::config::MemoryConfig {
                enabled: true,
                reflection: share::config::ReflectionConfig {
                    enabled: false,
                    ..Default::default()
                },
                ..Default::default()
            },
            "reflection_off",
        ),
        (
            share::config::MemoryConfig {
                enabled: true,
                reflection: share::config::ReflectionConfig {
                    enabled: true,
                    interval_runs: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
            "interval_zero",
        ),
    ] {
        let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));
        let _guard = crate::test_log::begin();
        let binding =
            crate::application::run::run_factory_support::doubles::fake_provider_binding();

        let outcome = adapter
            .run_complete(
                request(),
                config,
                binding.provider.clone(),
                binding.model.clone(),
                binding.max_tokens,
                binding.requested_reasoning,
                "system".to_string(),
                "en".to_string(),
                std::sync::Arc::new(memory::api::NoOpMemory),
                crate::application::reflection::test_support::noop_reflection_history(),
                tokio_util::sync::CancellationToken::new(),
            )
            .await;

        assert!(matches!(outcome, ReflectionRunOutcome::DisabledSkipped));
        let lines = captured_info_lines();
        let disabled = lines_matching(&lines, "[reflection_disabled]");
        assert_eq!(disabled.len(), 1, "{lines:?}");
        assert!(disabled[0].contains(expected_reason), "{}", disabled[0]);
    }
}
