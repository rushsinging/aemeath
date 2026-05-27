//! Skills commands.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::core::command::{
    Command, CommandAction, CommandCategory, CommandContext, CommandDescriptor, CommandResult,
};

use crate::api::prompt::skill;

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "skills".to_string(),
            "Manage skills".to_string(),
            CommandCategory::Tools,
            skills_execute,
        )
        .with_usage(vec![
            "/skills - List available skills".to_string(),
            "/skills run <name> - Run a skill".to_string(),
        ])
    })
}

fn skills_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim();
    if arg.is_empty() || arg == "list" {
        let cwd = std::env::current_dir().unwrap_or_default();
        let skills = skill::load_all_skills(&cwd, &[]);
        if skills.is_empty() {
            return CommandResult::Success("No skills available.\n\nSkills are loaded from:\n  - .aemeath/skills/\n  - ~/.aemeath/skills/\n  - ~/.agents/skills/".to_string());
        }
        let mut lines = vec!["Available Skills:\n".to_string()];
        let mut sorted: Vec<_> = skills.iter().collect();
        sorted.sort_by_key(|(name, _)| *name);
        for (name, skill) in sorted {
            let desc = if skill.description.is_empty() {
                String::new()
            } else {
                format!(" — {}", skill.description)
            };
            let aliases = if skill.aliases.is_empty() {
                String::new()
            } else {
                format!(" (aliases: {})", skill.aliases.join(", "))
            };
            lines.push(format!("  /{}{}", name, desc));
            if !aliases.is_empty() {
                lines.push(aliases);
            }
        }
        lines.push("\nUse /skills run <name> to execute a skill".to_string());
        CommandResult::Success(lines.join("\n"))
    } else if let Some(name) = arg.strip_prefix("run ").map(|s| s.trim()) {
        if name.is_empty() {
            return CommandResult::Error("Usage: /skills run <name>".to_string());
        }
        let cwd = std::env::current_dir().unwrap_or_default();
        let skills = skill::load_all_skills(&cwd, &[]);
        // Look up by name or alias
        let skill = skills.get(name).or_else(|| {
            skills
                .values()
                .find(|s| s.aliases.iter().any(|a| a == name))
        });
        match skill {
            Some(s) => {
                let content = s.content.clone();
                if content.is_empty() {
                    return CommandResult::Error(format!("Skill '{}' has no content", s.name));
                }
                CommandResult::Action(CommandAction::RunSkill(content))
            }
            None => {
                let available: Vec<&str> = skills.keys().map(|s| s.as_str()).collect();
                CommandResult::Error(format!(
                    "Skill '{}' not found. Available: {}",
                    name,
                    if available.is_empty() {
                        "(none)".to_string()
                    } else {
                        available.join(", ")
                    }
                ))
            }
        }
    } else {
        // Treat bare name as "run" for convenience
        let cwd = std::env::current_dir().unwrap_or_default();
        let skills = skill::load_all_skills(&cwd, &[]);
        let skill = skills
            .get(arg)
            .or_else(|| skills.values().find(|s| s.aliases.iter().any(|a| a == arg)));
        match skill {
            Some(s) => {
                if s.content.is_empty() {
                    return CommandResult::Error(format!("Skill '{}' has no content", s.name));
                }
                CommandResult::Action(CommandAction::RunSkill(s.content.clone()))
            }
            None => CommandResult::Error(format!("Unknown skill or sub-command: {}\nUse /skills to list available skills, or /skills run <name> to run one", arg)),
        }
    }
}
