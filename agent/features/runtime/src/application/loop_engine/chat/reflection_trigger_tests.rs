use provider::ProviderStopReason;

use super::reflection::should_run_turn_reflection;

fn enabled_config(interval_turns: usize) -> share::config::MemoryConfig {
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_turns = interval_turns;
    config
}

#[test]
fn turn_reflection_requires_enabled_interval_finish_boundary() {
    let config = enabled_config(2);

    assert!(should_run_turn_reflection(
        &config,
        2,
        false,
        &ProviderStopReason::EndTurn,
        false,
    ));
    assert!(!should_run_turn_reflection(
        &config,
        1,
        false,
        &ProviderStopReason::EndTurn,
        false,
    ));
    assert!(!should_run_turn_reflection(
        &config,
        2,
        false,
        &ProviderStopReason::EndTurn,
        true,
    ));

    let mut memory_disabled = config.clone();
    memory_disabled.enabled = false;
    assert!(!should_run_turn_reflection(
        &memory_disabled,
        2,
        false,
        &ProviderStopReason::EndTurn,
        false,
    ));

    let mut reflection_disabled = config.clone();
    reflection_disabled.reflection.enabled = false;
    assert!(!should_run_turn_reflection(
        &reflection_disabled,
        2,
        false,
        &ProviderStopReason::EndTurn,
        false,
    ));

    let zero_interval = enabled_config(0);
    assert!(!should_run_turn_reflection(
        &zero_interval,
        2,
        false,
        &ProviderStopReason::EndTurn,
        false,
    ));
}

#[test]
fn turn_reflection_skips_unfinished_tool_round_but_accepts_completed_end_turn() {
    let config = enabled_config(2);

    assert!(!should_run_turn_reflection(
        &config,
        2,
        true,
        &ProviderStopReason::ToolUse,
        false,
    ));
    assert!(should_run_turn_reflection(
        &config,
        2,
        true,
        &ProviderStopReason::EndTurn,
        false,
    ));
}
