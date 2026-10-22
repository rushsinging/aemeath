//! HarnessRsi 改进循环：材料分块 → LLM 逐块提案 → 块级落盘。
//!
//! 单轴单步（0.2.0 范围）：不做 invariant/domain 双轴与多 rollout 探测。
//! 行为信号与经验库（lesson）输入由 #2017 接入——本文件的
//! [`ImprovementMaterials`] 是唯一装配点，接入时扩字段即可。

use crate::application::ComponentRegistry;
use crate::constants::ERROR_DOMAIN;
use crate::domain::changeset::{apply_change_set, AppliedEdit, ChangeSet};
use crate::domain::component::{ComponentContent, ComponentId};
use crate::domain::errors::HarnessError;
use crate::domain::task::CycleOutcome;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use share::config::RsiConfig;

/// LLM 补全端口（daemon 侧唯一模型入口；生产装配在 composition 经
/// provider 实现，测试注入桩）。
#[async_trait]
pub trait RsiLlmPort: Send + Sync {
    /// 单轮文本补全（非流式）。
    async fn complete(&self, prompt: &str) -> Result<String, HarnessError>;
}

/// 一轮改进的材料（唯一装配点：行为信号与经验库随 #2017 扩入）。
pub struct ImprovementMaterials {
    /// 参与编辑面的组件当前内容。
    pub component_contents: Vec<(ComponentId, ComponentContent)>,
}

/// 逐块提案的 LLM 应答信封：NEVER 裸 ChangeSet——LLM 需要能表达
/// 「证据不支持任何变更」，该表达是合法结果而非失败。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum BlockProposal {
    /// 采纳提案（omitted-is-inherited：只携带要改的组件）。
    Propose { change_set: ChangeSet },
    /// 无变更：证据不支持任何编辑（合法结果）。
    NoChange { reason: String },
}

/// 按块大小分组组件材料。
pub fn chunk_materials(
    materials: ImprovementMaterials,
    chunk_max_components: usize,
) -> Vec<Vec<(ComponentId, ComponentContent)>> {
    let chunk_size = chunk_max_components.max(1);
    materials
        .component_contents
        .chunks(chunk_size)
        .map(<[(ComponentId, ComponentContent)]>::to_vec)
        .collect()
}

/// 组装单块提案 prompt（结构化材料 + 编辑契约 + 输出格式）。
pub fn build_block_prompt(chunk: &[(ComponentId, ComponentContent)]) -> String {
    let mut components_json = Vec::with_capacity(chunk.len());
    for (id, content) in chunk {
        components_json.push(serde_json::json!({
            "id": id,
            "frontmatter": content.frontmatter,
            "body": content.body,
        }));
    }
    let materials_block = serde_json::to_string(&components_json).expect("组件材料序列化不可失败");
    format!(
        r#"你是 harness 组件编辑器。以下是若干组件的当前内容：

{materials_block}

任务：基于材料判断是否存在值得改进的组件内容问题（缺失、冗余、错误）。
只有当材料本身能支撑某个具体编辑时才提案；证据不足时选择 no_change——
那是一个正当答案，比牵强的编辑更好。

编辑契约：
- 提案只携带要变更的组件（未提及的组件自动保持原样）
- 任一条目校验失败会作废整份提案

只输出一个 JSON 对象，格式二选一：
{{"decision":"propose","change_set":{{"edits":[{{"action":"upsert","id":{{"kind":"skill","name":"<名>","content":…}}]}}}}
{{"decision":"no_change","reason":"<一句话理由>"}}
不要输出 JSON 以外的任何文字。"#
    )
}

/// 解析单块 LLM 应答（信封 → BlockProposal）。
pub fn parse_block_proposal(response_text: &str) -> Result<BlockProposal, HarnessError> {
    serde_json::from_str::<BlockProposal>(response_text.trim())
        .map_err(|error| HarnessError::invalid(ERROR_DOMAIN, format!("提案应答解析失败：{error}")))
}

/// 单块执行：LLM 提案 → 解析 → registry 预检/落盘。
/// 返回 (applied, outcome_label)；解析失败或校验作废都只作废该块。
async fn run_block(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    chunk: &[(ComponentId, ComponentContent)],
) -> (Vec<AppliedEdit>, &'static str) {
    let prompt = build_block_prompt(chunk);
    let Ok(response) = llm.complete(&prompt).await else {
        return (Vec::new(), "作废（LLM 调用失败）");
    };
    let proposal = match parse_block_proposal(&response) {
        Ok(proposal) => proposal,
        Err(_) => return (Vec::new(), "作废（应答解析失败）"),
    };
    match proposal {
        BlockProposal::NoChange { .. } => (Vec::new(), "无变更"),
        BlockProposal::Propose { change_set } => match apply_change_set(registry, &change_set) {
            Ok(applied) => (applied, "采纳"),
            Err(_) => (Vec::new(), "作废（校验失败）"),
        },
    }
}

/// 执行一轮完整改进周期（分块串行、块级幂等：单块失败不影响其余块）。
pub async fn run_improvement_cycle(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    config: &RsiConfig,
) -> Result<CycleOutcome, HarnessError> {
    let ids = registry.list();
    if ids.is_empty() {
        return Ok(CycleOutcome {
            summary: "无组件：空周期".to_string(),
        });
    }
    let mut component_contents = Vec::with_capacity(ids.len());
    for id in &ids {
        component_contents.push((id.clone(), registry.read(id)?));
    }
    let materials = ImprovementMaterials { component_contents };
    let chunks = chunk_materials(materials, config.chunk_max_components);

    let mut adopted = 0usize;
    let mut no_change = 0usize;
    let mut discarded = 0usize;
    for chunk in &chunks {
        let (_applied, label) = run_block(registry, llm, chunk).await;
        match label {
            "采纳" => adopted += 1,
            "无变更" => no_change += 1,
            _ => discarded += 1,
        }
        log::info!(target: crate::LOG_TARGET, "改进块完成：{label}");
    }
    Ok(CycleOutcome {
        summary: format!("采纳 {adopted} / 无变更 {no_change} / 作废 {discarded}"),
    })
}
