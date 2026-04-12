//! Model guidance constants and resolution logic.
//!
//! Provides built-in execution discipline (injected for ALL models)
//! and per-provider guidance defaults that can be overridden via config.

/// Universal execution discipline — injected for ALL models, not overridable.
pub const UNIVERSAL_EXECUTION_DISCIPLINE: &str = r#"# Execution Discipline

<tool_persistence>
Keep calling tools until the task is complete AND the result is verified.
Do NOT stop to summarize what you did — the user wants the outcome, not a description.
</tool_persistence>

<mandatory_tool_use>
These scenarios MUST use tools — NEVER answer from memory or reasoning alone:
- File contents or structure → Read, Glob, Grep
- Code modification → Read first, then Edit. Never guess file content.
- System state or command output → Bash
- Math calculations → Bash
</mandatory_tool_use>

<act_dont_describe>
When you say you will do something, you MUST call the corresponding tool in the same response.
Never end your turn with a promise like "I will..." or "Let me..." without an actual tool call.
Every response must contain either a tool call or a final answer.
</act_dont_describe>

<agent_decomposition>
When dispatching sub-agents, each sub-agent handles ONE specific, verifiable task.
BAD:  "Analyze the architecture of the entire module"
GOOD: "Read src/config.rs lines 177-270, list all fields in ModelsConfig and ModelEntryConfig"
BAD:  "Review all error handling"
GOOD: "Check if compact_messages() in compact.rs handles the case where messages.len() <= 2"
</agent_decomposition>

<prerequisite_checks>
Before making changes, verify prerequisites:
- Before modifying a file → Read it to confirm current content
- Before running a command → Verify dependencies exist (Cargo.toml, package.json)
- Before calling an API → Verify config and auth info
</prerequisite_checks>

<verification>
After completing a task, verify the result:
- Code changes → Build or run to confirm no errors
- File creation → Glob or Read to confirm it exists
- Config changes → Load and test
Never claim "done" without verification.
</verification>
"#;

/// Provider-specific guidance defaults.
pub fn builtin_provider_guidance(provider_name: &str) -> &'static str {
    match provider_name {
        "zhipu" | "packyapi" => GUIDANCE_GLM,
        "minimax" => GUIDANCE_MINIMAX,
        "ollama" => GUIDANCE_OLLAMA,
        _ => "",
    }
}

const GUIDANCE_GLM: &str = r#"# GLM Model Guidance
- Do not paraphrase or repeat tool output in Chinese — refer to it directly.
- Tool call JSON parameters must be strictly valid JSON. Double-check before sending.
- When editing code, always show the exact old_string and new_string — never approximate.
"#;

const GUIDANCE_MINIMAX: &str = r#"# MiniMax Model Guidance
- Your thinking/reasoning content is displayed separately. In the main response, output conclusions and actions directly.
- Do not repeat your reasoning process in the response body.
"#;

const GUIDANCE_OLLAMA: &str = r#"# Local Model Guidance
- This is a local model — response may be slower. Avoid requesting very large tool outputs.
- Keep tool result sizes small: use Read with limit parameter, use Grep instead of reading entire files.
"#;

/// Resolve the guidance text for a given provider/model pair.
///
/// Priority: config guidance (glob match) > built-in provider default > empty string.
/// Universal execution discipline is always prepended by the caller.
pub fn resolve_guidance(
    provider_name: &str,
    model_id: &str,
    config_guidance: &std::collections::HashMap<String, String>,
) -> String {
    let target = format!("{}/{}", provider_name, model_id);

    // Try config guidance with glob matching
    if let Some(content) = find_matching_guidance(&target, config_guidance) {
        return content;
    }

    // Fall back to built-in provider guidance
    builtin_provider_guidance(provider_name).to_string()
}

/// Find the best matching guidance from config, supporting `*` glob patterns.
fn find_matching_guidance(
    target: &str,
    guidance_map: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let mut matches: Vec<(&str, &str, usize)> = guidance_map
        .iter()
        .filter(|(pattern, _)| glob_match(pattern, target))
        .map(|(pattern, path)| {
            let wildcards = pattern.chars().filter(|c| *c == '*').count();
            (pattern.as_str(), path.as_str(), wildcards)
        })
        .collect();

    // Sort by specificity: fewer wildcards first
    matches.sort_by_key(|(_, _, wildcards)| *wildcards);

    if let Some((_, path, _)) = matches.first() {
        let expanded = expand_tilde(path);
        match std::fs::read_to_string(&expanded) {
            Ok(content) => {
                let warnings = crate::security::scan_content(path, &content);
                if !warnings.is_empty() {
                    for w in &warnings {
                        log::warn!("[Security] {} in {} line {}: {}", w.threat_type, w.filename, w.line_number, w.matched_text);
                    }
                    if let Some(prefix) = crate::security::format_warnings(&warnings) {
                        return Some(format!("{}\n\n{}", prefix, content));
                    }
                }
                return Some(content);
            }
            Err(e) => {
                log::warn!("Failed to read guidance file {}: {}", expanded, e);
            }
        }
    }
    None
}

/// Simple glob matching: `*` matches any sequence of characters.
fn glob_match(pattern: &str, target: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == target;
    }

    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match target[pos..].find(part) {
            Some(found) => {
                if i == 0 && found != 0 {
                    return false;
                }
                pos += found + part.len();
            }
            None => return false,
        }
    }
    if !parts.last().unwrap_or(&"").is_empty() {
        return pos == target.len();
    }
    true
}

/// Expand `~` to home directory.
fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}/{}", home.display(), &path[2..]);
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("zhipu/glm-5.1", "zhipu/glm-5.1"));
        assert!(!glob_match("zhipu/glm-5", "zhipu/glm-5.1"));
    }

    #[test]
    fn test_glob_match_wildcard() {
        assert!(glob_match("zhipu/*", "zhipu/glm-5.1"));
        assert!(glob_match("*/glm-*", "zhipu/glm-5.1"));
        assert!(!glob_match("zhipu/*", "minimax/MiniMax-M2.7"));
    }

    #[test]
    fn test_glob_match_double_wildcard() {
        assert!(glob_match("*/*", "zhipu/glm-5.1"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn test_builtin_provider_guidance_known() {
        assert!(!builtin_provider_guidance("zhipu").is_empty());
        assert!(!builtin_provider_guidance("packyapi").is_empty());
        assert!(!builtin_provider_guidance("minimax").is_empty());
        assert!(!builtin_provider_guidance("ollama").is_empty());
    }

    #[test]
    fn test_builtin_provider_guidance_unknown() {
        assert!(builtin_provider_guidance("unknown_provider").is_empty());
        assert!(builtin_provider_guidance("anthropic").is_empty());
    }
}
