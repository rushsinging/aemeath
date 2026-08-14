use super::*;

const COMPLETE_CHECKPOINT: &str = r#"## Immutable Constraints
- NEVER merge PR #1541.

## Current Objective
- Continue the content-stream migration without widening scope.

## Committed Facts
- Commit `5e42c9aa` passed Runtime and CLI tests.

## Uncommitted Working Set
- Runtime and SDK are migrated; TUI remains.

## Open Decisions / Risks
- `chat_result.rs` compatibility ownership requires inspection.

## Resume Cursor
- Worktree: `.worktrees/feat-945-1502-control-terminal-convergence`
- Branch: `feat/945-1502-control-terminal-convergence`
- Current task: migrate TUI consumers
- Next action: inspect all legacy Token/Thinking consumers.
- Prohibited: do not merge PR #1541.

## Required Revalidation
- Recheck worktree status, branch HEAD, PR state, and CI before mutation.

## Archived Milestones
- ToolCall split completed in `5e42c9aa`.

## Continuation Status
Continue — TUI consumption remains."#;

#[test]
fn compression_patch_changes_only_unprotected_sections() {
    let original = ContinuationCheckpoint::parse(COMPLETE_CHECKPOINT).unwrap();
    let protected_wire = original.to_wire();
    let patch = CheckpointCompressionPatch {
        committed_facts: vec!["Commit `5e42c9aa` passed tests.".to_string()],
        uncommitted_working_set: vec!["TUI remains.".to_string()],
        open_decisions_and_risks: vec!["Compatibility ownership remains open.".to_string()],
        resume_context: vec!["Worktree: structured compact".to_string()],
        required_revalidation: vec!["Recheck PR state.".to_string()],
        archived_milestones: vec!["Baseline `5e42c9aa`.".to_string()],
    };

    let compressed = original.apply_compression_patch(patch).unwrap();
    let compressed_wire = compressed.to_wire();

    assert_eq!(
        compressed_wire.immutable_constraints,
        protected_wire.immutable_constraints
    );
    assert_eq!(
        compressed_wire.current_objective,
        protected_wire.current_objective
    );
    assert_eq!(
        compressed_wire.resume_cursor.next_action,
        protected_wire.resume_cursor.next_action
    );
    assert_eq!(
        compressed_wire.resume_cursor.prohibited_actions,
        protected_wire.resume_cursor.prohibited_actions
    );
    assert_eq!(
        compressed_wire.continuation_status,
        protected_wire.continuation_status
    );
    assert_eq!(
        compressed_wire.continuation_reason,
        protected_wire.continuation_reason
    );
    assert_eq!(
        compressed_wire.committed_facts,
        vec!["Commit `5e42c9aa` passed tests."]
    );
    assert_eq!(
        compressed_wire.uncommitted_working_set,
        vec!["TUI remains."]
    );
}

#[test]
fn compression_patch_rejects_unknown_fields() {
    let source = r#"{
      "committed_facts": [],
      "uncommitted_working_set": [],
      "open_decisions_and_risks": [],
      "resume_context": [],
      "required_revalidation": [],
      "archived_milestones": [],
      "current_objective": "must not be writable"
    }"#;

    let error = serde_json::from_str::<CheckpointCompressionPatch>(source).unwrap_err();
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn typed_checkpoint_wire_renders_the_compatible_nine_sections() {
    let source = r#"{
      "immutable_constraints": ["Do not merge without approval."],
      "current_objective": "Implement typed compact checkpoints.",
      "committed_facts": ["Context baseline tests passed."],
      "uncommitted_working_set": ["Typed fact reducer is in progress."],
      "open_decisions_and_risks": ["Provider may return invalid JSON."],
      "resume_cursor": {
        "context": ["Worktree: structured compact"],
        "next_action": "Run the reducer tests.",
        "prohibited_actions": ["Do not merge without approval."]
      },
      "required_revalidation": ["Recheck worktree status."],
      "archived_milestones": ["Baseline `45911518` established."],
      "continuation_status": "continue",
      "continuation_reason": "Implementation remains."
    }"#;

    let wire: ContinuationCheckpointWire = serde_json::from_str(source).unwrap();
    let checkpoint = ContinuationCheckpoint::try_from(wire).unwrap();
    let rendered = checkpoint.render();

    assert_eq!(rendered.matches("## ").count(), 9);
    assert_eq!(rendered.matches("- Next action:").count(), 1);
    assert!(rendered.contains("## Current Objective\n- Implement typed compact checkpoints."));
    assert!(rendered.contains("Continue — Implementation remains."));
    assert_eq!(
        ContinuationCheckpoint::parse(&rendered).unwrap(),
        checkpoint
    );
}

