//! wire 装配工厂契约测试（#1863）：composition 直构点收口后的装配入口。

use std::sync::Arc;

use sdk::RunId;
use share::ids::{RunStepId, SessionId};
use share::reasoning::ReasoningLevel;
use tools::published::skill::{SkillCatalogPort, SkillDescriptor, SkillQuery};

use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::session_types::ProjectIdentityData;

use crate::{ContextRequestData, ContextRequestId, Language, SystemPromptSpecData};

fn test_project_identity() -> ProjectIdentityData {
    ProjectIdentityData {
        initial_cwd: "/tmp/wire-factory-test".to_string(),
        git_common_dir: None,
    }
}

/// `wire_session_management` 返回的 port 在空 dataset 上 list 成功。
#[tokio::test]
async fn wire_session_management_lists_empty_on_fresh_dataset() {
    let tempdir = tempfile::tempdir().unwrap();
    let dataset = storage::wire_file_system_dataset(tempdir.path().join("dataset")).unwrap();
    let blob = storage::wire_file_system_blob(tempdir.path().join("blob")).unwrap();

    let management = crate::wire_session_management(dataset, blob);
    let sessions = management
        .list_for_project(&test_project_identity())
        .await
        .expect("list_for_project 在空 dataset 上应成功");
    assert!(sessions.is_empty());
}

/// `wire_production_main_context_factory` 返回的 factory 可构建 ContextPort
/// 并完成 build_window 最小闭环。
#[tokio::test]
async fn wire_production_main_context_factory_builds_working_context() {
    let tempdir = tempfile::tempdir().unwrap();
    let dataset = storage::wire_file_system_dataset(tempdir.path().join("dataset")).unwrap();
    let blob = storage::wire_file_system_blob(tempdir.path().join("blob")).unwrap();
    let skill_catalog: Arc<dyn SkillCatalogPort> = Arc::new(EmptySkillCatalog);
    let workspace = project::wire_production_workspace(tempdir.path().to_path_buf(), None).unwrap();
    let generator: Arc<dyn crate::ports::CompactGenerator> = Arc::new(FailingGenerator);

    let factory = crate::wire_production_main_context_factory(
        dataset,
        blob,
        skill_catalog,
        workspace.read(),
        generator,
    );

    let session_id = SessionId::new("wire-factory");
    let session = Arc::new(std::sync::RwLock::new(Arc::new(
        crate::domain::session::CanonicalSession::fixture(session_id.as_str()),
    )));
    let task_store: Arc<dyn task::TaskPersist> = Arc::new(task::TaskStore::new());
    let memory_port: Arc<dyn memory::api::MemoryPort> =
        Arc::new(memory::api::InMemoryMemory::new(memory::api::MemoryPolicy::default()).unwrap());
    let context_port = factory.build(
        session,
        task_store,
        workspace.persist(),
        Arc::new(std::sync::RwLock::new(memory_port)),
        Arc::new(tokio::sync::Mutex::new(())),
    );
    let window = context_port
        .build_window(&test_request())
        .await
        .expect("build_window 应成功");
    assert!(window.messages.is_empty());
}

fn test_request() -> ContextRequestData {
    ContextRequestData {
        session_id: SessionId::new("wire-factory"),
        request_id: ContextRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![],
        system_prompt: SystemPromptSpecData::new("base system prompt"),
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::new("en"),
        agent_roles: Default::default(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_total_tokens: None,
        heuristic_calibration: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

struct EmptySkillCatalog;

impl SkillCatalogPort for EmptySkillCatalog {
    fn list(&self, _query: SkillQuery) -> Vec<SkillDescriptor> {
        Vec::new()
    }
}

struct FailingGenerator;

#[async_trait::async_trait]
impl crate::ports::CompactGenerator for FailingGenerator {
    async fn generate(
        &self,
        _request: Vec<share::message::Message>,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<
        crate::domain::CompactGenerationOutputData,
        crate::domain::CompactGenerationFailureData,
    > {
        unreachable!("本测试不触发 compact")
    }
}
