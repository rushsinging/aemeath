use super::{AgentClient, RunControlClient};

fn assert_agent_client_commands<T: AgentClient + ?Sized>(client: &T) {
    let deadline = crate::ControlDeadline::from_unix_millis(1_725_000_000_123);
    let _ = client.cancel_current_run(deadline);

    let request_id = crate::InteractionRequestId::new_v7();
    let _ = client.reply_interaction(&request_id, crate::InteractionReply::HardPauseContinue);
    let _ = client.cancel_interaction(&request_id, crate::InteractionCancelReason::UserCancelled);
}

fn assert_run_control_commands<T: RunControlClient + ?Sized>(client: &T) {
    let run_id = crate::RunId::new_v7();
    let step_id = crate::RunStepId::new_v7();
    let deadline = crate::ControlDeadline::from_unix_millis(1_725_000_000_123);
    let _ = client.cancel_run_step(&run_id, Some(&step_id), deadline);
    let _ = client.terminate_run(&run_id, crate::RunTerminationReason::UserExit, deadline);
}

#[test]
fn agent_client_only_publishes_current_run_control() {
    let signature = assert_agent_client_commands::<dyn AgentClient>;
    let _ = signature;
}

#[test]
fn run_control_client_publishes_addressable_management_commands() {
    let signature = assert_run_control_commands::<dyn RunControlClient>;
    let _ = signature;
}
