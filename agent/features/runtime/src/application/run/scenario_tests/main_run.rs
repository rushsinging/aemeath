use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::application::loop_engine::ScenarioLoopHarness;
use crate::application::run::active_registry::ActiveRunRegistry;
use crate::application::run::launcher::{self, RunLaunchResult};
use crate::application::run::run_factory_support::SessionRunFixture;
use crate::domain::agent_run::{RunSpec, RunStatus};

#[tokio::test]
async fn session_run_launcher_registers_identity_free_cancel_target() {
    let fixture = SessionRunFixture::default();
    let mut instance = fixture
        .create(RunSpec::main())
        .expect("production RunFactory creates session Run");
    instance.initialize(Vec::new(), 0);
    let mut harness = ScenarioLoopHarness::blocks_in_model();
    let cancel = CancellationToken::new();
    let active_run = Arc::new(ActiveRunRegistry::default());
    let control = active_run.clone();
    harness.use_active_run_control(active_run.clone());
    let model_started = harness.model_started();
    let deadline = sdk::ControlDeadline::from_unix_millis(1_725_000_000_123);

    let cancel_task = tokio::task::spawn_blocking(move || {
        model_started.wait();
        control.cancel_current_main(deadline)
    });
    let result = launcher::launch(&mut instance, cancel, active_run, &mut harness.run_loop()).await;

    assert_eq!(
        cancel_task.await.expect("cancel task completes"),
        sdk::CancelCurrentRunOutcome::Accepted
    );
    assert!(matches!(result, RunLaunchResult::Terminal));
    assert_eq!(instance.run().status(), RunStatus::Completed);
    assert_eq!(harness.cancelled_step_count(), 1);
    assert_eq!(harness.completed_terminal_event_count(), 1);
}

#[tokio::test]
async fn main_run_uses_single_factory_launcher_and_loop() {
    let fixture = SessionRunFixture::default();
    let committed_context = fixture.committed_context().clone();
    let mut instance = fixture
        .create(RunSpec::main())
        .expect("production RunFactory creates Main Run");
    instance.initialize(Vec::new(), 0);
    let mut harness = ScenarioLoopHarness::completes_with("main done");
    let cancel = CancellationToken::new();
    let active_run = Arc::new(ActiveRunRegistry::default());

    let result = launcher::launch(&mut instance, cancel, active_run, &mut harness.run_loop()).await;

    assert!(matches!(result, RunLaunchResult::Terminal));
    assert_eq!(instance.run().status(), RunStatus::Completed);
    assert!(Arc::ptr_eq(
        &instance.context().context(),
        &committed_context
    ));
    assert!(harness.saw_input_and_model());
    assert_eq!(harness.terminal_event_count(), 1);
}
