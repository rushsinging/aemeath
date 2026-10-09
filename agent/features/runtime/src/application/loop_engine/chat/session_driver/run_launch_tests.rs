//! Structural guards for the session command driver.
//!
//! These assertions exist because the wrong value here is invisible at runtime:
//! the log context only reaches the formatter, so a mis-assigned field still
//! prints and only misleads whoever reads the log later.

const RUN_LAUNCH: &str = include_str!("run_launch.rs");

/// `run_step` is the schema's LLM-step counter (`specs/3.15` field 7, `null` when
/// unset). The Main Run launch wrapper used to bind the session's Run ordinal to
/// it, so the same field meant "which LLM call" inside a Run and "how many Runs
/// so far" at Run level. Only the LLM scope may set it.
#[test]
fn run_launch_never_binds_the_run_ordinal_to_the_run_step_log_field() {
    assert!(
        !RUN_LAUNCH.contains("run_step: logging::FieldPatch::Set("),
        "Run-level logs must leave run_step unset; only run_services.rs may set it"
    );
}

/// The ordinal that drives the reflection interval is a Run counter, not a
/// step counter. The name is what let the wrong log assignment look correct.
#[test]
fn the_reflection_interval_ordinal_is_named_after_runs() {
    assert!(
        !RUN_LAUNCH.contains("step_count"),
        "the session-level Run ordinal must be named run_count"
    );
}

mod resume_failure_classification {
    use super::super::run_launch::classify_resume_failure;
    use sdk::SessionResumeFailureKind;

    #[test]
    fn not_found_codes_map_to_not_found() {
        for code in [
            context::error_codes::SESSION_NOT_FOUND,
            context::error_codes::SESSION_PROJECT_MISMATCH,
        ] {
            assert_eq!(
                classify_resume_failure(Some(code)),
                SessionResumeFailureKind::NotFound
            );
        }
    }

    #[test]
    fn corrupt_codes_map_to_corrupt() {
        for code in [
            context::error_codes::SESSION_CORRUPT,
            context::error_codes::SESSION_UNSUPPORTED_VERSION,
        ] {
            assert_eq!(
                classify_resume_failure(Some(code)),
                SessionResumeFailureKind::Corrupt
            );
        }
    }

    #[test]
    fn storage_resume_unknown_and_missing_codes_fall_back_to_io() {
        for code in [
            Some(context::error_codes::SESSION_STORAGE),
            Some(context::error_codes::SESSION_RESUME),
            Some("unknown_future_code"),
            None,
        ] {
            assert_eq!(classify_resume_failure(code), SessionResumeFailureKind::Io);
        }
    }
}
