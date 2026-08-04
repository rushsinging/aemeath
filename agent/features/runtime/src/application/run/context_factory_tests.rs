//! RuntimeContextFactory 的单一生产装配契约测试。
//!
//! 这些测试只通过 RunFactory 创建 RunInstance；不在测试中复制
//! RuntimeContextFactory 的 capability 选择或 Context 装配算法。

use std::sync::Arc;

use crate::application::interaction::port::InteractionBridge;
use crate::application::loop_engine::chat::{ChatEventSink, RuntimeStreamEvent};
use crate::application::run::creation::{RunCreationRequest, SessionState};
use crate::application::run::factory::RunFactory;
use crate::application::run::run_factory_support::SessionRunFixture;
use crate::domain::agent_run::RunSpec;

fn main_spec() -> RunSpec {
    RunSpec::main()
}

#[test]
fn test_fixture_uses_the_production_run_factory_chain() {
    let fixture_source = include_str!("tests/run_factory_support.rs");
    let session_source = include_str!("tests/run_factory_support/session_run.rs");
    let derived_source = include_str!("tests/run_factory_support/derived_run.rs");

    for source in [fixture_source, session_source, derived_source] {
        assert!(!source.contains("RuntimeContext::new("));
        assert!(!source.contains(".prepare("));
        assert!(!source.contains("RunCapabilityBindings"));
    }
    assert!(session_source.contains("RunFactory::for_session"));
    assert!(session_source.contains(".create(request)"));
    assert!(derived_source.contains("RunFactory::for_parent"));
    assert!(derived_source.contains(".create(request)"));
}

#[test]
fn session_run_factory_preserves_committed_capability_bindings() {
    let interaction: Arc<dyn crate::application::interaction::port::InteractionPort> =
        Arc::new(InteractionBridge::new());
    let reasoning = Arc::new(std::sync::Mutex::new(provider::ReasoningLevel::High));
    let event_sink =
        crate::application::run::run_factory_support::doubles::RecordingEventSink::default();
    let fixture = SessionRunFixture::builder()
        .with_interaction(interaction.clone())
        .with_reasoning(reasoning.clone())
        .with_event_sink(event_sink.clone())
        .build();
    let instance = fixture
        .create(main_spec())
        .expect("production RunFactory should create a session instance");
    let context = instance.context();

    assert!(instance.run().parent_id().is_none());
    assert_eq!(
        instance.session().revision(),
        fixture.session_snapshot().revision()
    );
    assert!(Arc::ptr_eq(&context.context(), fixture.committed_context()));
    assert!(Arc::ptr_eq(&context.memory(), fixture.memory()));
    assert!(Arc::ptr_eq(&context.provider(), fixture.provider()));
    assert!(Arc::ptr_eq(&context.tool_catalog(), fixture.tool_catalog()));
    assert!(Arc::ptr_eq(
        &context.tool_execution(),
        fixture.tool_execution()
    ));
    assert!(Arc::ptr_eq(&context.policy(), fixture.policy()));
    assert!(Arc::ptr_eq(&context.hooks(), fixture.hooks()));
    assert!(Arc::ptr_eq(&context.interaction(), fixture.interaction()));
    assert!(Arc::ptr_eq(&context.reasoning(), fixture.reasoning()));
    assert_eq!(context.usage().get(), None);
    assert!(!context.input().is_sealed());
    assert!(!context.cancel().token().is_cancelled());

    let marker = RuntimeStreamEvent::SystemMessage("session-route-marker".to_string());
    context.event_sink().try_send_event(marker);
    assert!(matches!(
        event_sink.events().as_slice(),
        [RuntimeStreamEvent::SystemMessage(message)] if message == "session-route-marker"
    ));
}

#[test]
fn session_run_factory_creates_independent_per_run_resources() {
    let fixture = SessionRunFixture::default();
    let first = fixture.create(main_spec()).expect("create first run");
    let second = fixture.create(main_spec()).expect("create second run");

    assert!(Arc::ptr_eq(
        &first.context().reasoning(),
        &second.context().reasoning()
    ));
    first
        .context()
        .input()
        .push(sdk::ChatInputEvent::UserMessage {
            id: sdk::InputId::new_v7(),
            text: "first-only".to_string(),
            images: Vec::new(),
        });
    assert!(second
        .context()
        .input()
        .with_lock(|buffer| buffer.is_empty()));
    first.context().cancel().token().cancel();
    assert!(!second.context().cancel().token().is_cancelled());
    // usage tracker is now per-Session (shared across runs) — see
    // session_run_factory_shares_usage_tracker_across_runs
    first.context().usage().update(17);
    assert_eq!(second.context().usage().get(), Some(17));

    assert!(Arc::ptr_eq(
        &first.context().tool_execution(),
        &second.context().tool_execution()
    ));
    assert!(Arc::ptr_eq(
        &first.context().policy(),
        &second.context().policy()
    ));
}

