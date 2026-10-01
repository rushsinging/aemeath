use super::*;

#[test]
fn test_input_mode_uses_repl_for_terminal_stdin() {
    assert_eq!(input_mode(true), InputMode::Repl);
}

#[test]
fn test_input_mode_uses_pipe_once_for_non_terminal_stdin() {
    assert_eq!(input_mode(false), InputMode::PipeOnce);
}

#[test]
fn no_tui_uses_injected_router_for_exit_alias_and_reflect_arguments() {
    let wiring = composition::tools::wire_commands().expect("command wiring");
    assert!(matches!(
        resolve_slash_for_delivery(wiring.router().as_ref(), "  /quit  "),
        Ok(sdk::CommandRoute::ApplicationControl { command, .. })
            if command.command.as_str() == "exit"
    ));
    assert!(matches!(
        resolve_slash_for_delivery(wiring.router().as_ref(), "/reflect 3"),
        Ok(sdk::CommandRoute::SnapshotQuery { command, .. })
            if command.arguments.as_slice() == ["3"]
    ));
    assert!(resolve_slash_for_delivery(wiring.router().as_ref(), "/reflect 0").is_err());
}

#[test]
fn test_truncate_tool_output_keeps_short_output() {
    assert_eq!(truncate_tool_output("short"), "short");
}

#[test]
fn test_truncate_tool_output_marks_long_output() {
    let output = "x".repeat(MAX_TOOL_OUTPUT_CHARS + 1);

    assert!(truncate_tool_output(&output).ends_with("... (truncated)"));
}
