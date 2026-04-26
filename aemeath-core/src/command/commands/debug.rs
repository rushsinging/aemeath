//! Doctor command — run system diagnostics.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::command::{Command, CommandCategory, CommandContext, CommandResult, CommandDescriptor};

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "doctor".to_string(),
            "Run system diagnostics".to_string(),
            CommandCategory::Debug,
            doctor_execute,
        )
        .with_usage(vec!["/doctor - Run diagnostics".to_string()])
    })
}

fn doctor_execute(_args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let mut output = String::from("System Diagnostics:\n\n");
    let api_key_set = std::env::var("ANTHROPIC_API_KEY").is_ok();
    output.push_str(&format!("API Key: {}\n", if api_key_set { "✓ set" } else { "✗ not set" }));
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let config_path = home.join(".config").join("aemeath").join("config.json");
    output.push_str(&format!("Config file: {}\n", if config_path.exists() { "✓ exists" } else { "✗ not found" }));
    let sessions_path = home.join(".aemeath").join("sessions");
    output.push_str(&format!("Sessions dir: {}\n", if sessions_path.exists() { "✓ exists" } else { "✗ not found" }));
    output.push_str(&format!("Working dir: {}\n", std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| "✗ error".to_string())));
    let is_git = std::path::Path::new(".git").exists();
    output.push_str(&format!("Git repo: {}\n", if is_git { "✓ yes" } else { "✗ no" }));
    output.push_str(&format!("Version: {}\n", env!("CARGO_PKG_VERSION")));
    output.push_str("\nSystem OK\n");
    CommandResult::Success(output)
}
