use super::*;
use context::SessionManagementError;
use std::collections::VecDeque;
use task::{BatchCreateSpec, TaskAccess, TaskCreateSpec, TaskPersist, TaskPriority};

#[derive(Clone)]
struct ResumeSessionManagement {
    session: context::session::CanonicalSession,
}

#[async_trait::async_trait]
impl context::SessionManagementPort for ResumeSessionManagement {
    async fn load_for_project(
        &self,
        _id: &str,
        _project: &share::session_types::ProjectIdentity,
    ) -> Result<context::session::CanonicalSession, SessionManagementError> {
        Ok(self.session.clone())
    }

    async fn list_for_project(
        &self,
        _project: &share::session_types::ProjectIdentity,
    ) -> Result<Vec<context::SessionListEntry>, SessionManagementError> {
        Ok(Vec::new())
    }

    async fn export_for_project(
        &self,
        _id: &str,
        _project: &share::session_types::ProjectIdentity,
    ) -> Result<Vec<u8>, SessionManagementError> {
        Err(SessionManagementError::Storage("unused".to_owned()))
    }

    async fn import_for_project(
        &self,
        _bytes: &[u8],
        _project: &share::session_types::ProjectIdentity,
    ) -> Result<context::SessionListEntry, SessionManagementError> {
        Err(SessionManagementError::Storage("unused".to_owned()))
    }

    async fn update_metadata_for_project(
        &self,
        _id: &str,
        _project: &share::session_types::ProjectIdentity,
        _update: context::SessionMetadataUpdate,
    ) -> Result<context::SessionListEntry, SessionManagementError> {
        Err(SessionManagementError::Storage("unused".to_owned()))
    }

    async fn delete_for_project(
        &self,
        _id: &str,
        _project: &share::session_types::ProjectIdentity,
    ) -> Result<(), SessionManagementError> {
        Err(SessionManagementError::Storage("unused".to_owned()))
    }
}

#[derive(Clone, Default)]
struct ResumeRecordingSink {
    events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
}

impl ChatEventSink for ResumeRecordingSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        self.try_send_event(event);
        Box::pin(std::future::ready(()))
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        if matches!(
            event,
            RuntimeStreamEvent::SessionResumed { .. }
                | RuntimeStreamEvent::TaskStateChanged { .. }
                | RuntimeStreamEvent::SessionResumeFailed { .. }
        ) {
            self.events.lock().unwrap().push(event);
        }
    }
}

#[derive(Clone)]
struct ResumeInputEvents {
    events: Arc<Mutex<VecDeque<sdk::ChatInputEvent>>>,
}

impl ResumeInputEvents {
    fn new(session_id: &str) -> Self {
        Self {
            events: Arc::new(Mutex::new(VecDeque::from([
                sdk::ChatInputEvent::ResumeSession {
                    id: session_id.to_owned(),
                },
            ]))),
        }
    }
}

impl crate::application::loop_engine::input_strategy::SessionInputPort for ResumeInputEvents {
    fn defer(&self, event: sdk::ChatInputEvent) {
        self.events.lock().unwrap().push_back(event);
    }
}

impl InputEventDrainPort for ResumeInputEvents {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
        Box::pin(async move { self.events.lock().unwrap().drain(..).collect() })
    }

    fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
        Box::pin(async move { self.events.lock().unwrap().pop_front() })
    }
}

fn restored_session(
    session_id: &str,
    workspace: share::session_types::PersistedWorkspaceContext,
    tasks: task::TaskSnapshot,
) -> context::session::CanonicalSession {
    let now = chrono::Utc::now().to_rfc3339();
    context::session::CanonicalSession {
        id: session_id.to_owned(),
        chats: Vec::new(),
        created_at: now.clone(),
        updated_at: now,
        metadata: Default::default(),
        tasks: context::session::SnapshotState::Captured(tasks),
        workspace: context::session::SnapshotState::Captured(workspace),
        revision: 9,
        compact: None,
        run_slices: Vec::new().into(),
        committed_steps: Default::default(),
        skill_load_records: Vec::new(),
    }
}

