use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::application::loop_engine::ScenarioLoopHarness;
use crate::application::run::active_registry::ActiveRunRegistry;
use crate::application::run::launcher::{self, RunLaunchResult};
use crate::application::run::run_factory_support::SessionRunFixture;
use crate::domain::agent_run::{RunSpec, RunStatus};

#[tokio::test]
async fn cancelled_step_returns_to_drain_and_seals_without_input() {
    let fixture = SessionRunFixture::default();
    let mut instance = fixture
        .create(RunSpec::main())
        .expect("production RunFactory creates session Run");
    instance.initialize(Vec::new(), 0);
    let mut harness = ScenarioLoopHarness::cancels_in_model_then_seals();
    let cancel = CancellationToken::new();
    let active_run = Arc::new(ActiveRunRegistry::default());
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        launcher::launch(&mut instance, cancel, active_run, &mut harness.run_loop()),
    )
    .await
    .expect("cancelled step must return to drain and seal the Run");

    assert!(matches!(result, RunLaunchResult::Terminal));
    assert_eq!(instance.run().status(), RunStatus::Completed);
    assert_eq!(harness.cancelled_terminal_event_count(), 1);
    assert_eq!(harness.completed_terminal_event_count(), 0);
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
    assert_eq!(harness.completed_terminal_event_count(), 1);
}
