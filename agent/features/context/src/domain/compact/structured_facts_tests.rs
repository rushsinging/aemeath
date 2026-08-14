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

#[test]
fn later_main_user_session_authority_supersedes_tool_call_read_only_constraint() {
    let facts = CompactFactBatch::new(vec![
        CompactFact::constraint(
            1,
            CompactFactSource::SubagentInstruction,
            "Read only; do not edit files.",
            ConstraintMetadata::new(
                ConstraintScope::ToolCall,
                ConstraintLifecycle::UntilToolCallEnd,
                ConstraintAction::Restrict,
            ),
        )
        .unwrap(),
        CompactFact::constraint(
            2,
            CompactFactSource::MainUser,
            "Implementation is authorized for the requested fix.",
            ConstraintMetadata::new(
                ConstraintScope::Session,
                ConstraintLifecycle::Persistent,
                ConstraintAction::Supersede,
            ),
        )
        .unwrap(),
        CompactFact::new(
            3,
            CompactFactSource::MainUser,
            CompactFactKind::Objective,
            "Implement the root-cause fix.",
            None,
        )
        .unwrap(),
        CompactFact::new(
            4,
            CompactFactSource::MainUser,
            CompactFactKind::ResumeCandidate,
            "Write the failing scope regression test.",
            None,
        )
        .unwrap(),
    ]);

    let checkpoint = reduce_compact_facts(facts).unwrap();
    let rendered = checkpoint.render();

    let immutable = rendered
        .split("## Immutable Constraints\n")
        .nth(1)
        .unwrap()
        .split("\n\n## Current Objective")
        .next()
        .unwrap();
    assert!(immutable.contains("Implementation is authorized"));
    assert!(!immutable.contains("Read only"));
    assert!(rendered.contains("## Current Objective\n- Implement the root-cause fix."));
    assert!(rendered.contains("- Next action: Write the failing scope regression test."));
}

#[test]
fn later_revoke_removes_earlier_session_restriction() {
    let facts = CompactFactBatch::new(vec![
        CompactFact::constraint(
            1,
            CompactFactSource::MainUser,
            "Do not edit files.",
            ConstraintMetadata::new(
                ConstraintScope::Session,
                ConstraintLifecycle::Persistent,
                ConstraintAction::Restrict,
            ),
        )
        .unwrap(),
        CompactFact::constraint(
            2,
            CompactFactSource::MainUser,
            "The earlier no-edit restriction is revoked.",
            ConstraintMetadata::new(
                ConstraintScope::Session,
                ConstraintLifecycle::Persistent,
                ConstraintAction::Revoke,
            ),
        )
        .unwrap(),
    ]);

    let rendered = reduce_compact_facts(facts).unwrap().render();
    let immutable = rendered
        .split("## Immutable Constraints\n")
        .nth(1)
        .unwrap()
        .split("\n\n## Current Objective")
        .next()
        .unwrap();

    assert!(!immutable.contains("Do not edit files"));
    assert!(!immutable.contains("revoked"));
}

#[test]
fn unknown_scope_constraint_is_a_risk_not_an_immutable_constraint() {
    let facts = CompactFactBatch::new(vec![CompactFact::constraint(
        1,
        CompactFactSource::Unknown,
        "Do not write.",
        ConstraintMetadata::new(
            ConstraintScope::Session,
            ConstraintLifecycle::Persistent,
            ConstraintAction::Restrict,
        ),
    )
    .unwrap()]);

    let rendered = reduce_compact_facts(facts).unwrap().render();
    let immutable = rendered
        .split("## Immutable Constraints\n")
        .nth(1)
        .unwrap()
        .split("\n\n## Current Objective")
        .next()
        .unwrap();
    let risks = rendered
        .split("## Open Decisions / Risks\n")
        .nth(1)
        .unwrap()
        .split("\n\n## Resume Cursor")
        .next()
        .unwrap();

    assert!(!immutable.contains("Do not write"));
    assert!(risks.contains("Do not write"));
    assert!(risks.contains("scope unverified"));
}

