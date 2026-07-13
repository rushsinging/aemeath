//! Guidance resolution logic: loading, prefix-matching, and assembly.

use crate::prompt::LOG_TARGET;

use super::constants::{DEFAULT_FILES_EN, DEFAULT_FILES_ZH};
use super::guidance_dir;

#[async_trait::async_trait(?Send)]
pub trait InstructionsLoadedHook {
    async fn on_instructions_loaded(&self, file_path: &str, instruction_type: &str);
}

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
    language: &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // 1. Always inject _default guidance
    if let Some(content) = load_named_file_with_lang("_default", language) {
        parts.push(content);
    }

    // 2. Try prefix-matched file from guidance dir
    // 3. Fallback to config guidance map
    if let Some(content) = resolve_model_guidance(model_id, config_guidance, language) {
        parts.push(content);
    }

    // 4. Append reasoning guidance
    if reasoning {
        if let Some(content) = load_named_file_with_lang("_reasoning", language) {
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
    language: &str,
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // 1. Always inject _default guidance
    if let Some(content) = load_named_file_async_with_lang("_default", language, hook_runner).await
    {
        parts.push(content);
    }

    // 2. Try prefix-matched file from guidance dir
    // 3. Fallback to config guidance map
    if let Some(content) =
        resolve_model_guidance_async(model_id, config_guidance, language, hook_runner).await
    {
        parts.push(content);
    }

    // 4. Append reasoning guidance
    if reasoning {
        if let Some(content) =
            load_named_file_async_with_lang("_reasoning", language, hook_runner).await
        {
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
    language: &str,
) -> Option<String> {
    // Try guidance dir: prefix-matched file (longest match wins) with lang support
    if let Some(content) = load_prefix_matched_file_with_lang(model_id, language) {
        return Some(content);
    }

    // Try config guidance map
    find_matching_config_guidance(model_id, config_guidance)
}

/// Async version of resolve_model_guidance with hook support.
pub async fn resolve_model_guidance_async(
    model_id: &str,
    config_guidance: &std::collections::HashMap<String, String>,
    language: &str,
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) -> Option<String> {
    // Try guidance dir: prefix-matched file (longest match wins) with lang support
    if let Some(content) =
        load_prefix_matched_file_async_with_lang(model_id, language, hook_runner).await
    {
        return Some(content);
    }

    // Try config guidance map
    find_matching_config_guidance(model_id, config_guidance)
}

/// Load a named file with language subdirectory support.
/// Tries `{language}/{name}.md` first, falls back to `{name}.md`.
/// If file is empty or not found, falls back to built-in default content.
pub(crate) fn load_named_file_with_lang(name: &str, language: &str) -> Option<String> {
    let dir = guidance_dir()?;

    // Try language subdirectory first
    if !language.is_empty() {
        let lang_path = dir.join(language).join(format!("{}.md", name));
        if let Ok(content) = std::fs::read_to_string(&lang_path) {
            if !content.trim().is_empty() {
                log::debug!(target: LOG_TARGET, "Loaded guidance from {}", lang_path.display());
                return Some(content);
            }
        }
    }

    // Fallback to root directory
    let root_path = dir.join(format!("{}.md", name));
    if let Ok(content) = std::fs::read_to_string(&root_path) {
        if !content.trim().is_empty() {
            log::debug!(target: LOG_TARGET, "Loaded guidance from {}", root_path.display());
            return Some(content);
        }
    }

    // Fallback to built-in default content
    load_builtin_default(name, language)
}

/// Async version of load_named_file_with_lang.
async fn load_named_file_async_with_lang(
    name: &str,
    language: &str,
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) -> Option<String> {
    let dir = match guidance_dir() {
        Some(d) => d,
        None => return None,
    };

    // Try language subdirectory first
    if !language.is_empty() {
        let lang_path = dir.join(language).join(format!("{}.md", name));
        if let Ok(content) = std::fs::read_to_string(&lang_path) {
            if !content.trim().is_empty() {
                log::debug!(target: LOG_TARGET, "Loaded guidance from {}", lang_path.display());
                if let Some(hr) = hook_runner {
                    let file_path_str = lang_path.to_string_lossy().to_string();
                    hr.on_instructions_loaded(&file_path_str, "guidance").await;
                }
                return Some(content);
            }
        }
    }

    // Fallback to root directory
    let root_path = dir.join(format!("{}.md", name));
    if let Ok(content) = std::fs::read_to_string(&root_path) {
        if !content.trim().is_empty() {
            log::debug!(target: LOG_TARGET, "Loaded guidance from {}", root_path.display());
            if let Some(hr) = hook_runner {
                let file_path_str = root_path.to_string_lossy().to_string();
                hr.on_instructions_loaded(&file_path_str, "guidance").await;
            }
            return Some(content);
        }
    }

    // Fallback to built-in default content
    load_builtin_default(name, language)
}

/// Load prefix-matched guidance file with language subdirectory support.
/// Tries `{language}/{prefix}.md` first, falls back to `{prefix}.md`.
fn load_prefix_matched_file_with_lang(model_id: &str, language: &str) -> Option<String> {
    let dir = guidance_dir()?;
    let model_lower = model_id.to_lowercase();

    // Try language subdirectory first
    if !language.is_empty() {
        let lang_dir = dir.join(language);
        if let Some(content) = load_prefix_matched_from_dir(&lang_dir, &model_lower) {
            return Some(content);
        }
    }

    // Fallback to root directory
    load_prefix_matched_from_dir(&dir, &model_lower)
}

/// Scan a directory for prefix-matched guidance files.
fn load_prefix_matched_from_dir(dir: &std::path::Path, model_lower: &str) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;

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
        log::debug!(target: LOG_TARGET,
            "Matched guidance prefix '{}' for model '{}' in {}",
            best_prefix,
            model_lower,
            dir.display()
        );
    }
    best_content
}

/// Async version with hook support.
async fn load_prefix_matched_file_async_with_lang(
    model_id: &str,
    language: &str,
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) -> Option<String> {
    let dir = guidance_dir()?;
    let model_lower = model_id.to_lowercase();

    // Try language subdirectory first
    if !language.is_empty() {
        let lang_dir = dir.join(language);
        if let Some(content) =
            load_prefix_matched_from_dir_async(&lang_dir, &model_lower, hook_runner).await
        {
            return Some(content);
        }
    }

    // Fallback to root directory
    load_prefix_matched_from_dir_async(&dir, &model_lower, hook_runner).await
}

async fn load_prefix_matched_from_dir_async(
    dir: &std::path::Path,
    model_lower: &str,
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;

    let mut best_prefix = String::new();
    let mut best_path: Option<std::path::PathBuf> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if stem.starts_with('_') {
            continue;
        }
        if model_lower.starts_with(&stem.to_lowercase()) && stem.len() > best_prefix.len() {
            best_prefix = stem;
            best_path = Some(path);
        }
    }

    if let Some(path) = best_path {
        if let Some(hr) = hook_runner {
            let file_path_str = path.to_string_lossy().to_string();
            hr.on_instructions_loaded(&file_path_str, "guidance").await;
        }
        std::fs::read_to_string(&path).ok()
    } else {
        None
    }
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
                let warnings = crate::prompt::business::security::scan_content(path, &content);
                if !warnings.is_empty() {
                    for w in &warnings {
                        log::warn!(target: LOG_TARGET,
                            "[Security] {} in {} line {}: {}",
                            w.threat_type,
                            w.filename,
                            w.line_number,
                            w.matched_text
                        );
                    }
                    if let Some(prefix) =
                        crate::prompt::business::security::format_warnings(&warnings)
                    {
                        return Some(format!("{}\n\n{}", prefix, content));
                    }
                }
                Some(content)
            }
            Err(e) => {
                log::warn!(target: LOG_TARGET, "Failed to read guidance file {}: {}", expanded, e);
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
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}/{}", home.display(), stripped);
        }
    }
    path.to_string()
}

/// Load built-in default content for a given file name and language.
fn load_builtin_default(name: &str, language: &str) -> Option<String> {
    let filename = format!("{}.md", name);
    let files = match language {
        "zh" => DEFAULT_FILES_ZH,
        _ => DEFAULT_FILES_EN,
    };

    for &(file_name, content) in files {
        if file_name == filename {
            log::debug!(target: LOG_TARGET, "Using built-in default for {} ({})", filename, language);
            return Some(content.trim().to_string());
        }
    }

    None
}

#[cfg(test)]
#[path = "resolver_tests.rs"]
mod tests;
