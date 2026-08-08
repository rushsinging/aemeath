use super::events::{ChatEventSink, EventFuture, RuntimeRunContext, RuntimeStreamEvent};
use super::main_run_port::ChatToolRoundObserver;
use crate::application::run::run_factory_support::SessionRunFixture;
use crate::application::tool::agent::{ToolCall, ToolExecution};
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

fn task_execution_with_committed_change(store: &task::TaskStore) -> ToolExecution {
    let command_result = store
        .create_task(
            TaskCreateSpec::try_new(
                "second".to_owned(),
                String::new(),
                None,
                TaskPriority::Normal,
            )
            .unwrap(),
            3,
        )
        .unwrap();
    let call = ToolCall {
        id: sdk::ToolCallId::new_v7(),
        provider_id: "provider-call".to_owned(),
        name: "ordinary".to_owned(),
        index: 0,
        input: serde_json::json!({}),
    };
    ToolExecution::new(
        &call,
        tools::ToolOutcome::new("ok", serde_json::Value::Null, Vec::new()).with_task_change(
            tools::CommittedTaskChange::from_command_result(&command_result),
        ),
    )
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
    let observer = observer_with_task_store(sink.clone(), store.clone());
    let execution = task_execution_with_committed_change(store.as_ref());
    let call = ToolCall {
        id: execution.call_id.clone(),
        provider_id: execution.provider_id.clone(),
        name: execution.tool_name.clone(),
        index: 0,
        input: serde_json::json!({}),
    };
    let dispatcher = super::committed_side_effect::task_dispatcher(
        &observer.runtime_context,
        observer.session_id.clone(),
        observer.workspace_root.clone(),
    );

    dispatcher
        .observe(
            &call,
            &execution,
            &sdk::RunStepId::new_v7(),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await;

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

    observer.results_materialized(&execution).await;

    assert!(sink.states.lock().unwrap().is_empty());
}
