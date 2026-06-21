use super::types::ReflectionOutput;
use share::i18n::runtime::reflection as t;
use share::message::Message;

pub fn format_output(output: &ReflectionOutput, lang: &str) -> String {
    let mut sections = vec![t::section_title_reflection(lang).to_string()];
    if output.deviations.is_empty() {
        sections.push(t::deviations_empty(lang).to_string());
    } else {
        sections.push(format!(
            "{}{}",
            t::deviations_header(lang),
            output.deviations.join("\n- ")
        ));
    }

    if output.suggested_memories.is_empty() {
        sections.push(t::suggestions_empty(lang).to_string());
    } else {
        let suggestions = output
            .suggested_memories
            .iter()
            .map(|suggestion| format!("- [{:?}] {}", suggestion.category, suggestion.content))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("{}{suggestions}", t::suggestions_header(lang)));
    }

    if !output.outdated_memories.is_empty() {
        sections.push(format!(
            "{}{}",
            t::outdated_header(lang),
            output.outdated_memories.join(", ")
        ));
    }
    if let Some(alert) = &output.user_alert {
        sections.push(format!("{}{alert}", t::user_alert_header(lang)));
    }
    sections.join("\n\n")
}

pub fn build_prompt(project_memory: &str, recent_summary: &str, lang: &str) -> String {
    crate::business::reflection::prompt::build_reflection_prompt(
        project_memory,
        recent_summary,
        lang,
    )
}

pub fn recent_messages_summary(messages: &[Message], max_chars: usize) -> String {
    let mut lines = Vec::new();
    for message in messages.iter().rev() {
        let text = message
            .content
            .iter()
            .filter_map(|block| match block {
                share::message::ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            continue;
        }
        let role = match message.role {
            share::message::Role::User => "User",
            share::message::Role::Assistant => "Assistant",
        };
        lines.push(format!("[{role}]: {text}"));
        let joined = lines.iter().rev().cloned().collect::<Vec<_>>().join("\n");
        if joined.len() >= max_chars {
            return joined.chars().take(max_chars).collect();
        }
    }
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}