fn resumed_shell(
    task_store: Arc<task::TaskStore>,
    session_management: Arc<dyn context::SessionManagementPort>,
) -> crate::application::client::SessionRuntime {
    let mut shell = test_shell_with_task_store(noop_hook_port(), task_store.clone());
    let workspace = shell.workspace.clone();
    let config = Arc::new(config::ConfigAppService::new(Some(
        &workspace.read().initial_cwd(),
    )));
    shell.wiring = Arc::new(context::MainSessionWiring::build(
        context::MainSessionWiringBuilder {
            workspace_read: workspace.read(),
            workspace_persist: workspace.persist(),
            task_persist: task_store,
            config_reader: config.clone(),
            config_participant: config,
            memory_opener: Box::new(TestMemoryOpener),
            session_management,
            initial_session: shell.wiring.committed_session().as_ref().clone(),
            initial_memory: shell.wiring.committed_memory(),
            context_factory: Arc::new(context::ProductionMainContextFactory::new(Arc::new(
                context::NoOpCanonicalSessionWriter,
            ))),
        },
    ));
    shell
}

async fn run_resume(
    session_id: &str,
    shell: crate::application::client::SessionRuntime,
) -> ResumeRecordingSink {
    let sink = ResumeRecordingSink::default();
    let input = SessionCommandDriverInput {
        sink: sink.clone(),
        input_events: ResumeInputEvents::new(session_id),
        session: shell,
        read_files: Arc::new(Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(Mutex::new(::tools::SessionReminders::new())),
        session_queries: test_session_query_port(),
    };
    run_session_command_driver(input).await;
    sink
}

fn snapshot_with_one_task() -> task::TaskSnapshot {
    let store = task::TaskStore::new();
    store
        .create_batch(
            BatchCreateSpec::try_new("restored batch".to_owned()).unwrap(),
            1,
        )
        .unwrap();
    store
        .create_task(
            TaskCreateSpec::try_new(
                "restored task".to_owned(),
                String::new(),
                None,
                TaskPriority::High,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    store.collect_snapshot()
}

fn shell_workspace_snapshot() -> share::session_types::PersistedWorkspaceContext {
    project::wire_production_workspace(std::env::current_dir().unwrap())
        .expect("workspace")
        .into_views()
        .persist()
        .snapshot()
}

#[tokio::test]
async fn successful_resume_emits_session_then_same_complete_task_state_contract() {
    let session_id = "session-resumed";
    let task_store = Arc::new(task::TaskStore::new());
    let shell = resumed_shell(
        task_store,
        Arc::new(ResumeSessionManagement {
            session: restored_session(
                session_id,
                shell_workspace_snapshot(),
                snapshot_with_one_task(),
            ),
        }),
    );

    let sink = run_resume(session_id, shell).await;
    let events = sink.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0],
        RuntimeStreamEvent::SessionResumed { session_id: restored_id, .. }
            if restored_id == session_id
    ));
    match &events[1] {
        RuntimeStreamEvent::TaskStateChanged { state } => {
            assert_eq!(state.session_id, session_id);
            assert_eq!(state.total, 1);
            assert_eq!(state.items[0].subject, "restored task");
            assert_eq!(state.items[0].priority, sdk::TaskPriorityView::High);
        }
        other => panic!("expected TaskStateChanged after SessionResumed, got {other:?}"),
    }
}

#[tokio::test]
async fn successful_resume_without_active_batch_emits_empty_state_to_clear_old_session() {
    let session_id = "session-empty";
    let task_store = Arc::new(task::TaskStore::new());
    let shell = resumed_shell(
        task_store,
        Arc::new(ResumeSessionManagement {
            session: restored_session(
                session_id,
                shell_workspace_snapshot(),
                task::TaskSnapshot::empty(),
            ),
        }),
    );

    let sink = run_resume(session_id, shell).await;
    let events = sink.events.lock().unwrap();
    match &events[1] {
        RuntimeStreamEvent::TaskStateChanged { state } => {
            assert_eq!(state.session_id, session_id);
            assert!(state.current_batch.is_none());
            assert!(state.items.is_empty());
        }
        other => panic!("expected empty TaskStateChanged after SessionResumed, got {other:?}"),
    }
}
