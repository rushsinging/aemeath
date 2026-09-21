use super::{
    DisplayHistoryStepIndex, SessionCommitPlan, SessionGenerationCodec, SessionGenerationManifest,
    SessionGenerationWireError, SessionStepMember,
};
use crate::domain::session::{
    AcceptedInputProjection, CanonicalSession, CommittedRunSlice, CommittedRunStep, RunStepCursor,
    CURRENT_SESSION_SCHEMA_VERSION,
};
use share::message::{HookNotice, HookNoticeKind, Message, MessageSource};

fn session_with_steps(id: &str, revision: u64, steps: &[(&str, &str, &str)]) -> CanonicalSession {
    let mut session = CanonicalSession::fixture(id);
    session.revision = revision;
    session.run_slices = steps
        .iter()
        .map(|(run_id, step_id, text)| {
            CommittedRunSlice::new(
                *run_id,
                vec![CommittedRunStep::accepted_only(
                    *step_id,
                    AcceptedInputProjection::new(
                        vec![Message::user(*text)],
                        format!("{run_id}:{step_id}:{text}"),
                        revision,
                    ),
                )],
            )
        })
        .collect();
    session
}

#[test]
fn preserving_unloaded_history_never_infers_removal_from_partial_after_state() {
    let before = session_with_steps(
        "session",
        1,
        &[
            ("run-old", "step-old", "old"),
            ("run-current", "step-current", "current"),
        ],
    );
    let persisted_manifest = SessionGenerationManifest::new(
        before.id.clone(),
        before.revision,
        vec![
            RunStepCursor {
                run_id: "run-old".to_string(),
                step_id: "step-old".to_string(),
            },
            RunStepCursor {
                run_id: "run-current".to_string(),
                step_id: "step-current".to_string(),
            },
        ],
    )
    .expect("persisted manifest");
    let mut after = before.clone();
    after.revision = 2;
    after.run_slices = after.run_slices.cleared();

    let changes =
        SessionCommitPlan::between_preserving_unloaded_steps(&before, &after, &persisted_manifest)
            .expect("preserving change set");

    assert!(changes.removed_members().is_empty());
}
#[test]
fn finalized_append_changes_manifest_state_and_only_new_step_member() {
    let before = session_with_steps("session", 1, &[("run-a", "step-a", "a")]);
    let mut after = before.clone();
    after.revision = 2;
    after.run_slices = after.run_slices.append_accepted_input(
        "run-b",
        "step-b",
        AcceptedInputProjection::new(vec![Message::user("b")], "run-b:step-b:b", 2),
    );

    let changes = SessionCommitPlan::between(&before, &after).expect("change set");

    assert_eq!(changes.changed_members().len(), 3);
    assert_eq!(
        changes
            .changed_members()
            .iter()
            .map(|member| member.name())
            .collect::<Vec<_>>(),
        [
            "manifest.json",
            "metadata.json",
            "step-72756e2d62-737465702d62.json"
        ]
    );
    assert_eq!(
        changes.reused_members(),
        ["session-state.json", "step-72756e2d61-737465702d61.json"]
    );
    assert!(changes.removed_members().is_empty());
}

#[test]
fn metadata_mutation_changes_only_metadata_state_member() {
    let before = CanonicalSession::fixture("state-split");
    let mut after = before.clone();
    after.metadata.title = Some("updated title".to_string());
    after.revision += 1;

    let changes = SessionCommitPlan::between(&before, &after).expect("change set");
    let changed_names = changes
        .changed_members()
        .iter()
        .map(|member| member.name())
        .collect::<Vec<_>>();

    assert!(changed_names.contains(&"manifest.json"));
    assert!(changed_names.contains(&"metadata.json"));
    assert!(!changed_names.contains(&"workspace.json"));
    assert!(!changed_names.contains(&"task.json"));
    assert!(!changed_names.contains(&"compact.json"));
    assert!(!changed_names.contains(&"committed-steps.json"));
    assert!(!changed_names.contains(&"skill-loads.json"));
}

