//! 自动补全模块，处理 / 和 @ 触发的补全

pub mod commands;
pub mod files;
pub mod models;
pub mod parser;
pub mod types;

// 向后兼容的重新导出
pub use types::{Suggestion, SuggestionContext, SuggestionType, TriggerType};

pub use commands::generate_command_suggestions;
#[allow(unused_imports)]
pub use commands::get_slash_commands;
pub use files::generate_file_suggestions;
pub use models::{generate_model_suggestions, generate_model_subcommand_suggestions};
pub use parser::extract_completion_token;

/// 根据上下文生成建议
pub fn generate_suggestions(ctx: &SuggestionContext) -> Vec<Suggestion> {
    if let Some((token, _start_pos, trigger_type)) =
        extract_completion_token(&ctx.input, ctx.cursor_offset)
    {
        match trigger_type {
            TriggerType::SlashCommand => generate_command_suggestions(&token, &ctx.skills),
            TriggerType::AtSymbol => generate_file_suggestions(&token, &ctx.cwd),
            TriggerType::ModelArg => generate_model_suggestions(&token, &ctx.models),
            TriggerType::ModelSubCommand => generate_model_subcommand_suggestions(&token),
        }
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_slash_command_token() {
        let input = "/hel";
        let result = extract_completion_token(input, 4);
        // Token 应包含前导 /
        assert_eq!(
            result,
            Some(("/hel".to_string(), 0, TriggerType::SlashCommand))
        );
    }

    #[test]
    fn test_extract_at_token() {
        let input = "@src";
        let result = extract_completion_token(input, 4);
        // Token 应包含 @
        assert!(result.is_some());
        if let Some((token, pos, trigger)) = result {
            assert_eq!(pos, 0);
            assert_eq!(trigger, TriggerType::AtSymbol);
            assert!(token.starts_with('@'));
        }
    }

    #[test]
    fn test_generate_command_suggestions() {
        let suggestions = generate_command_suggestions("/hel", &[]);
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0].display_text, "/help");
    }

    #[test]
    fn test_generate_command_suggestions_empty() {
        let suggestions = generate_command_suggestions("", &[]);
        assert!(suggestions.len() > 5); // 应返回所有命令
    }

    #[test]
    fn test_generate_command_suggestions_with_skills() {
        let skills = vec![
            (
                "cm".to_string(),
                "commit message".to_string(),
                vec!["commit".to_string()],
            ),
            (
                "review".to_string(),
                "code review".to_string(),
                vec!["cr".to_string()],
            ),
        ];
        // 空部分输入 → 所有命令 + 所有技能
        let suggestions = generate_command_suggestions("", &skills);
        assert!(suggestions.iter().any(|s| s.display_text == "/cm"));
        assert!(suggestions.iter().any(|s| s.display_text == "/review"));
        assert!(suggestions.iter().any(|s| s.display_text == "/help"));

        // 部分 "c" → 匹配 /cm（名称）、/clear（命令）、/commit（命令）、/context（命令）
        let suggestions = generate_command_suggestions("/c", &skills);
        assert!(suggestions.iter().any(|s| s.display_text == "/cm"));

        // 部分 "cr" → 匹配技能别名 "cr" → 技能 "review"
        let suggestions = generate_command_suggestions("/cr", &skills);
        assert!(suggestions.iter().any(|s| s.display_text == "/review"));
    }
}
