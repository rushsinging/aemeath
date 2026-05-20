use std::io;
use std::path::Path;

pub(crate) fn ask_permission(tool_name: &str) -> bool {
    use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
    use crossterm::ExecutableCommand;
    use std::io::Write;

    let mut stdout = io::stdout();
    let _ = stdout.execute(SetForegroundColor(Color::Yellow));
    let _ = stdout.execute(Print(format!("  Allow {tool_name}? [Y/n] ")));
    let _ = stdout.execute(ResetColor);
    let _ = stdout.flush();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    let answer = input.trim().to_lowercase();
    answer.is_empty() || answer == "y" || answer == "yes"
}

pub(crate) fn format_tool_summary(name: &str, input: &serde_json::Value) -> String {
    match name {
        "TodoRun" => "execute all pending todos".to_string(),
        "TodoWrite" => {
            if let Some(todos) = input.get("todos").and_then(|t| t.as_array()) {
                let count = todos.len();
                let first = todos
                    .first()
                    .and_then(|t| t.get("subject").and_then(|s| s.as_str()))
                    .unwrap_or("?");
                if count == 1 {
                    format!("{} todo: {}", count, first)
                } else if count <= 3 {
                    let subjects: Vec<&str> = todos
                        .iter()
                        .filter_map(|t| t.get("subject").and_then(|s| s.as_str()))
                        .collect();
                    format!("{} todos: {}", count, subjects.join(", "))
                } else {
                    format!("{} todos: {}, ... +{} more", count, first, count - 1)
                }
            } else {
                input.to_string()
            }
        }
        _ => input.to_string(),
    }
}

pub(crate) async fn handle_commit(cwd: &Path) {
    use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
    use crossterm::ExecutableCommand;
    use std::io::Write;
    use tokio::process::Command;

    // Check if git repo
    let is_git = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_git {
        eprintln!("not a git repository");
        return;
    }

    // Show diff stat
    let diff = Command::new("git")
        .args(["diff", "--stat", "HEAD"])
        .current_dir(cwd)
        .output()
        .await;

    let status = Command::new("git")
        .args(["status", "--short"])
        .current_dir(cwd)
        .output()
        .await;

    if let Ok(output) = &status {
        let s = String::from_utf8_lossy(&output.stdout);
        if s.trim().is_empty() {
            println!("nothing to commit");
            return;
        }
        println!("Changes:");
        println!("{}", s.trim());
    }

    if let Ok(output) = &diff {
        let d = String::from_utf8_lossy(&output.stdout);
        if !d.trim().is_empty() {
            println!("\n{}", d.trim());
        }
    }

    // Ask for commit message
    let mut stdout = io::stdout();
    let _ = stdout.execute(SetForegroundColor(Color::Yellow));
    let _ = stdout.execute(Print("\nCommit message (empty to cancel): "));
    let _ = stdout.execute(ResetColor);
    let _ = stdout.flush();

    let mut msg = String::new();
    if io::stdin().read_line(&mut msg).is_err() || msg.trim().is_empty() {
        println!("[commit cancelled]");
        return;
    }
    let msg = msg.trim();

    // Stage all and commit
    let add = Command::new("git")
        .args(["add", "-A"])
        .current_dir(cwd)
        .output()
        .await;

    if let Err(e) = add {
        eprintln!("git add failed: {e}");
        return;
    }

    let commit = Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(cwd)
        .output()
        .await;

    match commit {
        Ok(output) => {
            let out = String::from_utf8_lossy(&output.stdout);
            if output.status.success() {
                let _ = stdout.execute(SetForegroundColor(Color::Green));
                println!("{}", out.trim());
                let _ = stdout.execute(ResetColor);
            } else {
                let err = String::from_utf8_lossy(&output.stderr);
                eprintln!("{}", err.trim());
            }
        }
        Err(e) => eprintln!("git commit failed: {e}"),
    }
}
