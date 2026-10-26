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
视为应修复的缺陷——为其写出有用的实际内容。每条编辑 MUST 指出它回应
证据中的哪条信号（reason 中引用其 signal id，如 repeated_failure:Bash:…）；
无法对应任何信号的编辑是投机，NEVER 提出。证据与组件现状都不足时选择
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
) -> (Vec<(AppliedEdit, String)>, &'static str) {
    // 单阶段（#2078 修订）：诊断/执行派发归 #2092，本 PR 的 diagnose_only
    // 模式走 run_diagnosis_cycle 的跨块汇总路径；本函数保持 v1 单阶段行为。
    let prompt = build_block_prompt(chunk, lessons, evidence);
    let Ok(response) = llm.complete(&prompt).await else {
        return (Vec::new(), "作废（LLM 调用失败）");
    };
    let proposal = match parse_block_proposal(&response) {
        Ok(proposal) => {
            if let Some(recorder) = recorder {
                let (reason, edits) = match &proposal {
                    BlockProposal::Propose { reason, change_set } => {
                        (reason.clone(), change_set.edits.len())
                    }
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
            return (Vec::new(), "作废（应答解析失败）");
        }
    };
    match proposal {
        BlockProposal::NoChange { .. } => (Vec::new(), "无变更"),
        BlockProposal::Propose { reason, change_set } => {
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
                    (Vec::new(), "预演（将采纳）")
                } else {
                    (Vec::new(), "预演（将作废）")
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
                    record_adopted_lesson(lesson_store, &change_set, &reason, cycle_ref, recorder);
                    let adoption: Vec<(AppliedEdit, String)> = applied
                        .into_iter()
                        .map(|edit| (edit, reason.clone()))
                        .collect();
                    (adoption, "采纳")
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
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
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
pub async fn run_diagnosis_cycle(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    config: &RsiConfig,
    evidence: crate::domain::evidence::EvidenceBundle,
    lessons: &[LessonEntry],
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
    report_lines: &mut Vec<String>,
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
    let mut per_block: Vec<Vec<DiagnosedIssue>> = Vec::new();
    for (block_index, chunk) in chunks.iter().enumerate() {
        let prompt = build_diagnosis_prompt(chunk, lessons, &evidence);
        let issues = match llm.complete(&prompt).await {
            Ok(response) => match parse_diagnosis(&response) {
                Ok(DiagnosisProposal::Diagnose { issues, .. }) => issues,
                Ok(DiagnosisProposal::NoIssue { .. }) => Vec::new(),
                Err(error) => {
                    if let Some(recorder) = recorder {
                        recorder.record(&crate::domain::events::RsiCycleEvent::ProposalRejected {
                            block_index,
                            stage: "diagnosis_parse".to_string(),
                            message: error.message().chars().take(120).collect(),
                        });
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
        for (edit, edit_reason) in &block_adoptions {
            adoption_records.push(crate::domain::task::AdoptionRecord {
                id: match edit {
                    AppliedEdit::Upserted { id } | AppliedEdit::Deleted { id } => id.clone(),
                },
                reason: edit_reason.clone(),
            });
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

/// 周期 Compose 汇总块（#2065 A，对齐 Harness² Compose）：全部数据源块
/// 执行完后纵观本周期全部采纳——检测矛盾/合并/精炼，产出终版变更。
/// 无采纳时跳过（NEVER 为凑 Compose 而造编辑）。
pub async fn run_compose_phase(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    lesson_store: &LessonStore,
    adoptions: &[crate::domain::task::AdoptionRecord],
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
    let prompt = format!(
        r#"你是 harness 仲裁器（Compose）。本周期多个证据块分别产生了以下采纳：

{adoptions}

涉及组件的当前（各块编辑叠加后的）终态：

{materials}

任务：纵观全部采纳与终态——检测块间矛盾（同一问题不同规则、重复条款、
相互冲突的指令）、可合并项（多块各写一半的同类规则）与可精炼项（冗余
展开）。仅当发现真实矛盾或明确可合并/精炼时产出修正编辑；各块结论
一致且终态良好时选择 no_change——那是一个正当答案。

仲裁的判断标准（每次落笔先自检）：
- **矛盾**：同一场景下两条规则给出相反指令 → 保留更具体、更接近证据的
  那条，删除另一条。
- **合并**：多条规则讲同一主题的不同侧面 → 合成一条，**保留每条的独特
  信息**（合并 NEVER 丢语义）。
- **精炼**：只删重复表述与冗余展开，NEVER 删掉任何一条实质规则。
- **不引入**：本阶段只仲裁现有采纳，NEVER 凭想象新增未被证据支持的规则。
- **可执行优先**：合并/精炼后必须仍是「条件 + 动作」的可执行形态。

编辑契约同改进块（omitted-is-inherited；upsert 的 content.body 为
完整正文）。只输出一个 JSON 对象，格式二选一（NEVER 输出其他形态）：
{{"decision":"propose","reason":"<仲裁依据，一句话>","change_set":{{"edits":[{{"action":"upsert","id":{{"kind":"<kind>","name":"<名>"}},"content":{{"frontmatter":null,"body":"<完整正文>"}}}}]}}}}
{{"decision":"no_change","reason":"<一句话理由>"}}
不要输出 JSON 以外的任何文字。"#,
        adoptions = adoptions_block.join("\n"),
        materials = materials_block,
    );
    let Ok(response) = llm.complete(&prompt).await else {
        return Ok(Some(CycleOutcome {
            summary: "Compose：LLM 调用失败（跳过仲裁）".to_string(),
            adopted: Vec::new(),
        }));
    };
    let label = match parse_block_proposal(&response) {
        Ok(BlockProposal::NoChange { .. }) => "无变更（各块结论一致）",
        Ok(BlockProposal::Propose { reason, change_set }) => {
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
                        record_adopted_lesson(
                            lesson_store,
                            &change_set,
                            &reason,
                            &cycle_ref,
                            recorder,
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
