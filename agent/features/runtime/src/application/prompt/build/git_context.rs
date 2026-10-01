use project::GitCommandOutcome;
use share::i18n::prompt::git_context_labels::git_context_labels;
use std::path::PathBuf;

/// 经 project 的全仓唯一 git spawn 窄面执行；git 子命令为短阻塞 IO，
/// 用 `spawn_blocking` 保持本模块的 async 签名不阻塞 worker 线程。
async fn git_output(cwd: &PathBuf, args: &[&str]) -> Option<GitCommandOutcome> {
    let cwd = cwd.clone();
    let owned_args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    tokio::task::spawn_blocking(move || {
        let arg_refs: Vec<&str> = owned_args.iter().map(String::as_str).collect();
        project::run_git_command(&cwd, &arg_refs).ok()
    })
    .await
    .ok()
    .flatten()
}

pub async fn is_git_repo(cwd: &PathBuf) -> bool {
    git_output(cwd, &["rev-parse", "--is-inside-work-tree"])
        .await
        .map(|outcome| outcome.is_success())
        .unwrap_or(false)
}

pub async fn collect_git_context(cwd: &PathBuf, lang: &str) -> String {
    let labels = git_context_labels(lang);

    let mut parts: Vec<String> = Vec::new();
    parts.push(labels.header.to_string());

    if let Some(outcome) = git_output(cwd, &["branch", "--show-current"]).await {
        let branch = String::from_utf8_lossy(outcome.stdout_bytes())
            .trim()
            .to_string();
        if !branch.is_empty() {
            parts.push(format!("{}: {branch}", labels.branch));
        }
    }

    if let Some(outcome) = git_output(cwd, &["rev-parse", "--abbrev-ref", "origin/HEAD"]).await {
        let default_branch = String::from_utf8_lossy(outcome.stdout_bytes())
            .trim()
            .to_string();
        if !default_branch.is_empty() && default_branch != "origin/HEAD" {
            let branch = default_branch
                .strip_prefix("origin/")
                .unwrap_or(&default_branch);
            parts.push(format!("{}: {branch}", labels.default_branch));
        }
    }

    if let Some(outcome) = git_output(cwd, &["config", "user.name"]).await {
        let name = String::from_utf8_lossy(outcome.stdout_bytes())
            .trim()
            .to_string();
        if !name.is_empty() {
            parts.push(format!("{}: {name}", labels.git_user));
        }
    }

    if let Some(outcome) = git_output(cwd, &["--no-optional-locks", "status", "--short"]).await {
        let status = String::from_utf8_lossy(outcome.stdout_bytes())
            .trim()
            .to_string();
        if !status.is_empty() {
            let lines: Vec<&str> = status.lines().take(20).collect();
            parts.push(format!("{}:\n{}", labels.status, lines.join("\n")));
        }
    }

    if let Some(outcome) =
        git_output(cwd, &["--no-optional-locks", "log", "--oneline", "-n", "5"]).await
    {
        let recent_commits = String::from_utf8_lossy(outcome.stdout_bytes())
            .trim()
            .to_string();
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
