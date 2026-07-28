use std::path::PathBuf;
use std::time::Duration;

use share::config::domain::snapshot::{ConfigRevision, ConfigSnapshot};
use share::config::Config;

use super::preparation::{ParentRunCapabilities, PreparedRun, RunPreparationRequest, SessionState};
use crate::domain::agent_run::{InteractionBindingMode, RunSpec, RunStatus};

fn config_snapshot(revision: u64) -> ConfigSnapshot {
    ConfigSnapshot::new_with_revision(ConfigRevision::new(revision), Config::default())
}

#[test]
fn snapshot_is_not_changed_by_later_session_updates() {
    let mut session = SessionState::new(
        "session-1",
        PathBuf::from("/workspace/one"),
        "model-a",
        config_snapshot(1),
    );
    let captured = session.snapshot_for_run();

    session.update_model("model-b", config_snapshot(2));
    session.update_workspace(PathBuf::from("/workspace/two"));

    assert_eq!(captured.session_id(), "session-1");
    assert_eq!(captured.workspace_root(), PathBuf::from("/workspace/one"));
    assert_eq!(captured.model_key(), "model-a");
    assert_eq!(captured.config().revision(), ConfigRevision::new(1));
    assert_eq!(captured.revision(), 0);

    let latest = session.snapshot_for_run();
    assert_eq!(latest.workspace_root(), PathBuf::from("/workspace/two"));
    assert_eq!(latest.model_key(), "model-b");
    assert_eq!(latest.config().revision(), ConfigRevision::new(2));
    assert_eq!(latest.revision(), 2);
}

#[test]
fn session_state_tracks_production_binding_model_identity() {
    let binding = crate::application::model::test_support::test_binding(vec!["response"]);
    let mut session = SessionState::new(
        "session-1",
        PathBuf::from("/workspace"),
        "stale-model",
        config_snapshot(1),
    );

    session.update_provider_binding(&binding, config_snapshot(2));

    let snapshot = session.snapshot_for_run();
    assert_eq!(
        snapshot.model_key(),
        format!("{}/{}", binding.model.provider, binding.model.model)
    );
    assert_eq!(snapshot.config().revision(), ConfigRevision::new(2));
}

#[test]
fn preparation_request_contains_spec_snapshot_and_parent_but_no_input() {
    let session = SessionState::new(
        "session-1",
        PathBuf::from("/workspace"),
        "model-a",
        config_snapshot(1),
    );
    let parent_spec = RunSpec::main();
    let child_spec = parent_spec
        .derive_sub("research", Duration::from_secs(30))
        .unwrap();
    let parent = ParentRunCapabilities::new(crate::domain::agent_run::RunId::new_v7(), parent_spec);

    let request = RunPreparationRequest::new(
        child_spec.clone(),
        session.snapshot_for_run(),
        Some(parent.clone()),
    )
    .unwrap();

    assert_eq!(request.spec(), &child_spec);
    assert_eq!(request.session().session_id(), "session-1");
    assert_eq!(request.parent(), Some(&parent));
}

#[test]
fn preparation_rejects_child_capability_above_parent_ceiling() {
    let session = SessionState::new(
        "session-1",
        PathBuf::from("/workspace"),
        "model-a",
        config_snapshot(1),
    );
    let parent = ParentRunCapabilities::new(
        crate::domain::agent_run::RunId::new_v7(),
        RunSpec::sub("restricted-parent", Duration::from_secs(30)),
    );
    let elevated = RunSpec::sub("child", Duration::from_secs(10))
        .with_interaction_kind(InteractionBindingMode::Client)
        .unwrap();

    let result = RunPreparationRequest::new(elevated, session.snapshot_for_run(), Some(parent));

    assert!(matches!(
        result,
        Err(super::preparation::RunPreparationError::CapabilityEscalation)
    ));
}

#[test]
fn prepared_run_consumes_into_parts_without_changing_snapshot() {
    let request = RunPreparationRequest::new(RunSpec::main(), session_snapshot(), None).unwrap();
    let expected_revision = request.session().revision();
    let prepared = PreparedRun::from_request(request);

    let (run, execution, session, context, workspace) = prepared.into_parts();

    assert_eq!(run.spec(), &RunSpec::main());
    assert_eq!(run.parent_id(), None);
    assert!(execution.messages().is_empty());
    assert_eq!(session.revision(), expected_revision);
    assert!(context.is_none());
    assert!(workspace.is_none());
}

#[test]
fn prepared_run_execution_can_be_initialized_after_preparation() {
    let prepared = PreparedRun::idle(RunSpec::main(), None, session_snapshot());
    let (run, mut execution, session, context, workspace) = prepared.into_parts();

    execution.initialize_for_launch(vec![share::message::Message::user("hello")], 3);

    assert_eq!(run.status(), RunStatus::Created);
    assert_eq!(execution.messages().len(), 1);
    assert_eq!(execution.turn_count(), 3);
    assert!(execution.elapsed() >= std::time::Duration::ZERO);
    assert_eq!(session.session_id(), "session-1");
    assert!(context.is_none());
    assert!(workspace.is_none());
}

#[test]
fn prepared_run_starts_created_with_empty_execution_state() {
    let prepared = PreparedRun::idle(RunSpec::main(), None, session_snapshot());

    assert_eq!(prepared.run().status(), RunStatus::Created);
    assert!(prepared.execution().messages().is_empty());
    assert_eq!(prepared.execution().turn_count(), 0);
    assert_eq!(prepared.session().session_id(), "session-1");
}

fn session_snapshot() -> super::preparation::SessionSnapshot {
    SessionState::new(
        "session-1",
        PathBuf::from("/workspace"),
        "model-a",
        config_snapshot(1),
    )
    .snapshot_for_run()
}
