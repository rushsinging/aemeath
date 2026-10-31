use crate::domain::component::{ComponentId, ComponentKind, ComponentName};
use crate::domain::evidence::{EvidenceSignal, SignalKind};

fn component_id(kind: ComponentKind, name: &str) -> ComponentId {
    ComponentId {
        kind,
        name: ComponentName::new(name),
    }
}

fn signal(kind: SignalKind, signal_id: &str) -> EvidenceSignal {
    EvidenceSignal {
        kind,
        signal_id: signal_id.to_string(),
        summary: String::new(),
        excerpt: None,
        occurrences: 1,
        sources: Vec::new(),
    }
}

fn tool_guidance() -> ComponentId {
    component_id(ComponentKind::ToolGuidance, "default")
}

fn guardrails() -> ComponentId {
    component_id(ComponentKind::Guardrails, "default")
}

fn signal_with_session(kind: SignalKind, signal_id: &str, session_ids: &[&str]) -> EvidenceSignal {
    EvidenceSignal {
        sources: session_ids
            .iter()
            .map(|session_id| crate::domain::evidence::EvidenceSourceRef {
                session_id: (*session_id).to_string(),
                run_id: "run-1".to_string(),
                step_id: "step-1".to_string(),
            })
            .collect(),
        ..signal(kind, signal_id)
    }
}

#[test]
fn project_overlay_only_accepts_matching_single_project_signal() {
    let project_a = component_id(ComponentKind::SystemPromptOverlay, "0123456789abcdef");
    let project_b = component_id(ComponentKind::SystemPromptOverlay, "fedcba9876543210");
    let candidates = vec![tool_guidance(), project_a.clone(), project_b.clone()];
    let only_a = signal_with_session(
        SignalKind::RepeatedFailure,
        "repeated_failure:Bash:project-a-only",
        &["0123456789abcdef/s1.dataset"],
    );
    let decision = crate::domain::routing::route_signal(
        &only_a,
        &candidates,
        &crate::domain::routing::RouteContext::default(),
    );
    match decision {
        crate::domain::routing::RouteDecision::Choose { candidates, .. } => {
            assert!(candidates.contains(&project_a));
            assert!(!candidates.contains(&project_b));
        }
        other => panic!("预期候选集：{other:?}"),
    }

    let cross_project = signal_with_session(
        SignalKind::RepeatedFailure,
        "repeated_failure:Bash:cross-project",
        &["0123456789abcdef/s1.dataset", "fedcba9876543210/s2.dataset"],
    );
    let decision = crate::domain::routing::route_signal(
        &cross_project,
        &candidates,
        &crate::domain::routing::RouteContext::default(),
    );
    assert_eq!(
        decision,
        crate::domain::routing::RouteDecision::Exact {
            component: tool_guidance(),
            reason: "内置工具 Bash 的执行纪律问题".to_string(),
        }
    );
}

#[test]
fn builtin_tool_failure_routes_to_guidance_or_guardrails() {
    let candidates = vec![tool_guidance(), guardrails()];
    let decision = crate::domain::routing::route_signal(
        &signal(
            SignalKind::RepeatedFailure,
            "repeated_failure:Bash:permission denied",
        ),
        &candidates,
        &crate::domain::routing::RouteContext::default(),
    );
    assert!(matches!(
        decision,
        crate::domain::routing::RouteDecision::Choose { ref candidates, .. }
            if *candidates == vec![tool_guidance(), guardrails()]
    ));
}

#[test]
fn builtin_tool_failure_exactly_routes_when_one_candidate_exists() {
    let decision = crate::domain::routing::route_signal(
        &signal(
            SignalKind::RetryPattern,
            "retry_pattern:Read:{\"file_path\":\"x\"}",
        ),
        &[tool_guidance()],
        &crate::domain::routing::RouteContext::default(),
    );
    assert_eq!(
        decision,
        crate::domain::routing::RouteDecision::Exact {
            component: tool_guidance(),
            reason: "内置工具 Read 的执行纪律问题".to_string(),
        }
    );
}

#[test]
fn skill_signal_requires_exact_skill_name() {
    let cmux = component_id(ComponentKind::Skill, "cmux-diagnostics");
    let other = component_id(ComponentKind::Skill, "archify");
    let context = crate::domain::routing::RouteContext {
        skill_name: Some("cmux-diagnostics".to_string()),
    };
    let decision = crate::domain::routing::route_signal(
        &signal(SignalKind::OversizedResult, "oversized_result:Skill"),
        &[cmux.clone(), other],
        &context,
    );
    assert_eq!(
        decision,
        crate::domain::routing::RouteDecision::Exact {
            component: cmux,
            reason: "Skill 证据精确匹配 cmux-diagnostics".to_string(),
        }
    );
}

#[test]
fn unknown_skill_name_is_unresolved_rather_than_overlay_fallback() {
    let decision = crate::domain::routing::route_signal(
        &signal(SignalKind::OversizedResult, "oversized_result:Skill"),
        &[
            tool_guidance(),
            component_id(ComponentKind::SystemPromptOverlay, "global"),
        ],
        &crate::domain::routing::RouteContext::default(),
    );
    assert!(matches!(
        decision,
        crate::domain::routing::RouteDecision::Unresolved { .. }
    ));
}

#[test]
fn route_context_extracts_skill_name_from_bounded_session_context() {
    let context = crate::domain::routing::RouteContext::from_context_excerpt(
        r#"前文\n工具调用 Skill（goal：诊断）：{"skill":"cmux-diagnostics","query":"x"}\n结果"#,
    );
    assert_eq!(context.skill_name.as_deref(), Some("cmux-diagnostics"));
    assert_eq!(
        crate::domain::routing::RouteContext::from_context_excerpt("没有 skill 输入"),
        crate::domain::routing::RouteContext::default()
    );
}

#[test]
fn aggregate_run_outcome_has_no_component_route() {
    let decision = crate::domain::routing::route_signal(
        &signal(SignalKind::RunOutcome, "run_outcome:aggregate"),
        &[tool_guidance(), guardrails()],
        &crate::domain::routing::RouteContext::default(),
    );
    assert!(matches!(
        decision,
        crate::domain::routing::RouteDecision::Unresolved { .. }
    ));
}

#[test]
fn builtin_tool_signal_never_falls_back_to_overlay_alone() {
    let overlay = component_id(ComponentKind::SystemPromptOverlay, "global");
    let decision = crate::domain::routing::route_signal(
        &signal(
            SignalKind::RepeatedFailure,
            "repeated_failure:Bash:permission denied",
        ),
        &[overlay],
        &crate::domain::routing::RouteContext::default(),
    );
    assert_eq!(
        decision,
        crate::domain::routing::RouteDecision::Unresolved {
            reason: "缺少 tool-guidance/guardrails 主候选，内置工具纪律 NEVER 兜底到 overlay"
                .to_string(),
        }
    );
}

#[test]
fn agent_signal_routes_to_sub_agents_config() {
    let sub_agents = component_id(ComponentKind::SubAgents, "default");
    let decision = crate::domain::routing::route_signal(
        &signal(
            SignalKind::RepeatedFailure,
            "repeated_failure:Agent:missing-role",
        ),
        std::slice::from_ref(&sub_agents),
        &crate::domain::routing::RouteContext::default(),
    );
    assert_eq!(
        decision,
        crate::domain::routing::RouteDecision::Exact {
            component: sub_agents,
            reason: "Agent 配置/派发问题".to_string(),
        }
    );
}
