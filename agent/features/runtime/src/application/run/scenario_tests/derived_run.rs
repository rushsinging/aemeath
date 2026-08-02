use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::application::loop_engine::chat::{ChatEventSink, RuntimeStreamEvent};
use crate::application::loop_engine::ScenarioLoopHarness;
use crate::application::run::active_registry::ActiveRunRegistry;
use crate::application::run::launcher::{self, RunLaunchResult};
use crate::application::run::run_factory_support::derived_run::ParentRunFixture;
use crate::application::run::run_factory_support::SessionRunFixture;
use crate::domain::agent_run::{RunSpec, RunStatus};

fn derived_run_config() -> share::config::domain::snapshot::ConfigSnapshot {
    let mut config = share::config::Config::default();
    config.agents.roles.insert(
        "derived".to_string(),
        share::config::AgentRoleConfig {
            model: "test-provider/test-model".to_string(),
            ..Default::default()
        },
    );
    config.models.default = "test-provider/test-model".to_string();
    config.models.providers.insert(
        "test-provider".to_string(),
        share::config::models::ProviderModelsConfig {
            driver: "openai".to_string(),
            models: vec![share::config::models::ModelEntryConfig {
                id: "test-model".to_string(),
                context_window: 128_000,
                max_tokens: 8192,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    share::config::domain::snapshot::ConfigSnapshot::new_with_revision(
        share::config::domain::snapshot::ConfigRevision::new(1),
        config,
    )
}

#[tokio::test]
async fn derived_run_uses_parent_factory_launcher_and_same_loop() {
    let session_fixture = SessionRunFixture::builder()
        .with_config(derived_run_config())
        .build();
    let parent = session_fixture
        .create(RunSpec::main())
        .expect("production RunFactory creates parent Run");
    let parent_run_id = parent.run().id().clone();
    let parent_spec = parent.run().spec().clone();
    let parent_context = Arc::new(parent.context().clone());
    let parent_workspace = session_fixture.workspace().clone();
    let derived_spec = parent_spec
        .derive_sub("derived", Duration::from_secs(30))
        .expect("parent ceiling permits restricted Derived Run");
    let mut derived = ParentRunFixture::new(session_fixture.context_factory())
        .create(
            derived_spec.clone(),
            session_fixture.session_snapshot(),
            parent_run_id.clone(),
            parent_spec,
            parent_context.clone(),
            parent_workspace,
        )
        .expect("same production RunFactory creates Derived Run");
    derived.initialize(Vec::new(), 0);
    let mut harness = ScenarioLoopHarness::completes_with("derived done");
    let cancel = CancellationToken::new();
    let active_run = Arc::new(ActiveRunRegistry::default());

    let result = launcher::launch(&mut derived, cancel, active_run, &mut harness.run_loop()).await;

    assert!(matches!(result, RunLaunchResult::Terminal));
    assert_eq!(derived.run().status(), RunStatus::Completed);
    assert_eq!(derived.run().parent_id(), Some(&parent_run_id));
    assert_eq!(derived.run().spec(), &derived_spec);
    assert!(!Arc::ptr_eq(
        &derived.context().context(),
        &parent_context.context()
    ));
    assert!(!Arc::ptr_eq(
        &derived.context().interaction(),
        &parent_context.interaction()
    ));
    derived
        .context()
        .event_sink()
        .try_send_event(RuntimeStreamEvent::SystemMessage(
            "derived-route-probe".to_string(),
        ));
    assert!(session_fixture.event_sink().events().is_empty());
    assert!(harness.saw_input_and_model());
    assert_eq!(harness.terminal_event_count(), 1);
}
