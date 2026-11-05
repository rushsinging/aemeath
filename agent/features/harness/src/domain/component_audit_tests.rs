use super::*;
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};

fn id(kind: ComponentKind, name: &str) -> ComponentId {
    ComponentId {
        kind,
        name: ComponentName::new(name),
    }
}

fn content(body: String) -> ComponentContent {
    ComponentContent {
        frontmatter: None,
        body,
    }
}

#[test]
fn clean_component_has_no_finding() {
    let body = "# Guardrails\n\n- 读取错误后再重试。\n- 无新增证据不重复调用。\n".to_string();
    assert!(audit_component(&id(ComponentKind::Guardrails, "default"), &content(body)).is_none());
}

#[test]
fn oversized_single_rule_triggers() {
    let body = format!("# Guardrails\n\n- {}\n", "规则".repeat(1200));
    let finding = audit_component(&id(ComponentKind::Guardrails, "default"), &content(body))
        .expect("oversized rule");
    assert!(finding.triggers.contains(&AuditTrigger::OversizedRule));
    assert_eq!(finding.oversized_rules.len(), 1);
}

#[test]
fn too_many_rules_trigger() {
    let lines: Vec<String> = (0..41).map(|index| format!("- rule {index}")).collect();
    let body = format!("# Tool Guidance\n\n{}\n", lines.join("\n"));
    let finding = audit_component(&id(ComponentKind::ToolGuidance, "default"), &content(body))
        .expect("rule count");
    assert!(finding.triggers.contains(&AuditTrigger::RuleCount));
    assert_eq!(finding.rule_count, 41);
}

#[test]
fn body_budget_trigger() {
    let body = format!("# Guardrails\n\n- {}\n", "长".repeat(5000));
    let finding = audit_component(&id(ComponentKind::Guardrails, "default"), &content(body))
        .expect("body budget");
    assert!(finding.triggers.contains(&AuditTrigger::BodyBudget));
}

#[test]
fn non_prose_component_is_ignored() {
    let body = format!("# Skill\n\n- {}\n", "规则".repeat(1200));
    assert!(audit_component(&id(ComponentKind::Skill, "demo"), &content(body)).is_none());
}