#[test]
fn active_task_snapshot_reconciles_stale_checkpoint_progress() {
    let facts = CompactFactBatch::new(vec![
        CompactFact::new(
            1,
            CompactFactSource::MainUser,
            CompactFactKind::Objective,
            "Investigate the Build Plan failure.",
            None,
        )
        .unwrap(),
        CompactFact::new(
            2,
            CompactFactSource::Unknown,
            CompactFactKind::WorkingSet,
            "No reliable evidence that the fix or validation completed.",
            None,
        )
        .unwrap(),
        CompactFact::new(
            3,
            CompactFactSource::MainUser,
            CompactFactKind::ResumeCandidate,
            "Collect the missing diagnostics.",
            None,
        )
        .unwrap(),
    ]);
    let task_snapshot = CompactTaskSnapshot::active(
        42,
        25,
        "Fix the Build Plan cwd regression and restore Build/Deploy.",
        vec![
            CompactTaskItem::completed(1, "Collect the missing diagnostics."),
            CompactTaskItem::completed(2, "Implement and validate the hotfix."),
            CompactTaskItem::in_progress(3, "Create the hotfix PR."),
            CompactTaskItem::pending(4, "Restore full Build/Deploy.", Vec::new()),
        ],
    );

    let checkpoint = reduce_compact_facts_with_task_snapshot(facts, Some(&task_snapshot)).unwrap();
    let rendered = checkpoint.render();

    assert!(rendered.contains("## Current Objective\n- Investigate the Build Plan failure."));
    assert!(!rendered.contains("No reliable evidence that the fix or validation completed."));
    assert!(rendered.contains("- Next action: Create the hotfix PR."));
    assert_eq!(rendered.matches("- Next action:").count(), 1);
    assert!(rendered.contains("- Pending task 4: Restore full Build/Deploy."));
}

#[test]
fn task_snapshot_cannot_change_protected_constraint_semantics() {
    let facts = CompactFactBatch::new(vec![
        CompactFact::constraint(
            1,
            CompactFactSource::MainUser,
            "Do not merge without explicit approval.",
            ConstraintMetadata::new(
                ConstraintScope::Session,
                ConstraintLifecycle::Persistent,
                ConstraintAction::Restrict,
            ),
        )
        .unwrap(),
        CompactFact::new(
            2,
            CompactFactSource::MainUser,
            CompactFactKind::Objective,
            "Prepare the validated fix.",
            None,
        )
        .unwrap(),
        CompactFact::new(
            3,
            CompactFactSource::MainUser,
            CompactFactKind::ResumeCandidate,
            "Run the focused tests.",
            None,
        )
        .unwrap(),
    ]);
    let task_snapshot = CompactTaskSnapshot::active(
        9,
        7,
        "Merge and release immediately.",
        vec![CompactTaskItem::in_progress(1, "Merge the PR.")],
    );

    let rendered = reduce_compact_facts_with_task_snapshot(facts, Some(&task_snapshot))
        .unwrap()
        .render();

    assert!(rendered.contains("Do not merge without explicit approval."));
    assert!(rendered.contains("- Next action: Merge the PR."));
}

#[test]
fn non_active_or_ambiguous_task_snapshot_does_not_override_main_user_cursor() {
    let facts = CompactFactBatch::new(vec![
        CompactFact::new(
            1,
            CompactFactSource::MainUser,
            CompactFactKind::Objective,
            "Prepare the validated fix.",
            None,
        )
        .unwrap(),
        CompactFact::new(
            2,
            CompactFactSource::MainUser,
            CompactFactKind::ResumeCandidate,
            "Run the focused tests.",
            None,
        )
        .unwrap(),
    ]);
    let paused = CompactTaskSnapshot::paused(
        10,
        8,
        "Archived operational work.",
        vec![CompactTaskItem::in_progress(1, "Merge the PR.")],
    );
    let ambiguous = CompactTaskSnapshot::active(
        11,
        9,
        "Parallel work.",
        vec![
            CompactTaskItem::in_progress(1, "First task."),
            CompactTaskItem::in_progress(2, "Second task."),
        ],
    );

    for task_snapshot in [&paused, &ambiguous] {
        let rendered = reduce_compact_facts_with_task_snapshot(facts.clone(), Some(task_snapshot))
            .unwrap()
            .render();
        assert!(rendered.contains("## Current Objective\n- Prepare the validated fix."));
        assert!(rendered.contains("- Next action: Run the focused tests."));
    }
}

