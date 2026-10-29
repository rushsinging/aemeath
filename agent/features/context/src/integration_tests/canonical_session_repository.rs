use share::ids::{InteractionRequestId, RunStepId, SessionId};
use std::sync::{Arc, Mutex, RwLock};

use crate::domain::session::SessionSnapshotState;
use crate::SessionRepository;
use crate::{
    AcceptedInputAppendData, AcceptedInputError, CompactRequestData, CompactTrigger,
    ContextAppendData, ContextAppendError, ContextRequestData, FinalizeCause, Language,
    SessionRevision, ToolCallIdentityData, ToolReceiptMutationData,
};
use crate::{
    AcceptedInputRecord, CanonicalSession, ChatSegment, CommittedRunSlice, CommittedRunStep,
    SessionCommitPlan,
};
use crate::{CanonicalSessionRepository, CanonicalSessionWriter};
use async_trait::async_trait;
use project::{WorkspaceRestoreData, WorkspaceWriter};
use sdk::RunId;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::Message;
use share::reasoning::ReasoningLevel;
use share::session_types::{
    PersistedWorkspaceContext, ProjectIdentityData, WorkspaceId, WorktreeKind,
};
use task::{PreparedTaskRestoreData, TaskPersist, TaskSnapshotData};

use crate::tool_receipt::ToolCallState;
use tools::published::skill::{
    SkillLoadDecision, SkillLoadMutation, SkillLoadScope, SkillLoadStateError,
};

#[derive(Debug)]
struct RecordedSessionCommit {
    session_id: String,
    expected_revision: u64,
    plan: SessionCommitPlan,
}

#[derive(Default)]
struct RecordingWriter {
    saved: Mutex<Vec<RecordedSessionCommit>>,
    fail: bool,
    /// 模拟数据集被外部清空：增量 commit 一律失败，仅全量重建可成功。
    wiped: bool,
    rebuilt: Mutex<Vec<(String, u64)>>,
}

#[async_trait]
impl CanonicalSessionWriter for RecordingWriter {
    async fn commit(
        &self,
        session_id: &str,
        expected_revision: u64,
        plan: SessionCommitPlan,
    ) -> Result<(), String> {
        if self.wiped {
            return Err(format!(
                "Session 数据集修订号已变更: expected={expected_revision}, actual=0"
            ));
        }
        if self.fail {
            return Err("disk full".to_string());
        }
        if plan.changed_members().is_empty() {
            return Err("missing changed member".to_string());
        }
        self.saved.lock().unwrap().push(RecordedSessionCommit {
            session_id: session_id.to_string(),
            expected_revision,
            plan,
        });
        Ok(())
    }

    async fn rebuild_empty_dataset(
        &self,
        session_id: &str,
        session: &CanonicalSession,
    ) -> Result<(), String> {
        if !self.wiped {
            return Err("Session 数据集非空，拒绝全量重建".to_string());
        }
        self.rebuilt
            .lock()
            .unwrap()
            .push((session_id.to_string(), session.revision));
        Ok(())
    }
}

struct EmptyTask;

impl TaskPersist for EmptyTask {
    fn collect_snapshot(&self) -> TaskSnapshotData {
        TaskSnapshotData::empty()
    }

    fn prepare_restore(
        &self,
        snapshot: &TaskSnapshotData,
    ) -> Result<PreparedTaskRestoreData, share::error::DomainError> {
        task::wire_task().persist().prepare_restore(snapshot)
    }

    fn commit_restore(&self, _token: PreparedTaskRestoreData) {}
}

struct FixedWorkspace(PersistedWorkspaceContext);
impl WorkspaceWriter for FixedWorkspace {
    fn snapshot(&self) -> PersistedWorkspaceContext {
        self.0.clone()
    }

    fn prepare_restore(
        &self,
        _dto: &PersistedWorkspaceContext,
    ) -> Result<WorkspaceRestoreData, share::error::DomainError> {
        panic!("not used")
    }

    fn commit_restore(&self, _prepared: WorkspaceRestoreData) {
        panic!("not used")
    }
}

fn workspace() -> PersistedWorkspaceContext {
    let project_identity = ProjectIdentityData {
        initial_cwd: "/tmp/project".to_string(),
        git_common_dir: None,
    };
    PersistedWorkspaceContext {
        workspace_id: WorkspaceId::derive(&project_identity, "/tmp/project"),
        project_identity,
        path_base: "/tmp/project".to_string(),
        workspace_root: "/tmp/project".to_string(),
        worktree_kind: WorktreeKind::Primary,
        context_stack: vec![],
    }
}

fn append() -> ContextAppendData {
    ContextAppendData {
        session_id: SessionId::new("session"),
        expected_revision: SessionRevision::new(0),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        source_request_id: InteractionRequestId::new("request"),
        finalize_cause: FinalizeCause::Completed,
        duration_ms: None,
        messages: vec![Message::user("fact")],
        receipts: vec![],
        api_input_tokens: None,
    }
}