#[test]
fn session_run_factory_shares_usage_tracker_across_runs() {
    let fixture = SessionRunFixture::default();
    let first = fixture.create(main_spec()).expect("create first run");
    let second = fixture.create(main_spec()).expect("create second run");

    // Per-Session tracker: second Run inherits the value set by the first,
    // so the compaction decision can use the last known API total instead
    // of falling back to a heuristic estimate on the first step.
    first.context().usage().update(42_000);
    assert_eq!(second.context().usage().get(), Some(42_000));

    // Mutations from the second Run are also visible to the first.
    second.context().usage().update(99_000);
    assert_eq!(first.context().usage().get(), Some(99_000));
}

#[test]
fn session_fixture_can_create_a_run_through_the_production_factory() {
    let fixture = SessionRunFixture::default();
    let instance = fixture
        .create(main_spec())
        .expect("production RunFactory should create a session instance");

    assert!(instance.run().parent_id().is_none());
    assert_eq!(
        instance.context().config().revision(),
        fixture.session_snapshot().config().revision()
    );
    assert_eq!(instance.session().session_id(), "test-session");
    assert_eq!(instance.session().revision(), fixture.session_revision());
}

#[test]
fn run_factory_create_accepts_only_the_creation_request() {
    let source = include_str!("factory.rs");
    let signature = source
        .split("pub(crate) fn create(")
        .nth(1)
        .and_then(|tail| tail.split(") -> Result<RunInstance").next())
        .expect("RunFactory::create signature");

    assert!(signature.contains("request: RunCreationRequest"));
    assert!(!signature.contains("RunCreationBindings"));
    assert!(!signature.contains("RunCapabilityBindings"));
    assert!(!signature.contains("RuntimeContext"));
    assert!(!signature.contains("parent:"));
    assert!(!source.contains("RunPreparer"));
    assert!(!source.contains("PreparedRun"));
}

#[test]
fn run_factory_depends_only_on_runtime_context_factory() {
    let source = include_str!("factory.rs");

    assert!(source.contains("context_factory: Arc<RuntimeContextFactory>"));
    assert!(!source.contains("RuntimeContextResolver"));
    assert!(!source.contains("context_resolver"));
}

#[test]
fn runtime_context_factory_owns_the_single_preparation_algorithm() {
    let source = include_str!("context_factory.rs");

    for retired in [
        "trait RuntimeContextResolver",
        "struct MainRunContextResolver",
        "struct SubRunContextResolver",
        "fn prepare_independent(",
        "fn prepare_derived(",
        "pub(crate) fn create(",
        "pub fn select_interaction(",
        "pub fn select_interaction_with_parent(",
        "pub fn select_hook(",
        "pub fn select_reasoning(",
    ] {
        assert!(
            !source.contains(retired),
            "retired or parallel preparation path remains: {retired}"
        );
    }
    assert!(source.contains("pub(crate) fn prepare("));
    assert!(source.contains("fn bind_runtime_context("));
}

#[test]
fn runtime_context_construction_is_factory_private() {
    let context_source = include_str!("context.rs");
    let factory_source = include_str!("context_factory.rs");

    assert!(!context_source.contains("new_for_test"));
    assert!(!context_source.contains("pub fn new(\n        services: RuntimeServices"));
    assert!(!factory_source.contains("pub fn assemble("));
    assert!(!factory_source.contains("pub fn create("));
}

#[test]
fn run_factory_without_bound_session_fails_closed() {
    let fixture = SessionRunFixture::default();
    let session = SessionState::new(
        "session-1",
        std::path::PathBuf::from("/workspace"),
        "test/test-model",
        share::config::domain::snapshot::ConfigSnapshot::new(share::config::Config::default()),
    );
    let request = RunCreationRequest::new(main_spec(), session.snapshot_for_run(), None).unwrap();
    let parent = fixture.create(main_spec()).expect("create parent run");
    let factory = RunFactory::for_parent(
        fixture.context_factory(),
        crate::application::run::creation::ParentRunBindings::from_active_run(
            Arc::new(parent.context().clone()),
            crate::application::run::workspace_test_support::test_runtime_workspace_access(),
        ),
    );

    assert!(matches!(
        factory.create(request),
        Err(crate::application::run::creation::RunCreationError::ContextAssembly)
    ));
}

#[test]
fn parent_value_facts_without_parent_bindings_fail_closed_at_request_boundary() {
    let parent_spec = main_spec();
    let parent_run_id = crate::domain::agent_run::RunId::new_v7();
    let session = SessionState::new(
        "session-sub",
        std::path::PathBuf::from("/workspace/sub"),
        "test-provider/test-model",
        share::config::domain::snapshot::ConfigSnapshot::new_with_revision(
            share::config::domain::snapshot::ConfigRevision::new(1),
            share::config::Config::default(),
        ),
    );
    let spec = parent_spec
        .derive_sub("coder", std::time::Duration::from_secs(30))
        .unwrap();
    let request = RunCreationRequest::new(
        spec,
        session.snapshot_for_run(),
        Some(crate::application::run::creation::ParentRunFacts::new(
            parent_run_id,
            parent_spec,
        )),
    )
    .unwrap();

    assert!(request.parent().is_some());
    assert!(!include_str!("creation.rs")
        .split("pub struct RunCreationRequest")
        .nth(1)
        .and_then(|tail| tail.split("impl RunCreationRequest").next())
        .expect("RunCreationRequest definition")
        .contains("ParentRunBindings"));
}
