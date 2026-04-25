//! 斜杠命令定义及命令建议生成

use super::types::{SlashCommand, Suggestion, SuggestionType};

/// 获取所有可用的斜杠命令
pub fn get_slash_commands() -> Vec<SlashCommand> {
    vec![
        SlashCommand {
            name: "help".to_string(),
            description: "Show available commands".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "exit".to_string(),
            description: "Exit the agent".to_string(),
            aliases: vec!["quit".to_string()],
        },
        SlashCommand {
            name: "clear".to_string(),
            description: "Clear conversation history".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "compact".to_string(),
            description: "Manually compact conversation".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "usage".to_string(),
            description: "Show token usage statistics".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "context".to_string(),
            description: "Show context window usage".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "save".to_string(),
            description: "Save current session to disk".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "sessions".to_string(),
            description: "List saved sessions".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "commit".to_string(),
            description: "Stage changes and create git commit".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "image".to_string(),
            description: "Add an image to next message".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "paste".to_string(),
            description: "Read image from clipboard".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "images".to_string(),
            description: "Show pending images".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "clear-images".to_string(),
            description: "Clear pending images".to_string(),
            aliases: vec![],
        },
        SlashCommand {
            name: "review".to_string(),
            description: "Review code changes (git diff)".to_string(),
            aliases: vec!["rev".to_string()],
        },
        // 模型相关命令
        SlashCommand {
            name: "model".to_string(),
            description: "Show/switch model (use /model list to see available)".to_string(),
            aliases: vec![],
        },
    ]
}

/// 根据部分输入生成斜杠命令建议
/// 同时包含技能名称/别名作为建议
pub fn generate_command_suggestions(partial: &str, skills: &[(String, String, Vec<String>)]) -> Vec<Suggestion> {
    let commands = get_slash_commands();
    let partial_lower = partial.to_lowercase();

    // 移除前导 /
    let search_term = if partial_lower.starts_with('/') {
        &partial_lower[1..]
    } else {
        &partial_lower
    };

    let mut results = Vec::new();

    if search_term.is_empty() {
        // 返回所有命令 + 所有技能
        for cmd in &commands {
            results.push(Suggestion {
                _id: format!("cmd-{}", cmd.name),
                display_text: format!("/{}", cmd.name),
                _description: Some(cmd.description.clone()),
                suggestion_type: SuggestionType::Command,
            });
        }
        for (name, desc, _aliases) in skills {
            results.push(Suggestion {
                _id: format!("skill-{}", name),
                display_text: format!("/{}", name),
                _description: Some(desc.clone()),
                suggestion_type: SuggestionType::Command,
            });
        }
        return results;
    }

    // 按名称或别名筛选命令
    for cmd in &commands {
        if cmd.name.starts_with(search_term)
            || cmd.aliases.iter().any(|a| a.starts_with(search_term))
        {
            results.push(Suggestion {
                _id: format!("cmd-{}", cmd.name),
                display_text: format!("/{}", cmd.name),
                _description: Some(cmd.description.clone()),
                suggestion_type: SuggestionType::Command,
            });
        }
    }

    // 按名称或别名筛选技能
    for (name, desc, aliases) in skills {
        if name.starts_with(search_term)
            || aliases.iter().any(|a| a.starts_with(search_term))
        {
            results.push(Suggestion {
                _id: format!("skill-{}", name),
                display_text: format!("/{}", name),
                _description: Some(desc.clone()),
                suggestion_type: SuggestionType::Command,
            });
        }
    }

    results
}