fn tool_identity() -> ToolCallIdentityData {
    ToolCallIdentityData {
        session_id: SessionId::new("session"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        runtime_call_id: "call-1".to_string(),
        provider_call_id: Some("provider-1".to_string()),
        tool_name: "Glob".to_string(),
        call_index: 0,
        agent: false,
    }
}

fn accepted_input() -> AcceptedInputAppendData {
    AcceptedInputAppendData {
        session_id: SessionId::new("session"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        source_request_id: InteractionRequestId::new("request"),
        messages: vec![Message::user("accepted fact")],
    }
}

/// 测试用固定文本 LLM 端口（#1863 反转后的 generator mock 替身）。
struct FixedTextInvoker {
    text: String,
    started: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    release: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<()>>>,
    fail_with_cancel: bool,
}

impl FixedTextInvoker {
    /// #2146：LlmInvokePort 只允许 provider 实现；测试经 provider 异步脚本。
    fn port(&self) -> std::sync::Arc<dyn provider::LlmInvokePort> {
        let text = self.text.clone();
        let started = self.started.clone();
        let release = self.release.clone();
        let fail_with_cancel = self.fail_with_cancel;
        provider::test_support::scripted_llm_async(move |_request| {
            let text = text.clone();
            let started = started.clone();
            let release = release.clone();
            Box::pin(async move {
                if let Some(started) = started.lock().unwrap().take() {
                    let _ = started.send(());
                }
                release.lock().await.recv().await;
                if fail_with_cancel {
                    return Err(provider::ProviderError::cancelled());
                }
                Ok(provider::test_support::text_stream(&text))
            })
        })
    }
}

/// 构造测试 binding（fixed invoker）。
fn test_llm_binding(
    invoker: std::sync::Arc<dyn provider::LlmInvokePort>,
) -> Option<crate::domain::CompactLlmBinding> {
    Some(crate::domain::CompactLlmBinding {
        invoker,
        model: provider::ModelInfo {
            provider: "test".to_string(),
            model: "compact-test".to_string(),
            supports_tools: false,
            supports_parallel_tool_calls: false,
            supports_streaming: true,
            supported_reasoning: vec![share::reasoning::ReasoningLevel::Off],
            context_limit: Some(100_000),
            output_limit: Some(4096),
            server_effort_map: Default::default(),
        },
        max_output_tokens: 1024,
        compact_context_window: None,
        reasoning: share::reasoning::ReasoningLevel::Off,
    })
}

/// 测试用 provider 失败端口（熔断测试：每次调用计数并报错）。
struct ProviderFailingInvoker {
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

impl ProviderFailingInvoker {
    /// #2146：LlmInvokePort 只允许 provider 实现；测试经 provider 脚本。
    fn port(&self) -> std::sync::Arc<dyn provider::LlmInvokePort> {
        use std::sync::atomic::Ordering;
        let calls = Arc::clone(&self.calls);
        provider::test_support::scripted_llm(move |_request| {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(provider::ProviderError::fatal(
                provider::ProviderErrorKind::UpstreamUnavailable,
                "provider failed",
            ))
        })
    }
}

fn valid_fact_batch(objective: &str) -> String {
    serde_json::json!({
        "facts": [
            {
                "sequence": 1,
                "source": "main_user",
                "kind": "constraint",
                "text": "preserve constraints",
                "constraint": {
                    "scope": "session",
                    "lifecycle": "persistent",
                    "action": "restrict"
                }
            },
            {
                "sequence": 2,
                "source": "main_user",
                "kind": "objective",
                "text": objective
            },
            {
                "sequence": 3,
                "source": "main_user",
                "kind": "resume_candidate",
                "text": "continue"
            }
        ]
    })
    .to_string()
}

fn compact_request(session_id: SessionId) -> ContextRequestData {
    ContextRequestData {
        session_id,
        request_id: InteractionRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![],
        workspace_root: std::path::PathBuf::from("/tmp/workspace"),
        workspace_is_git: false,
        model_id: "fake/model".to_string(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::Zh,
        agent_roles: Default::default(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 1,
        max_output_tokens: 1,
        last_api_total_tokens: Some(100),
        heuristic_calibration: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

fn repository_with_session(
    writer: Arc<dyn CanonicalSessionWriter>,
    session: CanonicalSession,
) -> (
    CanonicalSessionRepository,
    Arc<RwLock<Arc<CanonicalSession>>>,
) {
    let holder = Arc::new(RwLock::new(Arc::new(session)));
    (
        CanonicalSessionRepository::new(
            holder.clone(),
            Arc::new(EmptyTask),
            Arc::new(FixedWorkspace(workspace())),
            writer,
            Arc::new(tokio::sync::Mutex::new(())),
        ),
        holder,
    )
}

fn repository(
    writer: Arc<RecordingWriter>,
) -> (
    CanonicalSessionRepository,
    Arc<RwLock<Arc<CanonicalSession>>>,
) {
    let session_id = SessionId::new("session");
    repository_with_session(
        writer,
        CanonicalSession {
            id: session_id.to_string(),
            chats: vec![],
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            metadata: Default::default(),
            tasks: SessionSnapshotState::Missing,
            workspace: SessionSnapshotState::Captured(workspace()),
            revision: 0,
            compact: None,
            cleared_after: None,
            run_slices: vec![].into(),
            committed_steps: Default::default(),
            skill_load_records: Vec::new(),
        },
    )
}

fn ten_step_slices() -> Vec<CommittedRunSlice> {
    (0..10)
        .map(|index| {
            CommittedRunSlice::new(
                format!("run-{index}"),
                vec![CommittedRunStep::compatibility_outcome_only(
                    format!("step-{index}"),
                    vec![Message::user(format!("message-{index}"))],
                )],
            )
        })
        .collect()
}

fn ten_step_session(
    session_id: &SessionId,
    chats: Vec<ChatSegment>,
    revision: u64,
) -> CanonicalSession {
    CanonicalSession {
        id: session_id.to_string(),
        chats,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SessionSnapshotState::Missing,
        workspace: SessionSnapshotState::Captured(workspace()),
        revision,
        compact: None,
        cleared_after: None,
        run_slices: ten_step_slices().into(),
        committed_steps: Default::default(),
        skill_load_records: Vec::new(),
    }
}

async fn compact(repository: &CanonicalSessionRepository, session_id: SessionId, revision: u64) {
    let mut request = compact_request(session_id);
    request.context_size = 100_000;
    let outcome = repository
        .commit_compaction(&CompactRequestData {
            llm: None,
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(revision),
            source: request,
            trigger: CompactTrigger::Automatic,
            progress: None,
            task_snapshot: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
        })
        .await
        .unwrap();
    assert!(matches!(outcome, crate::CompactOutcome::Committed(_)));
}

#[tokio::test]
async fn skill_load_revision_is_atomic_idempotent_and_failure_safe() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository_under_test, holder) = repository(writer.clone());
    let session_id = holder.read().unwrap().id.clone();
    let mutation = SkillLoadMutation::new(
        session_id,
        SkillLoadScope::main(),
        "superpowers:brainstorming",
        "r1",
    )
    .unwrap();

    assert_eq!(
        repository_under_test
            .compare_and_record_skill_load(mutation.clone())
            .await
            .unwrap(),
        SkillLoadDecision::Fresh
    );
    assert_eq!(holder.read().unwrap().revision, 1);
    assert_eq!(writer.saved.lock().unwrap().len(), 1);

    assert_eq!(
        repository_under_test
            .compare_and_record_skill_load(mutation)
            .await
            .unwrap(),
        SkillLoadDecision::AlreadyLoaded
    );
    assert_eq!(holder.read().unwrap().revision, 1);
    assert_eq!(writer.saved.lock().unwrap().len(), 1);

    let updated = SkillLoadMutation::new(
        holder.read().unwrap().id.clone(),
        SkillLoadScope::main(),
        "superpowers:brainstorming",
        "r2",
    )
    .unwrap();
    assert_eq!(
        repository_under_test
            .compare_and_record_skill_load(updated)
            .await
            .unwrap(),
        SkillLoadDecision::Updated
    );
    assert_eq!(holder.read().unwrap().revision, 2);
    assert_eq!(
        holder
            .read()
            .unwrap()
            .loaded_skill_revision(&SkillLoadScope::main(), "superpowers:brainstorming"),
        Some("r2")
    );

    let failing_writer = Arc::new(RecordingWriter {
        saved: Mutex::new(Vec::new()),
        fail: true,
        ..RecordingWriter::default()
    });
    let (failing, failing_holder) = repository(failing_writer);
    let failing_session_id = failing_holder.read().unwrap().id.clone();
    let failed = failing
        .compare_and_record_skill_load(
            SkillLoadMutation::new(failing_session_id, SkillLoadScope::main(), "review", "r1")
                .unwrap(),
        )
        .await;
    assert!(matches!(
        failed,
        Err(SkillLoadStateError::Storage(message)) if message == "disk full"
    ));
    assert_eq!(failing_holder.read().unwrap().revision, 0);
    assert!(failing_holder.read().unwrap().skill_load_records.is_empty());
}

#[tokio::test]
async fn skill_load_rejects_foreign_session_identity() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let error = repository
        .compare_and_record_skill_load(
            SkillLoadMutation::new("foreign", SkillLoadScope::main(), "review", "r1").unwrap(),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        SkillLoadStateError::SessionNotFound("foreign".to_string())
    );
    assert!(holder.read().unwrap().skill_load_records.is_empty());
}

#[tokio::test]
async fn clear_removes_persisted_skill_load_records() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let session_id = holder.read().unwrap().id.clone();
    repository
        .compare_and_record_skill_load(
            SkillLoadMutation::new(session_id.clone(), SkillLoadScope::main(), "review", "r1")
                .unwrap(),
        )
        .await
        .unwrap();

    repository
        .clear(&SessionId::new(&session_id))
        .await
        .unwrap();

    assert!(holder.read().unwrap().skill_load_records.is_empty());
}

/// #1697：compact 替代窗口后 skill 注入内容不再可见，`skill_load_records`
/// 的去重前提失效——compact commit 必须清空记录，让再次调用返回 `Fresh`
/// 重新注入内容。旧断言「compact 保留」的行为被该设计决策替代（原测试
/// `compaction_preserves_skill_load_records`）：保留会让 `AlreadyLoaded`
/// 在内容已丢失时拒绝重发，模型拿不回 skill 内容。
#[tokio::test]
async fn compaction_clears_skill_load_records() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    // 十步会话保证 compact 走到 Committed（空历史会 Skipped，清空逻辑不执行）
    let (repository, holder) =
        repository_with_session(writer, ten_step_session(&session_id, vec![], 0));
    repository
        .compare_and_record_skill_load(
            SkillLoadMutation::new(
                session_id.to_string(),
                SkillLoadScope::main(),
                "review",
                "r1",
            )
            .unwrap(),
        )
        .await
        .unwrap();
    // 强断言 compact 已提交，避免 Skipped 路径让清空断言空过
    // （context_size=1 的默认请求会让尾部保留吞掉全部消息而 Skipped）
    let mut source = compact_request(SessionId::new(&session_id));
    source.context_size = 100_000;
    assert!(matches!(
        repository
            .commit_compaction(&CompactRequestData {
                llm: None,
                run_id: RunId::new("run"),
                source_revision: SessionRevision::new(1),
                source,
                trigger: CompactTrigger::Automatic,
                progress: None,
                task_snapshot: None,
                cancellation: tokio_util::sync::CancellationToken::new(),
            })
            .await
            .unwrap(),
        crate::CompactOutcome::Committed(_)
    ));

    assert!(
        holder
            .read()
            .unwrap()
            .loaded_skill_revision(&SkillLoadScope::main(), "review")
            .is_none(),
        "compact 替代窗口后注入记录必须失效"
    );
    assert_eq!(
        repository
            .compare_and_record_skill_load(
                SkillLoadMutation::new(
                    session_id.to_string(),
                    SkillLoadScope::main(),
                    "review",
                    "r1"
                )
                .unwrap()
            )
            .await
            .unwrap(),
        SkillLoadDecision::Fresh,
        "compact 后同 revision 重新加载必须返回 Fresh 以重发内容"
    );
}

#[tokio::test]
async fn concurrent_skill_load_records_return_one_fresh_decision() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let repository = Arc::new(repository);
    let mutation = SkillLoadMutation::new(
        holder.read().unwrap().id.clone(),
        SkillLoadScope::main(),
        "review",
        "r1",
    )
    .unwrap();
    let first = {
        let repository = repository.clone();
        let mutation = mutation.clone();
        tokio::spawn(async move {
            repository
                .compare_and_record_skill_load(mutation)
                .await
                .unwrap()
        })
    };
    let second = {
        let repository = repository.clone();
        tokio::spawn(async move {
            repository
                .compare_and_record_skill_load(mutation)
                .await
                .unwrap()
        })
    };
    let decisions = [first.await.unwrap(), second.await.unwrap()];

    assert_eq!(
        decisions
            .iter()
            .filter(|decision| **decision == SkillLoadDecision::Fresh)
            .count(),
        1
    );
    assert_eq!(
        decisions
            .iter()
            .filter(|decision| **decision == SkillLoadDecision::AlreadyLoaded)
            .count(),
        1
    );
}

#[cfg(feature = "dev")]
fn session_with_tool_result(session_id: &SessionId, revision: u64) -> CanonicalSession {
    let tool_result =
        Message::tool_results(vec![("tool-1".to_string(), "payload".to_string(), false)]);
    CanonicalSession {
        id: session_id.to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SessionSnapshotState::Missing,
        workspace: SessionSnapshotState::Captured(workspace()),
        revision,
        compact: None,
        cleared_after: None,
        run_slices: vec![CommittedRunSlice::new(
            "run",
            vec![CommittedRunStep {
                step_id: "step".to_string(),
                accepted_input: Some(AcceptedInputRecord::new(
                    vec![Message::user("accepted")],
                    "input",
                    revision,
                )),
                outcome: Some(crate::FinalizedOutcomeRecord::compatibility(vec![
                    tool_result,
                ])),
                tool_receipts: Vec::new(),
            }],
        )]
        .into(),
        committed_steps: vec![crate::domain::session::CommittedStep {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            fingerprint: "outcome".to_string(),
            committed_revision: revision,
        }]
        .into(),
        skill_load_records: Vec::new(),
    }
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn lifecycle_capture_reports_structure_and_releases_replaced_generation() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("lifecycle-session");
    let (repository, holder) =
        repository_with_session(writer, session_with_tool_result(&session_id, 1));

    let mut appended = append();
    appended.session_id = session_id;
    appended.expected_revision = SessionRevision::new(1);
    appended.run_id = RunId::new("run-2");
    appended.step_id = RunStepId::new("step-2");

    let (result, lifecycle) =
        crate::capture_session_lifecycle(repository.append_finalized(&appended)).await;
    result.unwrap();

    assert_eq!(lifecycle.transitions.len(), 1);
    let transition = &lifecycle.transitions[0];
    assert_eq!(transition.before.revision, 1);
    assert_eq!(transition.before.run_slices, 1);
    assert_eq!(transition.before.steps, 1);
    assert_eq!(transition.before.accepted_messages, 1);
    assert_eq!(transition.before.outcome_messages, 1);
    assert_eq!(transition.before.tool_result_blocks, 1);
    assert_eq!(transition.before.tool_result_content_bytes, 9);
    assert_eq!(transition.before.committed_steps, 1);
    assert_eq!(transition.after.revision, 2);
    assert_eq!(transition.after.run_slices, 2);
    assert_eq!(transition.after.steps, 2);
    assert_eq!(transition.after.outcome_messages, 2);
    assert_eq!(transition.after.committed_steps, 2);
    assert!(transition.replaced_generation.upgrade().is_none());
    assert_eq!(holder.read().unwrap().revision, 2);
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn lifecycle_weak_probe_stays_live_until_external_arc_is_dropped() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let external = holder.read().unwrap().clone();

    let (result, lifecycle) =
        crate::capture_session_lifecycle(repository.append_finalized(&append())).await;
    result.unwrap();

    let replaced = lifecycle.transitions[0].replaced_generation.clone();
    assert!(replaced.upgrade().is_some());
    drop(external);
    assert!(replaced.upgrade().is_none());
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn snapshot_does_not_publish_a_new_session_generation() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let committed = holder.read().unwrap().clone();

    let (snapshot, lifecycle) =
        crate::capture_session_lifecycle(repository.snapshot(&SessionId::new("session"))).await;
    snapshot.unwrap();

    assert!(lifecycle.transitions.is_empty());
    assert!(Arc::ptr_eq(&committed, &holder.read().unwrap()));
}

#[tokio::test]
async fn clear_after_partial_resume_keeps_persisted_steps_on_disk() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset: Arc<dyn storage::AtomicDatasetPort> =
        storage::wire_file_system_dataset(root.path()).expect("dataset adapter");
    let writer = Arc::new(crate::DatasetCanonicalSessionWriter::new(dataset.clone()));
    let session_id = SessionId::new("resume-clear-session");
    // 磁盘持久化完整历史：compact 边界前的 run-a 与边界后的 run-b。
    let persisted = CanonicalSession {
        run_slices: vec![
            CommittedRunSlice::new(
                "run-a",
                vec![CommittedRunStep::accepted_only(
                    "step-a",
                    AcceptedInputRecord::new(
                        vec![Message::user("archived")],
                        "run-a:step-a:archived",
                        1,
                    ),
                )],
            ),
            CommittedRunSlice::new(
                "run-b",
                vec![CommittedRunStep::accepted_only(
                    "step-b",
                    AcceptedInputRecord::new(
                        vec![Message::user("active")],
                        "run-b:step-b:active",
                        1,
                    ),
                )],
            ),
        ]
        .into(),
        ..ten_step_session(&session_id, vec![], 1)
    };
    writer
        .save_initial(&persisted)
        .await
        .expect("initial commit");

    // resume 后内存仅装载 compact 边界后的 active step。
    let resumed = CanonicalSession {
        run_slices: vec![CommittedRunSlice::new(
            "run-b",
            vec![CommittedRunStep::accepted_only(
                "step-b",
                AcceptedInputRecord::new(vec![Message::user("active")], "run-b:step-b:active", 1),
            )],
        )]
        .into(),
        ..persisted.clone()
    };
    let (repository, holder) = repository_with_session(writer, resumed);

    repository
        .clear(&session_id)
        .await
        .expect("clear must succeed");

    // 内存历史清空，clear 边界持久化到磁盘最后被清除的 step。
    assert!(holder.read().unwrap().run_slices.is_empty());
    assert_eq!(
        holder
            .read()
            .unwrap()
            .cleared_after
            .as_ref()
            .map(|c| c.step_id.as_str()),
        Some("step-b")
    );
    // 落盘位置随 project 分目录布局：workspace 捕获 identity 的 session
    // 写入 `<project-dir>/<id>.dataset`。
    let project_dir = crate::project_dir_segment(&workspace().project_identity);
    let manifest = dataset
        .read_manifest(
            &storage::DatasetKeyData::new(
                storage::StorageNamespaceData::Session,
                vec![
                    project_dir,
                    format!("{}.dataset", session_id.as_str())
                        .parse::<storage::SafePathSegmentData>()
                        .expect("safe dataset segment"),
                ],
            )
            .expect("valid dataset key"),
        )
        .await
        .expect("read committed manifest");
    let names = manifest
        .members()
        .iter()
        .map(storage::SafePathSegmentData::as_str)
        .collect::<Vec<_>>();
    // 逻辑断点：磁盘保留全部 step 成员（含内存未装载的 step-a）。
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("step-"))
            .count(),
        2
    );
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn clear_reports_zero_persisted_structure_and_releases_old_generation() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("clear-lifecycle-session");
    let (repository, _) = repository_with_session(writer, session_with_tool_result(&session_id, 3));

    let (result, lifecycle) = crate::capture_session_lifecycle(repository.clear(&session_id)).await;
    result.unwrap();

    let transition = &lifecycle.transitions[0];
    assert_eq!(transition.after.revision, 4);
    assert_eq!(transition.after.run_slices, 0);
    assert_eq!(transition.after.steps, 0);
    assert_eq!(transition.after.accepted_messages, 0);
    assert_eq!(transition.after.outcome_messages, 0);
    assert_eq!(transition.after.tool_result_blocks, 0);
    assert_eq!(transition.after.tool_result_content_bytes, 0);
    assert_eq!(transition.after.committed_steps, 0);
    assert!(transition.replaced_generation.upgrade().is_none());
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn compaction_changes_visibility_without_dropping_persisted_structure() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("compact-lifecycle-session");
    let (repository, _) = repository_with_session(writer, ten_step_session(&session_id, vec![], 0));

    let mut request = compact_request(session_id);
    request.context_size = 100_000;
    let (result, lifecycle) =
        crate::capture_session_lifecycle(repository.commit_compaction(&CompactRequestData {
            llm: None,
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(0),
            source: request,
            trigger: CompactTrigger::Automatic,
            progress: None,
            task_snapshot: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
        }))
        .await;
    result.unwrap();

    let transition = &lifecycle.transitions[0];
    assert_eq!(transition.before.run_slices, 10);
    assert_eq!(transition.after.run_slices, 10);
    assert_eq!(transition.before.steps, 10);
    assert_eq!(transition.after.steps, 10);
    assert_eq!(transition.before.outcome_messages, 10);
    assert_eq!(transition.after.outcome_messages, 10);
    assert!(transition.replaced_generation.upgrade().is_none());
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn lifecycle_workload_counts_100_500_and_1000_committed_steps() {
    for step_count in [100usize, 500, 1_000] {
        let writer = Arc::new(RecordingWriter::default());
        let session_id = SessionId::new(format!("lifecycle-{step_count}"));
        let run_slices = (0..step_count)
            .map(|index| {
                CommittedRunSlice::new(
                    format!("run-{index}"),
                    vec![CommittedRunStep::compatibility_outcome_only(
                        format!("step-{index}"),
                        vec![Message::user(format!("message-{index}"))],
                    )],
                )
            })
            .collect();
        let session = CanonicalSession {
            id: session_id.to_string(),
            chats: vec![],
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            metadata: Default::default(),
            tasks: SessionSnapshotState::Missing,
            workspace: SessionSnapshotState::Captured(workspace()),
            revision: step_count as u64,
            compact: None,
            cleared_after: None,
            run_slices,
            committed_steps: Default::default(),
            skill_load_records: Vec::new(),
        };
        let (repository, _) = repository_with_session(writer, session);
        let mut appended = append();
        appended.session_id = session_id;
        appended.expected_revision = SessionRevision::new(step_count as u64);
        appended.run_id = RunId::new("tail-run");
        appended.step_id = RunStepId::new("tail-step");

        let (result, lifecycle) =
            crate::capture_session_lifecycle(repository.append_finalized(&appended)).await;
        result.unwrap();

        let transition = &lifecycle.transitions[0];
        assert_eq!(transition.before.run_slices, step_count);
        assert_eq!(transition.before.steps, step_count);
        assert_eq!(transition.before.outcome_messages, step_count);
        assert_eq!(transition.after.run_slices, step_count + 1);
        assert_eq!(transition.after.steps, step_count + 1);
        assert_eq!(transition.after.outcome_messages, step_count + 1);
        assert!(transition.replaced_generation.upgrade().is_none());
    }
}

