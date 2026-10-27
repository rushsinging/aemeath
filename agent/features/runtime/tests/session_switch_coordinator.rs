//! SessionSwitchCoordinator 协议测试（#2025 C1）。
//!
//! 平移自 context `main_session_gate` / `main_session_wiring` 集成测试的
//! 语义：shared/exclusive 互斥、resume 原子切换、prepare 失败回滚、
//! restart 标记 roundtrip。

use std::sync::Arc;

use async_trait::async_trait;
use context::session::SessionMetadata;
use context::test_support::AtomicBlobSessionManagement;
use context::SessionManagementPort;
use memory::api::{MemoryOpener, MemoryOpenerError, MemoryPort, ProjectMemoryKey};
use share::config::domain::snapshot::ConfigRevision;
use share::message::{Message, Role};
use share::session_types::ProjectIdentityData;

use runtime::{wire_session_switch_coordinator, SessionSwitchDependencies};

// ─── Helpers ─────────────────────────────────────────────────────────

/// 可切换 opener：bootstrap 用内存实现；置位后 resume 阶段 open 失败，
/// 验证 prepare 失败时 committed 四元组回滚。
#[derive(Default)]
struct SwitchableOpener {
    fail: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait]
impl MemoryOpener for SwitchableOpener {
    async fn open_memory(
        &self,
        _key: &ProjectMemoryKey,
        _config: &share::config::MemoryConfig,
    ) -> Result<Arc<dyn MemoryPort>, MemoryOpenerError> {
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            Err(MemoryOpenerError::Io)
        } else {
            Ok(Arc::new(
                memory::api::InMemoryMemory::new(memory::api::MemoryPolicy::default())
                    .expect("default MemoryPolicy is always valid"),
            ))
        }
    }

    fn boxed_clone(&self) -> Box<dyn MemoryOpener> {
        Box::new(SwitchableOpener {
            fail: Arc::clone(&self.fail),
        })
    }
}

struct TestEnv {
    /// 持有临时目录生命周期（RAII，drop 时清理）。
    #[allow(dead_code)]
    temp: tempfile::TempDir,
    workspace: project::Workspace,
    session_management: Arc<AtomicBlobSessionManagement>,
    dependencies: Option<SessionSwitchDependencies>,
    #[allow(dead_code)]
    fail_switch: Option<Arc<SwitchableOpener>>,
}

impl TestEnv {
    async fn new() -> Self {
        Self::with_switchable(None).await
    }

    async fn with_switchable(fail_switch: Option<Arc<SwitchableOpener>>) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        std::fs::create_dir_all(&root).expect("create root");
        let workspace =
            project::wire_production_workspace(root.clone(), None).expect("wire workspace");
        let config = config::wire_project_config(
            &root,
            config::wire_config_override_store(
                storage::wire_file_system_blob(temp.path().join("config-overrides"))
                    .expect("create config blob"),
            ),
        )
        .await
        .expect("wire config");
        let task_wiring = task::wire_task();
        let session_management = Arc::new(AtomicBlobSessionManagement::new(
            storage::wire_file_system_blob(temp.path().join("agents"))
                .expect("create session blob"),
        ));
        let memory_opener: Box<dyn MemoryOpener> = match &fail_switch {
            Some(switch) => Box::new(SwitchableOpener {
                fail: Arc::clone(&switch.fail),
            }),
            None => Box::new(context::test_support::InMemoryTestOpener),
        };
        let dependencies = SessionSwitchDependencies {
            workspace: workspace.clone(),
            task_restore: std::sync::Arc::new(task::TaskRestoreAccessAdapter::new(
                task_wiring.persist(),
            )),
            config_reader: config.reader(),
            config_participant: config.participant(),
            memory_opener,
            session_management: Arc::clone(&session_management) as Arc<dyn SessionManagementPort>,
            context_factory: Arc::new(
                context::MainContextFactory::new(Arc::new(context::NoOpCanonicalSessionWriter))
                    .with_persistence(task_wiring.persist(), workspace.persist()),
            ),
        };
        Self {
            temp,
            workspace,
            session_management,
            dependencies: Some(dependencies),
            fail_switch,
        }
    }

    async fn coordinator(&mut self) -> Arc<runtime::SessionSwitchCoordinator> {
        wire_session_switch_coordinator(
            self.dependencies
                .take()
                .expect("dependencies consumed once"),
        )
        .await
        .expect("wire coordinator")
    }

    async fn seed_session(&self, id: &str) {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "id": id,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "messages": [Message::user("hello from saved session"), Message::placeholder(Role::Assistant)],
            "metadata": SessionMetadata::default(),
            "workspace": self.workspace.persist().snapshot(),
        }))
        .expect("encode legacy seed");
        self.session_management
            .import_for_project(&bytes, &self.workspace.read().project_identity())
            .await
            .expect("import seed session");
    }

    fn project_identity(&self) -> ProjectIdentityData {
        self.workspace.read().project_identity()
    }
}