#[test]
fn metadata_mutation_reuses_unchanged_step_member() {
    let before = session_with_steps("session", 1, &[("run", "step", "a")]);
    let mut after = before.clone();
    after.revision = 2;
    after.metadata.title = Some("renamed".to_string());

    let changes = SessionCommitPlan::between(&before, &after).expect("change set");

    assert_eq!(
        changes
            .changed_members()
            .iter()
            .map(|member| member.name())
            .collect::<Vec<_>>(),
        ["manifest.json", "metadata.json"]
    );
    assert_eq!(
        changes.reused_members(),
        ["session-state.json", "step-72756e-73746570.json"]
    );
    assert!(changes.removed_members().is_empty());
}

#[test]
fn receipt_update_replaces_only_affected_step_member() {
    let before = session_with_steps(
        "session",
        1,
        &[("run-a", "step-a", "a"), ("run-b", "step-b", "b")],
    );
    let mut after = before.clone();
    after.revision = 2;
    after.run_slices = after.run_slices.append_accepted_input(
        "run-b",
        "step-b",
        AcceptedInputProjection::new(vec![Message::user("changed")], "changed", 2),
    );

    let changes = SessionCommitPlan::between(&before, &after).expect("change set");

    assert_eq!(
        changes
            .changed_members()
            .iter()
            .map(|member| member.name())
            .collect::<Vec<_>>(),
        [
            "manifest.json",
            "metadata.json",
            "step-72756e2d62-737465702d62.json"
        ]
    );
    assert_eq!(
        changes.reused_members(),
        ["session-state.json", "step-72756e2d61-737465702d61.json"]
    );
}

#[test]
fn clear_removes_every_previous_step_member() {
    let before = session_with_steps(
        "session",
        1,
        &[("run-a", "step-a", "a"), ("run-b", "step-b", "b")],
    );
    let mut after = before.clone();
    after.revision = 2;
    after.run_slices = after.run_slices.cleared();

    let changes = SessionCommitPlan::between(&before, &after).expect("change set");

    assert_eq!(changes.reused_members(), ["session-state.json"]);
    assert_eq!(
        changes.removed_members(),
        [
            "step-72756e2d61-737465702d61.json",
            "step-72756e2d62-737465702d62.json"
        ]
    );
}

#[test]
fn compact_mutation_changes_state_member_without_reencoding_steps() {
    let before = session_with_steps(
        "compact-state",
        3,
        &[("run-a", "step-a", "a"), ("run-b", "step-b", "b")],
    );
    let mut after = before.clone();
    after.revision = 4;
    after.compact = Some(crate::domain::session::ActiveCompactMarker {
        summary: "summary".to_string(),
        start_at: Some(RunStepCursor {
            run_id: "run-b".to_string(),
            step_id: "step-b".to_string(),
        }),
        source_revision: 3,
    });

    let changes = SessionCommitPlan::between(&before, &after).expect("change set");
    let changed_names = changes
        .changed_members()
        .iter()
        .map(|member| member.name())
        .collect::<Vec<_>>();
    assert_eq!(
        changed_names,
        ["manifest.json", "metadata.json", "session-state.json"]
    );
    assert_eq!(
        changes.reused_members(),
        [
            "step-72756e2d61-737465702d61.json",
            "step-72756e2d62-737465702d62.json"
        ]
    );
}

#[test]
fn overlay_step_missing_from_persisted_generation_is_written_instead_of_reused() {
    let persisted = session_with_steps("session", 1, &[("run-a", "step-a", "a")]);
    let persisted_manifest = SessionGenerationManifest::new(
        persisted.id.clone(),
        persisted.revision,
        vec![RunStepCursor {
            run_id: "run-a".to_string(),
            step_id: "step-a".to_string(),
        }],
    )
    .expect("persisted manifest");
    let mut before = persisted.clone();
    before.run_slices = before.run_slices.append_accepted_input(
        "run-overlay",
        "step-overlay",
        AcceptedInputProjection::new(
            vec![Message::user("overlay")],
            "run-overlay:step-overlay:overlay",
            2,
        ),
    );
    let mut after = before.clone();
    after.revision = 2;
    after.metadata.title = Some("persist overlay".to_string());

    let changes =
        SessionCommitPlan::between_preserving_unloaded_steps(&before, &after, &persisted_manifest)
            .expect("preserving change set");
    let overlay_member_name = "step-72756e2d6f7665726c6179-737465702d6f7665726c6179.json";

    assert!(changes
        .changed_members()
        .iter()
        .any(|member| member.name() == overlay_member_name));
    assert!(!changes
        .reused_members()
        .iter()
        .any(|name| name == overlay_member_name));
    assert!(changes
        .reused_members()
        .iter()
        .any(|name| { name == "step-72756e2d61-737465702d61.json" }));
}