#[tokio::test]
async fn accepted_input_persists_before_publish() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer.clone());
    let accepted = accepted_input();

    let receipt = repository.append_accepted_input(&accepted).await.unwrap();

    assert_eq!(receipt.committed_revision, SessionRevision::new(1));
    // C 阶段：accepted input 随 Session member 单写提交（sidecar 退役）。
    assert_eq!(writer.saved.lock().unwrap().len(), 1);
    {
        let session = holder.read().unwrap();
        assert_eq!(session.revision, 1);
        let step = &session.run_slices[0].steps[0];
        assert_eq!(
            step.accepted_input.as_ref().unwrap().messages[0].text_content(),
            "accepted fact"
        );
        assert!(step.outcome.is_none());
    }
    assert_eq!(
        repository
            .snapshot(&accepted.session_id)
            .await
            .unwrap()
            .messages[0]
            .text_content(),
        "accepted fact"
    );
}

#[tokio::test]
async fn accepted_input_advances_revision_and_returns_post_commit_revision() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let accepted = accepted_input();

    let receipt = repository.append_accepted_input(&accepted).await.unwrap();

    // accepted input 是 durable commit：与 finalized append 语义一致，
    // 推进 session revision 并在回执中返回提交后版本（A1 统一语义）。
    assert_eq!(receipt.committed_revision, SessionRevision::new(1));
    assert_eq!(holder.read().unwrap().revision, 1);
}

