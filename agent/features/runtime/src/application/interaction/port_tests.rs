use std::sync::Arc;

use super::*;
use sdk::{
    InteractionCancelReason, InteractionCommandOutcome, InteractionReply, InteractionReplyError,
    InteractionRequest, InteractionRequestBody, RunId, UserAnswer, UserQuestion,
};

// ── Test helpers ──

fn question_request() -> InteractionRequest {
    InteractionRequest {
        id: sdk::InteractionRequestId::new_v7(),
        run_id: RunId::new_v7(),
        tool_call_id: None,
        body: InteractionRequestBody::UserQuestions(vec![UserQuestion {
            prompt: "继续？".to_string(),
            options: vec!["是".to_string()],
            allow_multi: false,
        }]),
    }
}

/// Helper: call `register` on a port (trait object or concrete).
fn port_register(
    port: &dyn InteractionPort,
    request: InteractionRequest,
) -> Result<tokio::sync::oneshot::Receiver<InteractionCompletion>, InteractionPortError> {
    port.register(request)
}

// ── InteractionBridge as InteractionPort ──

#[tokio::test]
async fn bridge_register_reply_cancel_through_trait() {
    let bridge = InteractionBridge::new();

    let request = question_request();
    let waiter = port_register(&bridge, request.clone()).unwrap();

    assert_eq!(
        bridge.reply(
            &request.id,
            InteractionReply::UserQuestions(vec![UserAnswer("是".to_string())]),
        ),
        InteractionCommandOutcome::Accepted
    );
    assert_eq!(
        waiter.await.unwrap(),
        InteractionCompletion::Replied(InteractionReply::UserQuestions(vec![UserAnswer(
            "是".to_string()
        )]))
    );
    assert_eq!(
        bridge.cancel(&request.id, InteractionCancelReason::UserCancelled),
        InteractionCommandOutcome::AlreadyCompleted
    );
}

#[test]
fn bridge_dropped_waiter_reports_run_cancelling() {
    let bridge = InteractionBridge::new();
    let request = question_request();
    let waiter = port_register(&bridge, request.clone()).unwrap();
    drop(waiter);

    assert_eq!(
        bridge.reply(
            &request.id,
            InteractionReply::UserQuestions(vec![UserAnswer("是".to_string())]),
        ),
        InteractionCommandOutcome::RunCancelling
    );
    assert_eq!(
        bridge.cancel(&request.id, InteractionCancelReason::UserCancelled),
        InteractionCommandOutcome::AlreadyCompleted
    );
}

#[test]
fn bridge_invalid_reply_does_not_consume_waiter() {
    let bridge = InteractionBridge::new();
    let request = question_request();
    let _waiter = port_register(&bridge, request.clone()).unwrap();

    assert_eq!(
        bridge.reply(&request.id, InteractionReply::HardPauseContinue),
        InteractionCommandOutcome::InvalidReply(InteractionReplyError::VariantMismatch)
    );
    assert!(bridge.contains(&request.id));
    assert_eq!(
        bridge.cancel(&request.id, InteractionCancelReason::UserCancelled),
        InteractionCommandOutcome::Accepted
    );
}

#[test]
fn bridge_unknown_duplicate_and_run_drain() {
    let bridge = InteractionBridge::new();
    let unknown = sdk::InteractionRequestId::new_v7();
    assert_eq!(
        bridge.cancel(&unknown, InteractionCancelReason::UserCancelled),
        InteractionCommandOutcome::NotFound
    );

    let request = question_request();
    let duplicate = request.clone();
    let _waiter = port_register(&bridge, request.clone()).unwrap();
    assert_eq!(
        port_register(&bridge, duplicate).unwrap_err(),
        InteractionPortError::AlreadyRegistered
    );
    assert_eq!(
        bridge.drain_run(&request.run_id, InteractionCancelReason::RunCancelled),
        1
    );
    assert!(!bridge.contains(&request.id));
    assert_eq!(
        bridge.reply(
            &request.id,
            InteractionReply::UserQuestions(vec![UserAnswer("是".to_string())]),
        ),
        InteractionCommandOutcome::AlreadyCompleted
    );
}

// ── UnavailableInteractionPort ──

#[test]
fn unavailable_port_register_immediately_fails() {
    let port = UnavailableInteractionPort;
    let request = question_request();
    // Must fail immediately – no async, no hanging.
    assert_eq!(
        port.register(request).unwrap_err(),
        InteractionPortError::Unavailable
    );
}

