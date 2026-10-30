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
use crate::domain::lesson::{EvidenceBinding, LessonEntry, LessonStore};
use crate::domain::task::CycleOutcome;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use share::config::RsiConfig;

/// RSI LLM usage 快照（provider 未报告的字段保持 `None`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RsiLlmUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
}

/// RSI LLM 终态：文本与 provider 报告的 usage MUST 一起保留。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RsiLlmCompletion {
    pub text: String,
    pub usage: RsiLlmUsage,
}

impl RsiLlmCompletion {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            usage: RsiLlmUsage::default(),
        }
    }
}

/// LLM 补全端口（daemon 侧唯一模型入口；生产装配在 composition 经
/// provider 实现，测试注入桩）。
#[async_trait]
pub trait RsiLlmPort: Send + Sync {
    /// 单轮文本补全（非流式；兼容简单调用面）。
    async fn complete(&self, prompt: &str) -> Result<String, HarnessError>;

    /// 单轮文本补全并保留 token usage（生产端口 MUST 覆写）。
    async fn complete_with_usage(&self, prompt: &str) -> Result<RsiLlmCompletion, HarnessError> {
        Ok(RsiLlmCompletion::text(self.complete(prompt).await?))
    }
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
    /// `reason` 为提案依据；`signal_ids` 是结构化证据引用（Block 路径非空）。
    Propose {
        reason: String,
        #[serde(default)]
        signal_ids: Vec<String>,
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

/// 诊断阶段产出：一条待落地的改进任务（lesson 的载体）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosedIssue {
    /// 目标组件（kind/name 形态字符串）。
    pub component: String,
    /// 具体证据信号 id（如 repeated_failure:Bash:permission denied）。
    pub target_signal: String,
    /// 问题陈述（有界）。
    pub problem: String,
    /// 改进方向（有界，不含具体正文）。
    pub direction: String,
}

/// 跨块诊断汇总（#2078 修订）：同一 (component, target_signal) 合并
/// 计数——执行派发与强度门禁（#2092）消费本结构，NEVER 逐块决策。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiagnosisSummary {
    /// 合并后的改进任务（按出现次数降序）。
    pub issues: Vec<AggregatedIssue>,
}

/// 汇总后的单条任务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregatedIssue {
    pub component: String,
    pub target_signal: String,
    pub problem: String,
    pub direction: String,
    /// 跨块出现次数（同 component+signal 计数）。
    pub occurrences: usize,
}

impl DiagnosisSummary {
    /// 把各块诊断结果合并（同 component+target_signal 累加次数，
    /// 保留首次出现的 problem/direction）。
    pub fn merge(per_block: Vec<Vec<DiagnosedIssue>>) -> Self {
        let mut aggregated: Vec<AggregatedIssue> = Vec::new();
        for issues in per_block {
            for issue in issues {
                match aggregated.iter_mut().find(|existing| {
                    existing.component == issue.component
                        && existing.target_signal == issue.target_signal
                }) {
                    Some(existing) => existing.occurrences += 1,
                    None => aggregated.push(AggregatedIssue {
                        component: issue.component,
                        target_signal: issue.target_signal,
                        problem: issue.problem,
                        direction: issue.direction,
                        occurrences: 1,
                    }),
                }
            }
        }
        aggregated.sort_by_key(|issue| std::cmp::Reverse(issue.occurrences));
        Self { issues: aggregated }
    }

    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }
}

/// 诊断应答信封：NEVER 裸数组——模型需要能表达「证据不足以支撑任何改进」。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum DiagnosisProposal {
    Diagnose {
        reason: String,
        issues: Vec<DiagnosedIssue>,
    },
    NoIssue {
        reason: String,
    },
}

/// 诊断阶段 prompt（#2078）：只回答「什么问题、依据哪条信号、往哪改」，
/// NEVER 产出具体正文——执行阶段才写内容。
pub fn build_diagnosis_prompt(
    chunk: &[(ComponentId, ComponentContent)],
    lessons: &[LessonEntry],
    evidence: &crate::domain::evidence::EvidenceBundle,
) -> String {
    build_diagnosis_prompt_with_strength(chunk, lessons, evidence, &[])
}

/// 带强度档位的诊断 prompt（#2092）：证据块逐条标注强度与处置，
/// 未达阈值（observe）的信号模型 MUST 不为其提案。
pub fn build_diagnosis_prompt_with_strength(
    chunk: &[(ComponentId, ComponentContent)],
    lessons: &[LessonEntry],
    evidence: &crate::domain::evidence::EvidenceBundle,
    verdicts: &[crate::domain::strength::StrengthVerdict],
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
    let evidence_block = if verdicts.is_empty() {
        render_evidence_block(evidence)
    } else {
        render_evidence_block_with_strength(evidence, verdicts)
    };
    let components_json: Vec<serde_json::Value> = chunk
        .iter()
        .map(|(id, content)| {
            serde_json::json!({
                "id": id,
                "frontmatter": content.frontmatter,
                "body": content.body,
            })
        })
        .collect();
    let materials_block = serde_json::to_string(&components_json).expect("组件材料序列化不可失败");
    format!(
        r#"你是 harness 问题诊断器（诊断阶段，不写正文）。以下是若干组件的当前内容：

{materials_block}

近期执行证据（session 行为聚合信号）：
{evidence_block}

历史经验（供参考、可被新证据推翻）：
{lessons_block}

任务：判断哪些问题值得改进，为每个问题产出一条「改进任务」：
- component：目标组件 id（必须是上面材料中的组件，形如 guardrails/default）
- target_signal：该问题对应证据中的**具体 signal id**（逐字引用，如
  repeated_failure:Bash:permission denied: /etc/shadow）
- problem：一句话说清问题是什么（谁在什么场景失败/代价多大）
- direction：改进方向（要覆盖什么场景、采取什么类型的措施；NEVER 写完整正文）

判断标准：
- 每条 task 的 target_signal MUST 逐字出现在上面的证据块中；无法对应的
  问题 NEVER 提出。
- 证据不足或组件现状已覆盖该场景时选择 no_issue——正当答案。
- 一个组件可有多条 task；总数不超过证据中不同 signal 的个数。
- 优先修：重复失败（repeated_failure）、同参重试（retry_pattern）、
  超长结果（oversized_result）、run 终态异常（run_outcome）。

只输出一个 JSON 对象，格式二选一：
{{"decision":"diagnose","reason":"<判断依据，一句话>","issues":[{{"component":"<kind>/<name>","target_signal":"<signal id>","problem":"<一句话>","direction":"<方向>"}}]}}
{{"decision":"no_issue","reason":"<一句话理由>"}}
不要输出 JSON 以外的任何文字。"#
    )
}

