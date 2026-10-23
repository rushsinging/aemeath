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
use crate::domain::lesson::{EvidenceBinding, LessonEntry, LessonStore, SignalKind};
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

/// 一轮改进的材料（行为信号自动采集随信号装配接入扩展）。
pub struct ImprovementMaterials {
    /// 参与编辑面的组件当前内容。
    pub component_contents: Vec<(ComponentId, ComponentContent)>,
    /// Active 经验（改进级递归的复利输入：历史 lesson 指导本次提案）。
    pub lessons: Vec<LessonEntry>,
    /// 执行证据（#2048：session 行为聚合信号，改进的主要依据）。
    pub evidence: crate::domain::evidence::EvidenceBundle,
}

/// 逐块提案的 LLM 应答信封：NEVER 裸 ChangeSet——LLM 需要能表达
/// 「证据不支持任何变更」，该表达是合法结果而非失败。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum BlockProposal {
    /// 采纳提案（omitted-is-inherited：只携带要改的组件）。
    /// `reason` 为提案依据（证据绑定的摘录来源）。
    Propose {
        reason: String,
        change_set: ChangeSet,
    },
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

/// 组装单块提案 prompt（结构化材料 + 经验 + 编辑契约 + 输出格式）。
pub fn build_block_prompt(
    chunk: &[(ComponentId, ComponentContent)],
    lessons: &[LessonEntry],
    evidence: &crate::domain::evidence::EvidenceBundle,
) -> String {
    let lessons_block = if lessons.is_empty() {
        "（暂无历史经验）".to_string()
    } else {
        lessons
            .iter()
            .map(|entry| format!("- [{}] {}", entry.component_kind, entry.lesson))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let evidence_block = render_evidence_block(evidence);
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

近期执行证据（session 行为聚合信号——改进的主要依据）：
{evidence_block}

历史经验（此前采纳的改进结论，供参考、可被新证据推翻）：
{lessons_block}

任务：基于执行证据判断组件是否存在值得改进的问题（缺失、冗余、错误、
空壳占位、重复失败未覆盖的规则）。空壳占位（TODO、无实际内容的正文）
视为应修复的缺陷——为其写出有用的实际内容。每条编辑 MUST 指出它回应
证据中的哪条信号（reason 中引用其 signal id，如 repeated_failure:Bash:…）；
无法对应任何信号的编辑是投机，NEVER 提出。证据与组件现状都不足时选择
no_change——那是一个正当答案，比牵强的编辑更好。

编辑契约：
- 提案只携带要变更的组件（未提及的组件自动保持原样）
- 任一条目校验失败会作废整份提案

只输出一个 JSON 对象，格式二选一：
{{"decision":"propose","reason":"<提案依据，一句话>","change_set":{{"edits":[{{"action":"upsert","id":{{"kind":"skill","name":"<名>"}},"content":{{"frontmatter":null,"body":"<完整正文>"}}}}]}}}}
{{"decision":"no_change","reason":"<一句话理由>"}}
不要输出 JSON 以外的任何文字。"#
    )
}

/// 解析单块 LLM 应答（信封 → BlockProposal）。
///
/// 流式取首个 JSON 值：容忍模型在 JSON 后追加的说明文字或
/// markdown 围栏残迹（对齐反思链路的解析容忍形态）。
pub fn parse_block_proposal(response_text: &str) -> Result<BlockProposal, HarnessError> {
    let trimmed = response_text.trim();
    let payload_start = trimmed
        .find('{')
        .ok_or_else(|| HarnessError::invalid(ERROR_DOMAIN, "提案应答不含 JSON 对象"))?;
    let json_source = trimmed
        .get(payload_start..)
        .ok_or_else(|| HarnessError::invalid(ERROR_DOMAIN, "提案应答载荷切分越界"))?;
    let mut stream = serde_json::Deserializer::from_str(json_source).into_iter::<BlockProposal>();
    match stream.next() {
        Some(Ok(proposal)) => Ok(proposal),
        Some(Err(error)) => Err(HarnessError::invalid(
            ERROR_DOMAIN,
            format!("提案应答解析失败：{error}"),
        )),
        None => Err(HarnessError::invalid(ERROR_DOMAIN, "提案应答 JSON 流为空")),
    }
}

