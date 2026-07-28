//! Skill metadata prompt directory（Issue #1438）。

use std::sync::Arc;

use async_trait::async_trait;
use tools::{SkillCatalogPort, SkillDescriptor, SkillQuery};

use crate::domain::ContextRequest;
use crate::ports::{
    ContextPromptSource, PromptMaterialization, PromptMaterializationError, SkillQueryFactory,
};

pub struct SkillPromptSource {
    catalog: Arc<dyn SkillCatalogPort>,
    query_factory: Arc<dyn SkillQueryFactory>,
}

impl SkillPromptSource {
    pub fn new(
        catalog: Arc<dyn SkillCatalogPort>,
        query_factory: Arc<dyn SkillQueryFactory>,
    ) -> Self {
        Self {
            catalog,
            query_factory,
        }
    }
}

pub struct WorkspaceSkillQueryFactory {
    workspace: Arc<dyn project::WorkspaceRead>,
}

impl WorkspaceSkillQueryFactory {
    pub fn new(workspace: Arc<dyn project::WorkspaceRead>) -> Self {
        Self { workspace }
    }
}

impl SkillQueryFactory for WorkspaceSkillQueryFactory {
    fn query(&self, request: &ContextRequest) -> SkillQuery {
        let project_root = self.workspace.current_workspace_root();
        let extra_dirs = request.config_snapshot.skills().dirs.clone();
        let available_tools = request
            .tool_schemas
            .iter()
            .map(|schema| schema.name.clone())
            .collect();
        SkillQuery::new(project_root, extra_dirs, available_tools)
    }
}

pub(crate) fn sort_and_dedup(mut descriptors: Vec<SkillDescriptor>) -> Vec<SkillDescriptor> {
    descriptors.sort_by(|left, right| left.name().cmp(right.name()));
    descriptors.dedup_by(|left, right| left.name() == right.name());
    descriptors
}

pub fn skill_prompt_budget(context_size: usize) -> usize {
    (context_size / 8).max(1_024)
}

fn render_entry(skill: &SkillDescriptor) -> String {
    let mut line = format!("- {}: {}", skill.name(), skill.description());
    if let Some(slash) = skill.slash_command() {
        line.push_str(&format!("\n  Usage: /{slash}"));
        if let Some(hint) = skill.argument_hint() {
            line.push(' ');
            line.push_str(hint);
        }
    }
    line
}

fn select_within_budget(
    descriptors: &[SkillDescriptor],
    budget_tokens: usize,
    header_tokens: usize,
) -> Vec<SkillDescriptor> {
    let mut used = header_tokens;
    let mut selected = Vec::new();
    for descriptor in descriptors {
        let rendered = render_entry(descriptor);
        let cost = crate::domain::estimate_tokens(&rendered);
        if used.saturating_add(cost) > budget_tokens {
            break;
        }
        used += cost;
        selected.push(descriptor.clone());
    }
    selected
}

fn render_skills_block(selected: &[SkillDescriptor], lang: &str) -> String {
    let header = if lang.eq_ignore_ascii_case("zh") {
        "# 可用技能\n"
    } else {
        "# Available Skills\n"
    };
    let body = selected
        .iter()
        .map(render_entry)
        .collect::<Vec<_>>()
        .join("\n");
    format!("{header}{body}\n")
}

fn metadata_revision(descriptors: &[SkillDescriptor]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for descriptor in descriptors {
        for value in [
            descriptor.name(),
            descriptor.description(),
            descriptor.slash_command().unwrap_or_default(),
            descriptor.argument_hint().unwrap_or_default(),
        ] {
            for byte in value.bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            hash ^= 0xff;
        }
        for alias in descriptor
            .aliases()
            .iter()
            .chain(descriptor.slash_aliases())
        {
            for byte in alias.bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    hash
}

#[async_trait]
impl ContextPromptSource for SkillPromptSource {
    async fn materialize(
        &self,
        request: &ContextRequest,
    ) -> Result<PromptMaterialization, PromptMaterializationError> {
        let (mut cacheable, uncached) =
            crate::adapters::BaselinePromptSource::baseline_blocks(request);
        let descriptors = sort_and_dedup(self.catalog.list(self.query_factory.query(request)));
        let budget = skill_prompt_budget(request.context_size);
        let header_tokens =
            crate::domain::estimate_tokens(&render_skills_block(&[], request.language.as_str()));
        let selected = select_within_budget(&descriptors, budget, header_tokens);

        if !selected.is_empty() {
            let block = crate::domain::SystemBlock {
                kind: "skills".to_string(),
                content: render_skills_block(&selected, request.language.as_str()),
                cacheable: true,
                cache_break: false,
            };
            let index = cacheable
                .iter()
                .position(|item| item.kind == "execution_discipline");
            match index {
                Some(index) => cacheable.insert(index + 1, block),
                None => cacheable.push(block),
            }
        }

        Ok(PromptMaterialization {
            cacheable,
            uncached,
            revision: metadata_revision(&descriptors),
        })
    }
}
