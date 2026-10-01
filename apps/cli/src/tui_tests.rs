#[test]
fn test_log_target_uses_cli_prefix() {
    assert_eq!(crate::LOG_TARGET, "aemeath:tui");
}
