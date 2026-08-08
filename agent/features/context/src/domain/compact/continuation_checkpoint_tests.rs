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
fn rejects_sections_in_wrong_order() {
    let source = COMPLETE_CHECKPOINT
        .replace("## Current Objective", "## TEMP")
        .replace("## Committed Facts", "## Current Objective")
        .replace("## TEMP", "## Committed Facts");

    let error = ContinuationCheckpoint::parse(&source).expect_err("wrong order must fail");

    assert!(matches!(error, CheckpointError::InvalidSectionOrder { .. }));
    assert!(error.to_string().contains("分区顺序"));
}