#[tokio::test]
async fn current_revision_reports_latest_committed_revision() {
    // Runtime 在 Step 收口前用它刷新 CAS 基准：窗口快照 revision
    // 早于本 step 自身的 accepted-input / 工具回执推进。
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let session_id = SessionId::new("session");

    assert_eq!(
        repository.current_revision(&session_id).await.unwrap(),
        Some(SessionRevision::new(0))
    );

    repository
        .append_accepted_input(&accepted_input())
        .await
        .unwrap();
    let committed = SessionRevision::new(holder.read().unwrap().revision);
    assert_eq!(committed, SessionRevision::new(1));
    assert_eq!(
        repository.current_revision(&session_id).await.unwrap(),
        Some(committed)
    );

    // 未知会话不报错，返回 None 让调用方沿用快照值。
    assert_eq!(
        repository
            .current_revision(&SessionId::new("missing"))
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn accepted_input_is_idempotent_but_rejects_content_conflict() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, _) = repository(writer);
    let accepted = accepted_input();

    let first = repository.append_accepted_input(&accepted).await.unwrap();
    let second = repository.append_accepted_input(&accepted).await.unwrap();
    assert_eq!(first, second);

    // 冲突载荷：同 run/step 不同内容（指纹由内容推导，改消息即改指纹）。
    let mut conflicting = accepted;
    conflicting.messages = vec![Message::user("conflicting input")];
    assert!(matches!(
        repository.append_accepted_input(&conflicting).await,
        Err(AcceptedInputError::ContentConflict { .. })
    ));
}

#[tokio::test]
async fn accepted_input_storage_failure_does_not_publish_or_advance_revision() {
    #[allow(clippy::field_reassign_with_default)]
    let writer = {
        let mut failing = RecordingWriter::default();
        failing.fail = true;
        Arc::new(failing)
    };
    let (repository, holder) = repository(writer);
    let accepted = accepted_input();

    let result = repository.append_accepted_input(&accepted).await;

    // C 阶段：Session member 提交失败 → 明确错误，不发布内存态、不推进 revision。
    assert!(matches!(result, Err(AcceptedInputError::Storage(_))));
    assert_eq!(holder.read().unwrap().revision, 0);
    assert!(holder.read().unwrap().run_slices.is_empty());
}

