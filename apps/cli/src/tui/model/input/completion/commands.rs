//! 斜杠命令补全建议生成

use super::types::{Suggestion, SuggestionType};

/// 根据部分输入生成斜杠命令建议
/// commands 从 CommandRegistry 动态获取，不再硬编码
pub fn generate_command_suggestions(
    partial: &str,
    commands: &[(String, String, Vec<String>)],
) -> Vec<Suggestion> {
    let partial_lower = partial.to_lowercase();

    // 移除前导 /
    let search_term = if partial_lower.starts_with('/') {
        partial_lower.strip_prefix('/').unwrap_or(&partial_lower)
    } else {
        &partial_lower
    };

    let mut results = Vec::new();

    if search_term.is_empty() {
        // Command Catalog 是 Slash 补全唯一来源；保持 Catalog 的确定性顺序。
        for (name, desc, aliases) in commands {
            results.push(Suggestion {
                _id: format!("cmd-{}", name),
                display_text: format!("/{}", name),
                _description: Some(desc.clone()),
                suggestion_type: SuggestionType::Command,
            });
            for alias in aliases {
                results.push(Suggestion {
                    _id: format!("cmd-{}-{}", name, alias),
                    display_text: format!("/{}", alias),
                    _description: Some(desc.clone()),
                    suggestion_type: SuggestionType::Command,
                });
            }
        }
        return results;
    }

    // 按名称或别名筛选命令，别名匹配时用别名作为 display_text
    for (name, desc, aliases) in commands {
        let matched_alias = aliases.iter().find(|a| a.starts_with(search_term));
        if name.starts_with(search_term) {
            results.push(Suggestion {
                _id: format!("cmd-{}", name),
                display_text: format!("/{}", name),
                _description: Some(desc.clone()),
                suggestion_type: SuggestionType::Command,
            });
        } else if let Some(alias) = matched_alias {
            results.push(Suggestion {
                _id: format!("cmd-{}-{}", name, alias),
                display_text: format!("/{}", alias),
                _description: Some(desc.clone()),
                suggestion_type: SuggestionType::Command,
            });
        }
    }

    results
}
