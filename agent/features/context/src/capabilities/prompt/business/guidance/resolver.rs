//! Guidance resolution logic: loading, prefix-matching, and assembly.

use crate::capabilities::prompt::LOG_TARGET;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::constants::{DEFAULT_FILES_EN, DEFAULT_FILES_ZH};
use super::guidance_dir;

#[async_trait::async_trait(?Send)]
pub trait InstructionsLoadedHook {
    async fn on_instructions_loaded(&self, file_path: &str, instruction_type: &str);
}

#[derive(Debug)]
struct LoadedGuidance {
    path: Option<PathBuf>,
    content: String,
}

impl LoadedGuidance {
    fn builtin(content: String) -> Self {
        Self {
            path: None,
            content,
        }
    }

    fn file(path: PathBuf, content: String) -> Self {
        Self {
            path: Some(path),
            content,
        }
    }
}

/// Resolve the guidance text for a given model.
///
/// Assembles all matching guidance in deterministic priority order:
///   1. `_default.md` content (always injected, if exists)
///   2. Every prefix-matched `{prefix}.md`, general to specific
///   3. Every matching config guidance entry, general to specific
///   4. If `reasoning == true`, append `_reasoning.md`
pub fn resolve_guidance(
    model_id: &str,
    config_guidance: &HashMap<String, String>,
    reasoning: bool,
    language: &str,
) -> String {
    collect_guidance(model_id, config_guidance, reasoning, language)
        .into_iter()
        .map(|guidance| guidance.content)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Resolve guidance and trigger `InstructionsLoaded` once per loaded file.
pub async fn resolve_guidance_async(
    model_id: &str,
    config_guidance: &HashMap<String, String>,
    reasoning: bool,
    language: &str,
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) -> String {
    let guidance = collect_guidance(model_id, config_guidance, reasoning, language);
    trigger_loaded_hooks(&guidance, hook_runner).await;
    guidance
        .into_iter()
        .map(|guidance| guidance.content)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Resolve all model-specific file and config guidance with hook support.
pub async fn resolve_model_guidance_async(
    model_id: &str,
    config_guidance: &HashMap<String, String>,
    language: &str,
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) -> Option<String> {
    let guidance = collect_model_guidance(model_id, config_guidance, language);
    trigger_loaded_hooks(&guidance, hook_runner).await;
    join_loaded_guidance(guidance)
}

fn collect_guidance(
    model_id: &str,
    config_guidance: &HashMap<String, String>,
    reasoning: bool,
    language: &str,
) -> Vec<LoadedGuidance> {
    let mut guidance = Vec::new();

    if let Some(default) = load_named_guidance_with_lang("_default", language) {
        guidance.push(default);
    }
    guidance.extend(collect_model_guidance(model_id, config_guidance, language));
    if reasoning {
        if let Some(reasoning) = load_named_guidance_with_lang("_reasoning", language) {
            guidance.push(reasoning);
        }
    }

    guidance
}

fn collect_model_guidance(
    model_id: &str,
    config_guidance: &HashMap<String, String>,
    language: &str,
) -> Vec<LoadedGuidance> {
    let mut guidance = load_prefix_matched_files_with_lang(model_id, language);
    guidance.extend(load_matching_config_guidance(model_id, config_guidance));
    guidance
}

fn join_loaded_guidance(guidance: Vec<LoadedGuidance>) -> Option<String> {
    if guidance.is_empty() {
        None
    } else {
        Some(
            guidance
                .into_iter()
                .map(|guidance| guidance.content)
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

async fn trigger_loaded_hooks(
    guidance: &[LoadedGuidance],
    hook_runner: Option<&dyn InstructionsLoadedHook>,
) {
    let Some(hook_runner) = hook_runner else {
        return;
    };
    for item in guidance {
        if let Some(path) = &item.path {
            hook_runner
                .on_instructions_loaded(&path.to_string_lossy(), "guidance")
                .await;
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Load a named file with language subdirectory support.
/// Tries `{language}/{name}.md` first, falls back to `{name}.md`, then built-in content.
pub(crate) fn load_named_file_with_lang(name: &str, language: &str) -> Option<String> {
    load_named_guidance_with_lang(name, language).map(|guidance| guidance.content)
}

fn load_named_guidance_with_lang(name: &str, language: &str) -> Option<LoadedGuidance> {
    let dir = guidance_dir()?;
    if !language.is_empty() {
        let lang_path = dir.join(language).join(format!("{name}.md"));
        if let Some(guidance) = read_guidance_file(lang_path, false) {
            return Some(guidance);
        }
    }

    let root_path = dir.join(format!("{name}.md"));
    if let Some(guidance) = read_guidance_file(root_path, false) {
        return Some(guidance);
    }

    load_builtin_default(name, language).map(LoadedGuidance::builtin)
}

/// Load every prefix-matched guidance file, general to specific.
/// A non-empty language file overrides only the root file with the same normalized stem.
fn load_prefix_matched_files_with_lang(model_id: &str, language: &str) -> Vec<LoadedGuidance> {
    let Some(dir) = guidance_dir() else {
        return Vec::new();
    };
    let model_lower = model_id.to_lowercase();
    let mut candidates: BTreeMap<String, LoadedGuidance> =
        scan_prefix_candidates(&dir, &model_lower)
            .into_iter()
            .filter_map(|(stem, path)| read_guidance_file(path, false).map(|item| (stem, item)))
            .collect();

    if !language.is_empty() {
        let lang_dir = dir.join(language);
        for (stem, path) in scan_prefix_candidates(&lang_dir, &model_lower) {
            if let Some(item) = read_guidance_file(path, false) {
                candidates.insert(stem, item);
            }
        }
    }

    let mut candidates: Vec<_> = candidates.into_iter().collect();
    candidates.sort_by(|(stem_a, item_a), (stem_b, item_b)| {
        stem_a
            .len()
            .cmp(&stem_b.len())
            .then_with(|| stem_a.cmp(stem_b))
            .then_with(|| item_a.path.cmp(&item_b.path))
    });

    candidates
        .into_iter()
        .map(|(stem, guidance)| {
            log::debug!(target: LOG_TARGET,
                "Matched guidance prefix '{}' for model '{}'",
                stem,
                model_lower
            );
            guidance
        })
        .collect()
}

fn scan_prefix_candidates(dir: &Path, model_lower: &str) -> BTreeMap<String, PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return BTreeMap::new();
    };

    let mut candidates = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if stem.starts_with('_') {
            continue;
        }
        let normalized = stem.to_lowercase();
        if !model_lower.starts_with(&normalized) {
            continue;
        }
        insert_prefix_candidate(&mut candidates, normalized, path);
    }
    candidates
}

fn insert_prefix_candidate(
    candidates: &mut BTreeMap<String, PathBuf>,
    normalized: String,
    path: PathBuf,
) {
    candidates
        .entry(normalized)
        .and_modify(|existing| {
            if path < *existing {
                *existing = path.clone();
            }
        })
        .or_insert(path);
}

/// Load every matching config guidance file, general to specific.
fn load_matching_config_guidance(
    model_id: &str,
    guidance_map: &HashMap<String, String>,
) -> Vec<LoadedGuidance> {
    let mut matches: Vec<_> = guidance_map
        .iter()
        .filter(|(pattern, _)| glob_match(pattern, model_id))
        .map(|(pattern, path)| {
            let literal_len = pattern
                .chars()
                .filter(|character| *character != '*')
                .count();
            let wildcards = pattern
                .chars()
                .filter(|character| *character == '*')
                .count();
            (pattern.as_str(), path.as_str(), literal_len, wildcards)
        })
        .collect();

    matches.sort_by(|a, b| {
        a.2.cmp(&b.2)
            .then_with(|| b.3.cmp(&a.3))
            .then_with(|| a.0.cmp(b.0))
            .then_with(|| a.1.cmp(b.1))
    });

    let mut loaded_paths = HashSet::new();
    matches
        .into_iter()
        .filter_map(|(_, path, _, _)| {
            let expanded = PathBuf::from(expand_tilde(path));
            let identity = std::fs::canonicalize(&expanded).unwrap_or_else(|_| expanded.clone());
            if !loaded_paths.insert(identity) {
                return None;
            }
            read_guidance_file(expanded, true)
        })
        .collect()
}

fn read_guidance_file(path: PathBuf, scan_security: bool) -> Option<LoadedGuidance> {
    let content = match std::fs::read_to_string(&path) {
        Ok(content) if !content.trim().is_empty() => content,
        Ok(_) => return None,
        Err(error) => {
            if scan_security || path.exists() {
                log::warn!(target: LOG_TARGET,
                    "Failed to read guidance file {}: {}",
                    path.display(),
                    error
                );
            }
            return None;
        }
    };

    log::debug!(target: LOG_TARGET, "Loaded guidance from {}", path.display());
    if !scan_security {
        return Some(LoadedGuidance::file(path, content));
    }

    let display_path = path.to_string_lossy();
    let warnings =
        crate::capabilities::prompt::business::security::scan_content(&display_path, &content);
    for warning in &warnings {
        log::warn!(target: LOG_TARGET,
            "[Security] {} in {} line {}: {}",
            warning.threat_type,
            warning.filename,
            warning.line_number,
            warning.matched_text
        );
    }
    let content = crate::capabilities::prompt::business::security::format_warnings(&warnings)
        .map(|prefix| format!("{prefix}\n\n{content}"))
        .unwrap_or(content);
    Some(LoadedGuidance::file(path, content))
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