/// 解析诊断应答（流式取首个 JSON 值，容忍 trailing）。
pub fn parse_diagnosis(response_text: &str) -> Result<DiagnosisProposal, HarnessError> {
    let trimmed = response_text.trim();
    let payload_start = trimmed
        .find('{')
        .ok_or_else(|| HarnessError::invalid(ERROR_DOMAIN, "诊断应答不含 JSON 对象"))?;
    let json_source = trimmed
        .get(payload_start..)
        .ok_or_else(|| HarnessError::invalid(ERROR_DOMAIN, "诊断应答载荷切分越界"))?;
    let mut stream =
        serde_json::Deserializer::from_str(json_source).into_iter::<DiagnosisProposal>();
    match stream.next() {
        Some(Ok(proposal)) => Ok(proposal),
        Some(Err(error)) => Err(HarnessError::invalid(
            ERROR_DOMAIN,
            format!("诊断应答解析失败：{error}"),
        )),
        None => Err(HarnessError::invalid(ERROR_DOMAIN, "诊断应答 JSON 流为空")),
    }
}

/// 执行阶段 prompt（#2078）：消费诊断出的改进任务，写出具体组件修改。
pub fn build_execution_prompt(
    issues: &[DiagnosedIssue],
    chunk: &[(ComponentId, ComponentContent)],
) -> String {
    let issues_block = issues
        .iter()
        .map(|issue| {
            format!(
                "- 组件 {}：问题「{}」；方向「{}」；对应证据 {}\n",
                issue.component, issue.problem, issue.direction, issue.target_signal
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let components_json: Vec<serde_json::Value> = chunk
        .iter()
        .map(|(id, content)| {
            serde_json::json!({
                "id": id,
                "frontmatter": content.frontmatter,
                "body": content.body,
            })
        })
        .collect();
    let materials_block = serde_json::to_string(&components_json).expect("组件材料序列化不可失败");
    format!(
        r#"你是 harness 组件执行器（执行阶段）。以下是诊断阶段产出的改进任务：

{issues_block}
以下是对应组件的当前内容：

{materials_block}

任务：为每条改进任务写出具体的组件修改（完整正文），要求：
- **可执行**：规则必须写清「什么条件下」+「必须/禁止做什么」。
- **不破坏**：只改任务指向的部分；与任务无关的现有规则 MUST 原样保留。
- **不冲突**：新规则不得与现有内容矛盾；重复时只补差异部分。
- **成本意识**：always-on 组件每轮付费，用最短篇幅表达完整语义。
- 每条编辑的 reason MUST 引用该任务的 target_signal。

编辑契约：
- 提案只携带要变更的组件（未提及的组件自动保持原样）
- 任一条目校验失败会作废整份提案

只输出一个 JSON 对象，格式二选一：
{{"decision":"propose","reason":"<依据，引用 signal id>","change_set":{{"edits":[{{"action":"upsert","id":{{"kind":"skill","name":"<名>"}},"content":{{"frontmatter":null,"body":"<完整正文>"}}}}]}}}}
{{"decision":"no_change","reason":"<一句话理由>"}}
不要输出 JSON 以外的任何文字。"#
    )
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
视为应修复的缺陷——为其写出有用的实际内容。每条编辑 MUST 在 signal_ids
中结构化列出它回应的证据信号 id（如 repeated_failure:Bash:…）；无法对应
任何信号的编辑是投机，NEVER 提出。证据与组件现状都不足时选择
no_change——那是一个正当答案，比牵强的编辑更好。

写规则的质量标准（每次落笔先自检）：
- **可执行**：每条规则必须写清「什么条件下」+「必须/禁止做什么」，
  让模型在下次同类场景能照着做；禁止「要小心」「注意安全」这类空泛话。
- **可验证**：写下后自问「下次出现 repeated_failure 里的场景，这条规则
  能阻止它吗？」——不能就不要写。
- **不破坏**：只改证据指向的部分；与证据无关的现有规则 MUST 原样保留
  （保留原有结构与有效条款，NEVER 顺手重写整篇）。
- **不冲突**：新规则不得与现有内容矛盾；与已有条款重复时只补差异部分。
- **成本意识**：always-on 组件（guardrails/tool_guidance/overlay）每轮
  对话都要付费，越简洁越好——用最短的篇幅表达完整语义。

改进时优先关注的失效模式（证据命中哪类就修哪类，未命中不臆测）：
**证据中的每一类 repeated_failure 都应至少得到一条对应规则（除非现有规则
已覆盖该场景——此时在 reason 中说明哪条已覆盖）。**
- **token 与上下文**：输出冗长/话痨、重复指令、上下文浪费（同一文件反复读、
  无效工具调用）、always-on 段落膨胀——规则能合并就合并，能删就删。
- **工具异常调用**：用错工具（搜代码该 Grep 却用 Bash cat）、参数错误、
  同参数机械重试、忽略 stderr/错误详情就下结论。
- **skills 异常**：description 缺少触发词导致该用未用；不该触发时误触发；
  skill 步骤与当前工具面不符（过时）。
- **subagent 异常**：简单任务过度委派（成本失控）；委派 prompt 不自包含导致
  子代理猜上下文；委派后不整合结果；并发数与任务量不匹配。
- **执行纪律**：未读先写（Write/Edit 前不 Read）、跳过验证就声称完成、
  破坏性操作无确认、失败后不诊断根因只重试。
- **结论质量**：空泛建议、无证据断言、不复述关键约束就动手。

编辑契约：
- 提案只携带要变更的组件（未提及的组件自动保持原样）
- signal_ids 必须非空，且每个 id 都来自上方证据块；未知引用会作废整份提案
- 任一条目校验失败会作废整份提案

只输出一个 JSON 对象，格式二选一：
{{"decision":"propose","reason":"<提案依据，一句话>","signal_ids":["<证据块中的 signal id>"],"change_set":{{"edits":[{{"action":"upsert","id":{{"kind":"skill","name":"<名>"}},"content":{{"frontmatter":null,"body":"<完整正文>"}}}}]}}}}
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

/// 校验 Block 提案的结构化证据引用：非空、无重复且全部来自本块证据。
fn validate_block_proposal_signals(
    proposal: &BlockProposal,
    evidence: &crate::domain::evidence::EvidenceBundle,
) -> Result<(), HarnessError> {
    let BlockProposal::Propose { signal_ids, .. } = proposal else {
        return Ok(());
    };
    if signal_ids.is_empty() {
        return Err(HarnessError::invalid(
            ERROR_DOMAIN,
            "提案缺少 signal_ids：无法绑定执行证据",
        ));
    }
    let mut seen: Vec<&str> = Vec::with_capacity(signal_ids.len());
    for signal_id in signal_ids {
        if seen.contains(&signal_id.as_str()) {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("提案 signal_ids 重复引用：{signal_id}"),
            ));
        }
        seen.push(signal_id);
        if !evidence
            .signals
            .iter()
            .any(|signal| &signal.signal_id == signal_id)
        {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("提案引用了本块不存在的 signal id：{signal_id}"),
            ));
        }
    }
    Ok(())
}

