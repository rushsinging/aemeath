use crate::tui::effect::effect::Effect;

const DEFAULT_REFLECTION_HISTORY_LIMIT: usize = 10;

impl super::super::App {
    /// `/reflect [limit]` only queries safe reflection history metadata.
    pub(crate) fn handle_reflect_command(&mut self, args: &str) -> Vec<Effect> {
        let arg = args.trim();
        let limit = if arg.is_empty() {
            DEFAULT_REFLECTION_HISTORY_LIMIT
        } else {
            match arg.parse::<usize>() {
                Ok(limit) if limit > 0 => limit,
                _ => {
                    self.append_error_notice("用法: /reflect [limit]，limit 必须是大于 0 的数字。");
                    return Vec::new();
                }
            }
        };

        vec![Effect::QueryReflectionHistory { limit }]
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::app::App;
    use crate::tui::effect::effect::Effect;

    fn make_app() -> App {
        App::new(
            "s".to_string(),
            std::path::PathBuf::from("/tmp"),
            "m".to_string(),
        )
    }

    #[test]
    fn reflect_queries_default_limit_without_processing_or_spinner() {
        let mut app = make_app();
        let effects = app.handle_reflect_command("");

        assert_eq!(effects, vec![Effect::QueryReflectionHistory { limit: 10 }]);
        assert!(!app.chat.is_processing);
        assert!(!app.model.conversation.runtime.spinner.chat_active);
        assert!(app.model.conversation.runtime.spinner.phase.is_none());
    }

    #[test]
    fn reflect_accepts_positive_limit_and_rejects_invalid_limit() {
        let mut app = make_app();
        assert_eq!(
            app.handle_reflect_command("3"),
            vec![Effect::QueryReflectionHistory { limit: 3 }]
        );
        assert!(app.handle_reflect_command("0").is_empty());
        assert!(app.handle_reflect_command("nope").is_empty());
    }
}
