//! 组件维护 Compose：确定性审计触发后，直接让 LLM 看原文并压缩。
//!
//! 试点范围仅 guardrails/tool-guidance。LLM 输出完整 body；宿主只校验
//! rule 数、token、超长 rule 与 heading 结构不恶化，不解析渐进披露格式。

use crate::application::ComponentRegistry;
use crate::constants::ERROR_DOMAIN;
use crate::domain::changeset::{apply_change_set, ChangeSet, ComponentEdit};
use crate::domain::component::ComponentContent;
use crate::domain::component_audit::{AuditTrigger, ComponentAuditFinding};
use crate::domain::errors::HarnessError;
use crate::domain::events::RsiCycleEvent;
use crate::domain::improvement::{
    complete_with_usage_event, component_body_budget_tokens, RsiLlmPort,
};
use crate::domain::scope_dedup::rule_units;
use crate::domain::versioning::ComponentVersionStore;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
enum MaintenanceProposal {
    Compact { reason: String, body: String },
    NoChange { reason: String },
}

fn trigger_name(trigger: AuditTrigger) -> &'static str {
    match trigger {
        AuditTrigger::RuleCount => "rule_count",
        AuditTrigger::OversizedRule => "oversized_rule",
        AuditTrigger::BodyBudget => "body_budget",
    }
}

fn headings(body: &str) -> Vec<&str> {
    body.lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .map(str::trim)
        .collect()
}

fn oversized_rule_count(body: &str) -> usize {
    rule_units(body)
        .iter()
        .filter(|rule| {
            share::token_budget::estimate_tokens(&rule.text)
                > crate::constants::COMPONENT_AUDIT_MAX_RULE_TOKENS
        })
        .count()
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

/// 对审计命中的组件逐个执行维护压缩；返回可读摘要。
#[allow(clippy::too_many_arguments)]
pub async fn run_maintenance_compose_phase(
    registry: &mut dyn ComponentRegistry,
    llm: &dyn RsiLlmPort,
    findings: &[ComponentAuditFinding],
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
        let rules = rule_units(&current.body);
        let numbered = rules
            .iter()
            .enumerate()
            .map(|(index, rule)| format!("[R{index}] {}", rule.text))
            .collect::<Vec<_>>()
            .join("\n");
        let triggers = finding
            .triggers
            .iter()
            .map(|trigger| trigger_name(*trigger))
            .collect::<Vec<_>>()
            .join(", ");
        let prompt = format!(
            r#"你是 harness 组件维护器。当前组件需要压缩，但只能在原文范围内合并重复、删除冗余、压缩超长规则；NEVER 新增规则，NEVER 改变原有规则语义，NEVER 删除 heading。

组件：{component}
触发原因：{triggers}
当前正文：
{body}

规则单元：
{rules}

要求：
- 只输出压缩后的完整 body；
- 保留所有 heading 与原 section 结构；
- 合并/删除只能针对同一 section；
- 不新增任何规则；
- 输出紧凑 JSON。

只输出一个 JSON 对象：
{{"decision":"compact","reason":"<一句话>","body":"<压缩后的完整正文>"}}
或：
{{"decision":"no_change","reason":"<一句话>"}}"#,
            component = finding.component,
            triggers = triggers,
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
        let MaintenanceProposal::Compact { reason, body } = proposal else {
            if let MaintenanceProposal::NoChange { reason } = proposal {
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
            }
            continue;
        };
        let after_rules = rule_units(&body).len();
        let after_tokens = share::token_budget::estimate_tokens(&body);
        let after_oversized = oversized_rule_count(&body);
        let budget = component_body_budget_tokens(&finding.component);
        let valid = !body.trim().is_empty()
            && headings(&current.body) == headings(&body)
            && after_rules <= finding.rule_count
            && after_tokens <= finding.body_tokens
            && after_oversized <= finding.oversized_rules.len()
            && after_tokens <= budget;
        if !valid {
            if let Some(recorder) = recorder {
                recorder.record(&RsiCycleEvent::MaintenanceComposeResult {
                    component: finding.component.to_string(),
                    action: "rejected".to_string(),
                    before_tokens: finding.body_tokens,
                    after_tokens,
                    before_rules: finding.rule_count,
                    after_rules,
                    reason: "压缩后 rule/token/heading 未改善或超预算".to_string(),
                });
            }
            summaries.push(format!(
                "维护 Compose：{} 校验失败（跳过）",
                finding.component
            ));
            continue;
        }
        let unchanged = after_rules == finding.rule_count
            && after_tokens == finding.body_tokens
            && after_oversized == finding.oversized_rules.len();
        if unchanged {
            summaries.push(format!("维护 Compose：{} 无实际改善", finding.component));
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
        if let Some(version_store) = version_store {
            if let Err(error) =
                version_store.capture(registry, &finding.component, "maintenance", "upsert")
            {
                log::warn!(target: crate::LOG_TARGET, "维护 Compose 快照失败：{error}");
            }
        }
        let change_set = ChangeSet {
            edits: vec![ComponentEdit::Upsert {
                id: finding.component.clone(),
                content: new_content,
            }],
        };
        if let Err(error) = apply_change_set(registry, &change_set) {
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
                after_tokens,
                before_rules: finding.rule_count,
                after_rules,
                reason: reason.chars().take(120).collect(),
            });
        }
        preview_lines.push(format!(
            "{}维护 Compose：{} {} rules / {} tokens → {} rules / {} tokens",
            if dry_run { "[dry-run]" } else { "" },
            finding.component,
            finding.rule_count,
            finding.body_tokens,
            after_rules,
            after_tokens
        ));
        summaries.push(format!(
            "维护 Compose：{}（{}→{} tokens）",
            finding.component, finding.body_tokens, after_tokens
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
