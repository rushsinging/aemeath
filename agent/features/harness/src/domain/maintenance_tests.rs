use super::*;
use crate::domain::component::{ComponentContent, ComponentId, ComponentKind, ComponentName};
use crate::domain::component_audit::audit_component;
use crate::domain::improvement::{RsiLlmCompletion, RsiLlmPort};
use crate::domain::test_support::FakeRegistry;

struct ScriptedLlm {
    response: String,
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
    let compacted = "# Guardrails\n\n- 读取错误后再重试。\n".to_string();
    let mut registry = FakeRegistry::with_entries(vec![(id.clone(), content(original.clone()))]);
    let finding = audit_component(&id, &content(original)).expect("audit trigger");
    let response = serde_json::json!({
        "decision": "compact",
        "reason": "压缩超长规则",
        "body": compacted
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
    assert_eq!(
        registry.content_of(&id).unwrap().body,
        "# Guardrails\n\n- 读取错误后再重试。\n"
    );
}