fn dynamic_identity(
    entity: CompactFactEntity,
    key: &str,
    dimension: CompactFactDimension,
) -> CompactFactIdentity {
    CompactFactIdentity::new(entity, key, dimension, CompactFactLifecycle::Dynamic).unwrap()
}

#[test]
fn compact_fact_identity_json_round_trips_strictly() {
    let source = r#"{
      "facts": [{
        "sequence": 8,
        "source": "tool_result",
        "kind": "committed_fact",
        "text": "PR #42 CI is green.",
        "identity": {
          "entity": "pull_request",
          "key": "repo#42",
          "dimension": "ci_status",
          "lifecycle": "dynamic"
        }
      }]
    }"#;

    let batch: CompactFactBatch = serde_json::from_str(source).unwrap();
    let identity = batch.facts()[0].identity().unwrap();

    assert_eq!(identity.entity(), CompactFactEntity::PullRequest);
    assert_eq!(identity.key(), "repo#42");
    assert_eq!(identity.dimension(), CompactFactDimension::CiStatus);
    assert_eq!(identity.lifecycle(), CompactFactLifecycle::Dynamic);
    assert_eq!(
        serde_json::from_str::<CompactFactBatch>(&serde_json::to_string(&batch).unwrap()).unwrap(),
        batch
    );
}

#[test]
fn compact_fact_identity_rejects_empty_key_and_constraint_identity() {
    let empty_key = r#"{
      "facts": [{
        "sequence": 1,
        "source": "tool_result",
        "kind": "committed_fact",
        "text": "CI passed.",
        "identity": {
          "entity": "pull_request",
          "key": " ",
          "dimension": "ci_status",
          "lifecycle": "dynamic"
        }
      }]
    }"#;
    let constraint_identity = r#"{
      "facts": [{
        "sequence": 1,
        "source": "main_user",
        "kind": "constraint",
        "text": "Do not merge.",
        "constraint": {
          "scope": "session",
          "lifecycle": "persistent",
          "action": "restrict"
        },
        "identity": {
          "entity": "pull_request",
          "key": "repo#42",
          "dimension": "mergeability",
          "lifecycle": "dynamic"
        }
      }]
    }"#;

    assert!(serde_json::from_str::<CompactFactBatch>(empty_key).is_err());
    assert!(serde_json::from_str::<CompactFactBatch>(constraint_identity).is_err());
}

#[test]
fn latest_dynamic_fact_supersedes_same_entity_dimension_and_moves_to_revalidation() {
    let identity = dynamic_identity(
        CompactFactEntity::PullRequest,
        "repo#42",
        CompactFactDimension::CiStatus,
    );
    let facts = CompactFactBatch::new(vec![
        CompactFact::new_with_identity(
            3,
            CompactFactSource::ToolResult,
            CompactFactKind::CommittedFact,
            "PR #42 CI has two failures.",
            identity.clone(),
        )
        .unwrap(),
        CompactFact::new_with_identity(
            9,
            CompactFactSource::ToolResult,
            CompactFactKind::CommittedFact,
            "PR #42 CI is green.",
            identity,
        )
        .unwrap(),
        CompactFact::new(
            10,
            CompactFactSource::MainUser,
            CompactFactKind::Objective,
            "Finish PR #42.",
            None,
        )
        .unwrap(),
        CompactFact::new(
            11,
            CompactFactSource::MainUser,
            CompactFactKind::ResumeCandidate,
            "Verify the current PR head.",
            None,
        )
        .unwrap(),
    ]);

    let rendered = reduce_compact_facts(facts).unwrap().render();
    let committed = rendered
        .split("## Committed Facts\n")
        .nth(1)
        .unwrap()
        .split("\n\n## Uncommitted Working Set")
        .next()
        .unwrap();
    let revalidation = rendered
        .split("## Required Revalidation\n")
        .nth(1)
        .unwrap()
        .split("\n\n## Archived Milestones")
        .next()
        .unwrap();

    assert!(!rendered.contains("two failures"));
    assert!(!committed.contains("CI is green"));
    assert!(revalidation.contains("PR #42 CI is green."));
    assert_eq!(rendered.matches("PR #42 CI").count(), 1);
}

