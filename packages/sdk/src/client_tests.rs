use super::AgentClient;

fn assert_agent_client_commands<T: AgentClient + ?Sized>(client: &T) {
    let run_id = crate::RunId::new_v7();
    let step_id = crate::RunStepId::new_v7();
    let deadline = crate::ControlDeadline::from_unix_millis(1_725_000_000_123);
    let _ = client.cancel_run(&run_id);
    let _ = client.cancel_run_step(&run_id, Some(&step_id), deadline);
    let _ = client.terminate_run(&run_id, crate::RunTerminationReason::UserExit, deadline);

    let request_id = crate::InteractionRequestId::new_v7();
    let _ = client.reply_interaction(&request_id, crate::InteractionReply::HardPauseContinue);
    let _ = client.cancel_interaction(&request_id, crate::InteractionCancelReason::UserCancelled);
}

#[test]
fn agent_client_publishes_main_run_control_commands() {
    let signature = assert_agent_client_commands::<dyn AgentClient>;
    let _ = signature;
}
