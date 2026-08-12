use super::*;

const VALID_BATCH_JSON: &str = r#"{
  "facts": [
    {
      "sequence": 7,
      "source": "main_user",
      "kind": "constraint",
      "text": "Do not merge the pull request.",
      "constraint": {
        "scope": "session",
        "lifecycle": "persistent",
        "action": "restrict"
      }
    }
  ]
}"#;

#[test]
fn compact_fact_batch_json_round_trips_strictly() {
    let batch: CompactFactBatch = serde_json::from_str(VALID_BATCH_JSON).unwrap();

    assert_eq!(batch.facts().len(), 1);
    assert_eq!(batch.facts()[0].sequence(), 7);
    assert_eq!(batch.facts()[0].source(), CompactFactSource::MainUser);
    assert_eq!(batch.facts()[0].kind(), CompactFactKind::Constraint);
    assert_eq!(
        batch.facts()[0].constraint_metadata().unwrap().scope(),
        ConstraintScope::Session
    );

    let encoded = serde_json::to_string(&batch).unwrap();
    let decoded: CompactFactBatch = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, batch);
}

#[test]
fn compact_fact_batch_rejects_unknown_fields() {
    let source =
        VALID_BATCH_JSON.replace("\"sequence\": 7,", "\"sequence\": 7, \"unexpected\": true,");

    let error = serde_json::from_str::<CompactFactBatch>(&source).unwrap_err();

    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn compact_fact_rejects_constraint_without_metadata() {
    let source = r#"{
      "facts": [{
        "sequence": 1,
        "source": "main_user",
        "kind": "constraint",
        "text": "Review only."
      }]
    }"#;

    let error = serde_json::from_str::<CompactFactBatch>(source).unwrap_err();

    assert!(error.to_string().contains("constraint metadata"));
}

#[test]
fn non_main_sources_cannot_establish_session_constraints() {
    for source in [
        CompactFactSource::AssistantReport,
        CompactFactSource::ToolInvocation,
        CompactFactSource::ToolResult,
        CompactFactSource::SystemGenerated,
        CompactFactSource::SubagentInstruction,
        CompactFactSource::Unknown,
    ] {
        let fact = CompactFact::constraint(
            1,
            source,
            "Read only",
            ConstraintMetadata::new(
                ConstraintScope::Session,
                ConstraintLifecycle::Persistent,
                ConstraintAction::Restrict,
            ),
        )
        .unwrap();

        let normalized = fact.normalize_scope();

        assert_eq!(
            normalized.constraint_metadata().unwrap().scope(),
            ConstraintScope::Unknown,
            "{source:?} must not establish a main session constraint"
        );
    }
}

#[test]
fn main_user_session_constraint_keeps_its_scope() {
    let fact = CompactFact::constraint(
        1,
        CompactFactSource::MainUser,
        "Do not merge",
        ConstraintMetadata::new(
            ConstraintScope::Session,
            ConstraintLifecycle::Persistent,
            ConstraintAction::Restrict,
        ),
    )
    .unwrap();

    assert_eq!(
        fact.normalize_scope()
            .constraint_metadata()
            .unwrap()
            .scope(),
        ConstraintScope::Session
    );
}
