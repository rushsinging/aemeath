use super::reflection::should_run_turn_reflection;

fn enabled_config(interval_runs: usize) -> share::config::MemoryConfig {
    let mut config = share::config::MemoryConfig::default();
    config.reflection.interval_runs = interval_runs;
    config
}

#[test]
fn turn_reflection_requires_enabled_interval_finish_boundary() {
    let config = enabled_config(2);

    assert!(should_run_turn_reflection(&config, 2));
    assert!(!should_run_turn_reflection(&config, 1));

    let mut memory_disabled = config.clone();
    memory_disabled.enabled = false;
    assert!(!should_run_turn_reflection(&memory_disabled, 2));

    let mut reflection_disabled = config.clone();
    reflection_disabled.reflection.enabled = false;
    assert!(!should_run_turn_reflection(&reflection_disabled, 2));

    let zero_interval = enabled_config(0);
    assert!(!should_run_turn_reflection(&zero_interval, 2));
}