/// 单块执行：LLM 提案 → 解析 → registry 预检/落盘。
/// 返回 (applied, outcome_label)；解析失败或校验作废都只作废该块。
async fn run_block(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    chunk: &[(ComponentId, ComponentContent)],
    lessons: &[LessonEntry],
    evidence: &crate::domain::evidence::EvidenceBundle,
    lesson_store: &LessonStore,
    cycle_ref: &str,
) -> (Vec<AppliedEdit>, &'static str) {
    let prompt = build_block_prompt(chunk, lessons, evidence);
    let Ok(response) = llm.complete(&prompt).await else {
        return (Vec::new(), "作废（LLM 调用失败）");
    };
    let proposal = match parse_block_proposal(&response) {
        Ok(proposal) => proposal,
        Err(error) => {
            eprintln!(
                "[e2e-diag] err={}; tail={}",
                error.message(),
                response
                    .chars()
                    .rev()
                    .take(300)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            );
            log::warn!(target: crate::LOG_TARGET,
                "提案应答解析失败（前 200 字符）：{}…；错误：{}",
                response.chars().take(200).collect::<String>(), error.message());
            return (Vec::new(), "作废（应答解析失败）");
        }
    };
    match proposal {
        BlockProposal::NoChange { .. } => (Vec::new(), "无变更"),
        BlockProposal::Propose { reason, change_set } => {
            match apply_change_set(registry, &change_set) {
                Ok(applied) => {
                    record_adopted_lesson(lesson_store, &change_set, &reason, cycle_ref);
                    (applied, "采纳")
                }
                Err(_) => (Vec::new(), "作废（校验失败）"),
            }
        }
    }
}

/// 采纳回写：每条采纳的 edit MUST 产生带证据的 Active lesson。
/// 回写失败只 warn（经验库 IO 故障 NEVER 作废已落盘的组件变更）。
fn record_adopted_lesson(
    lesson_store: &LessonStore,
    change_set: &ChangeSet,
    reason: &str,
    cycle_ref: &str,
) {
    for edit in &change_set.edits {
        let (id, action) = match edit {
            crate::domain::changeset::ComponentEdit::Upsert { id, .. } => (id, "upsert"),
            crate::domain::changeset::ComponentEdit::Delete { id } => (id, "delete"),
        };
        let entry = LessonEntry::new(
            format!("{cycle_ref}:{id}"),
            id.kind.as_str(),
            format!("{action} {id}：{reason}"),
            vec![EvidenceBinding {
                run_ref: cycle_ref.to_string(),
                signal: signal_kind_from_reason(reason),
                excerpt: reason.chars().take(200).collect(),
            }],
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0),
        );
        match entry {
            Ok(entry) => {
                if let Err(error) = lesson_store.append(&entry) {
                    log::warn!(target: crate::LOG_TARGET, "经验回写失败（组件变更已生效）：{error}");
                }
            }
            Err(error) => {
                log::warn!(target: crate::LOG_TARGET, "经验条目构造失败（跳过回写）：{error}");
            }
        }
    }
}

/// 执行一轮完整改进周期（分块串行、块级幂等：单块失败不影响其余块）。
pub async fn run_improvement_cycle(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    lesson_store: &LessonStore,
    config: &RsiConfig,
    evidence: crate::domain::evidence::EvidenceBundle,
) -> Result<CycleOutcome, HarnessError> {
    let cycle_ref = format!(
        "cycle-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0)
    );
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
    let lessons = lesson_store.load_active()?;
    let materials = ImprovementMaterials {
        component_contents,
        lessons: lessons.clone(),
        evidence: evidence.clone(),
    };
    let chunks = chunk_materials(materials, config.chunk_max_components);

    let mut adopted = 0usize;
    let mut no_change = 0usize;
    let mut discarded = 0usize;
    for chunk in &chunks {
        let (_applied, label) = run_block(
            registry,
            llm,
            chunk,
            &lessons,
            &evidence,
            lesson_store,
            &cycle_ref,
        )
        .await;
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

/// 渲染 evidence 块（空证据兜底；按优先级序，signal id 供提案引用）。
fn render_evidence_block(evidence: &crate::domain::evidence::EvidenceBundle) -> String {
    if evidence.signals.is_empty() {
        return "（近期无执行证据）".to_string();
    }
    // 渲染层按 kind 优先级排序（聚合保证顺序，此处防御外部构造的 bundle；
    // #2049 预算裁剪同用该优先级）。
    let mut ordered: Vec<_> = evidence.signals.iter().collect();
    ordered.sort_by_key(|signal| signal.kind.render_priority());
    ordered
        .into_iter()
        .map(|signal| {
            format!(
                "- {} | {}{}",
                signal.signal_id,
                signal.summary,
                signal
                    .excerpt
                    .as_deref()
                    .map(|excerpt| format!(" | 摘录: {excerpt}"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 从提案 reason 提取引用的信号类别（v1 启发：signal id 前缀匹配；
/// 无引用时兜底 CycleObservation——模型自述）。
pub(crate) fn signal_kind_from_reason(reason: &str) -> SignalKind {
    for (prefix, kind) in [
        ("repeated_failure:", SignalKind::RepeatedFailure),
        ("retry_pattern:", SignalKind::RetryPattern),
        ("oversized_result:", SignalKind::OversizedResult),
        ("run_outcome:", SignalKind::RunOutcome),
    ] {
        if reason.contains(prefix) {
            return kind;
        }
    }
    SignalKind::CycleObservation
}
