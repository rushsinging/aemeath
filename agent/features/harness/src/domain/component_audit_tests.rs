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

#[test]
fn parser_skips_frontmatter_tables_and_code_blocks() {
    let body = "---\nname: demo\n---\n# Section A\n\n| col | value |\n|---|---|\n| x | y |\n\n```bash\n- not a rule\n```\n\n- real rule one\n\nparagraph rule two\n\n## Section B\n- real rule three\n"
        .to_string();
    let rules = parse_audit_rules(&body);
    assert_eq!(rules.len(), 3, "{rules:#?}");
    assert_eq!(rules[0].section_path, vec!["Section A".to_string()]);
    assert_eq!(rules[1].normalized, "paragraph rule two");
    assert_eq!(rules[2].section_path, vec!["Section B".to_string()]);
    assert!(rules.iter().all(|rule| !rule.raw.contains("not a rule")));
}

#[test]
fn duplicate_rule_triggers_audit() {
    let body = "# Guardrails\n\n- same rule\n- same rule\n".to_string();
    let finding = audit_component(&id(ComponentKind::Guardrails, "default"), &content(body))
        .expect("duplicate");
    assert!(finding.triggers.contains(&AuditTrigger::DuplicateRule));
    assert_eq!(finding.duplicate_rules, 1);
}

#[test]
fn overlay_is_auditable() {
    let body = format!("# Overlay\n\n- {}\n", "长".repeat(5000));
    let finding = audit_component(&id(ComponentKind::SystemPromptOverlay, "global"), &content(body))
        .expect("overlay budget");
    assert!(finding.triggers.contains(&AuditTrigger::BodyBudget));
}
