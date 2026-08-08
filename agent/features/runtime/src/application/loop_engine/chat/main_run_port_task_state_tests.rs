use super::events::{ChatEventSink, EventFuture, RuntimeRunContext, RuntimeStreamEvent};
use super::main_run_port::ChatToolRoundObserver;
use crate::application::run::run_factory_support::SessionRunFixture;
use crate::application::tool::coordination::ToolRoundObserver;
use crate::application::tool::tool_result_materializer::{
    ToolResultMaterializationPolicy, ToolResultMaterializer,
};
use crate::ports::{ToolResultBlobError, ToolResultBlobPort, ToolResultBlobRef};
use std::sync::{Arc, Mutex};
use task::{BatchCreateSpec, TaskAccess, TaskCreateSpec, TaskPriority};

#[derive(Clone, Default)]
struct TaskStateRecordingSink {
    states: Arc<Mutex<Vec<sdk::TaskStateView>>>,
}

impl ChatEventSink for TaskStateRecordingSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        self.try_send_event(event);
        Box::pin(std::future::ready(()))
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        if let RuntimeStreamEvent::TaskStateChanged { state } = event {
            self.states.lock().unwrap().push(*state);
        }
    }
}

#[derive(Default)]
struct UnusedBlobPort;

#[async_trait::async_trait]
impl ToolResultBlobPort for UnusedBlobPort {
    async fn write_once(
        &self,
        _session_id: &str,
        _tool_use_id: &str,
        _bytes: &[u8],
    ) -> Result<ToolResultBlobRef, ToolResultBlobError> {
        panic!("task state test must not persist tool-result blobs")
    }
}

fn materializer() -> Arc<ToolResultMaterializer> {
    Arc::new(ToolResultMaterializer::new(
        Arc::new(UnusedBlobPort),
        ToolResultMaterializationPolicy::new(1, 1, 0),
    ))
}

fn create_task(access: &dyn TaskAccess, subject: &str, now_ms: u64) {
    access
        .create_task(
            TaskCreateSpec::try_new(
                subject.to_owned(),
                String::new(),
                None,
                TaskPriority::Normal,
            )
            .unwrap(),
            now_ms,
        )
        .unwrap();
}

fn observer_with_task_store(
    sink: TaskStateRecordingSink,
    store: Arc<task::TaskStore>,
) -> ChatToolRoundObserver {
    let fixture = SessionRunFixture::builder()
        .with_event_sink_handle(super::events::ChatEventSinkHandle::new(sink))
        .with_context_factory(Arc::new(
            crate::application::run::context_factory::RuntimeContextFactory::new(
                Arc::new(crate::application::run::run_factory_support::doubles::FakeToolCatalog),
                Arc::new(crate::application::run::run_factory_support::doubles::FakeToolExecution),
                Arc::new(crate::application::run::run_factory_support::doubles::FakePolicyPort),
                Arc::new(
                    crate::application::run::run_factory_support::doubles::FakeReflectionHistory,
                ),
                store,
                Arc::new(crate::application::run::run_factory_support::doubles::FakeHookPort),
            ),
        ))
        .with_session_id("session-live")
        .build();
    let instance = fixture
        .create(crate::domain::agent_run::RunSpec::main())
        .expect("create main run context");

    ChatToolRoundObserver {
        runtime_context: instance.context().clone(),
        workspace_root: std::env::temp_dir(),
        turn_context: RuntimeRunContext::new(
            sdk::ChatId::new("chat-live"),
            sdk::ChatRunId::new("run-live"),
        ),
        session_id: "session-live".to_owned(),
        materializer: materializer(),
    }
}

#[tokio::test]
async fn task_mutation_round_publishes_one_final_complete_authoritative_state() {
    let sink = TaskStateRecordingSink::default();
    let store = Arc::new(task::TaskStore::new());
    store
        .create_batch(BatchCreateSpec::try_new("batch".to_owned()).unwrap(), 1)
        .unwrap();
    create_task(store.as_ref(), "first", 2);
    let mut observer = observer_with_task_store(sink.clone(), store.clone());
    let execution = crate::application::run::execution_state::RunExecutionState::new();

    create_task(store.as_ref(), "second", 3);
    observer.results_materialized(&execution, true).await;

    let states = sink.states.lock().unwrap();
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].session_id, "session-live");
    assert_eq!(states[0].revision, store.revision().get());
    assert_eq!(states[0].total, 2);
    assert_eq!(
        states[0]
            .items
            .iter()
            .map(|item| item.subject.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );
}

#[tokio::test]
async fn round_without_committed_task_mutation_publishes_no_task_state() {
    let sink = TaskStateRecordingSink::default();
    let store = Arc::new(task::TaskStore::new());
    let mut observer = observer_with_task_store(sink.clone(), store);
    let execution = crate::application::run::execution_state::RunExecutionState::new();

    observer.results_materialized(&execution, false).await;

    assert!(sink.states.lock().unwrap().is_empty());
}
