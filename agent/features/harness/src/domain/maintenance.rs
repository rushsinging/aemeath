//! 组件维护 Compose：确定性审计触发后，直接让 LLM 看原文并压缩。
//!
//! 试点范围：guardrails/tool-guidance/overlay。LLM 只输出规则级
//! keep/merge/delete 决策；宿主校验覆盖完整性并重建完整 body，避免
//! “整份重写”导致规则被静默丢失或压成残片。

use crate::application::ComponentRegistry;
use crate::constants::ERROR_DOMAIN;
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
    complete_with_usage_event, component_body_budget_tokens, RsiLlmPort,
};
use crate::domain::versioning::ComponentVersionStore;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
enum MaintenanceProposal {
    Compact {
        reason: String,
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
        match edit {
            MaintenanceRuleEdit::Merge { text, .. } => {
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
            MaintenanceRuleEdit::Delete { reason, .. } => {
                if reason.trim().is_empty() {
                    return Err("delete 必须给出理由".to_string());
                }
            }
            MaintenanceRuleEdit::Keep { .. } => {}
        }
    }
    if let Some(index) = covered.iter().position(|value| !value) {
        return Err(format!("规则 R{index} 未被覆盖"));
    }
    Ok(())
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
        let triggers = finding
            .triggers
            .iter()
            .map(|trigger| trigger_name(*trigger))
            .collect::<Vec<_>>()
            .join(", ");
        let prompt = format!(
            r#"你是 harness 组件维护器。当前组件需要压缩，但只能在原文范围内合并重复、删除冗余、压缩超长规则；NEVER 新增规则，NEVER 改变原有规则语义。

组件：{component}
触发原因：{triggers}
规则数量：{rule_count}
当前正文：
{body}

规则单元：
{rules}

硬性要求：
- 每条规则 R0..R{n} MUST 被 keep/merge/delete 恰好覆盖一次；
- delete MUST 给出理由；
- merge 只输出合并后的单行规则文本，MUST 保留来源规则的可执行语义；
- NEVER 新增来源之外的知识；
- 只输出紧凑 JSON。

输出格式：
{{"decision":"compact","reason":"<一句话>","rules":[
  {{"action":"keep","source":[0]}},
  {{"action":"merge","source":[1,2],"text":"<合并后的单行规则>"}},
  {{"action":"delete","source":[3],"reason":"<理由>"}}
]}}
没有可维护变化时输出：
{{"decision":"no_change","reason":"<一句话>"}}"#,
            component = finding.component,
            triggers = triggers,
            rule_count = units.len(),
            n = units.len().saturating_sub(1),
            body = current.body,
            rules = numbered,
        );
        let completion =
            match complete_with_usage_event(llm, &prompt, "maintenance_compose", 0, recorder).await
            {
                Ok(completion) => completion,
                Err(error) => {
                    if let Some(recorder) = recorder {
                        recorder.record(&RsiCycleEvent::MaintenanceComposeResult {
                            component: finding.component.to_string(),
                            action: "llm_failed".to_string(),
                            before_tokens: finding.body_tokens,
                            after_tokens: finding.body_tokens,
                            before_rules: finding.rule_count,
                            after_rules: finding.rule_count,
                            reason: error.message().chars().take(120).collect(),
                        });
                    }
                    summaries.push(format!(
                        "维护 Compose：{} LLM 调用失败（跳过）",
                        finding.component
                    ));
                    continue;
                }
            };
        let proposal = match parse_proposal(&completion.text) {
            Ok(proposal) => proposal,
            Err(error) => {
                if let Some(recorder) = recorder {
                    recorder.record(&RsiCycleEvent::MaintenanceComposeResult {
                        component: finding.component.to_string(),
                        action: "parse_failed".to_string(),
                        before_tokens: finding.body_tokens,
                        after_tokens: finding.body_tokens,
                        before_rules: finding.rule_count,
                        after_rules: finding.rule_count,
                        reason: error.message().chars().take(120).collect(),
                    });
                }
                summaries.push(format!(
                    "维护 Compose：{} 解析失败（跳过）",
                    finding.component
                ));
                continue;
            }
        };
        let (reason, edits) = match proposal {
            MaintenanceProposal::Compact { reason, rules } => (reason, rules),
            MaintenanceProposal::NoChange { reason } => {
                if let Some(recorder) = recorder {
                    recorder.record(&RsiCycleEvent::MaintenanceComposeResult {
                        component: finding.component.to_string(),
                        action: "no_change".to_string(),
                        before_tokens: finding.body_tokens,
                        after_tokens: finding.body_tokens,
                        before_rules: finding.rule_count,
                        after_rules: finding.rule_count,
                        reason: reason.chars().take(120).collect(),
                    });
                }
                summaries.push(format!("维护 Compose：{} 无变更", finding.component));
                continue;
            }
        };
        if let Err(message) = validate_rule_edits(&edits, &units) {
            if let Some(recorder) = recorder {
                recorder.record(&RsiCycleEvent::MaintenanceComposeResult {
                    component: finding.component.to_string(),
                    action: "rejected".to_string(),
                    before_tokens: finding.body_tokens,
                    after_tokens: finding.body_tokens,
                    before_rules: finding.rule_count,
                    after_rules: finding.rule_count,
                    reason: message.chars().take(120).collect(),
                });
            }
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
        let budget = component_body_budget_tokens(&finding.component);
        let no_change = body == current.body;
        if no_change {
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