#[test]
fn overlay_step_missing_from_persisted_generation_is_not_removed() {
    let persisted = session_with_steps("session", 1, &[("run-a", "step-a", "a")]);
    let persisted_manifest = SessionGenerationManifest::new(
        persisted.id.clone(),
        persisted.revision,
        vec![RunStepCursor {
            run_id: "run-a".to_string(),
            step_id: "step-a".to_string(),
        }],
    )
    .expect("persisted manifest");
    let mut before = persisted.clone();
    before.run_slices = before.run_slices.append_accepted_input(
        "run-overlay",
        "step-overlay",
        AcceptedInputProjection::new(
            vec![Message::user("overlay")],
            "run-overlay:step-overlay:overlay",
            2,
        ),
    );
    let mut after = persisted.clone();
    after.revision = 2;
    after.metadata.title = Some("discard overlay".to_string());

    let changes =
        SessionCommitPlan::between_preserving_unloaded_steps(&before, &after, &persisted_manifest)
            .expect("preserving change set");

    assert!(!changes
        .removed_members()
        .iter()
        .any(|name| { name == "step-72756e2d6f7665726c6179-737465702d6f7665726c6179.json" }));
}

#[test]
fn display_history_index_excludes_steps_up_to_clear_boundary() {
    let before = session_with_steps(
        "session",
        1,
        &[
            ("run-a", "step-a", "archived"),
            ("run-b", "step-b", "active"),
        ],
    );
    let manifest = SessionGenerationManifest::new(
        before.id.clone(),
        before.revision,
        vec![
            RunStepCursor {
                run_id: "run-a".to_string(),
                step_id: "step-a".to_string(),
            },
            RunStepCursor {
                run_id: "run-b".to_string(),
                step_id: "step-b".to_string(),
            },
        ],
    )
    .expect("persisted manifest");
    let cleared_after = RunStepCursor {
        run_id: "run-a".to_string(),
        step_id: "step-a".to_string(),
    };

    let index = DisplayHistoryStepIndex::from_manifest_after_clear(&manifest, Some(&cleared_after));

    assert_eq!(
        index
            .steps()
            .iter()
            .map(|step| step.step_id.as_str())
            .collect::<Vec<_>>(),
        ["step-b"]
    );
}

#[test]
fn state_member_round_trips_clear_boundary() {
    let mut session = session_with_steps(
        "session",
        7,
        &[
            ("run-a", "step-a", "archived"),
            ("run-b", "step-b", "active"),
        ],
    );
    session.cleared_after = Some(RunStepCursor {
        run_id: "run-a".to_string(),
        step_id: "step-a".to_string(),
    });

    let state = super::SessionStateMember::from_session(&session);
    let state_bytes = SessionGenerationCodec::encode_state(&state).expect("encode state");
    let decoded = SessionGenerationCodec::decode_state(&state_bytes).expect("decode state");

    assert_eq!(decoded.cleared_after(), session.cleared_after.as_ref());
    let restored = decoded.into_session(
        super::SessionMetadataMember::from_session(&session),
        Default::default(),
    );
    assert_eq!(restored.cleared_after, session.cleared_after);
}