/// 调用 LLM 并记录 token/耗时事件；usage 缺失时字段为 null，NEVER 伪造。
async fn complete_with_usage_event(
    llm: &dyn RsiLlmPort,
    prompt: &str,
    stage: &str,
    block_index: usize,
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
) -> Result<RsiLlmCompletion, HarnessError> {
    let started = std::time::Instant::now();
    let prompt_chars = prompt.chars().count();
    let result = llm.complete_with_usage(prompt).await;
    let duration_ms = started.elapsed().as_millis() as u64;
    let event = match &result {
        Ok(completion) => crate::domain::events::RsiCycleEvent::LlmCall {
            stage: stage.to_string(),
            block_index,
            duration_ms,
            prompt_chars,
            response_chars: completion.text.chars().count(),
            usage: completion.usage,
            error: None,
        },
        Err(error) => crate::domain::events::RsiCycleEvent::LlmCall {
            stage: stage.to_string(),
            block_index,
            duration_ms,
            prompt_chars,
            response_chars: 0,
            usage: RsiLlmUsage::default(),
            error: Some(error.message().chars().take(120).collect()),
        },
    };
    if let Some(recorder) = recorder {
        recorder.record(&event);
    }
    result
}

/// 单块成功采纳后的材料（每个 edit 共享本次提案的证据绑定）。
struct AcceptedBlock {
    edits: Vec<AppliedEdit>,
    reason: String,
    evidence: Vec<EvidenceBinding>,
}

/// 单块执行：LLM 提案 → 解析 → registry 预检/落盘。
/// 返回 (applied, outcome_label)；解析失败或校验作废都只作废该块。
#[allow(clippy::too_many_arguments)]
async fn run_block(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    chunk: &[(ComponentId, ComponentContent)],
    lessons: &[LessonEntry],
    evidence: &crate::domain::evidence::EvidenceBundle,
    lesson_store: &LessonStore,
    cycle_ref: &str,
    dry_run: bool,
    preview_lines: &mut Vec<String>,
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
    block_index: usize,
    version_store: Option<&crate::domain::versioning::ComponentVersionStore>,
) -> (Option<AcceptedBlock>, &'static str) {
    // 单阶段（#2078 修订）：诊断/执行派发归 #2092，本 PR 的 diagnose_only
    // 模式走 run_diagnosis_cycle 的跨块汇总路径；本函数保持 v1 单阶段行为。
    let prompt = build_block_prompt(chunk, lessons, evidence);
    let completion =
        match complete_with_usage_event(llm, &prompt, "block", block_index, recorder).await {
            Ok(completion) => completion,
            Err(_) => return (None, "作废（LLM 调用失败）"),
        };
    let response = completion.text;
    let proposal = match parse_block_proposal(&response) {
        Ok(proposal) => {
            if let Some(recorder) = recorder {
                let (reason, edits) = match &proposal {
                    BlockProposal::Propose {
                        reason, change_set, ..
                    } => (reason.clone(), change_set.edits.len()),
                    BlockProposal::NoChange { reason } => (reason.clone(), 0),
                };
                recorder.record(&crate::domain::events::RsiCycleEvent::ProposalParsed {
                    block_index,
                    reason: reason.chars().take(200).collect(),
                    edits,
                });
            }
            proposal
        }
        Err(error) => {
            if let Some(recorder) = recorder {
                recorder.record(&crate::domain::events::RsiCycleEvent::ProposalRejected {
                    block_index,
                    stage: "parse".to_string(),
                    message: error.message().chars().take(120).collect(),
                });
            }
            log::warn!(target: crate::LOG_TARGET,
                "提案应答解析失败（前 200 字符）：{}…；错误：{}",
                response.chars().take(200).collect::<String>(), error.message());
            return (None, "作废（应答解析失败）");
        }
    };
    let proposal = match validate_block_proposal_signals(&proposal, evidence) {
        Ok(()) => proposal,
        Err(error) => {
            if let Some(recorder) = recorder {
                recorder.record(&crate::domain::events::RsiCycleEvent::ProposalRejected {
                    block_index,
                    stage: "signals".to_string(),
                    message: error.message().chars().take(120).collect(),
                });
            }
            log::warn!(target: crate::LOG_TARGET,
                "提案证据引用校验失败：{}", error.message());
            return (None, "作废（证据引用无效）");
        }
    };
    match proposal {
        BlockProposal::NoChange { .. } => (None, "无变更"),
        BlockProposal::Propose {
            reason,
            signal_ids,
            change_set,
        } => {
            let evidence_bindings = evidence_bindings_for(&signal_ids, evidence, cycle_ref);
            if dry_run {
                // 预演（#2050）：逐条校验（含整份作废预检）但不落盘、
                // 不写 lesson、不推游标；预览行供 preview 文件。
                let mut valid = true;
                for edit in &change_set.edits {
                    let (id, content) = match edit {
                        crate::domain::changeset::ComponentEdit::Upsert { id, content } => {
                            (id, Some(content))
                        }
                        crate::domain::changeset::ComponentEdit::Delete { id } => (id, None),
                    };
                    let outcome = match content {
                        Some(content) => registry.validate(id, content),
                        None => registry.read(id).map(|_| ()),
                    };
                    match outcome {
                        Ok(()) => preview_lines.push(render_dry_run_edit_summary(
                            registry,
                            id,
                            content,
                            reason.as_str(),
                        )),
                        Err(error) => {
                            valid = false;
                            preview_lines.push(format!("[dry-run] 拒绝 {id}：{}", error.message()));
                        }
                    }
                }
                return if valid {
                    // dry-run 的 registry 是内存 overlay：这里应用虚拟编辑，
                    // 让后续块与 Compose 看到叠加后的终态；真实 registry 不写。
                    match apply_change_set(registry, &change_set) {
                        Ok(applied) => (
                            Some(AcceptedBlock {
                                edits: applied,
                                reason,
                                evidence: evidence_bindings,
                            }),
                            "预演（将采纳）",
                        ),
                        Err(_) => (None, "预演（将作废）"),
                    }
                } else {
                    (None, "预演（将作废）")
                };
            }
            // #2074：采纳前逐 edit 快照旧内容（回滚依据；失败仅 warn——NEVER
            // 阻断采纳，但会丢失该次回滚能力）
            if let Some(version_store) = version_store {
                for edit in &change_set.edits {
                    let (id, action) = match edit {
                        crate::domain::changeset::ComponentEdit::Upsert { id, .. } => {
                            (id, "upsert")
                        }
                        crate::domain::changeset::ComponentEdit::Delete { id } => (id, "delete"),
                    };
                    if let Err(error) = version_store.capture(registry, id, cycle_ref, action) {
                        log::warn!(target: crate::LOG_TARGET, "组件快照失败（回滚能力缺失）：{error}");
                    }
                }
            }
            match apply_change_set(registry, &change_set) {
                Ok(applied) => {
                    if let Some(recorder) = recorder {
                        for edit in &change_set.edits {
                            let (component, action, old_chars, new_chars) = match edit {
                                crate::domain::changeset::ComponentEdit::Upsert { id, content } => {
                                    let old = registry
                                        .read(id)
                                        .map(|c| c.body.chars().count())
                                        .unwrap_or(0);
                                    (format!("{id}"), "upsert", old, content.body.chars().count())
                                }
                                crate::domain::changeset::ComponentEdit::Delete { id } => {
                                    let old = registry
                                        .read(id)
                                        .map(|c| c.body.chars().count())
                                        .unwrap_or(0);
                                    (format!("{id}"), "delete", old, 0)
                                }
                            };
                            recorder.record(&crate::domain::events::RsiCycleEvent::EditApplied {
                                component,
                                action: action.to_string(),
                                old_chars,
                                new_chars,
                            });
                        }
                    }
                    record_adopted_lesson(
                        lesson_store,
                        &change_set,
                        &reason,
                        cycle_ref,
                        recorder,
                        &|_| evidence_bindings.clone(),
                    );
                    (
                        Some(AcceptedBlock {
                            edits: applied,
                            reason,
                            evidence: evidence_bindings,
                        }),
                        "采纳",
                    )
                }
                Err(_) => (None, "作废（校验失败）"),
            }
        }
    }
}

