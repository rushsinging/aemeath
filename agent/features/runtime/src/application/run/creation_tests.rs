use std::path::PathBuf;
use std::time::Duration;

use share::config::domain::snapshot::{ConfigRevision, ConfigSnapshot};
use share::config::Config;

use super::creation::{ParentRunFacts, RunCreationRequest, SessionState};
use crate::domain::agent_run::{InteractionBindingMode, RunSpec};

#[test]
fn p6_9_5_session_snapshot_contains_only_value_facts() {
    let source = include_str!("creation.rs");
    let snapshot = source
        .split("pub struct SessionSnapshot")
        .nth(1)
        .and_then(|tail| tail.split("impl SessionSnapshot").next())
        .expect("SessionSnapshot definition");

    for forbidden in [
        "SessionCapabilitySnapshot",
        "Arc<",
        "RuntimeWorkspaceAccess",
        "MainSessionWiring",
        "InteractionPort",
        "ProviderBinding",
        "Mutex<",
    ] {
        assert!(
            !snapshot.contains(forbidden),
            "SessionSnapshot contains live capability: {forbidden}"
        );
    }
}

#[test]
fn p6_9_5_parent_facts_are_separate_from_live_bindings() {
    let source = include_str!("creation.rs");
    let facts = source
        .split("pub struct ParentRunFacts")
        .nth(1)
        .and_then(|tail| tail.split("impl ParentRunFacts").next())
        .expect("ParentRunFacts definition");

    for forbidden in [
        "RuntimeContext",
        "RuntimeWorkspaceAccess",
        "Arc<",
        "Option<",
    ] {
        assert!(
            !facts.contains(forbidden),
            "ParentRunFacts contains live binding: {forbidden}"
        );
    }

    assert!(source.contains("pub(crate) struct ParentRunBindings"));
    assert!(!source.contains("facts: ParentRunFacts"));
}

#[test]
fn p6_12_run_creation_request_contains_only_value_inputs() {
    let source = include_str!("creation.rs");
    let request = source
        .split("pub struct RunCreationRequest")
        .nth(1)
        .and_then(|tail| tail.split("impl RunCreationRequest").next())
        .expect("RunCreationRequest definition");

    assert!(request.contains("session: SessionSnapshot"));
    assert!(request.contains("parent: Option<ParentRunFacts>"));
    for forbidden in [
        "bindings:",
        "RunCreationBindings",
        "SessionRunBindings",
        "ParentRunBindings",
        "RuntimeContext",
        "Arc<",
        "dyn ",
    ] {
        assert!(
            !request.contains(forbidden),
            "RunCreationRequest contains live binding: {forbidden}"
        );
    }
}

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
fn creation_request_contains_spec_snapshot_and_parent_but_no_input() {
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
    let parent = ParentRunFacts::new(crate::domain::agent_run::RunId::new_v7(), parent_spec);

    let request = RunCreationRequest::new(
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
fn creation_rejects_child_capability_above_parent_ceiling() {
    let session = SessionState::new(
        "session-1",
        PathBuf::from("/workspace"),
        "model-a",
        config_snapshot(1),
    );
    let parent = ParentRunFacts::new(
        crate::domain::agent_run::RunId::new_v7(),
        RunSpec::sub("restricted-parent", Duration::from_secs(30)),
    );
    let elevated = RunSpec::sub("child", Duration::from_secs(10))
        .with_interaction_kind(InteractionBindingMode::Client)
        .unwrap();

    let result = RunCreationRequest::new(elevated, session.snapshot_for_run(), Some(parent));

    assert!(matches!(
        result,
        Err(super::creation::RunCreationError::CapabilityEscalation)
    ));
}