#[tokio::test]
async fn accepted_input_duplicate_after_persist_is_published_exactly_once() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer.clone());
    let accepted = accepted_input();

    repository.append_accepted_input(&accepted).await.unwrap();
    repository.append_accepted_input(&accepted).await.unwrap();

    assert_eq!(holder.read().unwrap().revision, 1);
    assert_eq!(holder.read().unwrap().run_slices[0].steps.len(), 1);
    assert_eq!(
        writer.saved.lock().unwrap().len(),
        1,
        "幂等重放不得二次提交 member"
    );
}

#[tokio::test]
async fn finalized_append_persists_empty_receipts_without_implicit_backfill() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    // 活账本先落一个 terminal receipt（供旧隐式回填路径"可见"）。
    repository
        .advance_tool_receipt(ToolReceiptMutationData::pending(
            tool_identity(),
            "safe preview",
        ))
        .await
        .unwrap();
    repository
        .advance_tool_receipt(ToolReceiptMutationData::running(tool_identity()))
        .await
        .unwrap();
    repository
        .advance_tool_receipt(ToolReceiptMutationData::terminal(
            tool_identity(),
            crate::ToolTerminalReceiptData::new(
                crate::ToolOutcomeKindData::Success,
                "tool completed",
                tools::CleanupConfirmation::NotApplicable,
            ),
        ))
        .await
        .unwrap();

    // 调用方携带空 receipts 提交：adapter 必须原样落盘，
    // 禁止在 fingerprint 计算之后隐式替换内容（A2 装配责任归调用方）。
    let empty_append = append();
    let receipt = repository.append_finalized(&empty_append).await.unwrap();
    // 3 次 advance（revision 1→3）+ append 提交（4）。
    assert_eq!(receipt.committed_revision, SessionRevision::new(4));

    let session = holder.read().unwrap();
    let outcome = session.run_slices[0].steps[0]
        .outcome
        .as_ref()
        .expect("outcome must exist");
    assert!(
        outcome.receipts.is_empty(),
        "落盘 receipts 必须等于载荷（空），不得隐式回填活账本"
    );
}

#[tokio::test]
async fn finalized_append_bridges_messages_into_structured_outcome() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let run_id = RunId::new("run");
    let step_id = RunStepId::new("step");
    let mut finalized = append();
    finalized.run_id = run_id.clone();
    finalized.step_id = step_id.clone();

    repository.append_finalized(&finalized).await.unwrap();

    let session = holder.read().unwrap();
    assert_eq!(session.run_slices.len(), 1);
    assert_eq!(session.run_slices[0].run_id, run_id.to_string());
    assert_eq!(session.run_slices[0].steps.len(), 1);
    assert_eq!(session.run_slices[0].steps[0].step_id, step_id.as_str());
    let outcome = session.run_slices[0].steps[0].outcome.as_ref().unwrap();
    assert_eq!(outcome.finalize_cause, FinalizeCause::Completed);
    assert_eq!(outcome.messages[0].text_content(), "fact");
    assert!(outcome.receipts.is_empty());
    assert_eq!(outcome.api_input_tokens, None);
    assert_eq!(
        outcome.fingerprint,
        finalized.fingerprint().unwrap().as_str()
    );
    assert_eq!(outcome.committed_revision, 1);
}

#[tokio::test]
async fn finalized_outcome_preserves_accepted_input_and_receipt_metadata() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let accepted = accepted_input();
    repository.append_accepted_input(&accepted).await.unwrap();

    let mut finalized = append();
    finalized.expected_revision = SessionRevision::new(1);
    finalized.finalize_cause = FinalizeCause::UserCancelledStep;
    finalized.api_input_tokens = Some(42);
    finalized.receipts = vec![crate::RunStepReceiptData {
        call_id: "agent-call".to_string(),
        index: 0,
        outcome: crate::ToolOutcomeKindData::CancellationUnconfirmed,
        agent: true,
        summary: None,
        cleanup: tools::CleanupConfirmation::NotApplicable,
        possible_side_effects: Vec::new(),
        unfinished_call_ids: Vec::new(),
    }];
    let receipt = repository.append_finalized(&finalized).await.unwrap();

    let session = holder.read().unwrap();
    let step = &session.run_slices[0].steps[0];
    assert_eq!(
        step.accepted_input.as_ref().unwrap().messages[0].text_content(),
        "accepted fact"
    );
    assert_eq!(
        step.accepted_input.as_ref().unwrap().fingerprint,
        accepted_input().fingerprint().unwrap().as_str()
    );
    assert_eq!(step.accepted_input.as_ref().unwrap().committed_revision, 1);
    let outcome = step.outcome.as_ref().unwrap();
    assert_eq!(outcome.finalize_cause, FinalizeCause::UserCancelledStep);
    assert_eq!(outcome.api_input_tokens, Some(42));
    assert_eq!(
        outcome.receipts[0].outcome,
        crate::ToolOutcomeKindData::CancellationUnconfirmed
    );
    assert_eq!(
        outcome.fingerprint,
        finalized.fingerprint().unwrap().as_str()
    );
    assert_eq!(outcome.committed_revision, receipt.committed_revision.get());
}

#[tokio::test]
async fn snapshot_shares_committed_step_message_backing() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    let session = ten_step_session(&session_id, vec![], 10);
    let original_ptr = session.run_slices[0].steps[0]
        .outcome
        .as_ref()
        .unwrap()
        .messages
        .as_ptr();
    let (repository, _) = repository_with_session(writer, session);

    let snapshot = repository.snapshot(&session_id).await.unwrap();

    assert_eq!(snapshot.messages.len(), 10);
    let structured_history = snapshot
        .structured_history
        .as_ref()
        .expect("canonical snapshot must expose structured visible history");
    assert_eq!(structured_history.len(), 10);
    assert_eq!(
        structured_history[0].steps[0]
            .outcome
            .as_ref()
            .unwrap()
            .messages
            .as_ptr(),
        original_ptr
    );
    assert_eq!(
        snapshot.messages.first().map(|message| message as *const _),
        Some(original_ptr)
    );
}

#[tokio::test]
async fn repeated_snapshots_share_every_committed_step_message_backing() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    let session = ten_step_session(&session_id, vec![], 10);
    let original_ptrs = session
        .run_slices
        .iter()
        .map(|slice| slice.steps[0].outcome.as_ref().unwrap().messages.as_ptr())
        .collect::<Vec<_>>();
    let (repository, _) = repository_with_session(writer, session);

    let first = repository.snapshot(&session_id).await.unwrap();
    let second = repository.snapshot(&session_id).await.unwrap();

    let first_ptrs = first
        .messages
        .iter()
        .map(|message| message as *const _)
        .collect::<Vec<_>>();
    let second_ptrs = second
        .messages
        .iter()
        .map(|message| message as *const _)
        .collect::<Vec<_>>();
    assert_eq!(first_ptrs, original_ptrs);
    assert_eq!(second_ptrs, original_ptrs);
}

#[tokio::test]
async fn snapshot_after_compact_shares_only_visible_step_backing() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("compact-shared-session");
    let session = ten_step_session(&session_id, vec![], 0);
    let all_ptrs = session
        .run_slices
        .iter()
        .map(|slice| slice.steps[0].outcome.as_ref().unwrap().messages.as_ptr())
        .collect::<Vec<_>>();
    let (repository, _) = repository_with_session(writer, session);
    compact(&repository, session_id.clone(), 0).await;

    let snapshot = repository.snapshot(&session_id).await.unwrap();
    let visible_ptrs = snapshot
        .messages
        .iter()
        .map(|message| message as *const _)
        .collect::<Vec<_>>();

    assert!(!visible_ptrs.is_empty());
    assert!(visible_ptrs.len() < all_ptrs.len());
    assert_eq!(
        visible_ptrs,
        all_ptrs[all_ptrs.len() - visible_ptrs.len()..]
    );
    let structured_history = snapshot
        .structured_history
        .as_ref()
        .expect("compacted canonical snapshot must keep visible Run identity");
    let structured_ptrs = structured_history
        .iter()
        .map(|slice| slice.steps[0].outcome.as_ref().unwrap().messages.as_ptr())
        .collect::<Vec<_>>();
    assert_eq!(structured_ptrs, visible_ptrs);
}