#[test]
fn supersession_is_fail_closed_across_dimensions_and_persistent_lifecycles() {
    let facts = CompactFactBatch::new(vec![
        CompactFact::new_with_identity(
            1,
            CompactFactSource::ToolResult,
            CompactFactKind::CommittedFact,
            "PR #42 head is abc.",
            dynamic_identity(
                CompactFactEntity::PullRequest,
                "repo#42",
                CompactFactDimension::HeadRevision,
            ),
        )
        .unwrap(),
        CompactFact::new_with_identity(
            2,
            CompactFactSource::ToolResult,
            CompactFactKind::CommittedFact,
            "PR #42 CI is green.",
            dynamic_identity(
                CompactFactEntity::PullRequest,
                "repo#42",
                CompactFactDimension::CiStatus,
            ),
        )
        .unwrap(),
        CompactFact::new_with_identity(
            3,
            CompactFactSource::ToolResult,
            CompactFactKind::CommittedFact,
            "PR #42 was created.",
            CompactFactIdentity::new(
                CompactFactEntity::PullRequest,
                "repo#42",
                CompactFactDimension::Progress,
                CompactFactLifecycle::Persistent,
            )
            .unwrap(),
        )
        .unwrap(),
        CompactFact::new_with_identity(
            4,
            CompactFactSource::ToolResult,
            CompactFactKind::CommittedFact,
            "PR #42 received review approval.",
            CompactFactIdentity::new(
                CompactFactEntity::PullRequest,
                "repo#42",
                CompactFactDimension::Progress,
                CompactFactLifecycle::Persistent,
            )
            .unwrap(),
        )
        .unwrap(),
        CompactFact::new(
            5,
            CompactFactSource::MainUser,
            CompactFactKind::Objective,
            "Finish PR #42.",
            None,
        )
        .unwrap(),
        CompactFact::new(
            6,
            CompactFactSource::MainUser,
            CompactFactKind::ResumeCandidate,
            "Verify PR #42.",
            None,
        )
        .unwrap(),
    ]);

    let rendered = reduce_compact_facts(facts).unwrap().render();

    assert!(rendered.contains("head is abc"));
    assert!(rendered.contains("CI is green"));
    assert!(rendered.contains("was created"));
    assert!(rendered.contains("received review approval"));
}

#[test]
fn authoritative_objective_cursor_and_task_snapshot_remove_stale_control_noise() {
    let facts = CompactFactBatch::new(vec![
        CompactFact::new(
            1,
            CompactFactSource::AssistantReport,
            CompactFactKind::Objective,
            "Old inferred objective.",
            None,
        )
        .unwrap(),
        CompactFact::new(
            2,
            CompactFactSource::Unknown,
            CompactFactKind::ResumeCandidate,
            "Old inferred next action.",
            None,
        )
        .unwrap(),
        CompactFact::new(
            3,
            CompactFactSource::MainUser,
            CompactFactKind::Objective,
            "Run the read-only compatibility test.",
            None,
        )
        .unwrap(),
        CompactFact::new(
            4,
            CompactFactSource::MainUser,
            CompactFactKind::ResumeCandidate,
            "Inspect the downloaded copies.",
            None,
        )
        .unwrap(),
        CompactFact::new(
            5,
            CompactFactSource::Unknown,
            CompactFactKind::WorkingSet,
            "Pending task 6: Summarize results.",
            None,
        )
        .unwrap(),
    ]);
    let task_snapshot = CompactTaskSnapshot::active(
        12,
        16,
        "Read-only compatibility test",
        vec![
            CompactTaskItem::in_progress(5, "Verify compatibility."),
            CompactTaskItem::pending(6, "Summarize results.", vec![5]),
        ],
    );

    let rendered = reduce_compact_facts_with_task_snapshot(facts, Some(&task_snapshot))
        .unwrap()
        .render();

    assert!(!rendered.contains("Old inferred objective"));
    assert!(!rendered.contains("Old inferred next action"));
    assert_eq!(rendered.matches("Pending task 6").count(), 1);
    assert_eq!(rendered.matches("- Next action:").count(), 1);
    assert!(rendered.contains("- Next action: Verify compatibility."));
}
