use std::path::PathBuf;

pub async fn is_git_repo(cwd: &PathBuf) -> bool {
    use tokio::process::Command;
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub async fn collect_git_context(cwd: &PathBuf, lang: &str) -> String {
    use tokio::process::Command;

    let l = match lang {
        "zh" => GitContextLabels {
            header: "# Git Context",
            branch: "当前分支",
            default_branch: "默认分支",
            git_user: "Git 用户",
            status: "状态",
            recent_commits: "最近提交",
        },
        _ => GitContextLabels {
            header: "# Git Context",
            branch: "Current branch",
            default_branch: "Default branch",
            git_user: "Git user",
            status: "Status",
            recent_commits: "Recent commits",
        },
    };

    let mut parts: Vec<String> = Vec::new();
    parts.push(l.header.to_string());

    // Branch name
    if let Ok(output) = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
        .await
    {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !branch.is_empty() {
            parts.push(format!("{}: {branch}", l.branch));
        }
    }

    // Default branch
    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "origin/HEAD"])
        .current_dir(cwd)
        .output()
        .await
    {
        let default = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !default.is_empty() && default != "origin/HEAD" {
            let branch = default.strip_prefix("origin/").unwrap_or(&default);
            parts.push(format!("{}: {branch}", l.default_branch));
        }
    }

    // Git user
    if let Ok(output) = Command::new("git")
        .args(["config", "user.name"])
        .current_dir(cwd)
        .output()
        .await
    {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            parts.push(format!("{}: {name}", l.git_user));
        }
    }

    // Status (short)
    if let Ok(output) = Command::new("git")
        .args(["--no-optional-locks", "status", "--short"])
        .current_dir(cwd)
        .output()
        .await
    {
        let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !status.is_empty() {
            let lines: Vec<&str> = status.lines().take(20).collect();
            parts.push(format!("{}:\n{}", l.status, lines.join("\n")));
        }
    }

    // Recent commits
    if let Ok(output) = Command::new("git")
        .args(["--no-optional-locks", "log", "--oneline", "-n", "5"])
        .current_dir(cwd)
        .output()
        .await
    {
        let log = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !log.is_empty() {
            parts.push(format!("{}:\n{log}", l.recent_commits));
        }
    }

    let result = parts.join("\n");
    // Truncate to ~2000 bytes, respecting UTF-8 char boundaries
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

struct GitContextLabels {
    header: &'static str,
    branch: &'static str,
    default_branch: &'static str,
    git_user: &'static str,
    status: &'static str,
    recent_commits: &'static str,
}