#[test]
fn state_member_without_clear_boundary_decodes_as_none() {
    let session = session_with_steps("session", 1, &[("run", "step", "body")]);
    let state = super::SessionStateMember::from_session(&session);
    let state_bytes = SessionGenerationCodec::encode_state(&state).expect("encode state");
    // 旧代 state member 不含 clear 边界字段，serde default 必须兼容读取。
    let raw: serde_json::Value = serde_json::from_slice(&state_bytes).expect("state json");
    let legacy_bytes = {
        let mut value = raw;
        let object = value.as_object_mut().expect("state object");
        object.remove("cleared_after");
        serde_json::to_vec(&object).expect("legacy state json")
    };

    let decoded = SessionGenerationCodec::decode_state(&legacy_bytes).expect("decode legacy");

    assert_eq!(decoded.cleared_after(), None);
}

#[test]
fn state_member_round_trips_non_step_session_state() {
    let mut session = session_with_steps("session", 7, &[("run", "step", "body")]);
    session.created_at = "2026-01-01T00:00:00Z".to_string();
    session.updated_at = "2026-01-02T00:00:00Z".to_string();
    session.metadata.title = Some("title".to_string());
    let metadata = super::SessionMetadataMember::from_session(&session);
    let bytes = SessionGenerationCodec::encode_metadata(&metadata).expect("encode metadata");
    let decoded = SessionGenerationCodec::decode_metadata(&bytes).expect("decode metadata");

    assert_eq!(decoded.session_id(), "session");
    assert_eq!(decoded.revision(), 7);
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("metadata json");
    assert_eq!(value["metadata"]["title"], "title");

    let state = super::SessionStateMember::from_session(&session);
    let state_bytes = SessionGenerationCodec::encode_state(&state).expect("encode state");
    let state_value: serde_json::Value = serde_json::from_slice(&state_bytes).expect("state json");
    assert!(state_value.get("run_slices").is_none());
}

#[test]
#[ignore = "性能基线；手动运行：cargo test -p context --release session_incremental_change_set_release_workload -- --ignored --nocapture"]
#[allow(clippy::print_stdout)]
fn session_incremental_change_set_release_workload() {
    const STEP_COUNT: usize = 10_000;
    let step_specs = (0..STEP_COUNT)
        .map(|index| {
            (
                format!("run-{index}"),
                format!("step-{index}"),
                format!("body-{index}"),
            )
        })
        .collect::<Vec<_>>();
    let borrowed_specs = step_specs
        .iter()
        .map(|(run_id, step_id, body)| (run_id.as_str(), step_id.as_str(), body.as_str()))
        .collect::<Vec<_>>();
    let before = session_with_steps("large-session", 1, &borrowed_specs);
    let mut after = before.clone();
    after.revision = 2;
    after.updated_at = "2026-01-02T00:00:00Z".to_string();

    let started = std::time::Instant::now();
    let changes = SessionCommitPlan::between(&before, &after).expect("large change set");
    let elapsed = started.elapsed();
    let changed_bytes = changes
        .changed_members()
        .iter()
        .map(|member| member.bytes().len())
        .sum::<usize>();
    assert_eq!(changes.reused_members().len(), STEP_COUNT + 1);
    assert_eq!(changes.changed_members().len(), 2);
    assert!(changed_bytes < 2_000_000);
    println!(
        "steps={STEP_COUNT} reused={} changed={} changed_bytes={} elapsed_ms={:.3}",
        changes.reused_members().len(),
        changes.changed_members().len(),
        changed_bytes,
        elapsed.as_secs_f64() * 1_000.0
    );
}

