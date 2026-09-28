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