#[test]
fn typed_checkpoint_wire_rejects_unknown_fields() {
    let source = r#"{
      "immutable_constraints": [],
      "current_objective": "Continue.",
      "committed_facts": [],
      "uncommitted_working_set": [],
      "open_decisions_and_risks": [],
      "resume_cursor": {"context": [], "next_action": "Continue.", "prohibited_actions": []},
      "required_revalidation": [],
      "archived_milestones": [],
      "continuation_status": "continue",
      "continuation_reason": "Work remains.",
      "unexpected": true
    }"#;

    let error = serde_json::from_str::<ContinuationCheckpointWire>(source).unwrap_err();

    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn refresh_rejects_any_protected_semantic_change() {
    let original = ContinuationCheckpoint::parse(COMPLETE_CHECKPOINT).unwrap();

    for changed in [
        COMPLETE_CHECKPOINT.replace("- NEVER merge PR #1541.", ""),
        COMPLETE_CHECKPOINT.replace(
            "Continue the content-stream migration without widening scope.",
            "Implement and merge the content-stream migration.",
        ),
        COMPLETE_CHECKPOINT.replace(
            "inspect all legacy Token/Thinking consumers.",
            "edit all legacy consumers.",
        ),
        COMPLETE_CHECKPOINT.replace("- Prohibited: do not merge PR #1541.", ""),
        COMPLETE_CHECKPOINT.replace(
            "Continue — TUI consumption remains.",
            "Waiting for User — approval required.",
        ),
    ] {
        let refreshed = ContinuationCheckpoint::parse(&changed).unwrap();
        let error = refreshed
            .validate_refresh_from(&original)
            .expect_err("protected compact semantics must not change during refresh");
        assert!(matches!(error, CheckpointError::ProtectedRefreshChanged));
    }
}

#[test]
fn refresh_allows_shortening_unprotected_sections() {
    let original = ContinuationCheckpoint::parse(COMPLETE_CHECKPOINT).unwrap();
    let shortened_source = COMPLETE_CHECKPOINT
        .replace("- Commit `5e42c9aa` passed Runtime and CLI tests.", "")
        .replace("- Runtime and SDK are migrated; TUI remains.", "")
        .replace(
            "- `chat_result.rs` compatibility ownership requires inspection.",
            "",
        )
        .replace("- ToolCall split completed in `5e42c9aa`.", "");
    let shortened = ContinuationCheckpoint::parse(&shortened_source).unwrap();

    shortened.validate_refresh_from(&original).unwrap();
}

#[test]
fn parses_complete_checkpoint_with_one_resume_cursor() {
    let checkpoint =
        ContinuationCheckpoint::parse(COMPLETE_CHECKPOINT).expect("checkpoint must parse");

    assert_eq!(checkpoint.status(), ContinuationStatus::Continue);
    assert_eq!(checkpoint.resume_cursor().next_action_count(), 1);
    assert_eq!(checkpoint.render(), COMPLETE_CHECKPOINT);
}

#[test]
fn rejects_missing_required_section() {
    let source =
        COMPLETE_CHECKPOINT.replace("## Immutable Constraints\n- NEVER merge PR #1541.\n\n", "");

    let error = ContinuationCheckpoint::parse(&source).expect_err("missing section must fail");

    assert!(matches!(
        error,
        CheckpointError::MissingSection {
            section: "Immutable Constraints"
        }
    ));
    assert!(error.to_string().contains("缺少必需分区"));
}