/// 从结构化 signal 引用构造 lesson/adoption 共用的证据绑定。
fn evidence_bindings_for(
    signal_ids: &[String],
    evidence: &crate::domain::evidence::EvidenceBundle,
    cycle_ref: &str,
) -> Vec<EvidenceBinding> {
    signal_ids
        .iter()
        .filter_map(|signal_id| {
            evidence
                .signals
                .iter()
                .find(|signal| &signal.signal_id == signal_id)
                .map(|signal| EvidenceBinding {
                    run_ref: cycle_ref.to_string(),
                    signal: signal.kind,
                    excerpt: signal
                        .excerpt
                        .clone()
                        .unwrap_or_else(|| signal.summary.clone()),
                    signal_id: signal.signal_id.clone(),
                    source: signal.sources.first().cloned(),
                })
        })
        .collect()
}

/// Compose 只能修正本周期已采纳的组件，并把被修正提案的真实证据带到新 lesson。
/// `signal_ids` 可空（继承采纳证据）；非空时必须是采纳证据的子集。
fn compose_evidence_by_component(
    change_set: &ChangeSet,
    adoptions: &[crate::domain::task::AdoptionRecord],
    signal_ids: &[String],
) -> Result<Vec<(crate::domain::component::ComponentId, Vec<EvidenceBinding>)>, HarnessError> {
    let all_signal_ids: Vec<&str> = adoptions
        .iter()
        .flat_map(|adoption| adoption.evidence.iter())
        .map(|binding| binding.signal_id.as_str())
        .filter(|signal_id| !signal_id.is_empty())
        .collect();
    let mut seen_signals: Vec<&str> = Vec::new();
    for signal_id in signal_ids {
        if seen_signals.contains(&signal_id.as_str()) {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("Compose signal_ids 重复引用：{signal_id}"),
            ));
        }
        if !all_signal_ids.contains(&signal_id.as_str()) {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("Compose 引用了采纳证据中不存在的 signal id：{signal_id}"),
            ));
        }
        seen_signals.push(signal_id);
    }

    let mut by_component = Vec::new();
    for edit in &change_set.edits {
        let id = match edit {
            crate::domain::changeset::ComponentEdit::Upsert { id, .. }
            | crate::domain::changeset::ComponentEdit::Delete { id } => id,
        };
        let evidence: Vec<EvidenceBinding> = adoptions
            .iter()
            .filter(|adoption| &adoption.id == id)
            .flat_map(|adoption| adoption.evidence.iter().cloned())
            .collect();
        if evidence.is_empty() {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("Compose 只能修正本周期已采纳组件，禁止改 {id}"),
            ));
        }
        by_component.push((id.clone(), evidence));
    }
    Ok(by_component)
}

