use super::*;
use sdk::{
    InteractionCancelReason, InteractionCommandOutcome, InteractionReply, InteractionReplyError,
    InteractionRequest, InteractionRequestBody, RunId, UserAnswer, UserQuestion,
};

fn request() -> InteractionRequest {
    InteractionRequest {
        id: sdk::InteractionRequestId::new_v7(),
        run_id: RunId::new_v7(),
        body: InteractionRequestBody::UserQuestions(vec![UserQuestion {
            prompt: "继续？".to_string(),
            options: vec!["是".to_string()],
            allow_multi: false,
        }]),
    }
}

#[tokio::test]
async fn registered_waiter_accepts_matching_reply_exactly_once() {
    let bridge = InteractionBridge::new();
    let request = request();
    let waiter = bridge.register(request.clone()).unwrap();

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
fn dropped_waiter_reports_run_cancelling_instead_of_accepting_reply() {
    let bridge = InteractionBridge::new();
    let request = request();
    let waiter = bridge.register(request.clone()).unwrap();
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
fn invalid_reply_does_not_consume_waiter() {
    let bridge = InteractionBridge::new();
    let request = request();
    let _waiter = bridge.register(request.clone()).unwrap();

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
fn unknown_duplicate_and_run_drain_have_typed_outcomes() {
    let bridge = InteractionBridge::new();
    let unknown = sdk::InteractionRequestId::new_v7();
    assert_eq!(
        bridge.cancel(&unknown, InteractionCancelReason::UserCancelled),
        InteractionCommandOutcome::NotFound
    );

    let request = request();
    let duplicate = request.clone();
    let _waiter = bridge.register(request.clone()).unwrap();
    assert_eq!(
        bridge.register(duplicate).unwrap_err(),
        InteractionCommandOutcome::AlreadyCompleted
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
