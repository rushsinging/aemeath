//! Skill metadata directory contract (Issue #1438).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use context::adapters::{SkillPromptSource, WorkspaceSkillQueryFactory};
use context::domain::ContextRequest;
use context::ports::{ContextPromptSource, SkillQueryFactory};
use provider::ReasoningLevel;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use tools::{SkillCatalogPort, SkillDescriptor, SkillQuery, SkillSource, SkillSourceKind};

struct FakeCatalog(Vec<SkillDescriptor>);
impl SkillCatalogPort for FakeCatalog {
    fn list(&self, _query: SkillQuery) -> Vec<SkillDescriptor> {
        self.0.clone()
    }
}

struct FixedQueryFactory;
impl SkillQueryFactory for FixedQueryFactory {
    fn query(&self, _request: &ContextRequest) -> SkillQuery {
        SkillQuery::new(PathBuf::from("/fake"), Vec::new(), BTreeSet::new())
    }
}

fn descriptor(name: &str, description: &str, hint: Option<&str>) -> SkillDescriptor {
    SkillDescriptor::new(
        name,
        description,
        SkillSource::file(
            SkillSourceKind::ProjectAgents,
            format!("/fake/{name}/SKILL.md"),
        ),
        Vec::new(),
        Some(name.to_string()),
        Vec::new(),
        hint.map(str::to_string),
    )
}

fn source(skills: Vec<SkillDescriptor>) -> SkillPromptSource {
    SkillPromptSource::new(Arc::new(FakeCatalog(skills)), Arc::new(FixedQueryFactory))
}

fn base_request() -> ContextRequest {
    use context::domain::*;
    ContextRequest {
        session_id: SessionId::new("session"),
        request_id: ContextRequestId::new("request"),
        run_id: sdk::RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![],
        invocation_reminders: vec![],
        system_prompt: SystemPromptSpec::new("base system prompt"),
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::new("en"),
        agent_roles: Default::default(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_total_tokens: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

#[tokio::test]
async fn renders_sorted_metadata_directory_without_skill_body() {
    let src = source(vec![
        descriptor("zeta", "last", None),
        descriptor("release", "create a release", Some("[version]")),
    ]);
    let result = src.materialize(&base_request()).await.unwrap();
    let block = result
        .cacheable
        .iter()
        .find(|block| block.kind == "skills")
        .unwrap();
    assert!(block.content.starts_with("# Available Skills\n"));
    assert!(block
        .content
        .contains("- release: create a release\n  Usage: /release [version]"));
    assert!(block.content.find("release").unwrap() < block.content.find("zeta").unwrap());
    assert!(!block.content.contains("FULL_SKILL_BODY_MUST_NOT_APPEAR"));
}

#[tokio::test]
async fn duplicate_identity_is_deduplicated_and_empty_catalog_omits_block() {
    let result = source(vec![
        descriptor("dup", "first", None),
        descriptor("dup", "second", None),
    ])
    .materialize(&base_request())
    .await
    .unwrap();
    let block = result
        .cacheable
        .iter()
        .find(|block| block.kind == "skills")
        .unwrap();
    assert!(block.content.contains("first"));
    assert!(!block.content.contains("second"));

    let empty = source(Vec::new())
        .materialize(&base_request())
        .await
        .unwrap();
    assert!(!empty.cacheable.iter().any(|block| block.kind == "skills"));
}

#[tokio::test]
async fn chinese_header_and_budget_are_deterministic() {
    let mut request = base_request();
    request.language = context::domain::Language::new("zh");
    request.context_size = 8_000;
    let result = source(vec![descriptor("alpha", &"x".repeat(8_000), None)])
        .materialize(&request)
        .await
        .unwrap();
    assert!(!result.cacheable.iter().any(|block| block.kind == "skills"));
    assert_eq!(context::adapters::skill_prompt_budget(8_000), 1_024);
}

struct FakeWorkspace(PathBuf);
impl project::WorkspaceRead for FakeWorkspace {
    fn workspace_id(&self) -> project::WorkspaceId {
        project::WorkspaceId::default()
    }
    fn project_identity(&self) -> project::ProjectIdentity {
        project::ProjectIdentity::default()
    }
    fn current_workspace_root(&self) -> PathBuf {
        self.0.clone()
    }
    fn current_path_base(&self) -> PathBuf {
        self.0.clone()
    }
    fn resolve(&self, rel: &std::path::Path) -> PathBuf {
        self.0.join(rel)
    }
    fn resolve_file_path(&self, rel: &std::path::Path) -> Result<PathBuf, project::WorkspaceError> {
        Ok(self.0.join(rel))
    }
    fn resolve_search_path(
        &self,
        rel: &std::path::Path,
    ) -> Result<PathBuf, project::WorkspaceError> {
        Ok(self.0.join(rel))
    }
    fn in_worktree(&self) -> bool {
        false
    }
    fn current_branch(&self) -> Result<Option<String>, project::WorkspaceError> {
        Ok(None)
    }
    fn initial_cwd(&self) -> PathBuf {
        self.0.clone()
    }
}

#[test]
fn query_factory_reads_live_workspace_root() {
    let factory = WorkspaceSkillQueryFactory::new(Arc::new(FakeWorkspace(PathBuf::from("/live"))));
    assert_eq!(
        factory.query(&base_request()).project_root,
        PathBuf::from("/live")
    );
}