#[tokio::test]
async fn snapshot_reads_structured_view_not_legacy_chats() {
    let writer = Arc::new(RecordingWriter::default());
    let mut legacy = ChatSegment::normal(None);
    legacy.messages = vec![Message::user("legacy-only")];
    let session_id = SessionId::new("session");
    let session = CanonicalSession {
        id: session_id.to_string(),
        chats: vec![legacy],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SessionSnapshotState::Missing,
        workspace: SessionSnapshotState::Captured(workspace()),
        revision: 0,
        compact: None,
        cleared_after: None,
        run_slices: vec![CommittedRunSlice::new(
            "run",
            vec![CommittedRunStep::accepted_only(
                "step",
                AcceptedInputRecord::new(vec![Message::user("structured-only")], "fp", 0),
            )],
        )]
        .into(),
        committed_steps: Default::default(),
        skill_load_records: Vec::new(),
    };
    let (repository, _) = repository_with_session(writer, session);

    let snapshot = repository.snapshot(&session_id).await.unwrap();
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.messages[0].text_content(), "structured-only");
    assert!(snapshot.active_summary.is_none());
}

#[tokio::test]
async fn finalized_append_reuses_existing_committed_step_entry_backing() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("shared-ledger");
    let mut session = CanonicalSession::fixture(session_id.as_str());
    session.revision = 1;
    session.committed_steps = vec![crate::domain::session::CommittedStep::fixture(
        "run-existing",
        "step-existing",
        "existing",
        1,
    )]
    .into();
    let existing_entry = Arc::clone(&session.committed_steps.entries()[0]);
    let (repository, holder) = repository_with_session(writer, session);
    let mut mutation = append();
    mutation.session_id = session_id;
    mutation.expected_revision = SessionRevision::new(1);
    mutation.run_id = RunId::new("run-new");
    mutation.step_id = RunStepId::new("step-new");

    repository
        .append_finalized(&mutation)
        .await
        .expect("append finalized outcome");

    let committed = holder.read().unwrap();
    assert!(Arc::ptr_eq(
        &existing_entry,
        &committed.committed_steps.entries()[0]
    ));
    assert_eq!(committed.committed_steps.len(), 2);
}

#[tokio::test]
async fn finalized_append_reuses_unchanged_run_slice_backing() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("shared-history");
    let session = CanonicalSession {
        id: session_id.to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SessionSnapshotState::Missing,
        workspace: SessionSnapshotState::Captured(workspace()),
        revision: 1,
        compact: None,
        cleared_after: None,
        run_slices: vec![CommittedRunSlice::new(
            "run-existing",
            vec![CommittedRunStep::accepted_only(
                "step-existing",
                AcceptedInputRecord::new(vec![Message::user("existing")], "existing", 1),
            )],
        )]
        .into(),
        committed_steps: Default::default(),
        skill_load_records: Vec::new(),
    };
    let existing_slice = Arc::clone(&session.run_slices.slices()[0]);
    let (repository, holder) = repository_with_session(writer, session);
    let mut mutation = append();
    mutation.session_id = session_id;
    mutation.expected_revision = SessionRevision::new(1);
    mutation.run_id = RunId::new("run-new");
    mutation.step_id = RunStepId::new("step-new");

    repository
        .append_finalized(&mutation)
        .await
        .expect("append finalized outcome");

    let committed = holder.read().unwrap();
    assert!(Arc::ptr_eq(
        &existing_slice,
        &committed.run_slices.slices()[0]
    ));
    assert_eq!(committed.run_slices.len(), 2);
}

#[tokio::test]
async fn advance_tool_receipt_persists_before_publish_and_is_idempotent() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer.clone());
    let mutation = ToolReceiptMutationData::pending(tool_identity(), "safe preview");

    let first = repository
        .advance_tool_receipt(mutation.clone())
        .await
        .unwrap();
    let second = repository.advance_tool_receipt(mutation).await.unwrap();

    assert!(first.changed);
    assert!(!second.changed);
    // C 阶段：sidecar 退役，receipt 变更随 Session member 一次原子提交（不再双写）；
    // dataset commit boundary 要求每次提交 revision 递增，幂等重放不重复提交。
    assert_eq!(holder.read().unwrap().revision, 1);
    assert_eq!(
        writer.saved.lock().unwrap().len(),
        1,
        "Tool receipt mutation must commit the canonical Session member exactly once"
    );
    assert_eq!(
        holder.read().unwrap().run_slices[0].steps[0].tool_receipts[0].state,
        ToolCallState::Pending
    );
}

#[tokio::test]
async fn advance_tool_receipt_write_failure_does_not_publish_candidate() {
    // C 阶段：失败的 Session member 提交不得发布内存 candidate（单写一致性）。
    #[allow(clippy::field_reassign_with_default)]
    let writer = {
        let mut failing = RecordingWriter::default();
        failing.fail = true;
        Arc::new(failing)
    };
    let (repository, holder) = repository(writer);

    assert!(matches!(
        repository
            .advance_tool_receipt(ToolReceiptMutationData::pending(
                tool_identity(),
                "safe preview"
            ))
            .await,
        Err(crate::ToolReceiptMutationError::Storage(_))
    ));
    assert_eq!(holder.read().unwrap().revision, 0);
    assert!(holder.read().unwrap().run_slices.is_empty());
}

#[tokio::test]
async fn concurrent_appends_with_same_revision_allow_one_commit_and_reject_one_cas() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);

    let mut first = append();
    first.run_id = RunId::new("run-first");
    first.step_id = RunStepId::new("step-first");
    let mut second = append();
    second.run_id = RunId::new("run-second");
    second.step_id = RunStepId::new("step-second");

    let (first_result, second_result) = tokio::join!(
        repository.append_finalized(&first),
        repository.append_finalized(&second)
    );
    let results = [first_result, second_result];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(ContextAppendError::RevisionConflict { .. })))
            .count(),
        1
    );
    assert_eq!(holder.read().unwrap().revision, 1);
    assert_eq!(holder.read().unwrap().committed_steps.len(), 1);
}

#[tokio::test]
async fn failed_durable_write_does_not_publish_candidate() {
    let writer = Arc::new(RecordingWriter {
        saved: Mutex::new(vec![]),
        fail: true,
        ..RecordingWriter::default()
    });
    let (repository, holder) = repository(writer);

    assert!(matches!(
        repository.append_finalized(&append()).await,
        Err(ContextAppendError::Storage(message)) if message == "disk full"
    ));
    assert_eq!(holder.read().unwrap().revision, 0);
}

