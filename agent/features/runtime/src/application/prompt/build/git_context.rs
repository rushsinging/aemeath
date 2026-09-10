use share::i18n::prompt::git_context_labels::git_context_labels;
use std::path::PathBuf;
use tokio::process::Command;

async fn git_output(cwd: &PathBuf, args: &[&str]) -> Option<std::process::Output> {
    let mut command = Command::new("git");
    command.args(args).current_dir(cwd);
    utils::configure_tokio_noninteractive(&mut command).ok()?;
    command.output().await.ok()
}

pub async fn is_git_repo(cwd: &PathBuf) -> bool {
    git_output(cwd, &["rev-parse", "--is-inside-work-tree"])
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub async fn collect_git_context(cwd: &PathBuf, lang: &str) -> String {
    let labels = git_context_labels(lang);

    let mut parts: Vec<String> = Vec::new();
    parts.push(labels.header.to_string());

    if let Some(output) = git_output(cwd, &["branch", "--show-current"]).await {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !branch.is_empty() {
            parts.push(format!("{}: {branch}", labels.branch));
        }
    }

    if let Some(output) = git_output(cwd, &["rev-parse", "--abbrev-ref", "origin/HEAD"]).await {
        let default_branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !default_branch.is_empty() && default_branch != "origin/HEAD" {
            let branch = default_branch
                .strip_prefix("origin/")
                .unwrap_or(&default_branch);
            parts.push(format!("{}: {branch}", labels.default_branch));
        }
    }

    if let Some(output) = git_output(cwd, &["config", "user.name"]).await {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            parts.push(format!("{}: {name}", labels.git_user));
        }
    }

    if let Some(output) = git_output(cwd, &["--no-optional-locks", "status", "--short"]).await {
        let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !status.is_empty() {
            let lines: Vec<&str> = status.lines().take(20).collect();
            parts.push(format!("{}:\n{}", labels.status, lines.join("\n")));
        }
    }

    if let Some(output) =
        git_output(cwd, &["--no-optional-locks", "log", "--oneline", "-n", "5"]).await
    {
        let recent_commits = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !recent_commits.is_empty() {
            parts.push(format!("{}:\n{recent_commits}", labels.recent_commits));
        }
    }

    let result = parts.join("\n");
    if result.len() > 2000 {
        let mut end = 2000;
        while end > 0 && !result.is_char_boundary(end) {
            end -= 1;
        }
        result[..end].to_string()
    } else {
        result
    }
}
