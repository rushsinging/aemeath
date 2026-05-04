//! Guidance resolution logic: loading, prefix-matching, and assembly.

use std::path::PathBuf;

use super::guidance_dir;
use crate::hook::HookRunner;

/// Resolve the guidance text for a given model.
///
/// Assembles the final guidance string:
///   1. `_default.md` content (always injected, if exists)
///   2. Model-specific guidance from prefix-matched `{prefix}.md` file
///   3. Fallback to config guidance map (glob match from config)
///   4. If `reasoning == true`, append `_reasoning.md`
pub fn resolve_guidance(
    model_id: &str,
    config_guidance: &std::collections::HashMap<String, String>,
    reasoning: bool,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // 1. Always inject _default guidance
    if let Some(content) = load_named_file("_default") {
        parts.push(content);
    }

    // 2. Try prefix-matched file from guidance dir
    // 3. Fallback to config guidance map
    if let Some(content) = resolve_model_guidance(model_id, config_guidance) {
        parts.push(content);
    }

    // 4. Append reasoning guidance
    if reasoning {
        if let Some(content) = load_named_file("_reasoning") {
            parts.push(content);
        }
    }

    parts.join("\n")
}

/// Resolve the guidance text for a given model with InstructionsLoaded hook support.
///
/// Assembles the final guidance string and triggers hooks for loaded files:
///   1. `_default.md` content (always injected, if exists)
///   2. Model-specific guidance from prefix-matched `{prefix}.md` file
///   3. Fallback to config guidance map (glob match from config)
///   4. If `reasoning == true`, append `_reasoning.md`
pub async fn resolve_guidance_async(
    model_id: &str,
    config_guidance: &std::collections::HashMap<String, String>,
    reasoning: bool,
    hook_runner: Option<&HookRunner>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // 1. Always inject _default guidance
    if let Some(content) = load_named_file_async("_default", hook_runner).await {
        parts.push(content);
    }

    // 2. Try prefix-matched file from guidance dir
    // 3. Fallback to config guidance map
    if let Some(content) =
        resolve_model_guidance_async(model_id, config_guidance, hook_runner).await
    {
        parts.push(content);
    }

    // 4. Append reasoning guidance
    if reasoning {
        if let Some(content) = load_named_file_async("_reasoning", hook_runner).await {
            parts.push(content);
        }
    }

    parts.join("\n")
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Resolve model-specific guidance: guidance dir prefix match → config map.
fn resolve_model_guidance(
    model_id: &str,
    config_guidance: &std::collections::HashMap<String, String>,
) -> Option<String> {
    // Try guidance dir: prefix-matched file (longest match wins)
    if let Some(content) = load_prefix_matched_file(model_id) {
        return Some(content);
    }

    // Try config guidance map
    find_matching_config_guidance(model_id, config_guidance)
}

/// Async version of resolve_model_guidance with hook support.
pub async fn resolve_model_guidance_async(
    model_id: &str,
    config_guidance: &std::collections::HashMap<String, String>,
    hook_runner: Option<&HookRunner>,
) -> Option<String> {
    // Try guidance dir: prefix-matched file (longest match wins) with hook
    if let Some(content) = load_prefix_matched_file_async(model_id, hook_runner).await {
        return Some(content);
    }

    // Try config guidance map
    find_matching_config_guidance(model_id, config_guidance)
}

/// Load prefix-matched guidance file with hook support.
async fn load_prefix_matched_file_async(
    model_id: &str,
    hook_runner: Option<&HookRunner>,
) -> Option<String> {
    let dir = guidance_dir()?;
    let mut best_match: Option<(String, PathBuf)> = None;

    // Collect all .md files in the guidance dir
    let entries = std::fs::read_dir(&dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };

        // Prefix match: file name must be a prefix of the model id
        if !stem.starts_with('_') && model_id.starts_with(stem) {
            match &best_match {
                None => best_match = Some((stem.to_string(), path)),
                Some((prev_stem, _)) if stem.len() > prev_stem.len() => {
                    best_match = Some((stem.to_string(), path));
                }
                _ => {}
            }
        }
    }

    if let Some((_, path)) = best_match {
        // Trigger hook for prefix-matched guidance file
        if let Some(hr) = hook_runner {
            let file_path_str = path.to_string_lossy().to_string();
            hr.on_instructions_loaded(&file_path_str, "guidance").await;
        }
        std::fs::read_to_string(&path).ok()
    } else {
        None
    }
}