#[tokio::test]
async fn compact_generation_does_not_hold_session_mutation_gate() {
    use tokio::sync::{mpsc, oneshot};

    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("unlocked-generation-session");
    let (repository, holder) =
        repository_with_session(writer, ten_step_session(&session_id, vec![], 0));
    let (started_sender, started_receiver) = oneshot::channel();
    let (release_sender, release_receiver) = mpsc::channel(1);
    let llm = test_llm_binding(
        FixedTextInvoker {
            text: valid_fact_batch("generated"),
            started: Arc::new(Mutex::new(Some(started_sender))),
            release: Arc::new(tokio::sync::Mutex::new(release_receiver)),
            fail_with_cancel: false,
        }
        .port(),
    );
    let repository = Arc::new(repository);
    let mut request = compact_request(session_id.clone());
    request.context_size = 100_000;
    let compact_task = {
        let repository = Arc::clone(&repository);
        tokio::spawn(async move {
            repository
                .commit_compaction(&CompactRequestData {
                    run_id: request.run_id.clone(),
                    source_revision: SessionRevision::new(0),
                    source: request,
                    trigger: CompactTrigger::Automatic,
                    progress: None,
                    task_snapshot: None,
                    llm,
                    cancellation: tokio_util::sync::CancellationToken::new(),
                })
                .await
        })
    };

    started_receiver
        .await
        .expect("compact generator should begin before concurrent mutation");
    let mut concurrent_append = append();
    concurrent_append.session_id = session_id;
    concurrent_append.run_id = RunId::new("concurrent-run");
    concurrent_append.step_id = RunStepId::new("concurrent-step");
    let append_receipt = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        repository.append_finalized(&concurrent_append),
    )
    .await
    .expect("Provider await 期间合法 Session mutation 必须能取得 mutation gate")
    .expect("concurrent append should commit");
    assert_eq!(append_receipt.committed_revision, SessionRevision::new(1));

    release_sender.send(()).await.unwrap();
    let compact_result = compact_task.await.unwrap();
    assert!(matches!(
        compact_result,
        Err(crate::ContextPortError::Compact(ref message))
            if message.contains("Session revision 冲突")
    ));
    let session = holder.read().unwrap();
    assert_eq!(session.revision, 1);
    assert!(session.compact.is_none());
    assert!(session
        .structured_messages()
        .iter()
        .any(|message| message.text_content() == "fact"));
}

#[tokio::test]
async fn cancelled_compaction_does_not_commit_local_fallback() {
    use tokio_util::sync::CancellationToken;

    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("cancelled-compact-session");
    let (repository, holder) =
        repository_with_session(writer, ten_step_session(&session_id, vec![], 0));
    let llm = test_llm_binding(
        FixedTextInvoker {
            text: String::new(),
            started: Arc::new(Mutex::new(None)),
            release: Arc::new(tokio::sync::Mutex::new(tokio::sync::mpsc::channel(1).1)),
            fail_with_cancel: true,
        }
        .port(),
    );
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let request = compact_request(session_id);

    let outcome = repository
        .commit_compaction(&CompactRequestData {
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(0),
            source: request,
            trigger: CompactTrigger::Automatic,
            progress: None,
            task_snapshot: None,
            llm,
            cancellation,
        })
        .await
        .unwrap();

    assert!(matches!(
        outcome,
        crate::CompactOutcome::Skipped(crate::domain::CompactSkipReason::Cancelled)
    ));
    let session = holder.read().unwrap();
    assert_eq!(session.revision, 0);
    assert!(session.compact.is_none());
}

#[tokio::test]
async fn automatic_compact_circuit_breaker_opens_after_configured_failures() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FailingWriter;

    #[async_trait]
    impl CanonicalSessionWriter for FailingWriter {
        async fn commit(
            &self,
            _session_id: &str,
            _expected_revision: u64,
            _plan: SessionCommitPlan,
        ) -> Result<(), String> {
            Err("disk full".to_string())
        }
    }

    let session_id = SessionId::new("circuit-breaker-session");
    let (repository, _) = repository_with_session(
        Arc::new(FailingWriter),
        ten_step_session(&session_id, vec![], 0),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let llm = test_llm_binding(
        ProviderFailingInvoker {
            calls: Arc::clone(&calls),
        }
        .port(),
    );
    let mut request = compact_request(session_id.clone());
    request.context_size = 100_000;
    request.config_snapshot = ConfigSnapshot::from_arc(Arc::new(Config {
        context: share::config::ContextConfig {
            auto_compact_failure_limit: 2,
            ..Default::default()
        },
        ..Config::default()
    }));

    for _ in 0..2 {
        assert!(repository
            .commit_compaction(&CompactRequestData {
                run_id: request.run_id.clone(),
                source_revision: SessionRevision::new(0),
                source: request.clone(),
                trigger: CompactTrigger::Automatic,
                progress: None,
                task_snapshot: None,
                llm: llm.clone(),
                cancellation: tokio_util::sync::CancellationToken::new(),
            })
            .await
            .is_err());
    }
    let outcome = repository
        .commit_compaction(&CompactRequestData {
            llm: None,
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(0),
            source: request,
            trigger: CompactTrigger::Automatic,
            progress: None,
            task_snapshot: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
        })
        .await
        .unwrap();

    assert!(matches!(
        outcome,
        crate::CompactOutcome::Skipped(crate::domain::CompactSkipReason::CircuitBreakerOpen)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn automatic_compaction_executes_after_actual_token_decision() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("actual-token-session");
    let (repository, _) = repository_with_session(writer, ten_step_session(&session_id, vec![], 0));
    let mut request = compact_request(session_id);
    request.context_size = 1_000_000;
    request.max_output_tokens = 8_192;
    request.last_api_total_tokens = Some(900_000);

    let outcome = repository
        .commit_compaction(&CompactRequestData {
            llm: None,
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(0),
            source: request,
            trigger: CompactTrigger::Automatic,
            progress: None,
            task_snapshot: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
        })
        .await
        .unwrap();

    assert!(matches!(outcome, crate::CompactOutcome::Committed(_)));
}

#[tokio::test]
async fn compact_marker_keeps_new_steps_visible() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("marker-session");
    let (repository, holder) =
        repository_with_session(writer, ten_step_session(&session_id, vec![], 0));
    compact(&repository, session_id.clone(), 0).await;

    let mut appended = append();
    appended.session_id = session_id;
    appended.expected_revision = SessionRevision::new(1);
    appended.run_id = RunId::new("run-new");
    appended.step_id = RunStepId::new("step-new");
    appended.messages = vec![Message::user("newly-visible")];
    repository.append_finalized(&appended).await.unwrap();

    let session = holder.read().unwrap();
    assert!(session
        .compact
        .as_ref()
        .and_then(|marker| marker.start_at.as_ref())
        .is_some());
    assert!(session
        .structured_messages()
        .iter()
        .any(|message| message.text_content() == "newly-visible"));
}

#[tokio::test]
async fn second_compact_advances_single_marker() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("second-marker-session");
    let (repository, holder) =
        repository_with_session(writer, ten_step_session(&session_id, vec![], 0));
    compact(&repository, session_id.clone(), 0).await;
    let first = holder
        .read()
        .unwrap()
        .compact
        .as_ref()
        .unwrap()
        .start_at
        .clone()
        .unwrap();

    for index in 0..6 {
        let mut appended = append();
        appended.session_id = session_id.clone();
        appended.expected_revision = SessionRevision::new(1 + index);
        appended.run_id = RunId::new(format!("run-new-{index}"));
        appended.step_id = RunStepId::new(format!("step-new-{index}"));
        appended.messages = vec![Message::user(format!("newly-visible-{index}"))];
        repository.append_finalized(&appended).await.unwrap();
    }
    compact(&repository, session_id, 7).await;

    let session = holder.read().unwrap();
    let marker = session.compact.as_ref().unwrap();
    assert_ne!(marker.start_at.as_ref(), Some(&first));
    assert!(
        crate::compact::ContinuationCheckpoint::parse(&marker.summary).is_ok(),
        "second compact marker must contain a valid continuation checkpoint: {}",
        marker.summary
    );
    assert!(
        marker.summary.contains("newly-visible-0") && marker.summary.contains("newly-visible-1"),
        "second compact must preserve prior and newly compacted context: {}",
        marker.summary
    );
    assert!(session
        .structured_messages()
        .iter()
        .any(|message| message.text_content() == "newly-visible-5"));
}

#[tokio::test]
async fn compaction_rejects_stale_source_revision() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    let (repository, holder) =
        repository_with_session(writer, ten_step_session(&session_id, vec![], 2));
    let request = compact_request(session_id);

    let result = repository
        .commit_compaction(&CompactRequestData {
            llm: None,
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(1),
            source: request,
            trigger: CompactTrigger::Automatic,
            progress: None,
            task_snapshot: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
        })
        .await;

    assert!(matches!(result, Err(crate::ContextPortError::Compact(_))));
    assert_eq!(holder.read().unwrap().revision, 2);
}