#[test]
fn generation_manifest_builds_body_free_display_history_index() {
    let manifest = SessionGenerationManifest::new(
        "session",
        7,
        vec![
            RunStepCursor {
                run_id: "run-before".to_string(),
                step_id: "step-before".to_string(),
            },
            RunStepCursor {
                run_id: "run-active".to_string(),
                step_id: "step-active".to_string(),
            },
        ],
    )
    .expect("manifest");

    let session = session_with_steps(
        "session",
        7,
        &[
            ("run-before", "step-before", "before body"),
            ("run-active", "step-active", "active body"),
        ],
    );
    let index = DisplayHistoryStepIndex::from_session_and_manifest(&session, &manifest);

    assert_eq!(index.session_id(), "session");
    assert_eq!(index.generation_revision(), 7);
    assert_eq!(index.steps().len(), 2);
    assert_eq!(index.steps()[0].run_id(), "run-before");
    assert_eq!(index.steps()[0].step_id(), "step-before");
    assert_eq!(
        index.steps()[0].member_name(),
        "step-72756e2d6265666f7265-737465702d6265666f7265.json"
    );
    assert_eq!(index.steps()[1].run_id(), "run-active");
    assert_eq!(index.steps()[1].step_id(), "step-active");
    assert_eq!(index.steps()[0].estimated_lines(), 1);
    assert_eq!(index.steps()[0].finalize_cause(), None);
    assert_eq!(index.steps()[0].duration_ms(), None);

    let encoded = serde_json::to_value(&index).expect("history index json");
    assert!(encoded.get("messages").is_none());
    assert!(encoded.get("message_segments").is_none());
    assert!(encoded.get("tool_receipts").is_none());
    assert!(encoded.get("accepted_input").is_none());
}

#[test]
fn generation_manifest_round_trips_ordered_step_references() {
    let manifest = SessionGenerationManifest::new(
        "session",
        7,
        vec![
            RunStepCursor {
                run_id: "run-b".to_string(),
                step_id: "step-2".to_string(),
            },
            RunStepCursor {
                run_id: "run-a".to_string(),
                step_id: "step-1".to_string(),
            },
        ],
    )
    .expect("manifest");

    let bytes = SessionGenerationCodec::encode_manifest(&manifest).expect("encode manifest");
    let decoded = SessionGenerationCodec::decode_manifest(&bytes).expect("decode manifest");

    assert_eq!(decoded, manifest);
    assert_eq!(
        decoded.session_schema_version(),
        CURRENT_SESSION_SCHEMA_VERSION
    );
    assert_eq!(decoded.steps()[0].cursor().run_id, "run-b");
    assert_eq!(decoded.steps()[1].cursor().run_id, "run-a");
}

#[test]
fn generation_manifest_rejects_duplicate_step_identity() {
    let cursor = RunStepCursor {
        run_id: "run".to_string(),
        step_id: "step".to_string(),
    };

    let error = SessionGenerationManifest::new("session", 1, vec![cursor.clone(), cursor])
        .expect_err("duplicate identity must fail");

    assert!(matches!(
        error,
        SessionGenerationWireError::DuplicateStepIdentity { run_id, step_id }
            if run_id == "run" && step_id == "step"
    ));
}

#[test]
fn step_member_name_is_stable_and_safe_for_legacy_identity_characters() {
    let cursor = RunStepCursor {
        run_id: "legacy:run/one".to_string(),
        step_id: "synthetic:step one".to_string(),
    };

    let member_name = SessionGenerationManifest::step_member_name(&cursor);

    assert_eq!(
        member_name,
        "step-6c65676163793a72756e2f6f6e65-73796e7468657469633a73746570206f6e65.json"
    );
    assert!(!member_name.contains('/'));
    assert!(!member_name.contains(' '));
}

#[test]
fn step_member_round_trips_without_storage_types() {
    let member = SessionStepMember::new(
        RunStepCursor {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
        },
        CommittedRunStep {
            step_id: "step".to_string(),
            accepted_input: None,
            outcome: None,
            tool_receipts: Vec::new(),
        },
    )
    .expect("step member");

    let bytes = SessionGenerationCodec::encode_step(&member).expect("encode step");
    let decoded = SessionGenerationCodec::decode_step(&bytes).expect("decode step");

    assert_eq!(decoded.cursor(), member.cursor());
    assert_eq!(
        serde_json::to_value(decoded.step()).expect("decoded step value"),
        serde_json::to_value(member.step()).expect("member step value")
    );
}

