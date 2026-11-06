use super::*;
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use crate::domain::component_audit::audit_component;
use crate::domain::improvement::{RsiLlmCompletion, RsiLlmPort};
use crate::domain::test_support::FakeRegistry;

struct ScriptedLlm {
    response: String,
}

struct SequentialLlm {
    responses: Vec<String>,
    calls: std::sync::atomic::AtomicUsize,
}

impl SequentialLlm {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses,
            calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl RsiLlmPort for SequentialLlm {
    async fn complete(&self, _prompt: &str) -> Result<String, HarnessError> {
        self.complete_with_usage(_prompt)
            .await
            .map(|value| value.text)
    }

    async fn complete_with_usage(&self, _prompt: &str) -> Result<RsiLlmCompletion, HarnessError> {
        let index = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.responses
            .get(index)
            .cloned()
            .map(RsiLlmCompletion::text)
            .ok_or_else(|| HarnessError::unavailable(crate::constants::ERROR_DOMAIN, "桩耗尽"))
    }
}

#[async_trait::async_trait]
impl RsiLlmPort for ScriptedLlm {
    async fn complete(&self, _prompt: &str) -> Result<String, HarnessError> {
        Ok(self.response.clone())
    }

    async fn complete_with_usage(&self, _prompt: &str) -> Result<RsiLlmCompletion, HarnessError> {
        Ok(RsiLlmCompletion::text(self.response.clone()))
    }
}

fn guardrails_id() -> ComponentId {
    ComponentId {
        kind: ComponentKind::Guardrails,
        name: ComponentName::new("default"),
    }
}

fn content(body: String) -> ComponentContent {
    ComponentContent {
        frontmatter: None,
        body,
    }
}

#[tokio::test]
async fn maintenance_compacts_oversized_rule() {
    let id = guardrails_id();
    let original = format!("# Guardrails\n\n- {}\n", "规则".repeat(1200));
    let compacted =
        "# Guardrails\n\n- 读取错误与输出后再重试，未诊断前禁止原样重复调用。\n".to_string();
    let mut registry = FakeRegistry::with_entries(vec![(id.clone(), content(original.clone()))]);
    let finding = audit_component(&id, &content(original)).expect("audit trigger");
    let response = serde_json::json!({
        "decision": "compact",
        "reason": "压缩超长规则",
        "rules": [{"action": "merge", "source": [0], "text": "读取错误与输出后再重试，未诊断前禁止原样重复调用。"}]
    })
    .to_string();
    let llm = ScriptedLlm { response };
    let mut preview = Vec::new();

    let summaries = run_maintenance_compose_phase(
        &mut registry,
        &llm,
        &[finding],
        false,
        &mut preview,
        None,
        None,
    )
    .await
    .expect("maintenance");

    assert!(summaries[0].contains("维护 Compose"));
    assert_eq!(registry.content_of(&id).unwrap().body, compacted);
}

#[tokio::test]
async fn maintenance_rejects_missing_rule_coverage() {
    let id = guardrails_id();
    let original = format!(
        "# Guardrails\n\n{}\n",
        (0..41)
            .map(|index| format!("- rule {index}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut registry = FakeRegistry::with_entries(vec![(id.clone(), content(original.clone()))]);
    let finding = audit_component(&id, &content(original.clone())).expect("audit");
    let response = serde_json::json!({
        "decision": "compact",
        "reason": "只覆盖第一条",
        "rules": [{"action": "keep", "source": [0]}]
    })
    .to_string();
    let llm = ScriptedLlm { response };
    let mut preview = Vec::new();

    let summaries = run_maintenance_compose_phase(
        &mut registry,
        &llm,
        &[finding],
        false,
        &mut preview,
        None,
        None,
    )
    .await
    .expect("maintenance");

    assert!(summaries[0].contains("覆盖校验失败"), "{}", summaries[0]);
    assert_eq!(registry.content_of(&id).unwrap().body, original);
}

#[tokio::test]
async fn maintenance_rejects_semantic_fragment() {
    let id = guardrails_id();
    let original = format!("# Guardrails\n\n- {}\n", "规则".repeat(1200));
    let mut registry = FakeRegistry::with_entries(vec![(id.clone(), content(original.clone()))]);
    let finding = audit_component(&id, &content(original.clone())).expect("audit");
    let response = serde_json::json!({
        "decision": "compact",
        "reason": "压成残片",
        "rules": [{"action": "merge", "source": [0], "text": "规则"}]
    })
    .to_string();
    let llm = ScriptedLlm { response };
    let mut preview = Vec::new();

    let summaries = run_maintenance_compose_phase(
        &mut registry,
        &llm,
        &[finding],
        false,
        &mut preview,
        None,
        None,
    )
    .await
    .expect("maintenance");

    assert!(summaries[0].contains("覆盖校验失败"), "{}", summaries[0]);
    assert_eq!(registry.content_of(&id).unwrap().body, original);
}

#[tokio::test]
async fn dry_run_maintenance_does_not_write_version_snapshot() {
    let id = guardrails_id();
    let original = format!("# Guardrails\n\n- {}\n", "规则".repeat(1200));
    let mut registry = FakeRegistry::with_entries(vec![(id.clone(), content(original.clone()))]);
    let finding = audit_component(&id, &content(original.clone())).expect("audit");
    let response = serde_json::json!({
        "decision": "compact",
        "reason": "压缩超长规则",
        "rules": [{"action": "merge", "source": [0], "text": "读取错误与输出后再重试，未诊断前禁止原样重复调用。"}]
    })
    .to_string();
    let llm = ScriptedLlm { response };
    let version_dir = tempfile::tempdir().expect("tempdir");
    let version_store = crate::domain::versioning::ComponentVersionStore::new(version_dir.path());
    let mut preview = Vec::new();

    let summaries = run_maintenance_compose_phase(
        &mut registry,
        &llm,
        &[finding],
        true,
        &mut preview,
        None,
        Some(&version_store),
    )
    .await
    .expect("maintenance");

    assert!(summaries[0].contains("维护 Compose"));
    assert!(
        !version_dir
            .path()
            .join("guardrails/default/maintenance.json")
            .exists(),
        "dry-run NEVER 写 version snapshot"
    );
}

#[tokio::test]
async fn maintenance_strips_bullet_marker_from_merge_text() {
    let id = guardrails_id();
    let original = format!("# Guardrails\n\n- {}\n", "规则".repeat(1200));
    let mut registry = FakeRegistry::with_entries(vec![(id.clone(), content(original.clone()))]);
    let finding = audit_component(&id, &content(original.clone())).expect("audit");
    let response = serde_json::json!({
        "decision": "compact",
        "reason": "压缩超长规则",
        "rules": [{"action": "merge", "source": [0], "text": "- 读取错误与输出后再重试，未诊断前禁止原样重复调用。"}]
    })
    .to_string();
    let llm = ScriptedLlm { response };
    let mut preview = Vec::new();

    run_maintenance_compose_phase(
        &mut registry,
        &llm,
        &[finding],
        false,
        &mut preview,
        None,
        None,
    )
    .await
    .expect("maintenance");

    assert_eq!(
        registry.content_of(&id).unwrap().body,
        "# Guardrails\n\n- 读取错误与输出后再重试，未诊断前禁止原样重复调用。\n"
    );
}

#[tokio::test]
async fn oversized_body_is_chunked_and_compacted() {
    let id = guardrails_id();
    let lines: Vec<String> = (0..61)
        .map(|index| format!("- {} rule {index}", "长".repeat(150)))
        .collect();
    let original = format!("# Guardrails\n\n{}\n", lines.join("\n"));
    let mut registry = FakeRegistry::with_entries(vec![(id.clone(), content(original.clone()))]);
    let finding = audit_component(&id, &content(original.clone())).expect("audit");
    assert!(finding.body_tokens > 3800);
    let responses = vec![
        serde_json::json!({
            "decision": "compact",
            "reason": "chunk-1",
            "rules": [{"action": "merge", "source": (0..30).collect::<Vec<_>>(), "text": "第一块合并规则，保留执行语义与诊断顺序。"}]
        })
        .to_string(),
        serde_json::json!({
            "decision": "compact",
            "reason": "chunk-2",
            "rules": [{"action": "merge", "source": (0..30).collect::<Vec<_>>(), "text": "第二块合并规则，保留执行语义与诊断顺序。"}]
        })
        .to_string(),
        serde_json::json!({
            "decision": "compact",
            "reason": "chunk-3",
            "rules": [{"action": "merge", "source": [0], "text": "第三块规则，保留执行语义。"}]
        })
        .to_string(),
    ];
    let llm = SequentialLlm::new(responses);
    let mut preview = Vec::new();

    let summaries = run_maintenance_compose_phase(
        &mut registry,
        &llm,
        &[finding],
        true,
        &mut preview,
        None,
        None,
    )
    .await
    .expect("maintenance");

    assert!(summaries[0].contains("维护 Compose"), "{}", summaries[0]);
    assert_eq!(llm.calls.load(std::sync::atomic::Ordering::SeqCst), 3);
    let body = registry.content_of(&id).unwrap().body.clone();
    assert!(body.contains("第一块合并规则"), "{body}");
    assert!(body.contains("第二块合并规则"), "{body}");
    assert!(body.contains("第三块规则"), "{body}");
    assert_eq!(
        crate::domain::component_audit::AuditSnapshot::from_body(&body).rule_count,
        3
    );
}

#[tokio::test]
async fn body_beyond_double_budget_is_skipped_without_llm_calls() {
    let id = guardrails_id();
    let lines: Vec<String> = (0..200)
        .map(|index| format!("- {} rule {index}", "长".repeat(150)))
        .collect();
    let original = format!("# Guardrails\n\n{}\n", lines.join("\n"));
    let mut registry = FakeRegistry::with_entries(vec![(id.clone(), content(original.clone()))]);
    let finding = audit_component(&id, &content(original.clone())).expect("audit");
    let llm = SequentialLlm::new(Vec::new());
    let mut preview = Vec::new();

    let summaries = run_maintenance_compose_phase(
        &mut registry,
        &llm,
        &[finding],
        true,
        &mut preview,
        None,
        None,
    )
    .await
    .expect("maintenance");

    assert!(summaries[0].contains("超预算过大"), "{}", summaries[0]);
    assert_eq!(llm.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}
