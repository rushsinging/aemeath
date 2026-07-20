pub(crate) fn command_help_lines(catalog: &dyn sdk::CommandCatalogPort) -> Vec<String> {
    let mut lines = vec!["Commands:".to_string()];
    for command in catalog.list() {
        let aliases = if command.aliases.is_empty() {
            String::new()
        } else {
            format!(
                " (aliases: {})",
                command
                    .aliases
                    .iter()
                    .map(|alias| format!("/{}", alias.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        lines.push(format!(
            "  /{:<14} - {}{}",
            command.name.as_str(),
            command.description,
            aliases
        ));
    }
    lines.extend([
        String::new(),
        "Scrolling:".to_string(),
        "  Mouse wheel     - scroll 3 lines".to_string(),
        "  PageUp/PageDown - scroll 10 lines".to_string(),
        "  Shift+Up/Down   - scroll 1 line".to_string(),
        "  Shift+Home      - scroll to top".to_string(),
        "  Shift+End       - scroll to bottom".to_string(),
        String::new(),
        "Input:".to_string(),
        "  Enter           - send message".to_string(),
        "  Alt+Enter       - new line".to_string(),
        "  Tab             - accept suggestion".to_string(),
        "  Ctrl+C          - interrupt / exit".to_string(),
        "  Ctrl+V          - paste image from clipboard".to_string(),
    ]);
    lines
}
