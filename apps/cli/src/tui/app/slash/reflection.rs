use super::constants::DEFAULT_REFLECTION_HISTORY_LIMIT;
use crate::tui::effect::effect::Effect;

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
#[path = "reflection_tests.rs"]
mod tests;