/// Load a file by exact name (without .md extension) from the guidance dir.
fn load_named_file(name: &str) -> Option<String> {
    load_named_file_impl(name, None)
}

/// Load a file by exact name with optional hook runner (async version).
pub async fn load_named_file_async(name: &str, hook_runner: Option<&HookRunner>) -> Option<String> {
    load_named_file_impl_async(name, hook_runner).await
}

fn load_named_file_impl(name: &str, _hook_runner: Option<&HookRunner>) -> Option<String> {
    let dir = guidance_dir()?;
    let path = dir.join(format!("{}.md", name));
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            log::debug!("Loaded guidance from {}", path.display());
            Some(content)
        }
        Err(_) => None,
    }
}

async fn load_named_file_impl_async(
    name: &str,
    hook_runner: Option<&HookRunner>,
) -> Option<String> {
    let dir = guidance_dir()?;
    let path = dir.join(format!("{}.md", name));
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            log::debug!("Loaded guidance from {}", path.display());
            // Trigger hook for guidance files
            if let Some(hr) = hook_runner {
                let file_path_str = path.to_string_lossy().to_string();
                hr.on_instructions_loaded(&file_path_str, "guidance").await;
            }
            Some(content)
        }
        Err(_) => None,
    }
}

/// Scan guidance dir for `.md` files whose stem is a prefix of `model_id`.
/// Returns the content of the longest matching prefix (most specific).
/// Case-insensitive matching.
fn load_prefix_matched_file(model_id: &str) -> Option<String> {
    let dir = guidance_dir()?;
    let entries = std::fs::read_dir(&dir).ok()?;
    let model_lower = model_id.to_lowercase();

    let mut best_prefix = String::new();
    let mut best_content: Option<String> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        // Skip special files
        if stem.starts_with('_') {
            continue;
        }
        if model_lower.starts_with(&stem.to_lowercase()) && stem.len() > best_prefix.len() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                best_prefix = stem;
                best_content = Some(content);
            }
        }
    }

    if best_content.is_some() {
        log::debug!(
            "Matched guidance prefix '{}' for model '{}'",
            best_prefix,
            model_id
        );
    }
    best_content
}

/// Find matching guidance from config map (glob patterns → file paths).
fn find_matching_config_guidance(
    model_id: &str,
    guidance_map: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let mut matches: Vec<(&str, &str, usize)> = guidance_map
        .iter()
        .filter(|(pattern, _)| glob_match(pattern, model_id))
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
                        log::warn!(
                            "[Security] {} in {} line {}: {}",
                            w.threat_type,
                            w.filename,
                            w.line_number,
                            w.matched_text
                        );
                    }
                    if let Some(prefix) = crate::security::format_warnings(&warnings) {
                        return Some(format!("{}\n\n{}", prefix, content));
                    }
                }
                Some(content)
            }
            Err(e) => {
                log::warn!("Failed to read guidance file {}: {}", expanded, e);
                None
            }
        }
    } else {
        None
    }
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
        assert!(glob_match("glm-5.1", "glm-5.1"));
        assert!(glob_match("deepseek-*", "deepseek-chat"));
        assert!(!glob_match("glm-5", "glm-5.1"));
    }

    #[test]
    fn test_glob_match_wildcard() {
        assert!(glob_match("glm-*", "glm-5.1"));
        assert!(glob_match("*-v4-*", "deepseek-v4-flash"));
        assert!(!glob_match("deepseek-*", "glm-5.1"));
    }

    #[test]
    fn test_glob_match_double_wildcard() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*glm*", "glm-5.1"));
    }

    #[test]
    fn test_prefix_match_case_insensitive() {
        let model_lower = "GLM-5.1".to_lowercase();
        assert!(model_lower.starts_with(&"glm".to_lowercase()));
        assert!(!model_lower.starts_with(&"deepseek".to_lowercase()));
    }
}
