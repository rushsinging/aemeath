//! Git commands: init.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::core::command::{
    Command, CommandCategory, CommandContext, CommandDescriptor, CommandResult,
};

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "init".to_string(),
            "Initialize project with aemeath".to_string(),
            CommandCategory::Git,
            init_execute,
        )
        .with_usage(vec![
            "/init - Initialize current directory".to_string(),
            "/init force - Force re-initialization".to_string(),
        ])
    })
}

fn init_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let force = args.trim().to_lowercase() == "force";
    let aemeath_dir = std::path::Path::new(".aemeath");
    if aemeath_dir.exists() && !force {
        return CommandResult::Error(
            "Already initialized. Use /init force to re-initialize".to_string(),
        );
    }
    let mut output = String::from("Initializing project...\n\n");
    if std::fs::create_dir_all(".aemeath").is_ok() {
        output.push_str("✓ Created .aemeath directory\n");
    } else {
        output.push_str("✗ Failed to create .aemeath directory\n");
    }
    let claude_md = std::path::Path::new("CLAUDE.md");
    if !claude_md.exists() {
        if std::fs::write(
            claude_md,
            "# Project Context\n\nThis file provides context for aemeath.\n",
        )
        .is_ok()
        {
            output.push_str("✓ Created CLAUDE.md\n");
        }
    } else {
        output.push_str("✓ CLAUDE.md already exists\n");
    }
    output.push_str("\nProject initialized!\n");
    CommandResult::Success(output)
}
