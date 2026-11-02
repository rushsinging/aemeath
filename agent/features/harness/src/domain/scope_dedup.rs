//! 跨 scope 组件重复检测与收敛（#2168）。
//!
//! global tool-guidance/guardrails/overlay 与 project overlay 会同时注入；
//! 同一语义规则 MUST 只有一个权威落点。本模块用「规则单元 + CJK bigram
//! 召回 + 结构化 LLM 判定 + 宿主处置」防止 global 与项目层互相复述。

use crate::constants::{
    CROSS_SCOPE_DUPLICATE_CANDIDATE_LIMIT, CROSS_SCOPE_DUPLICATE_RECALL_THRESHOLD,
};
use crate::domain::component::{ComponentId, ComponentKind};
use serde::Deserialize;

/// markdown 中的一条可比较规则（bullet / 编号条目）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuleUnit {
    /// 在原 body `split_lines()` 中的行号，用于安全删除。
    pub line_index: usize,
    pub text: String,
}

/// 相似召回得到的跨 scope 候选 pair。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CrossScopeCandidate {
    pub project_component: ComponentId,
    pub project_rule: String,
    pub project_line: usize,
    pub global_component: ComponentId,
    pub global_rule: String,
    pub global_line: usize,
    pub similarity: f64,
}

/// 结构化语义判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DuplicateVerdict {
    Duplicate,
    Specialization,
    Conflict,
    Distinct,
}

impl DuplicateVerdict {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::Specialization => "specialization",
            Self::Conflict => "conflict",
            Self::Distinct => "distinct",
        }
    }
}

/// LLM 对单个候选 pair 的判定。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct DuplicateAdjudication {
    pub index: usize,
    pub verdict: DuplicateVerdict,
    /// `global` / `project`；仅 duplicate 冲突权威时使用。
    #[serde(default)]
    pub authority: Option<String>,
    pub reason: String,
}

#[derive(Deserialize)]
pub(crate) struct DuplicateAdjudicationEnvelope {
    pub verdicts: Vec<DuplicateAdjudication>,
}

/// global 单项目规则的迁移判定。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct GlobalRuleMove {
    pub index: usize,
    pub project: String,
    pub reason: String,
}

#[derive(Deserialize)]
pub(crate) struct GlobalRuleMoveEnvelope {
    pub moves: Vec<GlobalRuleMove>,
}

/// 提取 body 中的规则单元；标题、代码块与说明段落不参与重复判定。
pub(crate) fn rule_units(body: &str) -> Vec<RuleUnit> {
    body.lines()
        .enumerate()
        .filter_map(|(line_index, line)| {
            let trimmed = line.trim_start();
            let is_bullet = trimmed.starts_with("- ") || trimmed.starts_with("* ");
            let after_digits = trimmed.trim_start_matches(|ch: char| ch.is_ascii_digit());
            let is_numbered = trimmed.starts_with(|ch: char| ch.is_ascii_digit())
                && after_digits.strip_prefix(". ").is_some_and(|value| {
                    let value = value.trim();
                    !value.is_empty() && value.chars().count() < 500
                });
            if !is_bullet && !is_numbered {
                return None;
            }
            let text = if is_bullet {
                trimmed
                    .strip_prefix("- ")
                    .or_else(|| trimmed.strip_prefix("* "))
                    .unwrap_or(trimmed)
                    .trim()
                    .to_string()
            } else {
                trimmed
                    .trim_start_matches(|ch: char| ch.is_ascii_digit())
                    .strip_prefix(". ")
                    .unwrap_or(trimmed)
                    .trim()
                    .to_string()
            };
            Some(RuleUnit { line_index, text })
        })
        .collect()
}

fn is_global_prose(id: &ComponentId) -> bool {
    matches!(
        id.kind,
        ComponentKind::ToolGuidance | ComponentKind::Guardrails
    ) || (id.kind == ComponentKind::SystemPromptOverlay && id.name.as_str() == "global")
}

fn is_project_overlay(id: &ComponentId) -> bool {
    id.kind == ComponentKind::SystemPromptOverlay && id.name.as_str() != "global"
}

