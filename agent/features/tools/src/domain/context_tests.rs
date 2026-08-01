use super::*;
use crate::{SkillLoadDecision, SkillLoadMutation, SkillLoadStateError, SkillLoadStatePort};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn scope_fixture() -> ExecutionScope {
    ExecutionScope::builder(
        "run-910",
        project::WorkspaceId::from("workspace-910"),
        PathBuf::from("/workspace"),
    )
    .build()
}

#[test]
fn execution_scope_builder_preserves_required_identity() {
    let scope = scope_fixture();

    assert_eq!(scope.run_id(), "run-910");
    assert_eq!(
        scope.workspace_id(),
        &project::WorkspaceId::from("workspace-910")
    );
    assert_eq!(scope.workspace_root(), std::path::Path::new("/workspace"));
}

#[test]
fn execution_scope_builder_uses_main_run_defaults() {
    let scope = scope_fixture();

    assert_eq!(scope.parent_run_id(), None);
    assert_eq!(scope.invocation_source(), InvocationSource::MainRun);
    assert_eq!(scope.registry_scope().as_str(), "main");
    assert_eq!(scope.profile().as_str(), "main-full");
    assert_eq!(scope.deadline(), None);
}

#[test]
fn execution_scope_builder_preserves_parent_and_sub_agent_source() {
    let scope = ExecutionScope::builder(
        "child-run",
        project::WorkspaceId::from("workspace-910"),
        PathBuf::from("/workspace"),
    )
    .parent_run_id("parent-run")
    .invocation_source(InvocationSource::SubAgent)
    .build();

    assert_eq!(scope.parent_run_id(), Some("parent-run"));
    assert_eq!(scope.invocation_source(), InvocationSource::SubAgent);
}

#[test]
fn execution_scope_builder_preserves_registry_profile_and_deadline() {
    let deadline = SystemTime::now() + Duration::from_secs(30);
    let scope = ExecutionScope::builder(
        "run-910",
        project::WorkspaceId::from("workspace-910"),
        PathBuf::from("/workspace"),
    )
    .registry_scope(RegistryScopeName::new("sub-agent"))
    .profile(ToolProfileName::new("readonly"))
    .deadline(deadline)
    .build();

    assert_eq!(scope.registry_scope().as_str(), "sub-agent");
    assert_eq!(scope.profile().as_str(), "readonly");
    assert_eq!(scope.deadline(), Some(deadline));
}

#[test]
fn tool_execution_context_exposes_scope_and_read_only_workspace_port() {
    let ctx = crate::domain::test_support::TestToolExecutionContextBuilder::new(PathBuf::from(
        "/workspace",
    ))
    .build();

    assert_eq!(ctx.scope().run_id(), "test-run");
    assert_eq!(
        ctx.scope().workspace_id(),
        &ctx.workspace_read().workspace_id()
    );
    assert_eq!(
        ctx.scope().workspace_root(),
        ctx.workspace_read().current_workspace_root()
    );
    assert!(ctx.agent_dispatch().is_none());
    assert!(ctx.catalog_query().is_none());
    assert!(ctx.progress_sink().is_none());
    assert!(!ctx.cancellation().is_cancelled());
}

#[test]
fn tool_execution_context_can_replace_invocation_cancellation_without_changing_scope() {
    struct Cancelled;
    #[async_trait::async_trait]
    impl CancellationSignal for Cancelled {
        fn is_cancelled(&self) -> bool {
            true
        }
        async fn cancelled(&self) {}
        fn child_signal(&self) -> std::sync::Arc<dyn CancellationSignal> {
            std::sync::Arc::new(Self)
        }
    }

    let original = crate::domain::test_support::TestToolExecutionContextBuilder::new(
        PathBuf::from("/workspace"),
    )
    .build();
    let replaced = original.with_cancellation(std::sync::Arc::new(Cancelled));

    assert_eq!(replaced.scope(), original.scope());
    assert!(replaced.cancellation().is_cancelled());
    assert!(!original.cancellation().is_cancelled());
}

#[test]
fn tool_execution_context_read_set_records_evidence_through_port() {
    let ctx = crate::domain::test_support::TestToolExecutionContextBuilder::new(PathBuf::from(
        "/workspace",
    ))
    .build();

    assert!(!ctx.read_set().contains("src/lib.rs"));
    ctx.read_set().record("src/lib.rs");
    assert!(ctx.read_set().contains("src/lib.rs"));
}

#[test]
fn tool_execution_context_exposes_skill_load_scope_and_state_port() {
    struct State;

    #[async_trait::async_trait]
    impl SkillLoadStatePort for State {
        async fn compare_and_record(
            &self,
            _mutation: SkillLoadMutation,
        ) -> Result<SkillLoadDecision, SkillLoadStateError> {
            Ok(SkillLoadDecision::Fresh)
        }
    }

    let expected_scope = SkillLoadScope::subagent("agent-1").unwrap();
    let ctx = crate::domain::test_support::TestToolExecutionContextBuilder::new(PathBuf::from(
        "/workspace",
    ))
    .build();
    let ctx = ctx.with_skill_load_state(expected_scope.clone(), std::sync::Arc::new(State));

    assert_eq!(ctx.skill_load_scope(), Some(&expected_scope));
    assert!(ctx.skill_load_state().is_some());
}