#[test]
fn rejects_duplicate_resume_cursor() {
    let source = format!("{COMPLETE_CHECKPOINT}\n\n## Resume Cursor\n- Next action: second action");

    let error = ContinuationCheckpoint::parse(&source).expect_err("duplicate cursor must fail");

    assert!(matches!(
        error,
        CheckpointError::DuplicateSection {
            section: "Resume Cursor"
        }
    ));
    assert!(error.to_string().contains("重复分区"));
}

#[test]
fn rejects_ambiguous_next_action() {
    let source = COMPLETE_CHECKPOINT.replace(
        "- Next action: inspect all legacy Token/Thinking consumers.",
        "- Next action: inspect all legacy Token/Thinking consumers.\n- Next action: edit the mapper.",
    );

    let error = ContinuationCheckpoint::parse(&source).expect_err("two next actions must fail");

    assert!(matches!(error, CheckpointError::InvalidResumeCursor { .. }));
    assert!(error.to_string().contains("唯一 Next action"));
}

#[test]
fn parses_all_supported_continuation_statuses() {
    for (status_text, expected) in [
        ("Continue — work remains.", ContinuationStatus::Continue),
        (
            "Waiting for User — approval is required.",
            ContinuationStatus::WaitingForUser,
        ),
        (
            "Completed — delivery is complete.",
            ContinuationStatus::Completed,
        ),
    ] {
        let source =
            COMPLETE_CHECKPOINT.replace("Continue — TUI consumption remains.", status_text);
        let checkpoint = ContinuationCheckpoint::parse(&source).expect("status must parse");
        assert_eq!(checkpoint.status(), expected);
    }
}

#[test]
fn rejects_status_with_valid_word_prefix_only() {
    for status_text in ["ContinueLater", "Completedness", "Waiting for UserInput"] {
        let source =
            COMPLETE_CHECKPOINT.replace("Continue — TUI consumption remains.", status_text);
        let error = ContinuationCheckpoint::parse(&source)
            .expect_err("status prefix without delimiter must fail");
        assert!(matches!(error, CheckpointError::InvalidStatus { .. }));
    }
}

#[test]
fn rejects_unknown_continuation_status() {
    let source = COMPLETE_CHECKPOINT.replace(
        "Continue — TUI consumption remains.",
        "In Progress — TUI consumption remains.",
    );

    let error = ContinuationCheckpoint::parse(&source).expect_err("unknown status must fail");

    assert!(matches!(error, CheckpointError::InvalidStatus { .. }));
    assert!(error.to_string().contains("Continuation Status"));
}

