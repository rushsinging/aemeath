use crate::command::{Command, CommandCategory, CommandContext, CommandResult};

/// Tasks command - manage tasks
pub fn tasks_command() -> Command {
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
}

fn tasks_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim().to_lowercase();
    match arg.as_str() {
        "" | "all" => {
            CommandResult::Success(
                "Task Management:\n\nUse the following tools to manage tasks:\n  - TaskCreate: Create a new task\n  - TaskList: List all tasks\n  - TaskGet: Get task details\n  - TaskUpdate: Update task status\n  - TaskStop: Stop/delete a task\n  - TodoWrite: Create a todo list\n\nExample: Use 'TaskList' tool to see all tasks".to_string()
            )
        }
        "active" => {
            CommandResult::Success("Use 'TaskList' tool with status='in_progress' filter".to_string())
        }
        "completed" => {
            CommandResult::Success("Use 'TaskList' tool with status='completed' filter".to_string())
        }
        _ => CommandResult::Error(format!("Unknown argument: {}", arg)),
    }
}
