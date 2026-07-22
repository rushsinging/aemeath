//! Issue #912 — `isolated_context_with_skill` 构建出的 window 含 skills 块。
//!
//! 证明：当注入会返回 skill 片段的 fake supplier 与确定性 query factory 时，
//! `isolated_context_with_skill` 构建的 `ContextPort` 在 `build_window` 后
//! 产出的 `system_blocks` 中恰好包含一个 `skills` 块，且位于
//! `execution_discipline` 之后；而不注入 skill 的 `isolated_context`
//! 则不含 `skills` 块。

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use context::domain::ContextRequest;
use context::ports::SkillQueryFactory;
use provider::ReasoningLevel;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use tools::{
    CacheHint, PromptFragment, SkillError, SkillMaterializationPort, SkillMaterializationQuery,
    SkillMaterializationRevision, SkillMaterializationSnapshot, SkillSource, SkillSourceKind,
};

// ── 测试 doubles ─────────────────────────────────────────────────────────

/// 可控的 SkillMaterializationPort：返回预设的片段快照。
struct FakeSupplier {
    fragments: Vec<PromptFragment>,
}

#[async_trait]
impl SkillMaterializationPort for FakeSupplier {
    async fn materialize_available(
        &self,
        _query: SkillMaterializationQuery,
    ) -> Result<SkillMaterializationSnapshot, SkillError> {
        let revision = SkillMaterializationRevision::from_fragments(&self.fragments);
        Ok(SkillMaterializationSnapshot::new(
            self.fragments.clone(),
            revision,
        ))
    }
}

/// 固定 query factory：忽略 request，便于断言 pipeline 行为。
struct FixedQueryFactory;

impl SkillQueryFactory for FixedQueryFactory {
    fn materialize_query(&self, _request: &ContextRequest) -> SkillMaterializationQuery {
        SkillMaterializationQuery::new(PathBuf::from("/fake"), Vec::new(), BTreeSet::new())
    }
}

fn fragment(key: &str, content: &str) -> PromptFragment {
    PromptFragment::new(
        key,
        content,
        SkillSource::file(SkillSourceKind::ProjectAgents, format!("/fake/{key}.md")),
        CacheHint::Stable,
    )
}

fn base_request() -> ContextRequest {
    use context::domain::*;
    ContextRequest {
        session_id: SessionId::new("isolated-session"),
        request_id: ContextRequestId::new("request"),
        run_id: sdk::RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![],
        system_prompt: SystemPromptSpec::new("base system prompt"),
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        task_reminder: TaskReminderSnapshot::default(),
        language: Language::new("en"),
        agent_roles: Default::default(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_input_tokens: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
        prev_system_tokens: None,
        prev_tool_schema_tokens: None,
    }
}

// ── 契约：isolated_context_with_skill 构建的 window 含 skills 块 ────────────

#[tokio::test]
async fn isolated_context_with_skill_builds_window_containing_skills() {
    let port = context::adapters::isolated_context_with_skill(
        "isolated-session",
        Arc::new(FakeSupplier {
            fragments: vec![fragment("alpha", "do alpha"), fragment("beta", "do beta")],
        }),
        Arc::new(FixedQueryFactory),
    );

    let window = port.build_window(&base_request()).await.unwrap();

    let cacheable: Vec<&str> = window
        .system_blocks
        .iter()
        .map(|b| b.kind.as_str())
        .collect();
    let skills_indices: Vec<_> = cacheable
        .iter()
        .enumerate()
        .filter(|(_, k)| **k == "skills")
        .collect();
    // 恰好一个 skills 块
    assert_eq!(skills_indices.len(), 1, "expected exactly one skills block");
    // skills 块位于 execution_discipline 之后
    let discipline_idx = cacheable
        .iter()
        .position(|k| *k == "execution_discipline")
        .unwrap();
    assert!(skills_indices[0].0 > discipline_idx);
    // skills 块内容包含两个技能
    let skills_block = &window.system_blocks[skills_indices[0].0];
    assert!(skills_block.content.contains("do alpha"));
    assert!(skills_block.content.contains("do beta"));
    // skills 块可缓存
    assert!(skills_block.cacheable);
    assert!(
        window
            .system_blocks
            .iter()
            .all(|block| block.kind != "dynamic_system_context"),
        "动态系统上下文不得进入 LLM system blocks"
    );
}

// ── 对照：isolated_context（不带 skill）不含 skills 块 ──────────────────────

#[tokio::test]
async fn isolated_context_without_skill_has_no_skills_block() {
    let port = context::adapters::isolated_context("isolated-session");

    let window = port.build_window(&base_request()).await.unwrap();

    assert!(
        !window.system_blocks.iter().any(|b| b.kind == "skills"),
        "baseline isolated_context must not emit a skills block"
    );
}