#[tokio::test]
async fn duplicate_key_is_idempotent_and_conflicting_content_is_typed() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, _) = repository(writer.clone());

    let first = repository.append_finalized(&append()).await.unwrap();
    let second = repository.append_finalized(&append()).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(writer.saved.lock().unwrap().len(), 1);
    // 同 key 不同内容（指纹由内容推导）→ typed conflict。
    let mut conflicting = append();
    conflicting.messages = vec![Message::user("conflicting fact")];
    assert!(matches!(
        repository.append_finalized(&conflicting).await,
        Err(ContextAppendError::ContentConflict { .. })
    ));
}

#[tokio::test]
async fn finalized_append_commits_typed_plan_with_expected_revision() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, _) = repository(writer.clone());

    repository
        .append_finalized(&append())
        .await
        .expect("append should commit");

    let commits = writer.saved.lock().unwrap();
    let commit = commits.last().expect("typed commit should be recorded");
    assert!(!commit.session_id.is_empty());
    assert_eq!(commit.expected_revision, 0);
    assert!(
        commit
            .plan
            .changed_members()
            .iter()
            .any(|member| member.name() == "manifest.json"),
        "commit plan must carry the canonical generation manifest"
    );
    assert!(commit.plan.preserves_unloaded_steps());
    assert!(commit.plan.removed_members().is_empty());
}

/// #1486：请求带 LLM 绑定后 commit_compaction 走 LLM 语义压缩，
/// summary 来自生成器而非本地 fallback 模板。
#[tokio::test]
async fn commit_compaction_with_generator_uses_llm_summary() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    let (base_repository, _holder) =
        repository_with_session(writer.clone(), ten_step_session(&session_id, vec![], 0));
    let repository_under_test = base_repository;
    let llm = test_llm_binding(
        FixedTextInvoker {
            text: valid_fact_batch("LLM 生成的语义摘要"),
            started: Arc::new(Mutex::new(None)),
            release: Arc::new(tokio::sync::Mutex::new(tokio::sync::mpsc::channel(1).1)),
            fail_with_cancel: false,
        }
        .port(),
    );

    let mut generated_request = compact_request(session_id.clone());
    generated_request.context_size = 100_000;
    let outcome = repository_under_test
        .commit_compaction(&CompactRequestData {
            run_id: generated_request.run_id.clone(),
            source_revision: SessionRevision::new(0),
            source: generated_request,
            trigger: CompactTrigger::Automatic,
            progress: None,
            task_snapshot: None,
            llm,
            cancellation: tokio_util::sync::CancellationToken::new(),
        })
        .await
        .unwrap();
    assert!(matches!(outcome, crate::CompactOutcome::Committed(_)));

    let commits = writer.saved.lock().unwrap();
    let saved = &commits.last().expect("compact commit must exist").plan;
    let state_member = saved
        .changed_members()
        .iter()
        .find(|member| member.name() == "session-state.json")
        .expect("compact must replace state member");
    let state = crate::SessionGenerationCodec::decode_state(state_member.bytes())
        .expect("decode committed state");
    assert!(
        state
            .compact_summary()
            .is_some_and(|summary| summary.contains("LLM 生成的语义摘要")),
        "summary 应来自 LLM 生成器: {:?}",
        state.compact_summary()
    );
    assert!(
        state
            .compact_summary()
            .is_some_and(|summary| summary.contains("## Immutable Constraints")),
        "LLM summary 应满足 checkpoint schema"
    );
}

/// #1537：compact summary 出口拼接当前 TaskData 状态，防止递进压缩后上下文丢失。
#[tokio::test]
async fn commit_compaction_reconciles_typed_task_snapshot_and_companion() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    let (base_repository, _holder) =
        repository_with_session(writer.clone(), ten_step_session(&session_id, vec![], 0));

    let mut request = compact_request(session_id.clone());
    request.context_size = 100_000;
    let outcome = base_repository
        .commit_compaction(&crate::CompactRequestData {
            llm: None,
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(0),
            source: request,
            trigger: crate::CompactTrigger::Automatic,
            progress: None,
            task_snapshot: Some(crate::compact::CompactTaskSnapshotData::active(
                1,
                1,
                "实现压缩拼接",
                vec![crate::compact::CompactTaskItemData::in_progress(
                    1,
                    "实现压缩拼接",
                )],
            )),
            cancellation: tokio_util::sync::CancellationToken::new(),
        })
        .await
        .unwrap();
    let crate::CompactOutcome::Committed(result) = &outcome else {
        panic!("expected committed compact: {outcome:?}");
    };
    assert_eq!(result.summary.matches("## Current Task State").count(), 1);
    assert!(result.summary.contains("■ [task:1 seq:1] 实现压缩拼接"));
    assert!(result.summary.contains("- Next action: 实现压缩拼接"));
    assert!(result.summary.contains("## Current Objective"));
    assert!(
        crate::compact::estimate_tokens(&result.summary) <= crate::compact::summary_budget(100_000),
        "checkpoint 与 Current Task State companion 的完整持久化结果必须在预算内"
    );
}

#[tokio::test]
async fn commit_compaction_keeps_large_task_companion_within_summary_budget() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    let (base_repository, _holder) =
        repository_with_session(writer.clone(), ten_step_session(&session_id, vec![], 0));
    let mut request = compact_request(session_id.clone());
    request.context_size = 100_000;
    let task_items = std::iter::once(crate::compact::CompactTaskItemData::in_progress(
        1,
        "实现压缩预算闭环",
    ))
    .chain((2..=30).map(|sequence| {
        crate::compact::CompactTaskItemData::pending(
            sequence,
            format!("任务 {sequence}: {}", "需要保留的详细恢复信息 ".repeat(80)),
            Vec::new(),
        )
    }))
    .collect();

    let outcome = base_repository
        .commit_compaction(&crate::CompactRequestData {
            llm: None,
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(0),
            source: request,
            trigger: crate::CompactTrigger::Automatic,
            progress: None,
            task_snapshot: Some(crate::compact::CompactTaskSnapshotData::active(
                1,
                1,
                "实现压缩预算闭环",
                task_items,
            )),
            cancellation: tokio_util::sync::CancellationToken::new(),
        })
        .await
        .unwrap();
    let crate::CompactOutcome::Committed(result) = outcome else {
        panic!("expected committed compact: {outcome:?}")
    };

    assert!(
        crate::compact::estimate_tokens(&result.summary) <= crate::compact::summary_budget(100_000)
    );
    assert_eq!(result.summary.matches("## Current Task State").count(), 1);
    assert!(result.summary.contains("BatchData #1 — Tasks: 0/30"));
    assert!(result.summary.contains("- Next action:"));
}

#[tokio::test]
async fn append_recovers_from_wiped_dataset_by_full_rebuild() {
    let writer = Arc::new(RecordingWriter {
        wiped: true,
        ..RecordingWriter::default()
    });
    let (repository, holder) = repository(writer.clone());

    let receipt = repository.append_finalized(&append()).await;

    let receipt = receipt.expect("append must recover from a wiped dataset");
    assert_eq!(receipt.committed_revision, SessionRevision::new(1));
    assert_eq!(holder.read().unwrap().revision, 1);
    let rebuilt = writer.rebuilt.lock().unwrap().clone();
    assert_eq!(
        rebuilt,
        vec![(holder.read().unwrap().id.clone(), 1)],
        "full rebuild must carry the in-memory candidate revision"
    );
    assert!(
        writer.saved.lock().unwrap().is_empty(),
        "wiped dataset must never accept the incremental plan"
    );
}

#[tokio::test]
async fn append_surfaces_error_when_rebuild_also_fails() {
    let writer = Arc::new(RecordingWriter {
        fail: true,
        ..RecordingWriter::default()
    });
    let (repository, holder) = repository(writer.clone());

    let outcome = repository.append_finalized(&append()).await;

    assert!(
        outcome.is_err(),
        "when both incremental commit and rebuild fail the append must surface an error"
    );
    assert_eq!(holder.read().unwrap().revision, 0);
    assert!(
        writer.rebuilt.lock().unwrap().is_empty(),
        "plain disk-full failures have an intact dataset and must not trigger rebuild"
    );
}
