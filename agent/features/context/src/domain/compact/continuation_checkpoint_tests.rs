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
fn rejects_sections_in_wrong_order() {
    let source = COMPLETE_CHECKPOINT
        .replace("## Current Objective", "## TEMP")
        .replace("## Committed Facts", "## Current Objective")
        .replace("## TEMP", "## Committed Facts");

    let error = ContinuationCheckpoint::parse(&source).expect_err("wrong order must fail");

    assert!(matches!(error, CheckpointError::InvalidSectionOrder { .. }));
    assert!(error.to_string().contains("分区顺序"));
}
