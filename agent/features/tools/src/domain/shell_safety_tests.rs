use super::*;

#[test]
fn blocks_bash_file_read_commands_that_should_use_dedicated_tools() {
    for command in [
        "cat agent/features/runtime/src/lib.rs",
        "head agent/features/runtime/src/lib.rs",
        "tail -n 20 agent/features/runtime/src/lib.rs",
        "sed -n '1,20p' agent/features/runtime/src/lib.rs",
    ] {
        let reason = check_command_safety(command)
            .expect("file read command should be blocked by bash safety");
        assert!(reason.contains("dedicated file tools"));
    }
}

#[test]
fn allows_non_file_read_safe_commands() {
    assert_eq!(check_command_safety("cargo test -p runtime"), None);
    assert_eq!(check_command_safety("git status --short"), None);
}