/// 采纳回写：每条采纳的 edit MUST 产生带证据的 Active lesson。
/// 回写失败只 warn（经验库 IO 故障 NEVER 作废已落盘的组件变更）。
fn record_adopted_lesson(
    lesson_store: &LessonStore,
    change_set: &ChangeSet,
    reason: &str,
    cycle_ref: &str,
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
    evidence_for: &dyn Fn(&crate::domain::component::ComponentId) -> Vec<EvidenceBinding>,
) {
    for edit in &change_set.edits {
        let (id, action) = match edit {
            crate::domain::changeset::ComponentEdit::Upsert { id, .. } => (id, "upsert"),
            crate::domain::changeset::ComponentEdit::Delete { id } => (id, "delete"),
        };
        let evidence_bindings = {
            let bindings = evidence_for(id);
            if bindings.is_empty() {
                vec![EvidenceBinding {
                    run_ref: cycle_ref.to_string(),
                    signal: crate::domain::evidence::SignalKind::CycleObservation,
                    excerpt: reason.chars().take(200).collect(),
                    signal_id: String::new(),
                    source: None,
                }]
            } else {
                bindings
            }
        };
        let entry = LessonEntry::new(
            format!("{cycle_ref}:{id}"),
            id.kind.as_str(),
            format!("{action} {id}：{reason}"),
            evidence_bindings.clone(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0),
        );
        match entry {
            Ok(entry) => {
                let mut entry = entry;
                // #2080：验证目标来自结构化 signal 绑定；Compose 仲裁无新证据时为空。
                entry.verification.target_signal = entry
                    .evidence
                    .first()
                    .map(|binding| binding.signal_id.clone())
                    .unwrap_or_default();
                let entry_id = entry.id.clone();
                let entry_signal = entry
                    .evidence
                    .first()
                    .map(|binding| binding.signal)
                    .unwrap_or(crate::domain::evidence::SignalKind::CycleObservation);
                // #2065 B：覆盖组件前退役旧 Active lesson（新现实取代旧经验）
                match lesson_store.retire_for_component(id) {
                    Ok(retired) if retired > 0 => {
                        if let Some(recorder) = recorder {
                            recorder.record(&crate::domain::events::RsiCycleEvent::LessonRetired {
                                component: format!("{id}"),
                                count: retired,
                            });
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        log::warn!(target: crate::LOG_TARGET, "旧 lesson 退役失败（继续追加新 lesson）：{error}");
                    }
                }
                match lesson_store.append(&entry) {
                    Ok(()) => {
                        if let Some(recorder) = recorder {
                            recorder.record(
                                &crate::domain::events::RsiCycleEvent::LessonRecorded {
                                    lesson_id: entry_id.clone(),
                                    signal: entry_signal.as_str().to_string(),
                                },
                            );
                        }
                    }
                    Err(error) => {
                        log::warn!(target: crate::LOG_TARGET, "经验回写失败（组件变更已生效）：{error}");
                    }
                }
            }
            Err(error) => {
                log::warn!(target: crate::LOG_TARGET, "经验条目构造失败（跳过回写）：{error}");
            }
        }
    }
}

/// 诊断优先周期（#2078 修订）：每块 1 次诊断（仅产出 issue），
/// 跨块汇总后返回——**不落盘**、不写 lesson，执行派发由 #2092 门禁决定。
#[allow(clippy::too_many_arguments)]
pub async fn run_diagnosis_cycle(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    config: &RsiConfig,
    evidence: crate::domain::evidence::EvidenceBundle,
    lessons: &[LessonEntry],
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
    report_lines: &mut Vec<String>,
    history: &mut crate::domain::strength::SignalHistory,
    cycle_id: &str,
) -> Result<DiagnosisSummary, HarnessError> {
    let ids = registry.list();
    let mut component_contents = Vec::with_capacity(ids.len());
    for id in &ids {
        component_contents.push((id.clone(), registry.read(id)?));
    }
    let materials = ImprovementMaterials {
        component_contents,
        lessons: lessons.to_vec(),
        evidence: evidence.clone(),
    };
    let chunks = chunk_materials(materials, config.chunk_max_components);
    // #2092 强度门禁：累计本周期证据 → 逐信号判定档位
    history.accumulate(cycle_id, &evidence.signals);
    let thresholds = crate::domain::strength::StrengthThresholds::default();
    // 按 signal_id 去重（多块会各自产出同一信号）——判定与报告都只做一次
    let mut unique_signals: Vec<&crate::domain::evidence::EvidenceSignal> = Vec::new();
    for signal in &evidence.signals {
        if !unique_signals
            .iter()
            .any(|existing| existing.signal_id == signal.signal_id)
        {
            unique_signals.push(signal);
        }
    }
    let verdicts: Vec<crate::domain::strength::StrengthVerdict> = unique_signals
        .iter()
        .map(|signal| {
            let input = history.build_input(cycle_id, signal, lessons);
            crate::domain::strength::judge_strength(&input, &thresholds)
        })
        .collect();
    for verdict in &verdicts {
        if let Some(recorder) = recorder {
            recorder.record(&crate::domain::events::RsiCycleEvent::StrengthGate {
                signal_id: verdict.signal_id.clone(),
                tier: verdict.tier.as_str().to_string(),
                rationale: verdict.rationale.clone(),
            });
        }
        report_lines.push(format!(
            "  [强度] {} → {}（{}）",
            verdict.signal_id,
            verdict.tier.as_str(),
            verdict.rationale
        ));
    }
    // observe 档的信号不进入提案材料（只观察）
    let actionable: Vec<&crate::domain::evidence::EvidenceSignal> = evidence
        .signals
        .iter()
        .filter(|signal| {
            verdicts
                .iter()
                .find(|verdict| verdict.signal_id == signal.signal_id)
                .map(|verdict| verdict.tier.allows_apply())
                .unwrap_or(true)
        })
        .collect();
    let mut gated_evidence = evidence.clone();
    gated_evidence.signals = actionable.into_iter().cloned().collect();
    let mut per_block: Vec<Vec<DiagnosedIssue>> = Vec::new();
    for (block_index, chunk) in chunks.iter().enumerate() {
        let prompt =
            build_diagnosis_prompt_with_strength(chunk, lessons, &gated_evidence, &verdicts);
        let issues =
            match complete_with_usage_event(llm, &prompt, "diagnosis", block_index, recorder).await
            {
                Ok(completion) => match parse_diagnosis(&completion.text) {
                    Ok(DiagnosisProposal::Diagnose { issues, .. }) => issues,
                    Ok(DiagnosisProposal::NoIssue { .. }) => Vec::new(),
                    Err(error) => {
                        if let Some(recorder) = recorder {
                            recorder.record(
                                &crate::domain::events::RsiCycleEvent::ProposalRejected {
                                    block_index,
                                    stage: "diagnosis_parse".to_string(),
                                    message: error.message().chars().take(120).collect(),
                                },
                            );
                        }
                        Vec::new()
                    }
                },
                Err(_) => Vec::new(),
            };
        if let Some(recorder) = recorder {
            recorder.record(&crate::domain::events::RsiCycleEvent::DiagnosisParsed {
                block_index,
                issues: issues.len(),
            });
        }
        report_lines.push(format!(
            "  块 {}/{}：诊断 {} 条任务",
            block_index + 1,
            chunks.len(),
            issues.len()
        ));
        per_block.push(issues);
    }
    let summary = DiagnosisSummary::merge(per_block);
    // #2092：按强度档位过滤——observe 档不进执行派发
    let strong_ids: std::collections::BTreeSet<String> = verdicts
        .iter()
        .filter(|verdict| verdict.tier.allows_apply())
        .map(|verdict| verdict.signal_id.clone())
        .collect();
    let dispatchable = summary.issues.iter().any(|issue| {
        strong_ids.contains(&issue.target_signal)
            || strong_ids
                .iter()
                .any(|signal_id| signal_id.contains(&issue.target_signal))
    });
    if !dispatchable && !summary.is_empty() {
        report_lines.push("  [派发] 全部任务未达落盘阈值（observe）——仅观察，不执行".to_string());
    }
    if let Some(recorder) = recorder {
        recorder.record(&crate::domain::events::RsiCycleEvent::DiagnosisSummarized {
            issues: summary.issues.len(),
            occurrences: summary.issues.iter().map(|issue| issue.occurrences).sum(),
        });
    }
    for issue in &summary.issues {
        report_lines.push(format!(
            "  [诊断汇总] {} ← {}（×{}）：{} → {}",
            issue.component, issue.target_signal, issue.occurrences, issue.problem, issue.direction
        ));
    }
    Ok(summary)
}

/// 执行一轮完整改进周期（分块串行、块级幂等：单块失败不影响其余块）。
#[allow(clippy::too_many_arguments)]
pub async fn run_improvement_cycle(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    lesson_store: &LessonStore,
    config: &RsiConfig,
    evidence: crate::domain::evidence::EvidenceBundle,
    dry_run: bool,
    preview_lines: &mut Vec<String>,
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
    version_store: Option<&crate::domain::versioning::ComponentVersionStore>,
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
            adopted: Vec::new(),
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
    let mut adoption_records: Vec<crate::domain::task::AdoptionRecord> = Vec::new();
    for (block_index, chunk) in chunks.iter().enumerate() {
        let (block_adoptions, label) = run_block(
            registry,
            llm,
            chunk,
            &lessons,
            &evidence,
            lesson_store,
            &cycle_ref,
            dry_run,
            preview_lines,
            recorder,
            block_index,
            version_store,
        )
        .await;
        if let Some(accepted) = block_adoptions {
            for edit in &accepted.edits {
                adoption_records.push(crate::domain::task::AdoptionRecord {
                    id: match edit {
                        AppliedEdit::Upserted { id } | AppliedEdit::Deleted { id } => id.clone(),
                    },
                    reason: accepted.reason.clone(),
                    evidence: accepted.evidence.clone(),
                });
            }
        }
        match label {
            "采纳" => adopted += 1,
            "无变更" => no_change += 1,
            // 预演 label（将采纳/将作废）由 preview_lines 呈现详情，
            // 计数只归入对应桶（#2050）
            "预演（将采纳）" => adopted += 1,
            "预演（将作废）" => discarded += 1,
            _ => discarded += 1,
        }
        log::info!(target: crate::LOG_TARGET, "改进块完成：{label}");
    }
    let prefix = if dry_run { "（dry-run）" } else { "" };
    Ok(CycleOutcome {
        summary: format!("{prefix}采纳 {adopted} / 无变更 {no_change} / 作废 {discarded}"),
        adopted: adoption_records,
    })
}

/// 渲染 evidence 块（空证据兜底；按优先级序，signal id 供提案引用）。
/// 渲染带强度档位的证据块（#2092）：每条标注 tier 与处置依据。
fn render_evidence_block_with_strength(
    evidence: &crate::domain::evidence::EvidenceBundle,
    verdicts: &[crate::domain::strength::StrengthVerdict],
) -> String {
    if evidence.signals.is_empty() {
        return "（近期无执行证据）".to_string();
    }
    let mut ordered: Vec<_> = evidence.signals.iter().collect();
    ordered.sort_by_key(|signal| signal.kind.render_priority());
    ordered
        .into_iter()
        .map(|signal| {
            let strength = verdicts
                .iter()
                .find(|verdict| verdict.signal_id == signal.signal_id)
                .map(|verdict| {
                    format!(" [强度: {} | {}]", verdict.tier.as_str(), verdict.rationale)
                })
                .unwrap_or_default();
            format!(
                "- {} | {}{}{}",
                signal.signal_id,
                signal.summary,
                strength,
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

/// dry-run 单条编辑的实在总结：动作 + 前后规模对比 + 新内容要点
/// （标题行；旧内容规模来自盘上版本，缺失标记新建）。
pub(crate) fn render_dry_run_edit_summary(
    registry: &mut dyn ComponentRegistry,
    id: &ComponentId,
    content: Option<&ComponentContent>,
    reason: &str,
) -> String {
    let action = if content.is_some() {
        "upsert"
    } else {
        "delete"
    };
    let mut summary = format!("[dry-run] {action} {id}");
    if let Some(content) = content {
        let new_lines = content.body.lines().count();
        let new_chars = content.body.chars().count();
        match registry.read(id) {
            Ok(old) => {
                let old_lines = old.body.lines().count();
                let old_chars = old.body.chars().count();
                summary.push_str(&format!(
                    "\n  变更：正文 {old_lines} 行 → {new_lines} 行（{old_chars} → {new_chars} 字符）"
                ));
            }
            Err(_) => summary.push_str(&format!(
                "\n  变更：新建正文（{new_lines} 行 / {new_chars} 字符）"
            )),
        }
        let highlights: Vec<&str> = content
            .body
            .lines()
            .map(str::trim_start)
            .filter(|line| line.starts_with('#'))
            .take(4)
            .collect();
        if !highlights.is_empty() {
            summary.push_str(&format!("\n  新内容要点：{}", highlights.join(" / ")));
        } else if let Some(first_meaningful) = content
            .body
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
        {
            let bounded: String = first_meaningful.chars().take(60).collect();
            summary.push_str(&format!("\n  新内容首行：{bounded}"));
        }
    } else {
        summary.push_str("（整组件删除）");
    }
    summary.push_str(&format!("\n  依据：{reason}"));
    summary
}

/// Compose 相似候选对：相似度只用于召回，最终由模型结合上下文判断。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SimilarCandidatePair {
    pub left_label: String,
    pub left_text: String,
    pub right_label: String,
    pub right_text: String,
    pub left_source: Option<crate::domain::evidence::EvidenceSourceRef>,
    pub right_source: Option<crate::domain::evidence::EvidenceSourceRef>,
    pub left_context: Option<String>,
    pub right_context: Option<String>,
    pub score: f64,
}

struct SimilarItem {
    label: String,
    text: String,
    signal_id: String,
    source: Option<crate::domain::evidence::EvidenceSourceRef>,
}

/// 从本周期提案与 Active lesson 中召回可能相似的对，并回读有界 session 上下文。
pub(crate) fn select_similar_candidate_pairs(
    adoptions: &[crate::domain::task::AdoptionRecord],
    lessons: &[LessonEntry],
    limit: usize,
    context_reader: &dyn crate::domain::stream_port::EvidenceStreamPort,
) -> Vec<SimilarCandidatePair> {
    let mut proposals: Vec<SimilarItem> = Vec::new();
    for adoption in adoptions {
        let binding = adoption.evidence.first();
        let signal_id = binding
            .map(|binding| binding.signal_id.clone())
            .unwrap_or_default();
        let text = format!("{} {}", adoption.reason, signal_id);
        proposals.push(SimilarItem {
            label: format!("本周期提案：{}", adoption.id),
            text,
            signal_id,
            source: binding.and_then(|binding| binding.source.clone()),
        });
    }
    let mut lesson_items: Vec<SimilarItem> = Vec::new();
    for lesson in lessons {
        let binding = lesson.evidence.first();
        let signal_id = if !lesson.verification.target_signal.is_empty() {
            lesson.verification.target_signal.clone()
        } else {
            binding
                .map(|binding| binding.signal_id.clone())
                .unwrap_or_default()
        };
        let text = format!("{} {} {}", lesson.lesson, lesson.component_kind, signal_id);
        lesson_items.push(SimilarItem {
            label: format!("Active lesson：{}", lesson.id),
            text,
            signal_id,
            source: binding.and_then(|binding| binding.source.clone()),
        });
    }

    let mut scored: Vec<(f64, SimilarItem, SimilarItem)> = Vec::new();
    for left_index in 0..proposals.len() {
        for right_index in (left_index + 1)..proposals.len() {
            push_scored_pair(&mut scored, &proposals[left_index], &proposals[right_index]);
        }
        for lesson in &lesson_items {
            push_scored_pair(&mut scored, &proposals[left_index], lesson);
        }
    }
    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(limit.max(1));

    let mut remaining_context_chars = crate::constants::COMPOSE_SIMILAR_CONTEXT_TOTAL_CHARS;
    let mut context_cache: std::collections::HashMap<
        String,
        Option<crate::domain::evidence::SessionContextExcerpt>,
    > = std::collections::HashMap::new();
    scored
        .into_iter()
        .map(|(score, left, right)| {
            let left_context = take_source_context(
                &left.source,
                context_reader,
                &mut context_cache,
                &mut remaining_context_chars,
            );
            let right_context = take_source_context(
                &right.source,
                context_reader,
                &mut context_cache,
                &mut remaining_context_chars,
            );
            SimilarCandidatePair {
                left_label: left.label,
                left_text: left.text,
                right_label: right.label,
                right_text: right.text,
                left_source: left.source,
                right_source: right.source,
                left_context,
                right_context,
                score,
            }
        })
        .collect()
}

fn push_scored_pair(
    scored: &mut Vec<(f64, SimilarItem, SimilarItem)>,
    left: &SimilarItem,
    right: &SimilarItem,
) {
    let score = if !left.signal_id.is_empty() && left.signal_id == right.signal_id {
        1.0
    } else {
        share::memory::dedup::jaccard_similarity(&left.text, &right.text)
    };
    if score < crate::constants::COMPOSE_SIMILARITY_CANDIDATE_THRESHOLD {
        return;
    }
    scored.push((
        score,
        SimilarItem {
            label: left.label.clone(),
            text: left.text.clone(),
            signal_id: left.signal_id.clone(),
            source: left.source.clone(),
        },
        SimilarItem {
            label: right.label.clone(),
            text: right.text.clone(),
            signal_id: right.signal_id.clone(),
            source: right.source.clone(),
        },
    ));
}

fn take_source_context(
    source: &Option<crate::domain::evidence::EvidenceSourceRef>,
    context_reader: &dyn crate::domain::stream_port::EvidenceStreamPort,
    cache: &mut std::collections::HashMap<
        String,
        Option<crate::domain::evidence::SessionContextExcerpt>,
    >,
    remaining: &mut usize,
) -> Option<String> {
    let source = source.as_ref()?;
    let key = format!("{}:{}:{}", source.session_id, source.run_id, source.step_id);
    let context = cache
        .entry(key)
        .or_insert_with(|| context_reader.read_context(source))
        .clone()?;
    if *remaining == 0 || context.text.trim().is_empty() {
        return None;
    }
    let text = crate::domain::evidence::truncate_with_marker(&context.text, *remaining);
    *remaining = remaining.saturating_sub(text.chars().count());
    Some(text)
}

/// 渲染 Compose 的候选相似对材料与判定规则。
pub(crate) fn render_compose_similarity_block(pairs: &[SimilarCandidatePair]) -> String {
    let mut lines = vec![
        "判定规则：".to_string(),
        "- 初筛相似度只是召回线索，NEVER 直接据此合并。".to_string(),
        "- 必须比较双方 session 上下文中的任务目标、工具行为、失败原因与约束。".to_string(),
        "- 上下文不同或缺失时不得判定为同类：保留各自规则，不得合并。".to_string(),
        "- 只有同一根因、同一场景、同一约束才可合并；合并 NEVER 丢语义。".to_string(),
        "- 与既有 lesson 语义重复时，优先保留既有规则，不重复新增条款。".to_string(),
    ];
    if pairs.is_empty() {
        lines.push("（无候选相似对）".to_string());
        return lines.join("\n");
    }
    for (index, pair) in pairs.iter().enumerate() {
        lines.push(format!(
            "\n候选 {}（初筛相似度 {:.2}）",
            index + 1,
            pair.score
        ));
        lines.push(format!("A｜{}：{}", pair.left_label, pair.left_text));
        lines.push(format!(
            "A 来源（session/run/step）：{}",
            render_evidence_source(pair.left_source.as_ref())
        ));
        lines.push(format!(
            "A 来源上下文：{}",
            pair.left_context.as_deref().unwrap_or("上下文不可用")
        ));
        lines.push(format!("B｜{}：{}", pair.right_label, pair.right_text));
        lines.push(format!(
            "B 来源（session/run/step）：{}",
            render_evidence_source(pair.right_source.as_ref())
        ));
        lines.push(format!(
            "B 来源上下文：{}",
            pair.right_context.as_deref().unwrap_or("上下文不可用")
        ));
    }
    lines.join("\n")
}

fn render_evidence_source(source: Option<&crate::domain::evidence::EvidenceSourceRef>) -> String {
    source
        .map(|source| format!("{}/{}/{}", source.session_id, source.run_id, source.step_id))
        .unwrap_or_else(|| "来源不可用".to_string())
}

/// 周期 Compose 汇总块（#2065 A，对齐 Harness² Compose）：全部数据源块
/// 执行完后纵观本周期全部采纳——检测矛盾/合并/精炼，产出终版变更。
/// 无采纳时跳过（NEVER 为凑 Compose 而造编辑）。
#[allow(clippy::too_many_arguments)]
pub async fn run_compose_phase(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    lesson_store: &LessonStore,
    adoptions: &[crate::domain::task::AdoptionRecord],
    active_lessons: &[LessonEntry],
    context_reader: &dyn crate::domain::stream_port::EvidenceStreamPort,
    dry_run: bool,
    preview_lines: &mut Vec<String>,
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
    version_store: Option<&crate::domain::versioning::ComponentVersionStore>,
) -> Result<Option<CycleOutcome>, HarnessError> {
    if adoptions.is_empty() {
        return Ok(None);
    }
    let cycle_ref = format!(
        "compose-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0)
    );
    // 材料：采纳清单 + 涉及组件的当前终态
    let mut touched_ids: Vec<ComponentId> = Vec::new();
    for adoption in adoptions {
        if !touched_ids.contains(&adoption.id) {
            touched_ids.push(adoption.id.clone());
        }
    }
    let mut components_json = Vec::new();
    for id in &touched_ids {
        if let Ok(content) = registry.read(id) {
            components_json.push(serde_json::json!({
                "id": id,
                "frontmatter": content.frontmatter,
                "body": content.body,
            }));
        }
    }
    let adoptions_block: Vec<String> = adoptions
        .iter()
        .map(|adoption| format!("- {}：{}", adoption.id, adoption.reason))
        .collect();
    let materials_block = serde_json::to_string(&components_json).expect("组件材料序列化不可失败");
    let lessons_block = if active_lessons.is_empty() {
        "（暂无 Active lesson）".to_string()
    } else {
        active_lessons
            .iter()
            .map(|entry| {
                format!(
                    "- [{}] {}（target_signal: {}）",
                    entry.component_kind, entry.lesson, entry.verification.target_signal
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let similar_pairs = select_similar_candidate_pairs(
        adoptions,
        active_lessons,
        crate::constants::COMPOSE_SIMILAR_PAIR_LIMIT,
        context_reader,
    );
    let similar_block = render_compose_similarity_block(&similar_pairs);
    let prompt = format!(
        r#"你是 harness 仲裁器（Compose）。本周期多个证据块分别产生了以下采纳：

{adoptions}

涉及组件的当前（各块编辑叠加后的）终态：

{materials}

历史经验（Active lesson，供识别语义重复与既有覆盖）：

{lessons}

候选相似对与来源 session 上下文：

{similar}

任务：纵观全部采纳、终态与候选相似上下文——检测块间矛盾（同一问题不同
规则、重复条款、相互冲突的指令）、可合并项（多块各写一半的同类规则）与
可精炼项（冗余展开）。仅当发现真实矛盾或明确可合并/精炼时产出修正编辑；
各块结论一致且终态良好时选择 no_change——那是一个正当答案。

仲裁的判断标准（每次落笔先自检）：
- **矛盾**：同一场景下两条规则给出相反指令 → 保留更具体、更接近证据的
  那条，删除另一条。
- **合并**：多条规则讲同一主题的不同侧面 → 合成一条，**保留每条的独特
  信息**（合并 NEVER 丢语义）；合并前 MUST 依据候选对的来源上下文确认
  同一根因、同一场景、同一约束，初筛相似度与文本相似 NEVER 作为依据。
- **精炼**：只删重复表述与冗余展开，NEVER 删掉任何一条实质规则。
- **不引入**：本阶段只仲裁现有采纳，NEVER 凭想象新增未被证据支持的规则。
- **可执行优先**：合并/精炼后必须仍是「条件 + 动作」的可执行形态。

编辑契约同改进块（omitted-is-inherited；upsert 的 content.body 为
完整正文）。只输出一个 JSON 对象，格式二选一（NEVER 输出其他形态）：
{{"decision":"propose","reason":"<仲裁依据，一句话>","signal_ids":[],"change_set":{{"edits":[{{"action":"upsert","id":{{"kind":"<kind>","name":"<名>"}},"content":{{"frontmatter":null,"body":"<完整正文>"}}}}]}}}}
{{"decision":"no_change","reason":"<一句话理由>"}}
不要输出 JSON 以外的任何文字。"#,
        adoptions = adoptions_block.join("\n"),
        materials = materials_block,
        lessons = lessons_block,
        similar = similar_block,
    );
    let completion =
        match complete_with_usage_event(llm, &prompt, "compose", adoptions.len(), recorder).await {
            Ok(completion) => completion,
            Err(_) => {
                return Ok(Some(CycleOutcome {
                    summary: "Compose：LLM 调用失败（跳过仲裁）".to_string(),
                    adopted: Vec::new(),
                }));
            }
        };
    let response = completion.text;
    let label = match parse_block_proposal(&response) {
        Ok(BlockProposal::NoChange { .. }) => "无变更（各块结论一致）",
        Ok(BlockProposal::Propose {
            reason,
            signal_ids,
            change_set,
        }) => {
            let evidence_by_component =
                match compose_evidence_by_component(&change_set, adoptions, &signal_ids) {
                    Ok(by_component) => by_component,
                    Err(error) => {
                        let label = "仲裁修正（证据引用无效作废）";
                        log::warn!(target: crate::LOG_TARGET,
                            "Compose 证据校验失败：{}", error.message());
                        if let Some(recorder) = recorder {
                            recorder.record(&crate::domain::events::RsiCycleEvent::ComposeResult {
                                label: label.to_string(),
                            });
                        }
                        return Ok(Some(CycleOutcome {
                            summary: format!("Compose：{label}"),
                            adopted: Vec::new(),
                        }));
                    }
                };
            if dry_run {
                for edit in &change_set.edits {
                    let (id, content) = match edit {
                        crate::domain::changeset::ComponentEdit::Upsert { id, content } => {
                            (id, Some(content))
                        }
                        crate::domain::changeset::ComponentEdit::Delete { id } => (id, None),
                    };
                    preview_lines.push(format!(
                        "[dry-run][Compose] {}",
                        render_dry_run_edit_summary(registry, id, content, &reason)
                            .trim_start_matches("[dry-run] ")
                    ));
                }
                "仲裁修正（将采纳）"
            } else {
                // #2074：采纳前逐 edit 快照旧内容（回滚依据；失败仅 warn——NEVER
                // 阻断采纳，但会丢失该次回滚能力）
                if let Some(version_store) = version_store {
                    for edit in &change_set.edits {
                        let (id, action) = match edit {
                            crate::domain::changeset::ComponentEdit::Upsert { id, .. } => {
                                (id, "upsert")
                            }
                            crate::domain::changeset::ComponentEdit::Delete { id } => {
                                (id, "delete")
                            }
                        };
                        if let Err(error) = version_store.capture(registry, id, &cycle_ref, action)
                        {
                            log::warn!(target: crate::LOG_TARGET, "组件快照失败（回滚能力缺失）：{error}");
                        }
                    }
                }
                match apply_change_set(registry, &change_set) {
                    Ok(_) => {
                        // Compose 修正继承被仲裁采纳的真实证据；NEVER 用空绑定
                        // 抹掉 lesson 的 signal/source。
                        record_adopted_lesson(
                            lesson_store,
                            &change_set,
                            &reason,
                            &cycle_ref,
                            recorder,
                            &|id| {
                                evidence_by_component
                                    .iter()
                                    .find(|(component, _)| component == id)
                                    .map(|(_, evidence)| evidence.clone())
                                    .unwrap_or_default()
                            },
                        );
                        "仲裁修正（已采纳）"
                    }
                    Err(_) => "仲裁修正（校验失败作废）",
                }
            }
        }
        Err(_) => "解析失败（跳过仲裁）",
    };
    let summary = if dry_run {
        format!("（dry-run）Compose：{label}")
    } else {
        format!("Compose：{label}")
    };
    if let Some(recorder) = recorder {
        recorder.record(&crate::domain::events::RsiCycleEvent::ComposeResult {
            label: label.to_string(),
        });
    }
    Ok(Some(CycleOutcome {
        summary,
        adopted: Vec::new(),
    }))
}
