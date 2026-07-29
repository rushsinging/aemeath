use std::sync::Arc;

use sdk::{InteractionReply, InteractionRequest, InteractionRequestBody, RunId, StuckDiagnostic};

use super::{InteractionCommand, SessionIngress};
use crate::application::interaction::port::InteractionPort;

#[test]
fn interaction_command_is_dispatched_through_ingress() {
    let bridge = Arc::new(crate::application::interaction::port::InteractionBridge::new());
    let ingress = SessionIngress::new(bridge.clone());
    let request = InteractionRequest {
        id: sdk::InteractionRequestId::new_v7(),
        run_id: RunId::new_v7(),
        body: InteractionRequestBody::HardPause(StuckDiagnostic {
            reason: "test".to_string(),
            recent_actions: Vec::new(),
        }),
    };
    let _receiver = bridge
        .register(request.clone())
        .expect("register interaction");
    let outcome = ingress.dispatch_interaction(InteractionCommand::Reply {
        request_id: request.id,
        reply: InteractionReply::HardPauseContinue,
    });
    assert_eq!(outcome, sdk::InteractionCommandOutcome::Accepted);
}
