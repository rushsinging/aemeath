//! /resume 命令的 session 补全建议生成

use super::types::{Suggestion, SuggestionType};

/// 根据 session 列表和部分输入生成 session 建议
/// sessions: (id, summary) 对
pub fn generate_resume_suggestions(
    partial: &str,
    sessions: &[(String, String)],
) -> Vec<Suggestion> {
    let partial_lower = partial.to_lowercase();

    if partial_lower.is_empty() {
        // 返回所有 session
        return sessions
            .iter()
            .take(10)
            .map(|(id, summary)| Suggestion {
                _id: format!("session-{}", id),
                display_text: format!("{}  {}", id, summary),
                _description: None,
                suggestion_type: SuggestionType::Session,
            })
            .collect();
    }

    // 按前缀或摘要筛选
    sessions
        .iter()
        .take(20)
        .filter(|(id, summary)| {
            id.to_lowercase().starts_with(&partial_lower)
                || summary.to_lowercase().contains(&partial_lower)
        })
        .take(10)
        .map(|(id, summary)| Suggestion {
            _id: format!("session-{}", id),
            display_text: format!("{}  {}", id, summary),
            _description: None,
            suggestion_type: SuggestionType::Session,
        })
        .collect()
}
