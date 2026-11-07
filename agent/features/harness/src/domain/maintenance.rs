//! 组件维护 Compose：确定性审计触发后，直接让 LLM 看原文并压缩。
//!
//! 试点范围：guardrails/tool-guidance/overlay。LLM 只输出规则级
//! keep/merge/delete 决策；宿主校验覆盖完整性并重建完整 body，避免
//! “整份重写”导致规则被静默丢失或压成残片。超预算 body 按规则块维护。

use crate::application::ComponentRegistry;
use crate::constants::{COMPONENT_MAINTENANCE_CHUNK_RULES, ERROR_DOMAIN};
use crate::domain::candidate::{CandidateRisk, CandidateScope, RsiCandidate, RsiCandidateStore};
use crate::domain::changeset::{
    apply_change_set_with_policy, ChangeSet, ComponentEdit, WritePolicy,
};
use crate::domain::component::ComponentContent;
use crate::domain::component_audit::{
    parse_audit_rules, AuditRuleUnit, AuditSnapshot, AuditTrigger,
};
use crate::domain::errors::HarnessError;
use crate::domain::events::RsiCycleEvent;
use crate::domain::improvement::{
    complete_with_usage_event, component_body_budget_tokens, PromotionDecision, RsiLlmPort,
};
use crate::domain::versioning::ComponentVersionStore;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
enum MaintenanceProposal {
    Compact {
        reason: String,
        #[serde(default)]
        promotion: PromotionDecision,
        rules: Vec<MaintenanceRuleEdit>,
    },
    NoChange {
        reason: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum MaintenanceRuleEdit {
    Keep { source: Vec<usize> },
    Merge { source: Vec<usize>, text: String },
    Delete { source: Vec<usize>, reason: String },
}

struct MaintenanceFailure {
    action: &'static str,
    message: String,
}

impl MaintenanceFailure {
    fn llm(message: impl Into<String>) -> Self {
        Self {
            action: "llm_failed",
            message: message.into(),
        }
    }
    fn parse(message: impl Into<String>) -> Self {
        Self {
            action: "parse_failed",
            message: message.into(),
        }
    }
    fn rejected(message: impl Into<String>) -> Self {
        Self {
            action: "rejected",
            message: message.into(),
        }
    }
}

fn trigger_name(trigger: AuditTrigger) -> &'static str {
    match trigger {
        AuditTrigger::RuleCount => "rule_count",
        AuditTrigger::OversizedRule => "oversized_rule",
        AuditTrigger::BodyBudget => "body_budget",
        AuditTrigger::DuplicateRule => "duplicate_rule",
    }
}

fn headings(body: &str) -> Vec<&str> {
    body.lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .map(str::trim)
        .collect()
}

fn parse_proposal(text: &str) -> Result<MaintenanceProposal, HarnessError> {
    let start = text
        .find('{')
        .ok_or_else(|| HarnessError::invalid(ERROR_DOMAIN, "维护 Compose 应答不含 JSON 对象"))?;
    let json_source = text
        .get(start..)
        .ok_or_else(|| HarnessError::invalid(ERROR_DOMAIN, "维护 Compose 应答载荷切分越界"))?;
    let mut stream =
        serde_json::Deserializer::from_str(json_source).into_iter::<MaintenanceProposal>();
    match stream.next() {
        Some(Ok(value)) => Ok(value),
        Some(Err(error)) => Err(HarnessError::invalid(
            ERROR_DOMAIN,
            format!("维护 Compose 解析失败：{error}"),
        )),
        None => Err(HarnessError::invalid(
            ERROR_DOMAIN,
            "维护 Compose JSON 流为空",
        )),
    }
}

fn clean_merge_text(text: &str) -> String {
    let trimmed = text.trim();
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            return rest.trim().to_string();
        }
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        if let Some(rest) = trimmed.get(digits..) {
            if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
                return rest.trim().to_string();
            }
        }
    }
    trimmed.to_string()
}