#[test]
fn unavailable_port_all_methods_are_noop() {
    let port = UnavailableInteractionPort;
    let id = sdk::InteractionRequestId::new_v7();
    assert!(!port.contains(&id));
    assert_eq!(
        port.reply(&id, InteractionReply::HardPauseContinue),
        InteractionCommandOutcome::NotFound
    );
    assert_eq!(
        port.cancel(&id, InteractionCancelReason::UserCancelled),
        InteractionCommandOutcome::NotFound
    );
    assert_eq!(
        port.drain_run(&RunId::new_v7(), InteractionCancelReason::RunCancelled),
        0
    );
}

// ── Trait-object coverage: InteractionBridge through Arc<dyn InteractionPort> ──

#[test]
fn bridge_as_trait_object_register_and_reply() {
    let port: Arc<dyn InteractionPort> = Arc::new(InteractionBridge::new());
    let request = question_request();
    let waiter = port.register(request.clone()).unwrap();
    assert!(port.contains(&request.id));

    let outcome = port.reply(
        &request.id,
        InteractionReply::UserQuestions(vec![UserAnswer("是".to_string())]),
    );
    assert_eq!(outcome, InteractionCommandOutcome::Accepted);
    assert!(!port.contains(&request.id));

    // Reply again on completed ID
    assert_eq!(
        port.reply(
            &request.id,
            InteractionReply::UserQuestions(vec![UserAnswer("是".to_string())]),
        ),
        InteractionCommandOutcome::AlreadyCompleted
    );
    drop(waiter);
}

#[test]
fn bridge_as_trait_object_cancel_then_drain() {
    let port: Arc<dyn InteractionPort> = Arc::new(InteractionBridge::new());
    let request = question_request();
    let _waiter = port.register(request.clone()).unwrap();
    assert!(port.contains(&request.id));

    assert_eq!(
        port.cancel(&request.id, InteractionCancelReason::UserCancelled),
        InteractionCommandOutcome::Accepted
    );
    assert!(!port.contains(&request.id));
    // Drain on an empty run
    assert_eq!(
        port.drain_run(&request.run_id, InteractionCancelReason::RunCancelled),
        0
    );
}

#[test]
fn bridge_as_trait_object_drain_run_cancels_pending() {
    let port: Arc<dyn InteractionPort> = Arc::new(InteractionBridge::new());
    let run_id = RunId::new_v7();
    let r1 = InteractionRequest {
        id: sdk::InteractionRequestId::new_v7(),
        run_id: run_id.clone(),
        tool_call_id: None,
        body: InteractionRequestBody::HardPause(sdk::StuckDiagnostic {
            reason: "stuck".into(),
            recent_actions: vec![],
        }),
    };
    let r2 = InteractionRequest {
        id: sdk::InteractionRequestId::new_v7(),
        run_id: RunId::new_v7(),
        tool_call_id: None, // different run
        body: InteractionRequestBody::HardPause(sdk::StuckDiagnostic {
            reason: "other".into(),
            recent_actions: vec![],
        }),
    };

    let _w1 = port.register(r1.clone()).unwrap();
    let _w2 = port.register(r2.clone()).unwrap();
    assert_eq!(
        port.drain_run(&run_id, InteractionCancelReason::RunCancelled),
        1
    );
    assert!(!port.contains(&r1.id));
    assert!(port.contains(&r2.id)); // different run, not drained
}

// ── Shared validate_reply ──

#[test]
fn shared_validate_user_questions_accepts_matching() {
    let body = InteractionRequestBody::UserQuestions(vec![UserQuestion {
        prompt: "Q".into(),
        options: vec![],
        allow_multi: false,
    }]);
    let reply = InteractionReply::UserQuestions(vec![UserAnswer("A".into())]);
    assert!(validate_reply(&body, &reply).is_ok());
}

#[test]
fn shared_validate_user_questions_rejects_count_mismatch() {
    let body = InteractionRequestBody::UserQuestions(vec![
        UserQuestion {
            prompt: "Q1".into(),
            options: vec![],
            allow_multi: false,
        },
        UserQuestion {
            prompt: "Q2".into(),
            options: vec![],
            allow_multi: false,
        },
    ]);
    let reply = InteractionReply::UserQuestions(vec![UserAnswer("A1".into())]);
    assert_eq!(
        validate_reply(&body, &reply).unwrap_err(),
        InteractionReplyError::AnswerCountMismatch
    );
}

#[test]
fn shared_validate_rejects_variant_mismatch() {
    let body = InteractionRequestBody::UserQuestions(vec![UserQuestion {
        prompt: "Q".into(),
        options: vec![],
        allow_multi: false,
    }]);
    let reply = InteractionReply::HardPauseContinue;
    assert_eq!(
        validate_reply(&body, &reply).unwrap_err(),
        InteractionReplyError::VariantMismatch
    );
}
