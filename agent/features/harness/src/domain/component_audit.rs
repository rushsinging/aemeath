//! 组件维护审计：确定性触发与写入前快照。
//!
//! 规则单元支持 bullet/编号/嵌套/段落，跳过 frontmatter、代码块与表格
//! （Progressive Disclosure 索引表不参与规则计数/去重）。维护 Compose
//! 只看 raw；宿主用 normalized 做计数、重复与回归门禁。

use crate::constants::{
    COMPONENT_AUDIT_BODY_BUDGET_PERCENT, COMPONENT_AUDIT_MAX_RULES, COMPONENT_AUDIT_MAX_RULE_TOKENS,
};
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind};
use crate::domain::improvement::component_body_budget_tokens;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuditRuleUnit {
    pub index: usize,
    pub line_start: usize,
    pub line_end: usize,
    pub section_path: Vec<String>,
    pub raw: String,
    pub normalized: String,
    pub prefix: String,
    pub is_list: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OversizedRule {
    pub line_index: usize,
    pub tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditSnapshot {
    pub body_tokens: usize,
    pub rule_count: usize,
    pub oversized_rules: Vec<OversizedRule>,
    pub duplicate_rules: usize,
    pub empty_rules: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditTrigger {
    RuleCount,
    OversizedRule,
    BodyBudget,
    DuplicateRule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentAuditFinding {
    pub component: ComponentId,
    pub body_tokens: usize,
    pub rule_count: usize,
    pub oversized_rules: Vec<OversizedRule>,
    pub duplicate_rules: usize,
    pub triggers: Vec<AuditTrigger>,
}

impl AuditSnapshot {
    pub(crate) fn from_body(body: &str) -> Self {
        let rules = parse_audit_rules(body);
        let mut seen = HashSet::new();
        let duplicate_rules = rules
            .iter()
            .filter(|rule| !rule.normalized.is_empty() && !seen.insert(rule.normalized.clone()))
            .count();
        let empty_rules = rules
            .iter()
            .filter(|rule| rule.normalized.is_empty())
            .count();
        let oversized_rules = rules
            .iter()
            .filter_map(|rule| {
                let tokens = share::token_budget::estimate_tokens(&rule.raw);
                (tokens > COMPONENT_AUDIT_MAX_RULE_TOKENS).then_some(OversizedRule {
                    line_index: rule.line_start,
                    tokens,
                })
            })
            .collect();
        Self {
            body_tokens: share::token_budget::estimate_tokens(body),
            rule_count: rules.len(),
            oversized_rules,
            duplicate_rules,
            empty_rules,
        }
    }
}

fn heading_level(trimmed: &str) -> Option<usize> {
    let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
    (hashes > 0 && hashes <= 6).then_some(hashes)
}

fn list_prefix(line: &str) -> Option<(String, String)> {
    let indent = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return Some((
            line.get(..indent + 2).unwrap_or("- ").to_string(),
            rest.to_string(),
        ));
    }
    if let Some(rest) = trimmed.strip_prefix("* ") {
        return Some((
            line.get(..indent + 2).unwrap_or("* ").to_string(),
            rest.to_string(),
        ));
    }
    if let Some(rest) = trimmed.strip_prefix("+ ") {
        return Some((
            line.get(..indent + 2).unwrap_or("+ ").to_string(),
            rest.to_string(),
        ));
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let rest = trimmed.get(digits..)?;
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((
                line.get(..indent + digits + 2).unwrap_or("- ").to_string(),
                rest.to_string(),
            ));
        }
    }
    None
}

fn normalize_rule_text(text: &str) -> String {
    let mut value = text.trim().to_string();
    if let Some(rest) = value.strip_prefix('#') {
        value = rest.trim_start_matches('#').trim().to_string();
    }
    value = value.replace("**", "").replace("__", "");
    value = value.replace('`', "");
    while let Some(start) = value.find('[') {
        let Some(end_label) = value[start..].find(']') else {
            break;
        };
        let label_end = start + end_label;
        let Some(close) = value[label_end..].find(')') else {
            break;
        };
        let close = label_end + close;
        let label = value
            .get(start + 1..label_end)
            .unwrap_or_default()
            .to_string();
        value.replace_range(start..=close, &label);
    }
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// 解析组件正文中的规则单元（含行范围与 section 路径）。
pub(crate) fn parse_audit_rules(body: &str) -> Vec<AuditRuleUnit> {
    let lines: Vec<&str> = body.split('\n').collect();
    let mut units: Vec<AuditRuleUnit> = Vec::new();
    let mut sections: Vec<(usize, String)> = Vec::new();
    let mut in_frontmatter = false;
    let mut frontmatter_seen = false;
    let mut in_code = false;
    let mut paragraph: Option<usize> = None;

    for (line_index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if line_index == 0 && trimmed == "---" {
            in_frontmatter = true;
            frontmatter_seen = true;
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
            }
            continue;
        }
        if !frontmatter_seen && line_index == 0 {
            frontmatter_seen = true;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            paragraph = None;
            continue;
        }
        if in_code {
            continue;
        }
        if trimmed.is_empty() {
            paragraph = None;
            continue;
        }
        if let Some(level) = heading_level(trimmed) {
            sections.retain(|(current, _)| *current < level);
            sections.push((level, trimmed.trim_start_matches('#').trim().to_string()));
            paragraph = None;
            continue;
        }
        if trimmed.starts_with('|') {
            paragraph = None;
            continue;
        }
        let section_path = sections.iter().map(|(_, name)| name.clone()).collect();
        if let Some((prefix, text)) = list_prefix(line) {
            let index = units.len();
            units.push(AuditRuleUnit {
                index,
                line_start: line_index,
                line_end: line_index,
                section_path,
                raw: (*line).to_string(),
                normalized: normalize_rule_text(&text),
                prefix,
                is_list: true,
            });
            paragraph = None;
            continue;
        }
        if let Some(index) = paragraph {
            if let Some(unit) = units.get_mut(index) {
                if unit.line_end + 1 == line_index {
                    unit.line_end = line_index;
                    unit.raw.push('\n');
                    unit.raw.push_str(line);
                    unit.normalized = normalize_rule_text(&unit.raw);
                    continue;
                }
            }
        }
        if let Some(last) = units.last_mut() {
            if last.is_list
                && last.line_end + 1 == line_index
                && line.starts_with(char::is_whitespace)
            {
                last.line_end = line_index;
                last.raw.push('\n');
                last.raw.push_str(line);
                last.normalized = normalize_rule_text(&last.raw);
                continue;
            }
        }
        let index = units.len();
        units.push(AuditRuleUnit {
            index,
            line_start: line_index,
            line_end: line_index,
            section_path,
            raw: (*line).to_string(),
            normalized: normalize_rule_text(line),
            prefix: String::new(),
            is_list: false,
        });
        paragraph = Some(index);
    }
    units
}

/// 只对 prose 组件做审计：guardrails/tool-guidance/overlay。
pub(crate) fn is_auditable_kind(kind: ComponentKind) -> bool {
    matches!(
        kind,
        ComponentKind::Guardrails
            | ComponentKind::ToolGuidance
            | ComponentKind::SystemPromptOverlay
    )
}

pub fn audit_component(
    id: &ComponentId,
    content: &ComponentContent,
) -> Option<ComponentAuditFinding> {
    if !is_auditable_kind(id.kind) || content.body.trim().is_empty() {
        return None;
    }
    let snapshot = AuditSnapshot::from_body(&content.body);
    let mut triggers = Vec::new();
    if snapshot.rule_count > COMPONENT_AUDIT_MAX_RULES {
        triggers.push(AuditTrigger::RuleCount);
    }
    if !snapshot.oversized_rules.is_empty() {
        triggers.push(AuditTrigger::OversizedRule);
    }
    if snapshot.duplicate_rules > 0 {
        triggers.push(AuditTrigger::DuplicateRule);
    }
    let budget = component_body_budget_tokens(id);
    if snapshot.body_tokens.saturating_mul(100)
        > budget.saturating_mul(COMPONENT_AUDIT_BODY_BUDGET_PERCENT)
    {
        triggers.push(AuditTrigger::BodyBudget);
    }
    (!triggers.is_empty()).then_some(ComponentAuditFinding {
        component: id.clone(),
        body_tokens: snapshot.body_tokens,
        rule_count: snapshot.rule_count,
        oversized_rules: snapshot.oversized_rules,
        duplicate_rules: snapshot.duplicate_rules,
        triggers,
    })
}
