//! 自动补全模块，处理 / 和 @ 触发的补全

pub mod commands;
pub mod models;
pub mod parser;
pub mod sessions;
pub mod types;

// 向后兼容的重新导出
pub use types::{Suggestion, SuggestionContext, SuggestionType, TriggerType};

pub use commands::generate_command_suggestions;
#[allow(unused_imports)]
pub use commands::generate_command_suggestions as get_slash_commands_compat;
pub use models::{generate_model_subcommand_suggestions, generate_model_suggestions};
pub use parser::extract_completion_token;
pub use sessions::generate_resume_suggestions;

/// 根据上下文生成建议
pub fn generate_suggestions(ctx: &SuggestionContext) -> Vec<Suggestion> {
    if let Some((token, _start_pos, trigger_type)) =
        extract_completion_token(&ctx.input, ctx.cursor_offset)
    {
        match trigger_type {
            TriggerType::SlashCommand => {
                generate_command_suggestions(&token, &ctx.skills, &ctx.commands)
            }
            TriggerType::AtSymbol => {
                crate::tui::effect::completion::generate_file_suggestions(&token, &ctx.cwd)
            }
            TriggerType::ModelArg => generate_model_suggestions(&token, &ctx.models),
            TriggerType::ModelSubCommand => generate_model_subcommand_suggestions(&token),
            TriggerType::ResumeArg => generate_resume_suggestions(&token, &ctx.sessions),
        }
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod generation_tests {
    use super::*;

    fn test_commands() -> Vec<(String, String, Vec<String>)> {
        vec![
            ("help".into(), "Show available commands".into(), vec![]),
            ("exit".into(), "Exit the agent".into(), vec!["quit".into()]),
            ("clear".into(), "Clear conversation history".into(), vec![]),
            (
                "compact".into(),
                "Manually compact conversation".into(),
                vec![],
            ),
            (
                "commit".into(),
                "Stage changes and create git commit".into(),
                vec![],
            ),
            ("context".into(), "Show context window usage".into(), vec![]),
            ("model".into(), "Show/switch model".into(), vec![]),
            ("resume".into(), "Resume a previous session".into(), vec![]),
            ("think".into(), "Toggle thinking mode".into(), vec![]),
        ]
    }

    #[test]
    fn test_extract_slash_command_token() {
        let input = "/hel";
        let result = extract_completion_token(input, 4);
        assert_eq!(
            result,
            Some(("/hel".to_string(), 0, TriggerType::SlashCommand))
        );
    }

    #[test]
    fn test_extract_at_token() {
        let input = "@src";
        let result = extract_completion_token(input, 4);
        assert!(result.is_some());
        if let Some((token, pos, trigger)) = result {
            assert_eq!(pos, 0);
            assert_eq!(trigger, TriggerType::AtSymbol);
            assert!(token.starts_with('@'));
        }
    }

    #[test]
    fn test_generate_command_suggestions() {
        let cmds = test_commands();
        let suggestions = generate_command_suggestions("/hel", &[], &cmds);
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0].display_text, "/help");
    }

    #[test]
    fn test_generate_command_suggestions_empty() {
        let cmds = test_commands();
        let suggestions = generate_command_suggestions("", &[], &cmds);
        assert!(suggestions.len() > 5);
    }

    #[test]
    fn test_generate_command_suggestions_with_skills() {
        let cmds = test_commands();
        let skills = vec![
            ("cm".into(), "commit message".into(), vec!["commit".into()]),
            ("review".into(), "code review".into(), vec!["cr".into()]),
        ];
        // 空部分输入 → 所有命令 + 所有技能
        let suggestions = generate_command_suggestions("", &skills, &cmds);
        assert!(suggestions.iter().any(|s| s.display_text == "/cm"));
        assert!(suggestions.iter().any(|s| s.display_text == "/review"));
        assert!(suggestions.iter().any(|s| s.display_text == "/help"));

        // 部分 "c" → 匹配 /cm（技能）、/clear（命令）、/commit（命令）、/context（命令）
        let suggestions = generate_command_suggestions("/c", &skills, &cmds);
        assert!(suggestions.iter().any(|s| s.display_text == "/cm"));

        // 部分 "cr" → 匹配技能别名 "cr" → display_text 使用别名
        let suggestions = generate_command_suggestions("/cr", &skills, &cmds);
        assert!(suggestions.iter().any(|s| s.display_text == "/cr"));
    }
}

use crate::tui::model::input::completion_item::CompletionItem;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputCompletion {
    pub visible: bool,
    pub selected_index: Option<usize>,
    pub query: String,
    pub items: Vec<CompletionItem>,
}

impl InputCompletion {
    pub fn set_items(&mut self, items: Vec<CompletionItem>, query: String) {
        self.visible = !items.is_empty();
        self.selected_index = self.visible.then_some(0);
        self.items = items;
        self.query = query;
    }

    pub fn clear(&mut self) {
        self.visible = false;
        self.selected_index = None;
        self.items.clear();
        self.query.clear();
    }

    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            self.selected_index = None;
            return;
        }
        let current = self.selected_index.unwrap_or(0);
        self.selected_index = Some((current + 1) % self.items.len());
    }

    pub fn select_previous(&mut self) {
        if self.items.is_empty() {
            self.selected_index = None;
            return;
        }
        let current = self.selected_index.unwrap_or(0);
        self.selected_index = Some(if current == 0 {
            self.items.len() - 1
        } else {
            current - 1
        });
    }

    pub fn selected_item(&self) -> Option<&CompletionItem> {
        self.selected_index.and_then(|index| self.items.get(index))
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;

    #[test]
    fn test_completion_set_items_selects_first() {
        let mut completion = InputCompletion::default();
        completion.set_items(vec![CompletionItem::new("a", "a")], "a".to_string());
        assert_eq!(completion.selected_index, Some(0));
    }

    #[test]
    fn test_completion_set_empty_hides() {
        let mut completion = InputCompletion::default();
        completion.set_items(Vec::new(), "".to_string());
        assert!(!completion.visible);
    }

    #[test]
    fn test_completion_select_next_wraps() {
        let mut completion = InputCompletion::default();
        completion.set_items(
            vec![CompletionItem::new("a", "a"), CompletionItem::new("b", "b")],
            "".to_string(),
        );
        completion.select_next();
        completion.select_next();
        assert_eq!(completion.selected_index, Some(0));
    }
}