#[test]
fn step_member_round_trips_hook_notice_metadata() {
    let notice = HookNotice {
        point: "Stop".to_string(),
        kind: HookNoticeKind::Blocked,
        summary: "Stop hook prevented stopping.".to_string(),
        command: "check-agent-stop.sh".to_string(),
        exit_code: Some(1),
        reason: "exit code 1".to_string(),
        stdout_preview: "stdout".to_string(),
        stderr_preview: "stderr".to_string(),
        stdout_truncated: true,
        stderr_truncated: false,
        output_file: Some("/tmp/stop-hook.txt".to_string()),
    };
    let member = SessionStepMember::new(
        RunStepCursor {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
        },
        CommittedRunStep::accepted_only(
            "step",
            AcceptedInputProjection::new(
                vec![Message::hook_notice("model feedback", notice.clone())],
                "hook-notice-fingerprint",
                1,
            ),
        ),
    )
    .expect("step member");

    let bytes = SessionGenerationCodec::encode_step(&member).expect("encode step");
    let decoded = SessionGenerationCodec::decode_step(&bytes).expect("decode step");
    let message = decoded
        .step()
        .accepted_input
        .as_ref()
        .expect("accepted input")
        .messages
        .first()
        .expect("Hook notice message");

    assert_eq!(message.source(), MessageSource::Hook);
    assert_eq!(
        message
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.hook_notice.as_ref()),
        Some(&notice)
    );
}

#[test]
fn future_generation_manifest_preserves_original_bytes() {
    let bytes = br#"{"generation_schema_version":2,"unknown":{"keep":true}}"#.to_vec();

    let error = SessionGenerationCodec::decode_manifest(&bytes)
        .expect_err("future generation manifest must fail closed");

    assert!(matches!(
        error,
        SessionGenerationWireError::UnsupportedFutureVersion {
            version: 2,
            original_bytes
        } if original_bytes == bytes
    ));
}

#[test]
fn complete_snapshot_covers_every_member_with_session_revision() {
    let session = session_with_steps(
        "session",
        26,
        &[("run-a", "step-a", "a"), ("run-b", "step-b", "b")],
    );
    let plan = SessionCommitPlan::complete_snapshot(&session)
        .expect("complete snapshot plan for wiped dataset rebuild");
    plan.validate_rebuild_boundary("session", 25)
        .expect("rebuild boundary must accept target = expected + 1");

    let manifest_member = plan
        .changed_members()
        .iter()
        .find(|member| member.name() == "manifest.json")
        .expect("complete snapshot must carry the generation manifest");
    let manifest = SessionGenerationCodec::decode_manifest(manifest_member.bytes())
        .expect("decodable generation manifest");
    assert_eq!(
        manifest.revision(),
        26,
        "rebuilt manifest must align with in-memory revision"
    );
    assert_eq!(manifest.session_id(), "session");

    let changed_names = plan
        .changed_members()
        .iter()
        .map(|member| member.name().to_string())
        .collect::<Vec<_>>();
    assert!(
        changed_names.iter().any(|name| name.starts_with("step-")),
        "complete snapshot must carry step members: {changed_names:?}"
    );
    assert!(
        changed_names
            .iter()
            .any(|name| name.contains("session-state")),
        "complete snapshot must carry session state: {changed_names:?}"
    );
    assert!(
        plan.reused_members().is_empty(),
        "complete snapshot must not reuse members from a wiped dataset"
    );
}

#[test]
fn rebuild_boundary_rejects_non_incremental_or_mismatched_revisions() {
    let session = session_with_steps("session", 26, &[("run-a", "step-a", "a")]);
    let plan = SessionCommitPlan::complete_snapshot(&session)
        .expect("complete snapshot plan for wiped dataset rebuild");

    let stale = plan
        .validate_rebuild_boundary("session", 26)
        .expect_err("expected revision equal to target must be rejected");
    assert!(matches!(
        stale,
        SessionGenerationWireError::InvalidManifest(message)
            if message.contains("递增一次")
    ));

    let mismatch = plan
        .validate_rebuild_boundary("other-session", 25)
        .expect_err("session identity mismatch must be rejected");
    assert!(matches!(
        mismatch,
        SessionGenerationWireError::SessionIdentityMismatch { .. }
    ));
}
