//! Tasks command — manage task lists.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::core::command::{Command, CommandCategory, CommandContext, CommandDescriptor, CommandResult};

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "tasks".to_string(),
            "Manage tasks".to_string(),
            CommandCategory::Tasks,
            tasks_execute,
        )
        .with_usage(vec![
            "/tasks - List all tasks".to_string(),
            "/tasks active - Show active tasks".to_string(),
            "/tasks completed - Show completed tasks".to_string(),
        ])
    })
}

fn tasks_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    match args.trim().to_lowercase().as_str() {
        "" | "all" => CommandResult::Success(
            "Task Management:\n\nUse the following tools to manage tasks:\n  - TaskCreate: Create a new task\n  - TaskList: List all tasks\n  - TaskGet: Get task details\n  - TaskUpdate: Update task status\n  - TaskStop: Stop/delete a task\n  - TodoWrite: Create a todo list\n\nExample: Use 'TaskList' tool to see all tasks".to_string()
        ),
        "active" => CommandResult::Success("Use 'TaskList' tool with status='in_progress' filter".to_string()),
        "completed" => CommandResult::Success("Use 'TaskList' tool with status='completed' filter".to_string()),
        _ => CommandResult::Error(format!("Unknown argument: {}", args.trim())),
    }
}