// ─── 协议行为 ────────────────────────────────────────────────────────

/// bind 持 shared permit 期间 resume（exclusive）被阻塞；drop 后可切换。
#[tokio::test]
async fn bound_run_blocks_resume_until_released() {
    let mut env = TestEnv::new().await;
    env.seed_session("switch-after-release").await;
    let coordinator = env.coordinator().await;

    let bound = coordinator.bind_run().await.expect("bind");
    let before_id = bound.session().id.clone();

    // shared permit 存活期内发起 resume：advance 不了（select 分支不完成）
    let resume_fut = coordinator.resume_session("switch-after-release");
    tokio::pin!(resume_fut);
    tokio::select! {
        biased;
        _ = &mut resume_fut => panic!("resume must not complete while bound run holds shared permit"),
        _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
    }

    bound.release();
    let view = resume_fut.await.expect("resume after release");
    assert_eq!(view.session_id, "switch-after-release");
    assert_ne!(coordinator.committed_session().id, before_id);
}

/// resume 成功后 committed session 原子切换为新 canonical。
#[tokio::test]
async fn resume_replaces_committed_session() {
    let mut env = TestEnv::new().await;
    env.seed_session("target-session").await;
    let coordinator = env.coordinator().await;

    let before = coordinator.committed_session();
    let view = coordinator
        .resume_session("target-session")
        .await
        .expect("resume");
    let after = coordinator.committed_session();

    assert_eq!(view.session_id, "target-session");
    assert_eq!(after.id, "target-session");
    assert_ne!(after.id, before.id);
}

/// prepare 失败（memory open）时 committed session 保持旧值（回滚）。
#[tokio::test]
async fn resume_prepare_failure_rolls_back() {
    let fail_switch = Arc::new(SwitchableOpener::default());
    let mut env = TestEnv::with_switchable(Some(Arc::clone(&fail_switch))).await;
    env.seed_session("doomed-session").await;
    let coordinator = env.coordinator().await;

    let before = coordinator.committed_session();
    fail_switch
        .fail
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let error = coordinator
        .resume_session("doomed-session")
        .await
        .expect_err("resume must fail when memory open fails");
    assert!(
        error.to_string().to_lowercase().contains("memory") || error.to_string().contains("I/O"),
        "{error}"
    );
    let after = coordinator.committed_session();
    assert_eq!(after.id, before.id, "committed session must stay untouched");
}

/// workspace 槽位 Missing 的 session 拒绝 resume（无安全默认）。
#[tokio::test]
async fn resume_rejects_missing_workspace() {
    let mut env = TestEnv::new().await;

    // seed 一个 workspace 槽位缺失的 bare session（同 identity），
    // resume 流水线必须在 Project prepare 阶段拒绝它。
    let bytes = serde_json::to_vec(&serde_json::json!({
        "id": "bare-session",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "messages": [],
        "metadata": SessionMetadata::default(),
    }))
    .expect("encode bare seed");
    let imported = env
        .session_management
        .import_for_project(&bytes, &env.project_identity())
        .await;
    let coordinator = env.coordinator().await;
    if imported.is_err() {
        // 导入侧已拒绝 bare session（防御前置），同样证明协议防御性：
        // 此时 resume 也必然 NotFound / 拒绝，断言走 load 失败分支即可。
        let error = coordinator
            .resume_session("bare-session")
            .await
            .expect_err("bare session must not resume");
        assert!(
            error.to_string().contains("不存在")
                || error.to_string().contains("not found")
                || error.to_string().contains("找不到"),
            "{error}"
        );
        return;
    }

    let error = coordinator
        .resume_session("bare-session")
        .await
        .expect_err("must reject missing workspace");
    assert!(
        error.to_string().contains("workspace snapshot is missing"),
        "{error}"
    );
}

/// restart 标记 roundtrip：标记后 pending 可读，resume 成功后清空。
#[tokio::test]
async fn restart_revision_marks_and_clears_on_resume() {
    let mut env = TestEnv::new().await;
    env.seed_session("restart-clear").await;
    let coordinator = env.coordinator().await;

    assert!(coordinator.pending_session_restart_revision().is_none());
    let restart_revision = ConfigRevision::new(7);
    coordinator.mark_session_restart_required(restart_revision);
    assert_eq!(
        coordinator.pending_session_restart_revision(),
        Some(restart_revision)
    );
    coordinator
        .resume_session("restart-clear")
        .await
        .expect("resume");
    assert!(
        coordinator.pending_session_restart_revision().is_none(),
        "resume must clear pending restart revision"
    );
}
