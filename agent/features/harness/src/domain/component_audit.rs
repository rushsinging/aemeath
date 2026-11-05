//! 组件维护审计：确定性触发，不直接删除规则。
//!
//! 只覆盖 guardrails/tool-guidance 的 bullet/numbered 规则；overlay 的
//! 渐进披露/表格结构暂不自动维护。LLM 只接收编号规则并由维护 Compose 决策。

use crate::constants::{
    COMPONENT_AUDIT_BODY_BUDGET_PERCENT, COMPONENT_AUDIT_MAX_RULES, COMPONENT_AUDIT_MAX_RULE_TOKENS,
};
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind};
use crate::domain::improvement::component_body_budget_tokens;
use crate::domain::scope_dedup::rule_units;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OversizedRule {
    pub line_index: usize,
    pub tokens: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditTrigger {
    RuleCount,
    OversizedRule,
    BodyBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentAuditFinding {
    pub component: ComponentId,
    pub body_tokens: usize,
    pub rule_count: usize,
    pub oversized_rules: Vec<OversizedRule>,
    pub triggers: Vec<AuditTrigger>,
}

pub fn audit_component(
    id: &ComponentId,
    content: &ComponentContent,
) -> Option<ComponentAuditFinding> {
    if !matches!(
        id.kind,
        ComponentKind::Guardrails | ComponentKind::ToolGuidance
    ) || content.body.trim().is_empty()
    {
        return None;
    }
    let rules = rule_units(&content.body);
    let body_tokens = share::token_budget::estimate_tokens(&content.body);
    let oversized_rules: Vec<OversizedRule> = rules
        .iter()
        .filter_map(|rule| {
            let tokens = share::token_budget::estimate_tokens(&rule.text);
            (tokens > COMPONENT_AUDIT_MAX_RULE_TOKENS).then_some(OversizedRule {
                line_index: rule.line_index,
                tokens,
            })
        })
        .collect();
    let mut triggers = Vec::new();
    if rules.len() > COMPONENT_AUDIT_MAX_RULES {
        triggers.push(AuditTrigger::RuleCount);
    }
    if !oversized_rules.is_empty() {
        triggers.push(AuditTrigger::OversizedRule);
    }
    let budget = component_body_budget_tokens(id);
    if body_tokens.saturating_mul(100) > budget.saturating_mul(COMPONENT_AUDIT_BODY_BUDGET_PERCENT)
    {
        triggers.push(AuditTrigger::BodyBudget);
    }
    (!triggers.is_empty()).then_some(ComponentAuditFinding {
        component: id.clone(),
        body_tokens,
        rule_count: rules.len(),
        oversized_rules,
        triggers,
    })
}

#[cfg(test)]
#[path = "component_audit_tests.rs"]
mod tests;