/// 召回 project overlay 与 global prose 之间的疑似重复。相似度只用于召回；
/// 每个 pair 后续 MUST 经结构化 LLM 判定，NEVER 直接按分数删除。
pub(crate) fn find_cross_scope_candidates(
    project: (&ComponentId, &str),
    global_components: &[((ComponentId, String), &str)],
) -> Vec<CrossScopeCandidate> {
    let (project_id, project_body) = project;
    let project_rules = rule_units(project_body);
    let mut candidates = Vec::new();
    for ((global_id, _), global_body) in global_components {
        for global_rule in rule_units(global_body) {
            for project_rule in &project_rules {
                let similarity =
                    share::memory::dedup::jaccard_similarity(&project_rule.text, &global_rule.text);
                if similarity < CROSS_SCOPE_DUPLICATE_RECALL_THRESHOLD {
                    continue;
                }
                candidates.push(CrossScopeCandidate {
                    project_component: project_id.clone(),
                    project_rule: project_rule.text.clone(),
                    project_line: project_rule.line_index,
                    global_component: global_id.clone(),
                    global_rule: global_rule.text.clone(),
                    global_line: global_rule.line_index,
                    similarity,
                });
            }
        }
    }
    candidates.sort_by(|left, right| {
        right
            .similarity
            .partial_cmp(&left.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.truncate(CROSS_SCOPE_DUPLICATE_CANDIDATE_LIMIT);
    candidates
}

/// 构建结构化判定 prompt。判定只输出枚举与权威落点，NEVER 直接生成组件正文。
pub(crate) fn build_adjudication_prompt(candidates: &[CrossScopeCandidate]) -> String {
    let pairs = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            format!(
                "[{index}] project {}: {}\n    global {}: {}\n    lexical_similarity={:.2}\n",
                candidate.project_component,
                candidate.project_rule,
                candidate.global_component,
                candidate.global_rule,
                candidate.similarity
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"你是 harness scope 仲裁器。以下是 project overlay 与 global 组件的疑似重复规则：

{pairs}

判定口径：
- duplicate：语义等价，项目层只是复述 global（或 global 只是把项目规则泛化）。
- specialization：项目规则包含明确项目条件/差异，global 未覆盖该差异。
- conflict：两边对同一行为给出矛盾要求。
- distinct：约束对象或语义不同。

权威口径：
- duplicate 且规则是跨项目通用纪律 → authority=global；
- duplicate 且规则只适用于该项目 → authority=project；
- specialization/conflict/distinct 可省略 authority。

只输出 JSON：
{{"verdicts":[{{"index":0,"verdict":"duplicate","authority":"global","reason":"<不超过80字符>"}}]}}
不要输出 JSON 以外的任何文字。"#
    )
}

/// 解析结构化判定；格式不合法时由调用方按 fail-closed 处理。
pub(crate) fn parse_adjudications(
    text: &str,
) -> Result<Vec<DuplicateAdjudication>, serde_json::Error> {
    serde_json::from_str::<DuplicateAdjudicationEnvelope>(text).map(|envelope| envelope.verdicts)
}

/// 构建「global 单项目规则下沉」审计 prompt。只允许选择既有 project overlay，
/// 宿主按行移动；NEVER 让模型直接重写整份组件。
pub(crate) fn build_global_authority_prompt(
    global_component: &ComponentId,
    global_rules: &[RuleUnit],
    project_component: &ComponentId,
    project_body: &str,
    evidence_projects: &[String],
) -> String {
    let rules = global_rules
        .iter()
        .enumerate()
        .map(|(index, rule)| format!("[{index}] {}", rule.text))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"你是 harness global 规则权威审计器。

global 组件 {global_component} 的历史证据只来自单一项目段：
{evidence_projects}

global 待审规则：
{rules}

同名/对应 project overlay：{project_component}
当前项目正文：
{project_body}

任务：找出明确包含该项目专属路径、命令、栈、流程或约束的规则；只有这些规则才应迁移到 project overlay。跨项目通用工具纪律、与项目无关的 schema/输出/重试规则 MUST 留在 global。无法确定 NEVER 迁移。

只输出 JSON：
{{"moves":[{{"index":0,"project":"<project segment>","reason":"<不超过80字符>"}}]}}
无迁移时输出 {{"moves":[]}}。不要输出 JSON 以外的任何文字。"#,
        evidence_projects = evidence_projects.join(","),
    )
}

/// 解析 global 规则迁移判定。
pub(crate) fn parse_global_rule_moves(
    text: &str,
) -> Result<Vec<GlobalRuleMove>, serde_json::Error> {
    serde_json::from_str::<GlobalRuleMoveEnvelope>(text).map(|envelope| envelope.moves)
}

/// 删除指定行号并保留原有换行形态；NEVER 按字符硬截断。
pub(crate) fn remove_rule_lines(body: &str, mut lines: Vec<usize>) -> String {
    lines.sort_unstable();
    lines.dedup();
    let mut line_set = lines.into_iter().peekable();
    let mut output = String::with_capacity(body.len());
    for (index, line) in body.split_inclusive('\n').enumerate() {
        if line_set.peek().is_some_and(|remove| *remove == index) {
            line_set.next();
            continue;
        }
        output.push_str(line);
    }
    output
}

/// 按 scope 分组后的组件正文快照。
pub(crate) type ScopedComponentBodies = Vec<(ComponentId, String)>;

/// 对外过滤组件面：只比较 project overlay 与 global prose。
pub(crate) fn split_scope_components(
    components: &[(ComponentId, String)],
) -> (ScopedComponentBodies, ScopedComponentBodies) {
    let mut projects = Vec::new();
    let mut globals = Vec::new();
    for (id, body) in components {
        if is_project_overlay(id) {
            projects.push((id.clone(), body.clone()));
        } else if is_global_prose(id) {
            globals.push((id.clone(), body.clone()));
        }
    }
    (projects, globals)
}
