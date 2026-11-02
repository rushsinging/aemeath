use crate::domain::component::{ComponentId, ComponentKind, ComponentName};
use crate::domain::scope_dedup::{
    build_adjudication_prompt, find_cross_scope_candidates, parse_adjudications, remove_rule_lines,
    rule_units,
};

fn project(name: &str) -> ComponentId {
    ComponentId {
        kind: ComponentKind::SystemPromptOverlay,
        name: ComponentName::new(name),
    }
}

fn tool_guidance() -> ComponentId {
    ComponentId {
        kind: ComponentKind::ToolGuidance,
        name: ComponentName::new("default"),
    }
}

#[test]
fn extracts_bullets_and_numbered_rules_only() {
    let body = "# Title\n\n- Read 超长时分段读取。\n1. Bash 失败先读错误。\n说明文字\n";
    let rules = rule_units(body);
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[1].text, "Bash 失败先读错误。");
}

#[test]
fn recalls_cross_scope_duplicate_with_cjk_bigram() {
    let project_id = project("0123456789abcdef");
    let global_id = tool_guidance();
    let project_body = "- Read 超长时先限定行范围、分段、过滤或摘要，只取诊断片段。\n";
    let global_body = "- Read 超长时先限定行范围、分段、过滤或摘要，只保留诊断片段。\n";
    let candidates = find_cross_scope_candidates(
        (&project_id, project_body),
        &[((global_id.clone(), String::new()), global_body)],
    );
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].similarity > 0.5);
    assert_eq!(candidates[0].project_component, project_id);
    assert_eq!(candidates[0].global_component, global_id);
    assert_eq!(candidates[0].project_line, 0);
    assert_eq!(candidates[0].global_line, 0);
}

#[test]
fn adjudication_prompt_and_parser_use_structured_contract() {
    let project_id = project("0123456789abcdef");
    let global_id = tool_guidance();
    let candidates = find_cross_scope_candidates(
        (&project_id, "- Bash 失败后先读错误，再决定是否重试。\n"),
        &[(
            (global_id, String::new()),
            "- Bash 失败后先读错误，再决定是否重试。\n",
        )],
    );
    let prompt = build_adjudication_prompt(&candidates);
    assert!(prompt.contains("scope 仲裁器"));
    assert!(prompt.contains("authority=global"));
    assert!(prompt.contains("只输出 JSON"));
    let parsed = parse_adjudications(
        r#"{"verdicts":[{"index":0,"verdict":"duplicate","authority":"global","reason":"重复"}]}"#,
    )
    .expect("parse");
    assert_eq!(parsed.len(), 1);
    assert_eq!(
        parsed[0].verdict,
        crate::domain::scope_dedup::DuplicateVerdict::Duplicate
    );
}

#[test]
fn removes_only_requested_rule_lines_and_preserves_other_lines() {
    let body = "# Title\n- keep\n- remove\n- other\n";
    let output = remove_rule_lines(body, vec![2]);
    assert_eq!(output, "# Title\n- keep\n- other\n");
}
