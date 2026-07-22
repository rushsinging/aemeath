//! Skill prompt pipeline — Context-private capability (Issue #912).
//!
//! `SkillPromptSource` 实现 [`ContextPromptSource`]：组合始终存在的基线块
//! （system_prompt + execution_discipline）与经 Skill-owned
//! [`SkillMaterializationPort`] 物化的技能片段。Pipeline 负责：
//!
//! - 保留基线块；
//! - 调用 `SkillMaterializationPort::materialize_available`；
//! - 按 `stable_key` 确定性排序并保留首项去重；
//! - 对每个 content 调 `scan_content` 并 log warning；
//! - 在 Context-owned 预算内确定性选择；
//! - 将 skills 渲染为单个 cacheable system block，位于 execution_discipline 后；
//! - revision 确定性包含 supplier revision。

use std::sync::Arc;

use async_trait::async_trait;
use tools::{
    PromptFragment, SkillMaterializationPort, SkillMaterializationQuery,
    SkillMaterializationRevision,
};

use crate::domain::ContextRequest;
use crate::ports::{
    ContextPromptSource, PromptMaterialization, PromptMaterializationError, SkillQueryFactory,
};
use crate::LOG_TARGET;

/// Context-owned Skill prompt pipeline。
pub struct SkillPromptSource {
    materializer: Arc<dyn SkillMaterializationPort>,
    query_factory: Arc<dyn SkillQueryFactory>,
}

impl SkillPromptSource {
    pub fn new(
        materializer: Arc<dyn SkillMaterializationPort>,
        query_factory: Arc<dyn SkillQueryFactory>,
    ) -> Self {
        Self {
            materializer,
            query_factory,
        }
    }
}

/// 生产查询工厂：持有 live Project `WorkspaceRead`，每次 `materialize_query`
/// 从 `current_workspace_root()` 推导 project root，不捕获启动 cwd。
pub struct WorkspaceSkillQueryFactory {
    workspace: Arc<dyn project::WorkspaceRead>,
}

impl WorkspaceSkillQueryFactory {
    pub fn new(workspace: Arc<dyn project::WorkspaceRead>) -> Self {
        Self { workspace }
    }
}

impl SkillQueryFactory for WorkspaceSkillQueryFactory {
    fn materialize_query(&self, request: &ContextRequest) -> SkillMaterializationQuery {
        let project_root = self.workspace.current_workspace_root();
        let extra_dirs = request.config_snapshot.skills().dirs.clone();
        let available_tools = request
            .tool_schemas
            .iter()
            .map(|schema| schema.name.clone())
            .collect();
        SkillMaterializationQuery::new(project_root, extra_dirs, available_tools)
    }
}

// ── 纯管线逻辑（无 IO，可单测） ──────────────────────────────────────────

/// 按 `stable_key` 确定性排序（稳定排序）并保留首项去重。
pub(crate) fn sort_and_dedup(mut fragments: Vec<PromptFragment>) -> Vec<PromptFragment> {
    // 稳定排序：同 stable_key 的副本保留 supplier 顺序。
    fragments.sort_by(|a, b| a.stable_key().cmp(b.stable_key()));
    fragments.dedup_by(|a, b| a.stable_key() == b.stable_key());
    fragments
}

/// 确定性 skill prompt 预算：`context_size` 的明确分数上限（1/8，下限 1024）。
///
/// 该公式是显式的、确定性的，并被测试锁定。变化只在显式修改本函数时发生。
pub fn skill_prompt_budget(context_size: usize) -> usize {
    (context_size / 8).max(1_024)
}

/// 在预算内确定性选择片段。
///
/// 输入须已排序去重。按顺序贪心累加，直到累计 token（含 header 开销）超过预算。
fn select_within_budget(
    fragments: &[PromptFragment],
    budget_tokens: usize,
    header_tokens: usize,
) -> Vec<PromptFragment> {
    let mut used = header_tokens;
    let mut selected = Vec::new();
    for fragment in fragments {
        let cost = crate::domain::estimate_tokens(fragment.content());
        if used.saturating_add(cost) > budget_tokens {
            break;
        }
        used += cost;
        selected.push(fragment.clone());
    }
    selected
}

/// 将选中的片段渲染为单个 system block 正文。
fn render_skills_block(selected: &[PromptFragment], lang: &str) -> String {
    let header = if lang.eq_ignore_ascii_case("zh") {
        "# 可用技能\n"
    } else {
        "# Available Skills\n"
    };
    let body: Vec<&str> = selected.iter().map(|f| f.content()).collect();
    if body.is_empty() {
        format!("{header}\n")
    } else {
        format!("{header}{}\n", body.join("\n\n"))
    }
}

/// 确定性将 supplier revision 折叠为 `u64`，使 revision 变化可被上层缓存感知。
fn fold_supplier_revision(supplier: &SkillMaterializationRevision) -> u64 {
    // FNV-1a 64 over the supplier revision 字符串，确定性且分布均匀。
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in supplier.as_str().bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn scan_fragments(fragments: &[PromptFragment]) {
    for fragment in fragments {
        let warnings =
            crate::adapters::prompt::scan_content(fragment.stable_key(), fragment.content());
        if !warnings.is_empty() {
            log::warn!(
                target: LOG_TARGET,
                "Security scan warnings in skill `{}` from `{}`: {:?}",
                fragment.stable_key(),
                fragment.source().path,
                warnings
            );
        }
    }
}

#[async_trait]
impl ContextPromptSource for SkillPromptSource {
    async fn materialize(
        &self,
        request: &ContextRequest,
    ) -> Result<PromptMaterialization, PromptMaterializationError> {
        let (mut cacheable, uncached) =
            crate::adapters::BaselinePromptSource::baseline_blocks(request);

        // 1. 经 Context-owned query factory 构造查询（使用 live workspace root）。
        let query = self.query_factory.materialize_query(request);

        // 2. 调用 SkillMaterializationPort；typed 错误原样传播。
        let snapshot = self
            .materializer
            .materialize_available(query)
            .await
            .map_err(PromptMaterializationError::SkillSupplier)?;

        // 3. 按 stable_key 确定性排序并保留首项去重。
        let deduped = sort_and_dedup(snapshot.fragments().to_vec());

        // 4. 对每个 content 调 scan_content 并 log warning。
        scan_fragments(&deduped);

        // 5. 在 Context-owned 预算内确定性选择。
        let budget_tokens = skill_prompt_budget(request.context_size);
        // 先渲染 header 以计入开销，保证选择与最终渲染一致。
        let header_tokens =
            crate::domain::estimate_tokens(&render_skills_block(&[], request.language.as_str()));
        let selected = select_within_budget(&deduped, budget_tokens, header_tokens);

        // 6. 渲染为单个 cacheable system block，插入 execution_discipline 之后。
        if !selected.is_empty() {
            let content = render_skills_block(&selected, request.language.as_str());
            let discipline_idx = cacheable
                .iter()
                .position(|b| b.kind == "execution_discipline");
            let skills_block = crate::domain::SystemBlock {
                kind: "skills".to_string(),
                content,
                cacheable: true,
                cache_break: false,
            };
            match discipline_idx {
                Some(idx) => cacheable.insert(idx + 1, skills_block),
                None => cacheable.push(skills_block),
            }
        }

        // 7. revision 确定性包含 supplier revision。
        let revision = fold_supplier_revision(snapshot.revision());

        Ok(PromptMaterialization {
            cacheable,
            uncached,
            revision,
        })
    }
}