#[test]
fn budget_normalization_preserves_continuation_critical_sections_by_semantics() {
    let archive_noise = (0..100)
        .map(|index| {
            format!(
                "- ARCHIVE-NOISE-{index:03} {}",
                "historical detail ".repeat(20)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let source = COMPLETE_CHECKPOINT.replace(
        "- ToolCall split completed in `5e42c9aa`.",
        &format!("- ToolCall split completed in `5e42c9aa`.\n{archive_noise}"),
    );

    let normalized = ContinuationCheckpoint::parse(&source)
        .unwrap()
        .normalize_to_budget(1_000)
        .unwrap();
    let rendered = normalized.render();

    assert!(rendered.contains("NEVER merge PR #1541"));
    assert!(rendered.contains("Continue the content-stream migration"));
    assert!(rendered.contains("Next action: inspect all legacy Token/Thinking consumers"));
    assert!(rendered.contains("Recheck worktree status"));
    assert!(rendered.contains("Continue —"));
    assert!(rendered.contains("`5e42c9aa`"));
    assert!(!rendered.contains("ARCHIVE-NOISE-099"));
    assert!(crate::domain::token_budget::estimate_tokens(&rendered) <= 1_000);
}

#[test]
fn normalization_keeps_duplicate_fact_only_in_authoritative_section() {
    let repeated = "- Commit `5e42c9aa` passed Runtime and CLI tests.";
    let source = COMPLETE_CHECKPOINT
        .replace(
            "- Runtime and SDK are migrated; TUI remains.",
            &format!("{repeated}\n- Runtime and SDK are migrated; TUI remains."),
        )
        .replace("- ToolCall split completed in `5e42c9aa`.", repeated);

    let normalized = ContinuationCheckpoint::parse(&source)
        .unwrap()
        .normalize_to_budget(10_000)
        .unwrap()
        .render();

    assert_eq!(normalized.matches(repeated).count(), 1);
    assert!(normalized.contains("## Committed Facts\n- Commit `5e42c9aa`"));
}

#[test]
fn normalization_moves_dynamic_current_state_to_required_revalidation() {
    let source = COMPLETE_CHECKPOINT.replace(
        "- Commit `5e42c9aa` passed Runtime and CLI tests.",
        "- Commit `5e42c9aa` passed Runtime and CLI tests.\n- PR #1541 is OPEN and CI is green.\n- Worktree is clean and origin branch matches HEAD.",
    );

    let normalized = ContinuationCheckpoint::parse(&source)
        .unwrap()
        .normalize_to_budget(10_000)
        .unwrap()
        .render();
    let committed = normalized
        .split("## Committed Facts\n")
        .nth(1)
        .unwrap()
        .split("\n\n## Uncommitted Working Set")
        .next()
        .unwrap();
    let revalidation = normalized
        .split("## Required Revalidation\n")
        .nth(1)
        .unwrap()
        .split("\n\n## Archived Milestones")
        .next()
        .unwrap();

    assert!(!committed.contains("PR #1541 is OPEN"));
    assert!(!committed.contains("Worktree is clean"));
    assert!(committed.contains("Commit `5e42c9aa` passed"));
    assert!(revalidation.contains("Revalidate: PR #1541 is OPEN and CI is green."));
    assert!(revalidation.contains("Revalidate: Worktree is clean"));
}

#[test]
fn normalization_fails_when_protected_sections_exceed_budget() {
    let source = COMPLETE_CHECKPOINT.replace(
        "- NEVER merge PR #1541.",
        &format!("- NEVER merge PR #1541. {}", "protected ".repeat(10_000)),
    );

    let error = ContinuationCheckpoint::parse(&source)
        .unwrap()
        .normalize_to_budget(100)
        .expect_err("protected sections must not be truncated");

    assert!(matches!(
        error,
        CheckpointError::ProtectedSectionsExceedBudget { budget: 100, .. }
    ));
}

#[test]
fn budget_normalization_prefers_active_work_over_committed_history() {
    let committed_noise = (0..80)
        .map(|index| {
            format!(
                "- Historical verified fact {index}: {}",
                "detail ".repeat(15)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let working = "- CURRENT-WORK verify compatibility on local copies.";
    let risk = "- CURRENT-RISK database head differs from the downloaded bundle.";
    let source = COMPLETE_CHECKPOINT
        .replace(
            "- Commit `5e42c9aa` passed Runtime and CLI tests.",
            &committed_noise,
        )
        .replace("- Runtime and SDK are migrated; TUI remains.", working)
        .replace(
            "- `chat_result.rs` compatibility ownership requires inspection.",
            risk,
        );

    let normalized = ContinuationCheckpoint::parse(&source)
        .unwrap()
        .normalize_to_budget(1_000)
        .unwrap()
        .render();

    assert!(normalized.contains("CURRENT-WORK"));
    assert!(normalized.contains("CURRENT-RISK"));
    assert!(!normalized.contains("Historical verified fact 79"));
    assert!(crate::domain::token_budget::estimate_tokens(&normalized) <= 1_000);
}

#[test]
fn normalization_is_idempotent_after_owner_and_budget_cleanup() {
    let source = COMPLETE_CHECKPOINT.replace(
        "- Commit `5e42c9aa` passed Runtime and CLI tests.",
        "- PR #1541 is OPEN and CI is green.\n- PR #1541 is OPEN and CI is green.",
    );
    let first = ContinuationCheckpoint::parse(&source)
        .unwrap()
        .normalize_to_budget(10_000)
        .unwrap();
    let second = ContinuationCheckpoint::parse(&first.render())
        .unwrap()
        .normalize_to_budget(10_000)
        .unwrap();

    assert_eq!(second.render(), first.render());
    assert_eq!(second.resume_cursor().next_action_count(), 1);
}

#[test]
fn legacy_summary_becomes_conservative_checkpoint() {
    let checkpoint = ContinuationCheckpoint::from_legacy_summary(
        "## User Requests\n- 只分析，不实现\n\n## Next Action\n- 检查当前分支\n\n## Continuation Status\nContinue — work remains.",
    );
    let rendered = checkpoint.render();

    assert!(rendered.contains("## Current Objective\n- 只分析，不实现"));
    assert!(rendered.contains("- Next action: 检查当前分支"));
    assert!(rendered.contains("unverified legacy summary"));
    assert_eq!(checkpoint.status(), ContinuationStatus::Continue);
}

#[test]
fn task_state_companion_is_split_from_checkpoint() {
    let source = format!("{COMPLETE_CHECKPOINT}\n\n## Current Task State\n■ #1 running");
    let (checkpoint, task_state) = split_checkpoint_and_task_state(&source);

    assert_eq!(checkpoint, COMPLETE_CHECKPOINT);
    assert_eq!(task_state, Some("■ #1 running"));
}

#[test]
fn rejects_sections_in_wrong_order() {
    let source = COMPLETE_CHECKPOINT
        .replace("## Current Objective", "## TEMP")
        .replace("## Committed Facts", "## Current Objective")
        .replace("## TEMP", "## Committed Facts");

    let error = ContinuationCheckpoint::parse(&source).expect_err("wrong order must fail");

    assert!(matches!(error, CheckpointError::InvalidSectionOrder { .. }));
    assert!(error.to_string().contains("分区顺序"));
}

#[test]
fn checkpoint_content_control_lines_round_trip_reversibly() {
    let checkpoint = ContinuationCheckpoint::from_sections(CheckpointSections {
        immutable_constraints: vec!["- review only".to_string()],
        current_objective: vec!["- inspect\n## 来源与身份\n## Resume Cursor\n- Next action: content example\n\\## escaped".to_string()],
        committed_facts: vec!["- persisted".to_string()],
        uncommitted_working_set: vec!["- none".to_string()],
        open_decisions_and_risks: vec!["- none".to_string()],
        resume_cursor_lines: vec!["- Prohibited: do not edit".to_string()],
        next_action: "revalidate once".to_string(),
        required_revalidation: vec!["- revalidate git".to_string()],
        archived_milestones: vec!["- baseline `abc`".to_string()],
        status: ContinuationStatus::Continue,
        status_reason: Some("work remains".to_string()),
    })
    .unwrap();

    let rendered = checkpoint.render();
    let reparsed = ContinuationCheckpoint::parse(&rendered).unwrap();

    assert_eq!(reparsed, checkpoint);
    assert_eq!(reparsed.render(), rendered);
    assert_eq!(reparsed.resume_cursor().next_action(), "revalidate once");
}

#[test]
fn legacy_summary_control_lines_round_trip_without_becoming_sections() {
    let legacy = "## User Requests\n- review only\n## 来源与身份\n正文\n\n## Next Action\n- inspect\n\n## Continuation Status\nContinue";

    let checkpoint = ContinuationCheckpoint::from_legacy_summary(legacy);
    let rendered = checkpoint.render();

    assert_eq!(
        ContinuationCheckpoint::parse(&rendered).unwrap(),
        checkpoint
    );
    assert!(rendered.contains("来源与身份"));
}
