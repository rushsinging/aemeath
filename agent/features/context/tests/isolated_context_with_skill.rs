//! Isolated context with Skill metadata catalog (Issue #1438).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use context::domain::ContextRequest;
use context::ports::SkillQueryFactory;
use provider::ReasoningLevel;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use tools::{SkillCatalogPort, SkillDescriptor, SkillQuery, SkillSource, SkillSourceKind};

struct FakeCatalog;
impl SkillCatalogPort for FakeCatalog {
    fn list(&self, _query: SkillQuery) -> Vec<SkillDescriptor> {
        vec![SkillDescriptor::new(
            "release",
            "create a release",
            SkillSource::file(SkillSourceKind::ProjectAgents, "/fake/release/SKILL.md"),
            Vec::new(),
            Some("release".into()),
            Vec::new(),
            Some("[version]".into()),
        )]
    }
}

struct FixedQueryFactory;
impl SkillQueryFactory for FixedQueryFactory {
    fn query(&self, _request: &ContextRequest) -> SkillQuery {
        SkillQuery::new(PathBuf::from("/fake"), Vec::new(), BTreeSet::new())
    }
}

fn base_request() -> ContextRequest {
    use context::domain::*;
    ContextRequest {
        session_id: SessionId::new("isolated-session"),
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
async fn isolated_context_with_skill_catalog_builds_metadata_directory() {
    let port = context::adapters::isolated_context_with_skill(
        "isolated-session",
        Arc::new(FakeCatalog),
        Arc::new(FixedQueryFactory),
    );
    let window = port.build_window(&base_request()).await.unwrap();
    let block = window
        .system_blocks
        .iter()
        .find(|block| block.kind == "skills")
        .unwrap();
    assert!(block.content.contains("release: create a release"));
    assert!(block.content.contains("Usage: /release [version]"));
    assert!(!block.content.contains("FULL_SKILL_BODY_MUST_NOT_APPEAR"));
}

#[tokio::test]
async fn isolated_context_without_catalog_has_no_skills_block() {
    let port = context::adapters::isolated_context("isolated-session");
    let window = port.build_window(&base_request()).await.unwrap();
    assert!(!window
        .system_blocks
        .iter()
        .any(|block| block.kind == "skills"));
}