fn build_prompt(
    component: &str,
    triggers: &str,
    units: &[AuditRuleUnit],
    body: Option<&str>,
    chunked: bool,
    max_output_rules: Option<usize>,
) -> String {
    let numbered = units
        .iter()
        .map(|rule| {
            format!(
                "[R{}] section={} | {}",
                rule.index,
                rule.section_path.join(" > "),
                rule.raw
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let body_block = body
        .map(|body| format!("当前正文：\n{body}\n\n"))
        .unwrap_or_default();
    let chunk_note = if chunked {
        let target = max_output_rules.unwrap_or(1);
        format!(
            "当前为分块维护：只处理下列规则；其他规则由其他分块处理。\n本块 MUST 压缩到最多 {target} 条规则，且每条规则 MUST 保留可执行语义，禁止只输出占位词。\n\n"
        )
    } else {
        String::new()
    };
    format!(
        r#"你是 harness 组件维护器。当前组件需要压缩，但只能在原文范围内合并重复、删除冗余、压缩超长规则；NEVER 新增规则，NEVER 改变原有规则语义。

组件：{component}
触发原因：{triggers}
{chunk_note}规则数量：{rule_count}
{body_block}规则单元：
{rules}

硬性要求：
- 每条规则 R0..R{n} MUST 被 keep/merge/delete 恰好覆盖一次；
- delete MUST 给出理由；
- merge 只输出合并后的单行规则文本，MUST 保留来源规则的可执行语义；
- NEVER 新增来源之外的知识；
- 只输出紧凑 JSON。

输出格式：
{{"decision":"compact","reason":"<一句话>","promotion":"auto|review|reject","rules":[
  {{"action":"keep","source":[0]}},
  {{"action":"merge","source":[1,2],"text":"<合并后的单行规则>"}},
  {{"action":"delete","source":[3],"reason":"<理由>"}}
]}}
没有可维护变化时输出：
{{"decision":"no_change","reason":"<一句话>"}}"#,
        component = component,
        triggers = triggers,
        chunk_note = chunk_note,
        rule_count = units.len(),
        n = units.len().saturating_sub(1),
        body_block = body_block,
        rules = numbered,
    )
}

fn validate_rule_edits(
    edits: &[MaintenanceRuleEdit],
    units: &[AuditRuleUnit],
) -> Result<(), String> {
    if units.is_empty() {
        return Err("没有可维护的规则单元".to_string());
    }
    let mut covered = vec![false; units.len()];
    for edit in edits {
        let (source, action) = match edit {
            MaintenanceRuleEdit::Keep { source } => (source, "keep"),
            MaintenanceRuleEdit::Merge { source, .. } => (source, "merge"),
            MaintenanceRuleEdit::Delete { source, .. } => (source, "delete"),
        };
        if source.is_empty() {
            return Err(format!("{action} 未引用任何规则"));
        }
        if action == "keep" && source.len() != 1 {
            return Err("keep 只能引用一条规则".to_string());
        }
        for index in source {
            if *index >= units.len() {
                return Err(format!("引用不存在规则 R{index}"));
            }
            if covered[*index] {
                return Err(format!("规则 R{index} 被重复覆盖"));
            }
            covered[*index] = true;
        }
        if let MaintenanceRuleEdit::Merge { text, .. } = edit {
            let cleaned = clean_merge_text(text);
            if cleaned.is_empty() || cleaned.contains('\n') {
                return Err("merge 必须输出单行非空规则".to_string());
            }
            let source_max = source
                .iter()
                .filter_map(|index| units.get(*index))
                .map(|unit| share::token_budget::estimate_tokens(&unit.raw))
                .max()
                .unwrap_or(0);
            let text_tokens = share::token_budget::estimate_tokens(&cleaned);
            if source_max > 50 && text_tokens < 8 {
                return Err("merge 规则被压缩成无语义残片".to_string());
            }
        }
        if let MaintenanceRuleEdit::Delete { reason, .. } = edit {
            if reason.trim().is_empty() {
                return Err("delete 必须给出理由".to_string());
            }
        }
    }
    if let Some(index) = covered.iter().position(|value| !value) {
        return Err(format!("规则 R{index} 未被覆盖"));
    }
    Ok(())
}

fn map_edits(edits: Vec<MaintenanceRuleEdit>, mapping: &[usize]) -> Vec<MaintenanceRuleEdit> {
    edits
        .into_iter()
        .map(|edit| match edit {
            MaintenanceRuleEdit::Keep { source } => MaintenanceRuleEdit::Keep {
                source: source
                    .into_iter()
                    .filter_map(|index| mapping.get(index).copied())
                    .collect(),
            },
            MaintenanceRuleEdit::Merge { source, text } => MaintenanceRuleEdit::Merge {
                source: source
                    .into_iter()
                    .filter_map(|index| mapping.get(index).copied())
                    .collect(),
                text,
            },
            MaintenanceRuleEdit::Delete { source, reason } => MaintenanceRuleEdit::Delete {
                source: source
                    .into_iter()
                    .filter_map(|index| mapping.get(index).copied())
                    .collect(),
                reason,
            },
        })
        .collect()
}

fn keep_all(units: &[AuditRuleUnit], mapping: &[usize]) -> Vec<MaintenanceRuleEdit> {
    units
        .iter()
        .enumerate()
        .filter_map(|(local, _)| {
            mapping.get(local).map(|global| MaintenanceRuleEdit::Keep {
                source: vec![*global],
            })
        })
        .collect()
}

async fn collect_single_edits(
    llm: &dyn RsiLlmPort,
    component: &str,
    triggers: &str,
    body: &str,
    units: &[AuditRuleUnit],
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
) -> Result<Option<(String, PromotionDecision, Vec<MaintenanceRuleEdit>)>, MaintenanceFailure> {
    let prompt = build_prompt(component, triggers, units, Some(body), false, None);
    let completion = complete_with_usage_event(llm, &prompt, "maintenance_compose", 0, recorder)
        .await
        .map_err(|error| MaintenanceFailure::llm(format!("LLM 调用失败（{}）", error.message())))?;
    let proposal = parse_proposal(&completion.text)
        .map_err(|error| MaintenanceFailure::parse(format!("解析失败（{}）", error.message())))?;
    match proposal {
        MaintenanceProposal::NoChange { reason } => {
            let _ = reason;
            Ok(None)
        }
        MaintenanceProposal::Compact {
            reason,
            promotion,
            rules,
        } => {
            validate_rule_edits(&rules, units).map_err(|message| {
                MaintenanceFailure::rejected(format!("规则覆盖校验失败（{message}）"))
            })?;
            Ok(Some((reason, promotion, rules)))
        }
    }
}

async fn collect_chunked_edits(
    llm: &dyn RsiLlmPort,
    component: &str,
    triggers: &str,
    units: &[AuditRuleUnit],
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
) -> Result<Option<(String, PromotionDecision, Vec<MaintenanceRuleEdit>)>, MaintenanceFailure> {
    let mut all_edits = Vec::new();
    let mut reasons = Vec::new();
    let mut any_compact = false;
    let mut promotion = PromotionDecision::Auto;
    for (chunk_index, chunk) in units.chunks(COMPONENT_MAINTENANCE_CHUNK_RULES).enumerate() {
        let mapping: Vec<usize> = chunk.iter().map(|unit| unit.index).collect();
        let local_units: Vec<AuditRuleUnit> = chunk
            .iter()
            .enumerate()
            .map(|(local, unit)| {
                let mut local_unit = unit.clone();
                local_unit.index = local;
                local_unit
            })
            .collect();
        let target_rules = local_units.len().div_ceil(6).max(1);
        let prompt = build_prompt(
            component,
            triggers,
            &local_units,
            None,
            true,
            Some(target_rules),
        );
        let completion =
            complete_with_usage_event(llm, &prompt, "maintenance_compose", chunk_index, recorder)
                .await
                .map_err(|error| {
                    MaintenanceFailure::llm(format!(
                        "分块 {chunk_index} LLM 调用失败（{}）",
                        error.message()
                    ))
                })?;
        let proposal = parse_proposal(&completion.text).map_err(|error| {
            MaintenanceFailure::parse(format!(
                "分块 {chunk_index} 解析失败（{}）",
                error.message()
            ))
        })?;
        match proposal {
            MaintenanceProposal::NoChange { reason } => {
                let _ = reason;
                all_edits.extend(keep_all(&local_units, &mapping));
            }
            MaintenanceProposal::Compact {
                reason,
                promotion: chunk_promotion,
                rules,
            } => {
                if chunk_promotion == PromotionDecision::Reject {
                    promotion = PromotionDecision::Reject;
                } else if (chunk_promotion == PromotionDecision::Review
                    || chunk_promotion == PromotionDecision::Shadow)
                    && promotion != PromotionDecision::Reject
                {
                    promotion = chunk_promotion;
                }
                validate_rule_edits(&rules, &local_units).map_err(|message| {
                    MaintenanceFailure::rejected(format!(
                        "分块 {chunk_index} 规则覆盖校验失败（{message}）"
                    ))
                })?;
                let output_rules = rules
                    .iter()
                    .map(|edit| match edit {
                        MaintenanceRuleEdit::Keep { .. } | MaintenanceRuleEdit::Merge { .. } => 1,
                        MaintenanceRuleEdit::Delete { .. } => 0,
                    })
                    .sum::<usize>();
                if output_rules > target_rules {
                    return Err(MaintenanceFailure::rejected(format!(
                        "分块 {chunk_index} 压缩后仍有 {output_rules} 条规则，超过目标 {target_rules}"
                    )));
                }
                any_compact = true;
                reasons.push(reason);
                all_edits.extend(map_edits(rules, &mapping));
            }
        }
    }
    if !any_compact {
        return Ok(None);
    }
    Ok(Some((reasons.join("；"), promotion, all_edits)))
}

fn rebuild_body(
    original: &str,
    units: &[AuditRuleUnit],
    edits: &[MaintenanceRuleEdit],
) -> Result<String, String> {
    let mut lines: Vec<String> = original.split('\n').map(str::to_string).collect();
    let mut replacements: Vec<(usize, usize, Option<String>)> = Vec::new();
    for edit in edits {
        let source = match edit {
            MaintenanceRuleEdit::Keep { .. } => continue,
            MaintenanceRuleEdit::Merge { source, .. } => source,
            MaintenanceRuleEdit::Delete { source, .. } => source,
        };
        let mut ordered = source.clone();
        ordered.sort_unstable();
        for (position, index) in ordered.iter().enumerate() {
            let Some(unit) = units.get(*index) else {
                continue;
            };
            let replacement = match edit {
                MaintenanceRuleEdit::Merge { text, .. } if position == 0 => {
                    Some(format!("{}{}", unit.prefix, clean_merge_text(text)))
                }
                _ => None,
            };
            replacements.push((unit.line_start, unit.line_end, replacement));
        }
    }
    replacements.sort_by_key(|right| std::cmp::Reverse(right.0));
    for (start, end, replacement) in replacements {
        if end >= lines.len() || start > end {
            return Err("规则行范围越界".to_string());
        }
        lines.splice(start..=end, replacement);
    }
    Ok(lines.join("\n"))
}

/// 对审计命中的组件逐个执行维护压缩；返回可读摘要。
#[allow(clippy::too_many_arguments)]
pub async fn run_maintenance_compose_phase(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    findings: &[crate::domain::component_audit::ComponentAuditFinding],
    dry_run: bool,
    preview_lines: &mut Vec<String>,
    recorder: Option<&crate::domain::events::RsiEventRecorder>,
    version_store: Option<&ComponentVersionStore>,
    candidate_store: Option<&RsiCandidateStore>,
) -> Result<Vec<String>, HarnessError> {
    let mut summaries = Vec::new();
    for finding in findings {
        let Ok(current) = registry.read(&finding.component) else {
            continue;
        };
        let units = parse_audit_rules(&current.body);
        if units.is_empty() {
            summaries.push(format!(
                "维护 Compose：{} 无可维护规则（跳过）",
                finding.component
            ));
            continue;
        }
        let triggers = finding
            .triggers
            .iter()
            .map(|trigger| trigger_name(*trigger))
            .collect::<Vec<_>>()
            .join(", ");
        let budget = component_body_budget_tokens(&finding.component);
        if finding.body_tokens > budget.saturating_mul(2) {
            if let Some(recorder) = recorder {
                recorder.record(&RsiCycleEvent::MaintenanceComposeResult {
                    component: finding.component.to_string(),
                    action: "skipped_oversized".to_string(),
                    before_tokens: finding.body_tokens,
                    after_tokens: finding.body_tokens,
                    before_rules: finding.rule_count,
                    after_rules: finding.rule_count,
                    reason: format!(
                        "body {} tokens 超过可维护上限 {}",
                        finding.body_tokens,
                        budget * 2
                    ),
                });
            }
            summaries.push(format!(
                "维护 Compose：{} 超预算过大（{} > {} tokens），跳过维护",
                finding.component,
                finding.body_tokens,
                budget * 2
            ));
            continue;
        }
        let chunked =
            finding.body_tokens > budget && units.len() > COMPONENT_MAINTENANCE_CHUNK_RULES;
        let collected = if chunked {
            collect_chunked_edits(
                llm,
                &finding.component.to_string(),
                &triggers,
                &units,
                recorder,
            )
            .await
        } else {
            collect_single_edits(
                llm,
                &finding.component.to_string(),
                &triggers,
                &current.body,
                &units,
                recorder,
            )
            .await
        };
        let (reason, promotion, edits) = match collected {
            Ok(Some(value)) => value,
            Ok(None) => {
                if let Some(recorder) = recorder {
                    recorder.record(&RsiCycleEvent::MaintenanceComposeResult {
                        component: finding.component.to_string(),
                        action: "no_change".to_string(),
                        before_tokens: finding.body_tokens,
                        after_tokens: finding.body_tokens,
                        before_rules: finding.rule_count,
                        after_rules: finding.rule_count,
                        reason: "模型判定无需维护".to_string(),
                    });
                }
                summaries.push(format!("维护 Compose：{} 无变更", finding.component));
                continue;
            }
            Err(failure) => {
                if let Some(recorder) = recorder {
                    recorder.record(&RsiCycleEvent::MaintenanceComposeResult {
                        component: finding.component.to_string(),
                        action: failure.action.to_string(),
                        before_tokens: finding.body_tokens,
                        after_tokens: finding.body_tokens,
                        before_rules: finding.rule_count,
                        after_rules: finding.rule_count,
                        reason: failure.message.chars().take(120).collect(),
                    });
                }
                summaries.push(format!(
                    "维护 Compose：{} {}",
                    finding.component, failure.message
                ));
                continue;
            }
        };
        if promotion == PromotionDecision::Reject {
            summaries.push(format!(
                "维护 Compose：{} LLM 判断不写入",
                finding.component
            ));
            continue;
        }
        if let Err(message) = validate_rule_edits(&edits, &units) {
            summaries.push(format!(
                "维护 Compose：{} 规则覆盖校验失败（{}）",
                finding.component, message
            ));
            continue;
        }
        let body = match rebuild_body(&current.body, &units, &edits) {
            Ok(body) => body,
            Err(message) => {
                summaries.push(format!(
                    "维护 Compose：{} 重建失败（{}）",
                    finding.component, message
                ));
                continue;
            }
        };
        let after = AuditSnapshot::from_body(&body);
        if body == current.body {
            summaries.push(format!("维护 Compose：{} 无实际改善", finding.component));
            continue;
        }
        let valid = headings(&current.body) == headings(&body)
            && after.rule_count <= finding.rule_count
            && after.body_tokens <= finding.body_tokens
            && after.oversized_rules.len() <= finding.oversized_rules.len()
            && after.duplicate_rules <= finding.duplicate_rules
            && after.empty_rules == 0
            && after.body_tokens <= budget;
        if !valid {
            if let Some(recorder) = recorder {
                recorder.record(&RsiCycleEvent::MaintenanceComposeResult {
                    component: finding.component.to_string(),
                    action: "rejected".to_string(),
                    before_tokens: finding.body_tokens,
                    after_tokens: after.body_tokens,
                    before_rules: finding.rule_count,
                    after_rules: after.rule_count,
                    reason: "压缩后 rule/token/heading/duplicate 未改善或超预算".to_string(),
                });
            }
            summaries.push(format!(
                "维护 Compose：{} 校验失败（跳过）",
                finding.component
            ));
            continue;
        }
        if (promotion == PromotionDecision::Review || promotion == PromotionDecision::Shadow)
            && !dry_run
        {
            if let Some(candidate_store) = candidate_store {
                let candidate = RsiCandidate::new_pending(
                    CandidateScope::Global,
                    finding.component.to_string(),
                    current.body.clone(),
                    body.clone(),
                    finding
                        .triggers
                        .iter()
                        .map(|trigger| trigger_name(*trigger).to_string())
                        .collect(),
                    Vec::new(),
                    CandidateRisk::Medium,
                );
                candidate_store.append(&candidate).map_err(|error| {
                    HarnessError::storage(
                        ERROR_DOMAIN,
                        format!("候选写入失败：{}", error.message()),
                    )
                })?;
                summaries.push(format!(
                    "维护 Compose：{} 待确认（{}）",
                    finding.component, candidate.id
                ));
                continue;
            }
        }
        let new_content = ComponentContent {
            frontmatter: current.frontmatter.clone(),
            body,
        };
        if let Err(error) = registry.validate(&finding.component, &new_content) {
            summaries.push(format!(
                "维护 Compose：{} 校验失败：{}",
                finding.component,
                error.message()
            ));
            continue;
        }
        if !dry_run {
            if let Some(version_store) = version_store {
                if let Err(error) =
                    version_store.capture(registry, &finding.component, "maintenance", "upsert")
                {
                    log::warn!(target: crate::LOG_TARGET, "维护 Compose 快照失败：{error}");
                }
            }
        }
        let change_set = ChangeSet {
            edits: vec![ComponentEdit::Upsert {
                id: finding.component.clone(),
                content: new_content,
            }],
        };
        if let Err(error) =
            apply_change_set_with_policy(registry, &change_set, WritePolicy::ProseMaintenance)
        {
            summaries.push(format!(
                "维护 Compose：{} 写入失败：{}",
                finding.component,
                error.message()
            ));
            continue;
        }
        if let Some(recorder) = recorder {
            recorder.record(&RsiCycleEvent::MaintenanceComposeResult {
                component: finding.component.to_string(),
                action: "compact".to_string(),
                before_tokens: finding.body_tokens,
                after_tokens: after.body_tokens,
                before_rules: finding.rule_count,
                after_rules: after.rule_count,
                reason: reason.chars().take(120).collect(),
            });
        }
        preview_lines.push(format!(
            "{}维护 Compose：{} {} rules / {} tokens → {} rules / {} tokens",
            if dry_run { "[dry-run]" } else { "" },
            finding.component,
            finding.rule_count,
            finding.body_tokens,
            after.rule_count,
            after.body_tokens
        ));
        summaries.push(format!(
            "维护 Compose：{}（{}→{} tokens）",
            finding.component, finding.body_tokens, after.body_tokens
        ));
    }
    if let Some(recorder) = recorder {
        recorder.record(&RsiCycleEvent::ComposeResult {
            label: format!("维护 Compose完成（{} 个组件）", findings.len()),
        });
    }
    Ok(summaries)
}

#[cfg(test)]
#[path = "maintenance_tests.rs"]
mod tests;
