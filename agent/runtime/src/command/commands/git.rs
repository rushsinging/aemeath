//! Git commands: init, commit, review.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::command::{
    Command, CommandAction, CommandCategory, CommandContext, CommandDescriptor, CommandResult,
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

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "commit".to_string(),
            "Create a git commit with AI".to_string(),
            CommandCategory::Git,
            commit_execute,
        )
        .with_usage(vec![
            "/commit - Create commit".to_string(),
            "/commit message - Create commit with message".to_string(),
        ])
        .with_aliases(vec!["cm".to_string()])
    })
}

fn commit_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    if !std::path::Path::new(".git").exists() {
        return CommandResult::Error("Not a git repository. Use /init first".to_string());
    }
    let prompt = if args.trim().is_empty() {
        "请查看当前的 git diff 和 status，生成合适的 commit message，然后执行 git commit。如果有未暂存的文件，先确认是否需要 git add。commit message 使用中文，遵循 conventional commits 格式。".to_string()
    } else {
        format!(
            "请执行 git commit，使用以下 commit message：\n\n{}",
            args.trim()
        )
    };
    CommandResult::Action(CommandAction::InjectMessage(prompt))
}

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "review".to_string(),
            "Review code changes or files".to_string(),
            CommandCategory::Git,
            review_execute,
        )
        .with_usage(vec![
            "/review - Review current changes".to_string(),
            "/review diff - Review current diff".to_string(),
            "/review staged - Review staged changes only".to_string(),
            "/review last - Review last commit".to_string(),
            "/review <file> - Review changes in a specific file".to_string(),
            "/review HEAD~3..HEAD - Review a commit range".to_string(),
        ])
        .with_aliases(vec!["rev".to_string()])
    })
}

fn review_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    if !std::path::Path::new(".git").exists() {
        return CommandResult::Error("Not a git repository. Use /init first".to_string());
    }
    let arg = args.trim().to_lowercase();
    let cwd = std::env::current_dir().unwrap_or_default();
    let diff_text = match arg.as_str() {
        "" | "changes" | "diff" => run_git(&cwd, &["diff", "HEAD"])
            .or_else(|| run_git(&cwd, &["diff"]))
            .unwrap_or_default(),
        "staged" => run_git(&cwd, &["diff", "--cached"]).unwrap_or_default(),
        "last" | "last-commit" => {
            run_git(&cwd, &["show", "HEAD", "--format=fuller", "--patch"]).unwrap_or_default()
        }
        _ => {
            let original_arg = args.trim();
            if original_arg.starts_with('-') {
                return CommandResult::Error(format!(
                    "Invalid argument: {:?}. Flags are not allowed here.",
                    original_arg
                ));
            }
            if original_arg.contains("..") {
                run_git(&cwd, &["diff", original_arg]).unwrap_or_default()
            } else {
                run_git(&cwd, &["diff", "HEAD", "--", original_arg])
                    .or_else(|| run_git(&cwd, &["diff", "--", original_arg]))
                    .unwrap_or_default()
            }
        }
    };
    if diff_text.trim().is_empty() {
        return CommandResult::Success("No changes to review. Working tree is clean.".to_string());
    }
    let status_text = run_git(&cwd, &["status", "--short"]).unwrap_or_default();
    let mut review_prompt = String::from("请对以下代码变更进行 code review。\n\n");
    review_prompt.push_str("请关注以下方面：\n");
    review_prompt.push_str("1. **正确性**：逻辑错误、边界条件、潜在的 bug\n");
    review_prompt.push_str("2. **安全性**：注入漏洞、敏感信息泄露\n");
    review_prompt.push_str("3. **代码质量**：可读性、命名、重复代码\n");
    review_prompt.push_str("4. **性能**：不必要的开销、N+1 查询等\n");
    review_prompt.push_str("5. **设计**：职责分离、耦合度\n\n");
    if !status_text.is_empty() {
        review_prompt.push_str("## Changed files\n```\n");
        review_prompt.push_str(&status_text);
        review_prompt.push_str("\n```\n\n");
    }
    review_prompt.push_str("## Diff\n```diff\n");
    let max_diff = 50_000;
    if diff_text.len() > max_diff {
        let start_byte = diff_text
            .char_indices()
            .nth(diff_text.chars().count().saturating_sub(max_diff))
            .map(|(i, _)| i)
            .unwrap_or(0);
        review_prompt.push_str(&diff_text[start_byte..]);
        review_prompt.push_str("\n```\n\n(truncated — showing last ~50k characters)");
    } else {
        review_prompt.push_str(&diff_text);
        review_prompt.push_str("\n```");
    }
    CommandResult::Action(CommandAction::InjectMessage(review_prompt))
}

fn run_git(cwd: &std::path::Path, args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
}
