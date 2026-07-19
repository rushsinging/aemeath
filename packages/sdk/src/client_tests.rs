use super::AgentClient;

fn assert_agent_client_commands<T: AgentClient + ?Sized>(client: &T) {
    let request_id = crate::InteractionRequestId::new_v7();
    let _ = client.reply_interaction(&request_id, crate::InteractionReply::HardPauseContinue);
    let _ = client.cancel_interaction(&request_id, crate::InteractionCancelReason::UserCancelled);
}

#[test]
fn agent_client_publishes_interaction_commands() {
    let signature = assert_agent_client_commands::<dyn AgentClient>;
    let _ = signature;
}
